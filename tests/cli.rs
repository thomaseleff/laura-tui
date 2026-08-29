//! The `open`/`close`/`ready` verbs cross the real seam (binary → tab socket → `Tab::drain`) and land the right state; `--help`/`--version` work. State, not pixels.

use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

use anyhow::Result;
use assert_cmd::Command;
use laura::{Message, Tab};

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

/// Run `laura <args>` against a raw socket and return the one decoded message.
fn send_and_recv(args: &[&str]) -> Result<Message> {
    let name = format!("laura-test-{}-{}.sock", std::process::id(), args.join("-"));
    let rx = laura::protocol::serve(&name)?;
    Command::cargo_bin("laura")?
        .args(args)
        .env("LAURA_TAB", &name)
        .assert()
        .success();
    Ok(rx.recv_timeout(Duration::from_secs(5))?)
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

    Command::cargo_bin("laura")?
        .args(["open", path])
        .env("LAURA_TAB", &tab.name)
        .assert()
        .success();
    sleep(Duration::from_millis(200));
    tab.drain();
    assert!(tab.panel.is_some(), "open should have set a panel");

    Command::cargo_bin("laura")?
        .arg("close")
        .env("LAURA_TAB", &tab.name)
        .assert()
        .success();
    sleep(Duration::from_millis(200));
    tab.drain();
    assert!(tab.panel.is_none(), "close should clear the panel");
    Ok(())
}

#[test]
fn ready_marks_the_tab_as_agent() -> Result<()> {
    let mut tab = spawn_tab()?;
    assert!(!tab.agent, "tabs start non-agent (fail closed)");

    Command::cargo_bin("laura")?
        .arg("ready")
        .env("LAURA_TAB", &tab.name)
        .assert()
        .success();
    sleep(Duration::from_millis(200));
    tab.drain();
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
