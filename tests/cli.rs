//! The `open`/`close`/`ready` verbs cross the real seam (binary → tab socket → `Tab::drain` → response) and land the right state; `--help`/`--version` work. State, not pixels.

use std::io::Write;
use std::process::Output;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Message, Rect, Response, Tab};

/// `ready` journals into `target/tmp/.laura`, not the real `~/.laura`.
fn isolate_home() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: once per binary; std's env lock serializes it with std's own readers (`Command` spawn, `env::var`).
    ONCE.call_once(|| unsafe {
        std::env::set_var("HOME", env!("CARGO_TARGET_TMPDIR"));
        std::env::set_var("USERPROFILE", env!("CARGO_TARGET_TMPDIR"));
    });
}

fn spawn_tab() -> Result<Tab> {
    isolate_home();
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

/// Run `laura <args>` against a real tab, draining (and replying) until the client exits; return its output.
fn drive_tab(tab: &mut Tab, args: &[&str]) -> Output {
    let name = tab.socket.clone();
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let out = Command::cargo_bin("laura")
            .unwrap()
            .args(&a)
            .env("LAURA_TAB", &name)
            .output()
            .unwrap();
        tx.send(out).unwrap();
    });
    let start = Instant::now();
    let out = loop {
        tab.drain(area());
        if let Ok(out) = rx.try_recv() {
            break out;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    assert!(out.status.success(), "laura {args:?} exited non-zero");
    out
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
fn ready_marks_the_tab_and_returns_the_layout() -> Result<()> {
    let file = tempfile::NamedTempFile::new()?;
    let path = file.path().to_str().unwrap();
    let mut tab = spawn_tab()?;
    assert!(!tab.agent, "tabs start non-agent (fail closed)");

    drive_tab(&mut tab, &["open", path]);
    let out = drive_tab(&mut tab, &["ready"]);
    assert!(tab.agent, "ready should mark the tab as hosting an agent");

    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    assert!(v["journal"].as_str().unwrap().ends_with(".ndjson"), "{v}");
    let panes = v["layout"]["panes"].as_array().unwrap();
    assert!(
        panes
            .iter()
            .any(|p| p["id"].as_u64() == Some(1) && p["path"].as_str() == Some(path)),
        "{v}"
    );
    Ok(())
}

#[test]
fn ready_outside_a_workspace_fails() -> Result<()> {
    Command::cargo_bin("laura")?
        .arg("ready")
        .env_remove("LAURA_TAB")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("not inside a Laura tab"));
    Ok(())
}

/// A host from before `layout` (upgraded while running) still answers `ready`.
#[test]
fn ready_against_an_older_host_succeeds() -> Result<()> {
    use interprocess::local_socket::{GenericNamespaced, ListenerOptions, prelude::*};
    use std::io::{BufRead, BufReader};

    let name = format!("laura-test-{}-old-host.sock", std::process::id());
    let listener = ListenerOptions::new()
        .name(name.as_str().to_ns_name::<GenericNamespaced>()?)
        .create_sync()?;
    let host = thread::spawn(move || -> std::io::Result<()> {
        let mut conn = BufReader::new(listener.accept()?);
        conn.read_line(&mut String::new())?;
        conn.get_mut()
            .write_all(b"{\"type\":\"ready\",\"journal\":\"/old/s.ndjson\",\"experimental\":[]}\n")
    });
    let out = Command::cargo_bin("laura")?
        .arg("ready")
        .env("LAURA_TAB", &name)
        .output()?;
    host.join().unwrap()?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    assert_eq!(v["journal"], "/old/s.ndjson");
    assert!(v["layout"].is_null(), "{v}");
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
