//! Two tabs run independent shells; a `laura open` in tab A never appears in tab B. Through the `laura open` binary → tab socket → `drain()`. State, not pixels.

use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Rect, Tab};

/// Spawn a tab hosting a trivial, quick-exiting command (we only need its socket).
fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a real tab, draining (and replying) until the client exits.
fn drive_tab(tab: &mut Tab, args: &[&str]) {
    let name = tab.name.clone();
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
fn open_in_one_tab_does_not_reach_the_other() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "# spec\nknown content\n")?;
    let path = file.path().to_str().unwrap().to_string();

    let mut a = spawn_tab()?;
    let mut b = spawn_tab()?;

    drive_tab(&mut a, &["open", &path]);
    b.drain(area());

    let panel = a
        .panels
        .values()
        .next()
        .expect("tab A should have opened a panel");
    assert_eq!(panel.content, "# spec\nknown content\n");
    assert!(b.panels.is_empty(), "tab B must not see tab A's open");
    Ok(())
}

#[test]
fn tabs_get_distinct_socket_names() -> Result<()> {
    let a = spawn_tab()?;
    let b = spawn_tab()?;
    assert_ne!(a.name, b.name);
    Ok(())
}
