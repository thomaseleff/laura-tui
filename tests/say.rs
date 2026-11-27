//! `laura say` journals the agent's words, from an argument or piped stdin, and the workspace holds
//! the latest say, which the TUI shows as a notice. `laura comment` journals its body. A body that
//! starts with `-` is the body, not a flag.
//! Drives the real seam (binary → socket → `Tab::drain`) with `HOME` pointed at a tempdir.

mod common;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Rect, Tab};
use serde_json::Value;

/// Run `laura args` against `tab` with `stdin`, draining the tab until it exits. Returns its
/// success and stderr.
fn drive(tab: &mut Tab, args: &[&str], stdin: &str) -> (bool, String) {
    let name = tab.socket.clone();
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let stdin = stdin.to_string();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let out = Command::cargo_bin("laura")
            .unwrap()
            .args(&a)
            .env("LAURA_TAB", &name)
            .write_stdin(stdin)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        tx.send((out.status.success(), err)).unwrap();
    });
    let start = Instant::now();
    let out = loop {
        tab.drain(Rect::new(0, 0, 120, 40));
        if let Ok(out) = rx.try_recv() {
            break out;
        }
        assert!(start.elapsed() < common::DEADLINE, "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    out
}

fn events(home: &std::path::Path, session: &str, kind: &str) -> Vec<Value> {
    let path = home
        .join(".laura/sessions")
        .join(format!("{session}.ndjson"));
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|e| e["type"] == kind)
        .collect()
}

#[test]
fn say_journals_the_agents_words() -> Result<()> {
    let dir = tempfile::tempdir()?;
    // SAFETY: single test in this binary; nothing else reads the env concurrently.
    unsafe {
        std::env::set_var("HOME", dir.path());
        std::env::set_var("USERPROFILE", dir.path());
    }
    let file = tempfile::NamedTempFile::new()?;
    std::fs::write(file.path(), "one\ntwo\n")?;
    let path = file.path().to_str().unwrap();
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    let mut tab = Tab::spawn(cmd, 24, 80)?;

    assert!(drive(&mut tab, &["ready", "--session", "say-test"], "").0);
    assert!(drive(&mut tab, &["say", "hello"], "").0);
    assert!(drive(&mut tab, &["say"], "piped\nlines\n").0);
    let says: Vec<Value> = events(dir.path(), "say-test", "say")
        .into_iter()
        .map(|e| e["body"].clone())
        .collect();
    assert_eq!(says, ["hello", "piped\nlines"], "the says' bodies");
    assert_eq!(
        tab.latest_say(),
        Some((2, "piped\nlines")),
        "the latest say"
    );

    let (ok, err) = drive(&mut tab, &["say", "  "], "");
    assert!(!ok && err.contains("laura say needs a body"), "{err}");

    assert!(drive(&mut tab, &["open", path], "").0);
    assert!(drive(&mut tab, &["comment", "2", "look here", "--pane", "1"], "").0);
    let comment = &events(dir.path(), "say-test", "comment")[0];
    assert_eq!(comment["body"], "look here", "{comment}");
    assert_eq!(comment["line"], 2, "{comment}");

    // A body that starts with `-`, such as a markdown list, is the body, and a flag still parses.
    assert!(drive(&mut tab, &["say", "- one"], "").0);
    assert_eq!(events(dir.path(), "say-test", "say")[2]["body"], "- one");
    assert!(drive(&mut tab, &["comment", "2", "- two", "--pane", "1"], "").0);
    assert!(drive(&mut tab, &["comment", "--pane", "1", "2", "- three"], "").0);
    let bodies: Vec<Value> = events(dir.path(), "say-test", "comment")
        .into_iter()
        .map(|e| e["body"].clone())
        .collect();
    assert_eq!(
        bodies,
        ["look here", "- two", "- three"],
        "the comments' bodies"
    );
    assert!(drive(&mut tab, &["feedback", "--positive", "- four"], "").0);
    let feedback = &events(dir.path(), "say-test", "feedback")[0];
    assert_eq!(feedback["body"], "- four", "{feedback}");
    assert_eq!(feedback["sentiment"], "+", "{feedback}");
    assert!(drive(&mut tab, &["say", "--help"], "").0);

    // A new session starts an empty chat; a re-`ready` of the same one keeps it.
    assert!(
        drive(
            &mut tab,
            &["ready", "--session", "say-test", "--title", "x"],
            ""
        )
        .0
    );
    assert!(tab.latest_say().is_some(), "the same session kept its say");
    assert!(drive(&mut tab, &["ready", "--session", "other"], "").0);
    assert_eq!(tab.latest_say(), None, "a new session's chat is empty");
    Ok(())
}
