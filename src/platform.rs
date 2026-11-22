//! Laura's place on the user's computer: its folders, private files, the pages it writes for the
//! user's browser, and the opener that starts that browser.

use std::io::Write;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::http::nonce;

/// Laura's data dir: `~/.laura` on every OS.
pub fn data_dir() -> PathBuf {
    std::env::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".laura")
}

/// Scratch dir for `laura tail` spools: a per-user temp dir, so a crash's leftovers sit with the OS's temp files.
/// Always a `laura` subdir: `is_runtime_temp` is a direct-child check, and `close` deletes what it matches.
/// Inside a tab, `LAURA_RUNTIME` carries the host's choice, so a client with other temp env spools where `close` looks (#84).
/// A `LAURA_RUNTIME` the client can't create (sandbox) falls through; `close` can't find that spool, so it stays in the temp dir.
pub fn runtime_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("LAURA_RUNTIME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute() && d.ends_with("laura") && std::fs::create_dir_all(d).is_ok())
    {
        return d;
    }
    // An XDG_RUNTIME_DIR Laura can't create a dir in falls through to the per-OS dir.
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute() && std::fs::create_dir_all(d.join("laura")).is_ok())
    {
        return d.join("laura");
    }
    if cfg!(any(windows, target_os = "macos")) {
        std::env::temp_dir().join("laura") // per-user already
    } else {
        data_dir().join("runtime").join("laura") // a bare /tmp is shared between users
    }
}
/// A page that redirects to `url`, for the user's browser: the browser's command line, which
/// `ps` shows to other users, then holds only a path that they can't read (Jupyter's redirect
/// file). The page holds only the port and a hex token, so `url` needs no escaping.
pub(crate) fn redirect_page(dir: &Path, url: &str) -> Result<PathBuf> {
    page(
        dir,
        &format!(
            "<!doctype html><meta charset=\"utf-8\"><title>Laura</title>\
             <meta http-equiv=\"refresh\" content=\"0;url={url}\">\
             <script>location.replace(\"{url}\")</script>"
        ),
    )
}

/// Write `html` to a new `open-<nonce>.html` in `dir`, which only the user can read. First delete
/// the pages older than a minute: a process that exits at once (a second `laura --serve --open`)
/// leaves its page behind.
pub(crate) fn page(dir: &Path, html: &str) -> Result<PathBuf> {
    // `runtime_dir` creates only the env-chosen dirs; its per-OS fallback can be missing on a
    // first launch, as can a snap's folder.
    std::fs::create_dir_all(dir)?;
    for e in std::fs::read_dir(dir)?.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|d| d > Duration::from_secs(60)));
        if old && name.starts_with("open-") && name.ends_with(".html") {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let path = dir.join(format!("open-{}.html", nonce(8)?));
    let mut f = std::fs::OpenOptions::new();
    f.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut f, 0o600);
    f.open(&path)?.write_all(html.as_bytes())?;
    Ok(path)
}

/// Write `bytes` to `path`, a file that only the user can read on Unix. Writes a `.tmp` beside
/// `path` and renames it over `path`, so a reader never sees a half-written file. The `.tmp` has
/// the process id, so two processes that write `path` at once never share a `.tmp`.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut f = std::fs::OpenOptions::new();
    f.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut f, 0o600);
    f.open(&tmp)?.write_all(bytes)?;
    // Each process leaves its own `.tmp`, so remove the `.tmp` when the rename fails.
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// The folder Laura started in, for the top bar: under the home folder as `~/…`, with `/` between
/// names on every OS.
pub(crate) fn shown_root(root: &Path) -> String {
    let strip = crate::gitdiff::strip_verbatim;
    let root = strip(root.to_path_buf());
    let home = std::env::home_dir().and_then(|h| h.canonicalize().ok().map(strip));
    let shown = match home.as_deref().and_then(|h| root.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => root.display().to_string(),
    };
    if cfg!(windows) {
        shown.replace('\\', "/")
    } else {
        shown
    }
}
/// The program that opens the user's browser, and the folder where that browser can read the page.
pub struct Opener {
    browser: Option<PathBuf>,
    pub dir: PathBuf,
}

/// `LAURA_BROWSER`, else the OS's default browser, which opens the page in a new tab. Not a Chrome
/// app window: its taskbar shows the page's 16 px favicon scaled up, and each workspace gets a
/// new window (a desktop app is #164).
pub fn opener() -> Opener {
    let browser = std::env::var_os("LAURA_BROWSER")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from);
    let dir = snap_dir(browser.as_deref()).unwrap_or_else(runtime_dir);
    Opener { browser, dir }
}

impl Opener {
    /// Open the browser at `page`, a file in `self.dir`.
    pub fn open(self, page: &Path) -> std::io::Result<()> {
        let mut cmd = match self.browser {
            Some(p) => {
                let url = file_url(&p, page)?;
                let mut c = Command::new(p);
                c.arg(url);
                c
            }
            // See `win_env` for why `cmd.exe` by its full path, run in a Windows folder.
            None if cfg!(unix) && std::env::var_os("WSL_DISTRO_NAME").is_some() => {
                let mut c = Command::new("/mnt/c/Windows/System32/cmd.exe");
                c.args(["/d", "/c", "start", ""])
                    .arg(win_path(page)?)
                    .current_dir("/mnt/c/Windows/System32");
                c
            }
            None if cfg!(windows) => {
                let mut c = Command::new("cmd");
                c.args(["/C", "start", ""]).arg(page);
                // `lauraw` has no console, so `cmd` would open one.
                #[cfg(windows)]
                std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000); // CREATE_NO_WINDOW
                c
            }
            None => {
                let mut c = Command::new(if cfg!(target_os = "macos") {
                    "open"
                } else {
                    "xdg-open"
                });
                c.arg(page);
                c
            }
        };
        let name = cmd.get_program().to_string_lossy().into_owned();
        // The browser outlives Laura: the terminal's Ctrl+C and hangup go to Laura's group only.
        #[cfg(unix)]
        cmd.process_group(0);
        let start = Instant::now();
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| std::io::Error::new(e.kind(), format!("{name}: {e}")))?;
        // Chrome hands the page to a running Chrome and exits with 0; reap it so it leaves no zombie.
        // A failed exit is the only sign that no browser opened (no display over SSH).
        std::thread::spawn(move || {
            if let Ok(status) = child.wait()
                && !status.success()
                // Later, the exit is the end of the user's own Chrome session, not a failed open.
                && start.elapsed() < Duration::from_secs(5)
            {
                no_browser(format!("{name} exited with {status}"));
            }
        });
        Ok(())
    }
}

/// A snap browser (Ubuntu's Firefox, snap Chromium) can't read hidden folders or `/run/user`,
/// only its own `~/snap/<name>/common`.
fn snap_dir(app: Option<&Path>) -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let name = match app {
        // `/snap/bin/<name>` links to `/usr/bin/snap`, which runs the snap that the link names.
        Some(app) => {
            let target = std::fs::canonicalize(app).ok()?;
            if target.file_name()? != "snap" {
                return None;
            }
            app.file_name()?.to_str()?.to_string()
        }
        None => {
            let out = Command::new("xdg-mime")
                .args(["query", "default", "text/html"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            let desktop = String::from_utf8(out.stdout).ok()?.trim().to_string();
            // snapd names a snap's desktop file `<snap>_<app>.desktop`.
            let (name, _) = desktop.split_once('_')?;
            if name.contains('/')
                || !Path::new("/var/lib/snapd/desktop/applications")
                    .join(&desktop)
                    .is_file()
            {
                return None;
            }
            name.to_string()
        }
    };
    Some(std::env::home_dir()?.join("snap").join(name).join("common"))
}

/// The URL that Laura printed still works on this computer; over SSH, the TUI does.
pub fn no_browser(reason: impl std::fmt::Display) {
    eprintln!("Laura could not open a web browser: {reason}");
    eprintln!("To use Laura in this terminal instead, press Ctrl+C and run laura --tui.");
}

/// A folder in the user's own Windows environment, such as %USERPROFILE%, from WSL: the Windows
/// path and the WSL path. Not every folder under `/mnt/c/Users`: any Windows user can write to
/// `Public` and `All Users`. `cmd.exe` by its full path, since `appendWindowsPath=false` keeps it
/// off PATH; run in a Windows folder, so it doesn't warn about a UNC one.
pub(crate) fn win_env(name: &str) -> Option<(String, PathBuf)> {
    // `/u` makes `cmd` write UTF-16: its OEM code page is not UTF-8 (`C:\Users\André`).
    let out = Command::new("/mnt/c/Windows/System32/cmd.exe")
        .args(["/d", "/u", "/c", &format!("echo %{name}%")])
        .current_dir("/mnt/c/Windows/System32")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let units: Vec<u16> = out
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .collect();
    let win = String::from_utf16(&units).ok()?.trim().to_string();
    // An unset variable echoes as itself.
    if win.is_empty() || win.contains('%') {
        return None;
    }
    let out = Command::new("wslpath").args(["-u", &win]).output().ok()?;
    let unix = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !unix.is_empty()).then(|| (win, PathBuf::from(unix)))
}

/// `page` as Windows reads it from WSL, through `\\wsl.localhost\<distro>`.
fn win_path(page: &Path) -> std::io::Result<String> {
    let out = Command::new("wslpath").arg("-w").arg(page).output()?;
    let win = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || win.is_empty() {
        return Err(std::io::Error::other(format!(
            "wslpath could not convert {}",
            page.display()
        )));
    }
    Ok(win)
}

/// `page` as a `file:` URL for `program`. A Windows browser that WSL starts reads the page
/// through `\\wsl.localhost\<distro>`.
fn file_url(program: &Path, page: &Path) -> std::io::Result<String> {
    let exe = program
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    let path = if cfg!(unix) && exe {
        win_path(page)?
    } else {
        page.to_string_lossy().into_owned()
    };
    // A drive (`C:`) stays as is; `\\host\share` and `/abs` keep their leading slashes.
    let path = path
        .replace('\\', "/")
        .split('/')
        .map(|s| match s.as_bytes() {
            [d, b':'] if d.is_ascii_alphabetic() => s.to_string(),
            _ => crate::http::pct_encode(s),
        })
        .collect::<Vec<_>>()
        .join("/");
    Ok(if path.starts_with("//") {
        format!("file:{path}")
    } else if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    })
}
