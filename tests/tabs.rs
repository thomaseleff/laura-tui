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
    assert_ne!(a.socket, b.socket);
    Ok(())
}

/// #8: the minted name embeds the per-process nonce, so equal `pid+counter` across two
/// processes (PID reuse) can't collide. Cross-process reuse itself isn't CI-reproducible;
/// we assert the format invariant that makes it impossible.
#[test]
fn socket_name_embeds_process_nonce() -> Result<()> {
    let a = spawn_tab()?;
    let seg = format!("-{:x}-", laura::tab::process_nonce());
    assert!(
        a.socket.contains(&seg),
        "socket {} must embed nonce segment {seg}",
        a.socket
    );
    Ok(())
}

/// #8: a stale `LAURA_TAB` (a name this process never served — e.g. inherited across PID reuse)
/// fails to connect rather than silently routing into a live tab.
#[test]
fn stale_address_errors_rather_than_misroutes() {
    let bogus = format!("laura-{}-deadbeef-0.sock", std::process::id());
    let r = laura::protocol::request(&bogus, &laura::Message::Layout);
    assert!(r.is_err(), "connecting to an unserved name must Err");
}
