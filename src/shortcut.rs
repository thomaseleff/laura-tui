//! `laura --shortcut` (#165): write the Laura icon, which runs `laura --serve --open`. The icon
//! holds the full path of this program: a program that an icon starts may not have uv's folder on
//! its `PATH`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use base64::Engine;

/// Tidepool, the Laura logo, 256 × 256.
const PNG: &[u8] = include_bytes!("../assets/tidepool.png");
/// The same tile at each size that Windows shows, from 16 to 256 px: Windows scales one large
/// image down poorly.
const ICO: &[u8] = include_bytes!("../assets/tidepool.ico");

/// Write the Laura icon for this computer, and print each path that Laura wrote.
pub fn write() -> Result<()> {
    let exe = std::env::current_exe()?;
    let home = std::env::home_dir().context("Laura could not find the home folder")?;
    // A login shell, so the Laura server and its workspaces get the user's PATH (the agent CLI).
    // Not csh or tcsh: they take `-l` only as their one flag, so `-ilc` fails.
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| {
            let name = Path::new(s).file_name().and_then(|n| n.to_str());
            !s.is_empty() && !matches!(name, Some("csh" | "tcsh"))
        })
        .unwrap_or_else(|| "/bin/sh".into());
    let inner = format!("exec {} --serve --open", sh_quote(&exe.to_string_lossy()));
    if let Ok(distro) = std::env::var("WSL_DISTRO_NAME") {
        let (win, dir) = crate::browser::win_env("USERPROFILE")
            .context("Laura could not find the Windows home folder")?;
        let ico = save_ico(&dir.join(".laura"))?;
        // Minimized: the terminal that runs the Laura server stays out of the way. No quotes
        // around the distribution: `wsl.exe -d` keeps them in the name, and a name has no space.
        return lnk(
            "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
            r"C:\Windows\System32\wsl.exe",
            &format!("-d {distro} --cd ~ -e {shell} -ilc \"{inner}\""),
            &format!(r"{win}\.laura\{ico}"),
            7,
        );
    }
    if cfg!(windows) {
        let dir = home.join(".laura");
        let icon = dir.join(save_ico(&dir)?);
        let lauraw = exe.with_file_name("lauraw.exe");
        if !lauraw.is_file() {
            bail!("{} is missing: reinstall Laura", lauraw.display());
        }
        return lnk(
            "powershell.exe",
            &lauraw.to_string_lossy(),
            "",
            &icon.to_string_lossy(),
            1,
        );
    }
    if cfg!(target_os = "macos") {
        let app = home.join("Applications/Laura.app/Contents");
        save(&app.join("Info.plist"), INFO_PLIST.as_bytes())?;
        let script = app.join("MacOS/Laura");
        // In the background, so the script ends: LaunchServices runs it again on the next click
        // only once `Laura.app` ended.
        let body = format!(
            "#!/bin/sh\n{} -ilc {} </dev/null >/dev/null 2>&1 &\n",
            sh_quote(&shell),
            sh_quote(&inner)
        );
        save(&script, body.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        }
        return save(&app.join("Resources/laura.icns"), &icns());
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map_or_else(|| home.join(".local/share"), PathBuf::from);
    let png = data.join("icons/laura.png");
    save(&png, PNG)?;
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Laura\nComment=Open the Laura server\n\
         Exec={shell} -ilc \"{}\"\nIcon={}\nTerminal=false\nCategories=Development;\n",
        desktop_escape(&inner),
        png.display()
    );
    save(&data.join("applications/laura.desktop"), desktop.as_bytes())
}

const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Laura</string>
  <key>CFBundleExecutable</key><string>Laura</string>
  <key>CFBundleIconFile</key><string>laura</string>
  <key>CFBundleIdentifier</key><string>io.github.thomaseleff.laura</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSUIElement</key><true/>
</dict>
</plist>
"#;

/// Write the `.ico` in `dir`, named by version, and delete the `.ico` of other versions. Explorer
/// caches an icon by its path, so a new path makes Explorer read the new file after an upgrade.
fn save_ico(dir: &Path) -> Result<String> {
    let name = format!("laura-{}.ico", env!("CARGO_PKG_VERSION"));
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let old = e.file_name().to_string_lossy().into_owned();
        if old.starts_with("laura") && old.ends_with(".ico") && old != name {
            let _ = std::fs::remove_file(e.path());
        }
    }
    save(&dir.join(&name), ICO)?;
    Ok(name)
}

fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("couldn't write {}", path.display()))?;
    println!("{}", path.display());
    Ok(())
}

/// Write `Laura.lnk` on the Desktop and in the Start Menu with PowerShell, and print the paths.
fn lnk(powershell: &str, target: &str, args: &str, icon: &str, style: u8) -> Result<()> {
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let script = format!(
        "$ErrorActionPreference = 'Stop'; $s = New-Object -ComObject WScript.Shell; \
         foreach ($f in 'Desktop', 'Programs') {{ \
         $p = Join-Path ([Environment]::GetFolderPath($f)) 'Laura.lnk'; \
         $l = $s.CreateShortcut($p); $l.TargetPath = {}; $l.Arguments = {}; \
         $l.IconLocation = {}; $l.WindowStyle = {style}; $l.Save(); $p }}",
        q(target),
        q(args),
        q(icon)
    );
    // Encoded, so no quote in the script meets Windows's command-line quoting.
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut cmd = Command::new(powershell);
    cmd.args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(base64::engine::general_purpose::STANDARD.encode(utf16))
        .stdin(Stdio::null());
    // A Windows folder, so PowerShell from WSL doesn't warn about a UNC one.
    if cfg!(unix) {
        cmd.current_dir("/mnt/c/Windows/System32");
    }
    let out = cmd
        .output()
        .with_context(|| format!("couldn't run {powershell}"))?;
    if !out.status.success() {
        bail!(
            "PowerShell could not write the Laura icon: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if !line.trim().is_empty() {
            println!("{}", line.trim());
        }
    }
    Ok(())
}

/// `s` as one word for `sh`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `s` inside a double-quoted argument of a desktop entry's `Exec=`: the quoting rule escapes
/// `"`, `` ` ``, `$` and `\`, then the string rule escapes `\` again, and `%` is a field code.
fn desktop_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '"' | '`' | '$' => format!(r"\\{c}"),
            '\\' => r"\\\\".into(),
            '%' => "%%".into(),
            c => c.into(),
        })
        .collect()
}

/// An `.icns` that holds the PNG as its one entry, `ic08` (256 × 256).
fn icns() -> Vec<u8> {
    let entry = PNG.len() as u32 + 8;
    let mut v = b"icns".to_vec();
    v.extend((entry + 8).to_be_bytes());
    v.extend(b"ic08");
    v.extend(entry.to_be_bytes());
    v.extend(PNG);
    v
}
