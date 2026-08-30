//! The `open`/`close`/`ready` verbs cross the real seam (binary → tab socket → `Tab::drain` → response) and land the right state; `--help`/`--version` work. State, not pixels.

use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Message, Rect, Response, Tab};

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a raw socket and return the one decoded request (reply `Ok`).
fn send_and_recv(args: &[&str]) -> Result<Message> {
    let name = format!("laura-test-{}-{}.sock", std::process::id(), args.join("-"));
    let rx = laura::protocol::serve(&name)?;
    let n = name.clone();
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let h = thread::spawn(move || {
        Command::cargo_bin("laura")
            .unwrap()
            .args(&a)
            .env("LAURA_TAB", &n)
            .assert()
            .success();
    });
    let (msg, reply) = rx.recv_timeout(Duration::from_secs(5))?;
    reply.send(&Response::Ok);
    h.join().unwrap();
    Ok(msg)
}

/// Run `laura <args>` against a real tab, draining (and replying) until the client exits.
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
        tab.drain(area());
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
fn open_focuses_by_default_and_no_focus_opts_out() -> Result<()> {
    let file = tempfile::NamedTempFile::new()?;
    let path = file.path().to_str().unwrap();

    let Message::Open { focus, .. } = send_and_recv(&["open", path])? else {
        panic!("expected Open");
    };
    assert!(focus, "open defaults to focus");

    let Message::Open { focus, .. } = send_and_recv(&["open", path, "--no-focus"])? else {
        panic!("expected Open");
    };
    assert!(!focus, "--no-focus opts out");
    Ok(())
}

#[test]
fn close_clears_the_panel() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    writeln!(file, "content")?;
    let path = file.path().to_str().unwrap();

    let mut tab = spawn_tab()?;

    drive_tab(&mut tab, &["open", path]);
    assert!(!tab.panels.is_empty(), "open should have added a panel");
    // The run loop applies pending focus; simulate it so a default `close` targets the panel.
    if let Some(id) = tab.pending_focus.take() {
        tab.focus = id;
    }

    drive_tab(&mut tab, &["close"]);
    assert!(tab.panels.is_empty(), "close should remove the panel");
    Ok(())
}

#[test]
fn ready_marks_the_tab_as_agent() -> Result<()> {
    let mut tab = spawn_tab()?;
    assert!(!tab.agent, "tabs start non-agent (fail closed)");

    drive_tab(&mut tab, &["ready"]);
    assert!(tab.agent, "ready should mark the tab as hosting an agent");
    Ok(())
}

#[test]
fn help_and_version_exit_zero() -> Result<()> {
    Command::cargo_bin("laura")?
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("Usage"));
    Command::cargo_bin("laura")?
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains("laura"));
    Ok(())
}
