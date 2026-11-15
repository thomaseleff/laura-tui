//! A second `ready` with a new session sends later events to the new session's journal, and
//! `--title` labels the workspace. Own binary: the test overrides `HOME`.

mod common;

use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Rect, Tab};
use serde_json::Value;

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
        assert!(start.elapsed() < common::DEADLINE, "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    assert!(ok, "laura {args:?} exited non-zero");
}

fn events(home: &Path, session: &str, kind: &str) -> Result<Vec<Value>> {
    let path = home
        .join(".laura")
        .join("sessions")
        .join(format!("{session}.ndjson"));
    let mut out = Vec::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let v: Value = serde_json::from_str(line)?;
        if v["type"] == kind {
            out.push(v);
        }
    }
    Ok(out)
}

#[test]
fn a_second_ready_switches_the_session_and_keeps_the_title() -> Result<()> {
    let dir = tempfile::tempdir()?;
    // SAFETY: single test in this binary; nothing else reads the env concurrently.
    unsafe {
        std::env::set_var("HOME", dir.path());
        std::env::set_var("USERPROFILE", dir.path());
    }

    let f1 = tempfile::NamedTempFile::new()?;
    let f2 = tempfile::NamedTempFile::new()?;
    let (p1, p2) = (f1.path().to_str().unwrap(), f2.path().to_str().unwrap());

    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    let mut tab = Tab::spawn(cmd, 24, 80)?;

    drive_tab(&mut tab, &["ready", "--session", "a", "--title", "one"]);
    drive_tab(&mut tab, &["open", p1]);
    assert_eq!(tab.title.as_deref(), Some("one"));

    // A new chat without --title keeps the workspace's title.
    drive_tab(&mut tab, &["ready", "--session", "b"]);
    drive_tab(&mut tab, &["open", p2]);
    assert_eq!(tab.title.as_deref(), Some("one"));

    drive_tab(&mut tab, &["ready", "--session", "b", "--title", "two"]);
    assert_eq!(tab.title.as_deref(), Some("two"));

    let home = dir.path();
    let opens: Vec<_> = events(home, "a", "open")?
        .iter()
        .map(|e| e["path"].clone())
        .collect();
    assert_eq!(opens, [Value::from(p1)], "session a opens");
    let opens: Vec<_> = events(home, "b", "open")?
        .iter()
        .map(|e| e["path"].clone())
        .collect();
    assert_eq!(opens, [Value::from(p2)], "session b opens");

    let ready = &events(home, "a", "ready")?[0];
    assert!(ready["pid"].is_u64(), "ready has no pid: {ready}");
    assert_eq!(ready["title"], "one");
    let readies = events(home, "b", "ready")?;
    assert_eq!(readies[0]["title"], Value::Null);
    assert_eq!(readies.last().unwrap()["title"], "two");
    Ok(())
}
