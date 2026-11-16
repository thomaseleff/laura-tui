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

/// Deletes the files on drop: a `Laura.lnk` outside the temporary home would stay on the computer.
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
    // Windows builds both folders from `USERPROFILE`, and returns "" for a missing one.
    if cfg!(windows) {
        let start = r"AppData\Roaming\Microsoft\Windows\Start Menu\Programs";
        for dir in ["Desktop", start] {
            std::fs::create_dir_all(home.path().join(dir))?;
        }
    }
    let shortcut = |shell: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_laura"))
            .arg("--shortcut")
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("SHELL", shell)
            .env_remove("XDG_DATA_HOME")
            .output()
    };
    let out = shortcut("/bin/sh")?;
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
    assert!(paths.iter().all(|p| p.starts_with(home.path())), "{stdout}");
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
        // Named by version: Explorer caches an icon by its path.
        let ico = format!("laura-{}.ico", env!("CARGO_PKG_VERSION"));
        // An ICO header, then 8 frames: 16, 24, 32, 48, 64, 96, 128 and 256 px.
        assert!(std::fs::read(file(&ico))?.starts_with(&[0, 0, 1, 0, 8, 0]));
    } else if cfg!(target_os = "macos") {
        let plist = std::fs::read_to_string(file("Info.plist"))?;
        assert!(plist.contains("<key>LSUIElement</key><true/>"), "{plist}");
        let script = file("MacOS/Laura");
        let body = std::fs::read_to_string(script)?;
        assert!(body.contains(" -ilc "), "{body}");
        // In the background, so the script ends and a second click runs it again.
        assert!(body.ends_with(" &\n"), "{body}");
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

        // csh and tcsh take no `-ilc`: Laura uses `/bin/sh`.
        let out = shortcut("/bin/tcsh")?;
        assert!(out.status.success(), "{out:?}");
        let desktop = std::fs::read_to_string(file("laura.desktop"))?;
        assert!(desktop.contains(&exec), "{desktop}");
    }
    Ok(())
}

#[test]
fn laura_shortcut_takes_no_program() -> Result<()> {
    let home = temp()?;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_laura"))
        .args(["--shortcut", "--", "x"])
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .output()?;
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--shortcut takes no program"), "{stderr}");
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
                .strip_prefix("file://")
                .unwrap_or_else(|| panic!("not a file page: {line}"));
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
fn a_second_click_after_an_upgrade_opens_the_new_laura_server() -> Result<()> {
    use std::io::{Read, Write};

    let dir = temp()?;
    stub_browser(dir.path())?;
    // A Laura server of version 0.0.0 that the first click stops: it answers one request, so
    // the second click's **Stop** finds no Laura server.
    let old = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = old.local_addr()?.port();
    std::fs::create_dir_all(dir.path().join(".laura"))?;
    let state = serde_json::json!({"token": "old-key", "port": port});
    std::fs::write(dir.path().join(".laura/serve.json"), state.to_string())?;
    std::thread::spawn(move || {
        let Ok((mut s, _)) = old.accept() else { return };
        drop(old);
        let mut head = vec![];
        let mut b = [0; 1];
        while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).is_ok_and(|n| n == 1) {
            head.push(b[0]);
        }
        let _ = s.write_all(
            b"HTTP/1.1 200 OK\r\nX-Laura-Version: 0.0.0\r\nContent-Length: 0\r\n\
              Connection: close\r\n\r\n",
        );
    });
    let args = ["--serve", "--open", "--port", &port.to_string()];
    let (_server, url) = laura_start(dir.path(), &args, Some(dir.path()))?;
    let prefix = format!("http://127.0.0.1:{port}/?t=");
    assert!(url.starts_with(&prefix), "{url}");
    let page = opened(dir.path(), 1)?;
    assert!(page.contains(&url), "{page}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_second_click_opens_the_laura_server_that_the_first_click_started() -> Result<()> {
    use std::io::{Read, Write};

    let dir = temp()?;
    stub_browser(dir.path())?;
    // A Laura server of version 0.0.0 for the first request. Then the first click has replaced
    // it: the port answers with this version and a new key, and refuses the old key.
    let server = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = server.local_addr()?.port();
    let serve_json = dir.path().join(".laura/serve.json");
    std::fs::create_dir_all(dir.path().join(".laura"))?;
    let state = serde_json::json!({"token": "old-key", "port": port});
    std::fs::write(&serve_json, state.to_string())?;
    std::thread::spawn(move || {
        let mut version = "0.0.0";
        for mut s in server.incoming().flatten() {
            let mut head = vec![];
            let mut b = [0; 1];
            while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).is_ok_and(|n| n == 1) {
                head.push(b[0]);
            }
            let status = if head.starts_with(b"POST /quit ") {
                "401 Unauthorized"
            } else {
                "200 OK"
            };
            let _ = write!(
                s,
                "HTTP/1.1 {status}\r\nX-Laura-Version: {version}\r\nContent-Length: 0\r\n\
                 Connection: close\r\n\r\n"
            );
            if version == "0.0.0" {
                version = env!("CARGO_PKG_VERSION");
                let state = serde_json::json!({"token": "new-key", "port": port});
                let _ = std::fs::write(&serve_json, state.to_string());
            }
        }
    });
    let out = laura(dir.path(), &["--serve", "--open"])?;
    assert!(out.status.success(), "{out:?}");
    let url = format!("http://127.0.0.1:{port}/?t=new-key");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), url);
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
    let text = format!(
        "Another program uses port {port}. Stop that program, then select the Laura icon again."
    );
    assert!(page.contains(&text), "{page}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn two_fast_clicks_open_one_laura_server() -> Result<()> {
    use std::io::BufRead;

    let dir = temp()?;
    stub_browser(dir.path())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port()
        .to_string();
    // Both start before either writes `serve.json`: whichever binds first, the other opens it.
    let mut clicks = vec![];
    for _ in 0..2 {
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_laura"))
            .args(["--serve", "--open", "--port", &port])
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .env("USERPROFILE", dir.path())
            .env("LAURA_BROWSER", dir.path().join("browser.sh"))
            .env_remove("LAURA_TAB")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        clicks.push(Kill(child));
    }
    let mut urls = vec![];
    for click in &mut clicks {
        let stdout = click.0.stdout.take().expect("piped above");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        urls.push(rx.recv_timeout(common::DEADLINE)?.trim().to_string());
    }
    assert!(urls[0].contains("/?t="), "{urls:?}");
    assert_eq!(urls[0], urls[1]);

    // One click runs the Laura server, the other exits.
    let start = std::time::Instant::now();
    let exited = loop {
        let done: Vec<_> = clicks
            .iter_mut()
            .map(|c| c.0.try_wait())
            .collect::<Result<_, _>>()?;
        if done.iter().any(Option::is_some) {
            break done;
        }
        assert!(start.elapsed() < common::DEADLINE, "neither click exited");
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let exited: Vec<_> = exited.into_iter().flatten().collect();
    assert_eq!(exited.len(), 1, "{exited:?}");
    assert!(exited[0].success(), "{exited:?}");
    assert!(opened(dir.path(), 1)?.contains(&urls[0]));
    assert!(opened(dir.path(), 2)?.contains(&urls[0]));
    Ok(())
}
