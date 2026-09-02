//! `ready --session` names a journal; a later `open` tees an event into that session's NDJSON.
//! Drives the real seam (binary → socket → `Tab::drain`) with `LAURA_DATA_DIR` pointed at a tempdir.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Rect, Tab};

fn drive_tab(tab: &mut Tab, args: &[&str]) {
    let name = tab.socket.clone();
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let ok = Command::cargo_bin("laura")
            .unwrap()
            .args(&a)
            .env("LAURA_TAB", &name)
            .output()
            .unwrap()
            .status
            .success();
        tx.send(ok).unwrap();
    });
    let start = Instant::now();
    let ok = loop {
        tab.drain(Rect::new(0, 0, 120, 40));
        if let Ok(ok) = rx.try_recv() {
            break ok;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    assert!(ok, "laura {args:?} exited non-zero");
}

#[test]
fn ready_names_a_session_and_open_is_journaled() -> Result<()> {
    let dir = tempfile::tempdir()?;
    // SAFETY: single test in this binary; nothing else reads the env concurrently.
    unsafe { std::env::set_var("LAURA_DATA_DIR", dir.path()) };

    let file = tempfile::NamedTempFile::new()?;
    let path = file.path().to_str().unwrap();

    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    let mut tab = Tab::spawn(cmd, 24, 80)?;

    drive_tab(&mut tab, &["ready", "--session", "audit-test"]);
    drive_tab(&mut tab, &["open", path]);

    let ndjson = dir
        .path()
        .join("laura")
        .join("sessions")
        .join("audit-test.ndjson");
    let body = std::fs::read_to_string(&ndjson)?;
    assert!(
        body.contains(r#""type":"ready""#),
        "ready event missing: {body}"
    );
    assert!(
        body.contains(r#""type":"open""#),
        "open event missing: {body}"
    );
    assert!(
        body.contains(r#""session":"audit-test""#),
        "session id not stamped: {body}"
    );
    // #11: every event carries a build version; commit hash may be appended (X.Y.Z+abc1234)
    // off a checkout, so assert the CARGO_PKG_VERSION prefix rather than the full string.
    assert!(
        body.contains(&format!(r#""version":"{}"#, env!("CARGO_PKG_VERSION"))),
        "version not stamped: {body}"
    );
    Ok(())
}
