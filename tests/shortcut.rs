//! The Laura icon (#165): `laura --shortcut` writes it for the computer, and the icon runs
//! `laura --serve --open`, which opens the Laura server in the browser and starts it when it does
//! not run. Each test keeps Laura's data in its own home folder.

mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;

#[cfg(unix)]
use rig::*;

/// A fresh folder below the tests' own temp folder.
fn temp() -> Result<tempfile::TempDir> {
    Ok(tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?)
}

/// Deletes the files on drop: on Windows, `Laura.lnk` goes to the real Desktop and Start Menu.
struct Remove(Vec<PathBuf>);

impl Drop for Remove {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[test]
fn laura_shortcut_writes_the_laura_icon() -> Result<()> {
    // On WSL, Laura writes the icon on the real Windows Desktop.
    if std::env::var_os("WSL_DISTRO_NAME").is_some() {
        return Ok(());
    }
    let home = temp()?;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_laura"))
        .arg("--shortcut")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("SHELL", "/bin/sh")
        .env_remove("XDG_DATA_HOME")
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let paths: Vec<PathBuf> = stdout.lines().map(PathBuf::from).collect();
    let _lnk = Remove(
        paths
            .iter()
            .filter(|p| p.ends_with("Laura.lnk"))
            .cloned()
            .collect(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for p in &paths {
        assert!(p.exists(), "{} is missing", p.display());
    }
    let file = |name: &str| {
        paths
            .iter()
            .find(|p| p.ends_with(name))
            .unwrap_or_else(|| panic!("Laura wrote no {name}: {stdout}"))
    };
    let exe = env!("CARGO_BIN_EXE_laura");
    if cfg!(windows) {
        assert_eq!(
            paths.iter().filter(|p| p.ends_with("Laura.lnk")).count(),
            2,
            "{stdout}"
        );
        assert!(std::fs::read(file("laura.ico"))?.starts_with(&[0, 0, 1, 0]));
    } else if cfg!(target_os = "macos") {
        let plist = std::fs::read_to_string(file("Info.plist"))?;
        assert!(plist.contains("<key>LSUIElement</key><true/>"), "{plist}");
        let script = file("MacOS/Laura");
        assert!(std::fs::read_to_string(script)?.contains(" -ilc "));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(script)?.permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "the script doesn't run");
        }
        assert!(std::fs::read(file("laura.icns"))?.starts_with(b"icns"));
    } else {
        let desktop = std::fs::read_to_string(file("laura.desktop"))?;
        let exec = format!("Exec=/bin/sh -ilc \"exec '{exe}' --serve --open\"\n");
        assert!(desktop.contains(&exec), "{desktop}");
        assert!(desktop.contains("Terminal=false\n"), "{desktop}");
        assert!(std::fs::read(file("laura.png"))?.starts_with(b"\x89PNG"));
        assert!(paths.iter().all(|p| p.starts_with(home.path())));
    }
    Ok(())
}

/// `dir/browser.sh`, which `laura_start` names in `LAURA_BROWSER`: each run adds its arguments as
/// a line to `dir/opened`.
#[cfg(unix)]
fn stub_browser(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let stub = dir.join("browser.sh");
    std::fs::write(
        &stub,
        "#!/bin/sh\necho \"$@\" >> \"$(dirname \"$0\")/opened\"\n",
    )?;
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// The page of the stub browser's `n`th run (from 1), once it ran: Laura opens a file, so `ps`
/// never shows the key.
#[cfg(unix)]
fn opened(dir: &Path, n: usize) -> Result<String> {
    let start = std::time::Instant::now();
    loop {
        let runs = std::fs::read_to_string(dir.join("opened")).unwrap_or_default();
        if let Some(line) = runs.lines().nth(n - 1) {
            let page = line
                .strip_prefix("--app=file://")
                .unwrap_or_else(|| panic!("not an app window at a file: {line}"));
            return Ok(std::fs::read_to_string(page)?);
        }
        assert!(start.elapsed() < common::DEADLINE, "no browser opened");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// `laura <args>` in `dir`, with `dir` as its home and its stub browser, run to the end.
#[cfg(unix)]
fn laura(dir: &Path, args: &[&str]) -> Result<std::process::Output> {
    Ok(std::process::Command::new(env!("CARGO_BIN_EXE_laura"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("USERPROFILE", dir)
        .env("LAURA_BROWSER", dir.join("browser.sh"))
        .env_remove("LAURA_TAB")
        .stdin(std::process::Stdio::null())
        .output()?)
}

#[cfg(unix)]
#[test]
fn laura_serve_open_opens_the_browser_and_a_second_one_exits() -> Result<()> {
    let dir = temp()?;
    stub_browser(dir.path())?;
    let args = ["--serve", "--open", "--port", "0"];
    let (_server, url) = laura_start(dir.path(), &args, Some(dir.path()))?;
    assert!(url.contains("/?t="), "{url}");
    assert!(opened(dir.path(), 1)?.contains(&url));

    // The Laura icon again: the browser opens at the running Laura server, and Laura exits.
    let out = laura(dir.path(), &["--serve", "--open"])?;
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), url);
    assert!(opened(dir.path(), 2)?.contains(&url));
    Ok(())
}

#[cfg(unix)]
#[test]
fn laura_serve_open_replaces_a_laura_server_of_another_version() -> Result<()> {
    use std::io::{Read, Write};

    let dir = temp()?;
    stub_browser(dir.path())?;
    // A Laura server of version 0.0.0, which stops on its page's **Stop**.
    let old = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = old.local_addr()?.port();
    std::fs::create_dir_all(dir.path().join(".laura"))?;
    let state = serde_json::json!({"token": "old-key", "port": port});
    std::fs::write(dir.path().join(".laura/serve.json"), state.to_string())?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for mut s in old.incoming().flatten() {
            let mut head = vec![];
            let mut b = [0; 1];
            while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).is_ok_and(|n| n == 1) {
                head.push(b[0]);
            }
            let _ = s.write_all(
                b"HTTP/1.1 200 OK\r\nX-Laura-Version: 0.0.0\r\nContent-Length: 0\r\n\
                  Connection: close\r\n\r\n",
            );
            let head = String::from_utf8_lossy(&head).into_owned();
            if head.starts_with("POST /quit ") {
                let _ = tx.send(head);
                return; // closes the listener
            }
        }
    });
    let args = ["--serve", "--open", "--port", &port.to_string()];
    let (_server, url) = laura_start(dir.path(), &args, Some(dir.path()))?;
    let quit = rx.recv_timeout(common::DEADLINE)?;
    assert!(quit.contains("\r\nX-Laura-Key: old-key\r\n"), "{quit}");
    let origin = format!("\r\nOrigin: http://127.0.0.1:{port}\r\n");
    assert!(quit.contains(&origin), "{quit}");
    let prefix = format!("http://127.0.0.1:{port}/?t=");
    assert!(url.starts_with(&prefix), "{url}");
    assert!(opened(dir.path(), 1)?.contains(&url));
    Ok(())
}

#[cfg(unix)]
#[test]
fn laura_serve_open_shows_its_error_in_the_browser() -> Result<()> {
    let dir = temp()?;
    stub_browser(dir.path())?;
    let taken = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = taken.local_addr()?.port().to_string();
    let out = laura(dir.path(), &["--serve", "--open", "--port", &port])?;
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let page = opened(dir.path(), 1)?;
    assert!(page.contains("Laura could not start"), "{page}");
    assert!(
        page.contains(&format!("port {port} is in use: pass --port")),
        "{page}"
    );
    Ok(())
}
