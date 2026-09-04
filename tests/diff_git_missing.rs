//! Feat #26: with no `git` on PATH, opening a file warns the agent (stderr) to
//! install git. Own test binary + sole test, so emptying `PATH` for this process
//! (the socket server that spawns git) races nothing. Toast is UI — assert the
//! warning channel, not pixels, per CLAUDE.md.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Rect, Tab};

#[test]
fn git_absent_warns_the_agent() -> Result<()> {
    // Empty PATH: `Command::new("git")` finds nothing (git isn't in System32), so the
    // spawn returns NotFound → NoGit. Sole test in this binary, so no PATH race.
    unsafe {
        std::env::set_var("PATH", "");
    }

    let file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    let path = file.path().to_str().unwrap().to_string();

    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    let mut tab = Tab::spawn(cmd, 24, 80)?;

    let name = tab.socket.clone();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let out = Command::cargo_bin("laura")
            .unwrap()
            .args(["open", &path, "--no-focus"])
            .env("LAURA_TAB", &name)
            .env("PATH", "")
            .output()
            .unwrap();
        tx.send(out).unwrap();
    });
    let area = Rect::new(0, 0, 120, 40);
    let start = Instant::now();
    let out = loop {
        tab.drain(area);
        if let Ok(out) = rx.try_recv() {
            break out;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();

    // The warning rides `Response::Opened.warnings` → the CLI prints it to stderr.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("diff markers unavailable") && stderr.contains("install `git`"),
        "expected git-missing warning on stderr, got: {stderr}"
    );
    Ok(())
}
