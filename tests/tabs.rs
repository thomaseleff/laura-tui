//! Two tabs run independent shells; a `laura open` in tab A never appears in tab B. Through the `laura open` binary → tab socket → `drain()`. State, not pixels.

use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

use anyhow::Result;
use assert_cmd::Command;
use laura::Tab;

/// Spawn a tab hosting a trivial, quick-exiting command (we only need its socket).
fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

#[test]
fn open_in_one_tab_does_not_reach_the_other() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "# spec\nknown content\n")?;
    let path = file.path().to_str().unwrap().to_string();

    let mut a = spawn_tab()?;
    let mut b = spawn_tab()?;

    Command::cargo_bin("laura")?
        .args(["open", &path])
        .env("LAURA_TAB", &a.name)
        .assert()
        .success();

    // Let the server thread accept the connection before draining.
    sleep(Duration::from_millis(200));
    a.drain();
    b.drain();

    let panel = a.panel.expect("tab A should have opened a panel");
    assert_eq!(panel.content, "# spec\nknown content\n");
    assert!(b.panel.is_none(), "tab B must not see tab A's open");
    Ok(())
}

#[test]
fn tabs_get_distinct_socket_names() -> Result<()> {
    let a = spawn_tab()?;
    let b = spawn_tab()?;
    assert_ne!(a.name, b.name);
    Ok(())
}
