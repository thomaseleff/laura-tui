//! Starting the web app: `laura` and `laura --web` end to end, the user's browser, and the strings
//! a view gets.

use super::*;

#[test]
fn the_shell_has_the_browser_url() -> Result<()> {
    isolate_home();
    let mut rig = Rig::host(laura::Tab::spawn_web(shell(), 24, 80, Ok("chrome".into()))?)?;
    let v: Value = serde_json::from_str(&rig.drive(&["ready"]))?;
    assert_eq!(v["experimental"], json!([]), "{v}");
    let url = v["browser_url"]
        .as_str()
        .expect("a browser URL")
        .to_string();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    let echo = if cfg!(windows) {
        "echo %LAURA_BROWSER_URL%\r"
    } else {
        "echo $LAURA_BROWSER_URL\r"
    };
    rig.tab.pty.write(echo.as_bytes())?;
    // The typed line names the variable; only the echo shows the URL.
    rig.shell_shows(&url);
    Ok(())
}

#[test]
fn the_strings_carry_the_root_path() -> Result<()> {
    isolate_home();
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let (_child, mut ws) = laura_web(dir.path(), &["--web"])?;
    let start = Instant::now();
    let strings = loop {
        if let Some(f) = read(&mut ws).filter(|f| f.is("strings")) {
            break f.json();
        }
        assert!(start.elapsed() < common::DEADLINE, "no strings");
    };
    let name = dir.path().file_name().expect("a named folder");
    assert_eq!(
        strings["root"],
        format!("~/{}", name.to_string_lossy()),
        "{strings}"
    );
    Ok(())
}

#[test]
fn the_strings_carry_the_web_failed_text() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let strings = rig.until(&mut ws, "strings", |f| f.is("strings")).json();
    // The web app's review popover names the keystrokes on its buttons, so its text names none.
    assert_eq!(
        strings["draft"]["failed"],
        "⚠ Laura did not submit the inline review, but kept its threads and review body."
    );
    Ok(())
}

#[test]
fn laura_web_serves_a_workspace_until_the_user_quits() -> Result<()> {
    let (mut child, mut ws) = laura_web(Path::new(env!("CARGO_MANIFEST_DIR")), &["--web"])?;
    let start = Instant::now();
    while !read(&mut ws).is_some_and(|f| f.is("layout")) {
        assert!(start.elapsed() < common::DEADLINE, "no layout");
    }
    send(&mut ws, json!({"quit": true}));
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.0.try_wait()? {
            break s;
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "laura --web kept running"
        );
        let _ = read(&mut ws);
    };
    assert!(status.success(), "{status}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn plain_laura_serves_the_web_app_and_opens_no_browser_from_a_pipe() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let stub = dir.path().join("browser.sh");
    std::fs::write(&stub, "#!/bin/sh\ntouch \"$(dirname \"$0\")/opened\"\n")?;
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))?;
    let (_child, mut ws) = laura_web(dir.path(), &[])?;
    let start = Instant::now();
    while !read(&mut ws).is_some_and(|f| f.is("layout")) {
        assert!(start.elapsed() < common::DEADLINE, "no layout");
    }
    // Nothing to wait for: the stub would run right after Laura prints the URL.
    thread::sleep(Duration::from_millis(500));
    assert!(
        !dir.path().join("opened").exists(),
        "Laura opened a browser"
    );
    Ok(())
}

/// Kills the stub browser whose pid is in `0`, also when an assert fails.
#[cfg(unix)]
struct KillPid(std::path::PathBuf);

#[cfg(unix)]
impl Drop for KillPid {
    fn drop(&mut self) {
        if let Ok(pid) = std::fs::read_to_string(&self.0) {
            let _ = StdCommand::new("kill").args(["-9", pid.trim()]).status();
        }
    }
}

/// Laura in a terminal: the process, the terminal and the terminal's output.
#[cfg(unix)]
type Terminal = (
    Box<dyn portable_pty::Child + Send + Sync>,
    Box<dyn portable_pty::MasterPty + Send>,
    mpsc::Receiver<Vec<u8>>,
);

/// `laura -- /bin/sh` in a terminal in `dir`, with `LAURA_BROWSER` = `stub`. See `laura_cmd`.
#[cfg(unix)]
fn laura_in_a_terminal(dir: &Path, stub: &str) -> Result<Terminal> {
    in_a_terminal(laura_cmd(dir, stub)?)
}

/// `laura -- /bin/sh` in `dir`, with `LAURA_BROWSER` = `stub`, as a first launch: `HOME` and
/// `TMPDIR` are `dir` and no runtime env is set, so the per-OS runtime folder, where Laura writes
/// the page that it opens the browser at, doesn't exist yet.
#[cfg(unix)]
fn laura_cmd(dir: &Path, stub: &str) -> Result<portable_pty::CommandBuilder> {
    use std::os::unix::fs::PermissionsExt;

    isolate_home();
    let browser = dir.join("browser.sh");
    std::fs::write(&browser, format!("#!/bin/sh\n{stub}"))?;
    std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o755))?;
    let mut cmd = portable_pty::CommandBuilder::new(env!("CARGO_BIN_EXE_laura"));
    cmd.args(["--", "/bin/sh"]);
    cmd.cwd(dir);
    cmd.env("HOME", dir);
    cmd.env("TMPDIR", dir);
    cmd.env("LAURA_BROWSER", &browser);
    cmd.env_remove("LAURA_RUNTIME");
    cmd.env_remove("XDG_RUNTIME_DIR");
    cmd.env_remove("LAURA_TAB");
    Ok(cmd)
}

#[cfg(unix)]
fn in_a_terminal(cmd: portable_pty::CommandBuilder) -> Result<Terminal> {
    let pty = portable_pty::native_pty_system().openpty(portable_pty::PtySize {
        rows: 24,
        cols: 200,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let laura = pty.slave.spawn_command(cmd)?;
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader()?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = reader.read(&mut buf) {
            let _ = tx.send(buf[..n].to_vec());
        }
    });
    Ok((laura, pty.master, rx))
}

#[cfg(unix)]
#[test]
fn plain_laura_in_a_terminal_opens_the_browser_and_keeps_it_on_ctrl_c() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    // The traps go in before `args`, so the test sends Ctrl+C only once they catch it.
    let (mut laura, master, rx) = laura_in_a_terminal(
        dir.path(),
        "d=\"$(dirname \"$0\")\"\necho $$ > \"$d/pid\"\n\
         trap 'echo INT >> \"$d/signals\"' INT\ntrap 'echo HUP >> \"$d/signals\"' HUP\n\
         echo \"$@\" > \"$d/args.tmp\" && mv \"$d/args.tmp\" \"$d/args\"\n\
         while :; do sleep 1; done\n",
    )?;
    let _stub = KillPid(dir.path().join("pid"));
    let mut out = String::new();
    let start = Instant::now();
    let url = loop {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        if let Some(line) = out.split('\n').find(|l| l.contains("/?t="))
            && line.ends_with('\r')
        {
            break line.trim().to_string();
        }
        assert!(start.elapsed() < common::DEADLINE, "no URL: {out:?}");
    };
    let token = port_token(&url).1;
    let args = dir.path().join("args");
    while !args.exists() {
        assert!(
            start.elapsed() < common::DEADLINE,
            "Laura opened no browser"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let args = std::fs::read_to_string(&args)?;
    let page = std::path::PathBuf::from(
        args.trim()
            .strip_prefix("file://")
            .unwrap_or_else(|| panic!("not a file page: {args}")),
    );
    assert!(page.starts_with(dir.path()), "{page:?}");
    assert!(
        page.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("open-")),
        "{page:?}"
    );
    assert!(
        !args.contains(&token),
        "the token is on the command line: {args}"
    );
    assert!(std::fs::read_to_string(&page)?.contains(&url));
    assert_eq!(
        std::fs::metadata(&page)?.permissions().mode() & 0o777,
        0o600,
        "other users can read the page"
    );
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    // Nothing to wait for: a signal would reach the stub right away.
    thread::sleep(Duration::from_millis(500));
    assert!(
        !dir.path().join("signals").exists(),
        "the browser got a signal from the terminal"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_browser_that_fails_shows_why() -> Result<()> {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let (mut laura, master, rx) = laura_in_a_terminal(dir.path(), "exit 3\n")?;
    let mut out = String::new();
    let start = Instant::now();
    while !out.contains("exited with") {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(start.elapsed() < common::DEADLINE, "no message: {out:?}");
    }
    assert!(
        out.contains("Laura could not open a web browser:"),
        "{out:?}"
    );
    while !out.contains("laura --tui") {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(start.elapsed() < common::DEADLINE, "no TUI hint: {out:?}");
    }
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn a_snap_browser_reads_the_page_from_its_own_folder() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    // `/snap/bin/chromium` links to `/usr/bin/snap`, which runs the snap that the link names.
    let snap = dir.path().join("usr/bin/snap");
    std::fs::create_dir_all(snap.parent().expect("a folder"))?;
    std::fs::write(
        &snap,
        "#!/bin/sh\nd=\"$(dirname \"$0\")\"\n\
         echo \"$@\" > \"$d/args.tmp\" && mv \"$d/args.tmp\" \"$d/args\"\n",
    )?;
    std::fs::set_permissions(&snap, std::fs::Permissions::from_mode(0o755))?;
    let chromium = dir.path().join("bin/chromium");
    std::fs::create_dir_all(chromium.parent().expect("a folder"))?;
    std::os::unix::fs::symlink(&snap, &chromium)?;
    let mut cmd = laura_cmd(dir.path(), "")?;
    cmd.env("LAURA_BROWSER", &chromium);
    let (mut laura, master, rx) = in_a_terminal(cmd)?;
    let args = dir.path().join("bin/args");
    let start = Instant::now();
    let mut out = String::new();
    while !args.exists() {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(50)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "Laura opened no browser: {out:?}"
        );
    }
    let args = std::fs::read_to_string(&args)?;
    let page = std::path::PathBuf::from(
        args.trim()
            .strip_prefix("file://")
            .unwrap_or_else(|| panic!("not a file page: {args}")),
    );
    assert_eq!(
        page.parent(),
        Some(dir.path().join("snap/chromium/common").as_path()),
        "{page:?}"
    );
    assert!(
        page.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("open-")),
        "{page:?}"
    );
    assert!(std::fs::read_to_string(&page)?.contains("/?t="));
    assert_eq!(
        std::fs::metadata(&page)?.permissions().mode() & 0o777,
        0o600,
        "other users can read the page"
    );
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn laura_opens_the_default_browser_without_laura_browser() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin)?;
    let opener = bin.join(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    });
    std::fs::write(
        &opener,
        "#!/bin/sh\nd=\"$(dirname \"$0\")\"\n\
         echo \"$@\" > \"$d/args.tmp\" && mv \"$d/args.tmp\" \"$d/args\"\n",
    )?;
    std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755))?;
    let mut cmd = laura_cmd(dir.path(), "")?;
    cmd.env_remove("LAURA_BROWSER");
    cmd.env_remove("WSL_DISTRO_NAME");
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![bin.clone()];
    dirs.extend(std::env::split_paths(&path));
    cmd.env("PATH", std::env::join_paths(dirs)?);
    let (mut laura, master, rx) = in_a_terminal(cmd)?;
    let args = bin.join("args");
    let start = Instant::now();
    let mut out = String::new();
    while !args.exists() {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(50)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "Laura opened no browser: {out:?}"
        );
    }
    let page = std::path::PathBuf::from(std::fs::read_to_string(&args)?.trim());
    assert!(
        page.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("open-") && n.ends_with(".html")),
        "{page:?}"
    );
    let html = std::fs::read_to_string(&page)?;
    assert!(html.contains("/?t="), "{html}");
    assert!(
        !page.to_string_lossy().contains("t="),
        "the token is on the command line: {page:?}"
    );
    assert_eq!(
        std::fs::metadata(&page)?.permissions().mode() & 0o777,
        0o600,
        "other users can read the page"
    );
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_browser_that_fails_later_shows_nothing() -> Result<()> {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    // A Chrome that opened the window, and then exited with an error when the user closed it.
    let (mut laura, master, rx) = laura_in_a_terminal(dir.path(), "sleep 6; exit 3\n")?;
    let mut out = String::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(7) {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
    }
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    assert!(!out.contains("could not open a web browser"), "{out:?}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_missing_browser_names_the_program_and_the_tui() -> Result<()> {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin)?;
    // No LAURA_BROWSER and an empty PATH: the OS opener is missing.
    let mut cmd = laura_cmd(dir.path(), "")?;
    cmd.env_remove("LAURA_BROWSER");
    cmd.env_remove("WSL_DISTRO_NAME");
    cmd.env("PATH", &bin);
    let (mut laura, master, rx) = in_a_terminal(cmd)?;
    let opener = if cfg!(target_os = "macos") {
        "open: "
    } else {
        "xdg-open: "
    };
    let mut out = String::new();
    let start = Instant::now();
    while !(out.contains(opener) && out.contains("laura --tui")) {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(start.elapsed() < common::DEADLINE, "no message: {out:?}");
    }
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[test]
fn the_open_page_redirects_to_the_workspace_until_a_view_connects() -> Result<()> {
    let mut rig = Rig::new()?;
    let dir = tempfile::tempdir()?;
    let page = rig.web.open_page(dir.path())?;
    assert!(
        std::fs::read_to_string(&page)?.contains(rig.web.url()),
        "the page doesn't redirect to the workspace"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&page)?.permissions().mode() & 0o777,
            0o600,
            "other users can read the page"
        );
    }
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    assert!(!page.exists(), "the page outlived the view's connection");
    Ok(())
}

#[test]
fn a_relative_link_opens_its_file() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[b](sub/b.md#second-part)\n")?;
    std::fs::create_dir_all(dir.path().join("docs/sub"))?;
    std::fs::write(
        dir.path().join("docs/sub/b.md"),
        "# B\n\ntext\n\n## Second part\n\n![s](../../img/s.png)\n",
    )?;
    std::fs::write(dir.path().join("c.md"), "# C\n")?;
    let ready: Value = serde_json::from_str(&rig.drive(&["ready"]))?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "sub/b.md#second-part"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&2].cursor_line(), 5);
    let journal = std::fs::read_to_string(ready["journal"].as_str().unwrap_or(""))?;
    assert!(
        journal
            .lines()
            .any(|l| l.contains(r#""type":"open""#) && l.contains("b.md")),
        "{journal}"
    );
    let (html, src) = img_src(&mut rig, &mut ws, 2);
    // As the browser resolves the `..`s.
    let resp = get(rig.port, &src.replace("/docs/sub/../../", "/"))?;
    assert!(resp.starts_with("HTTP/1.1 200"), "{html}\n{resp}");
    // The same link again, with a query, moves to pane 2 instead of opening another.
    rig.tab.focus = 1;
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "sub/b.md?v=2#second-part"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert!(!rig.tab.panels.contains_key(&3));
    assert_eq!(rig.tab.focus, 2);
    // A `../` link names the file as `laura open` does.
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 2, "href": "../../c.md"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        rig.tab.panels[&3].path,
        std::path::absolute(dir.path().join("docs/sub/../../c.md"))?.to_string_lossy()
    );
    Ok(())
}
