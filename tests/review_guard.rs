//! Agent-verb guards that protect the user's in-progress review: a verb that would discard or
//! corrupt unsubmitted comment threads is refused and changes nothing; opening a file that's
//! already on screen warns and points at the existing pane. Drives the real seam
//! (binary → socket → `Tab::drain`) and reads pane state back off the tab.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::protocol::{self, Message, Response};
use laura::{Rect, Tab};

fn run_tab(tab: &mut Tab, args: &[&str]) -> bool {
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
    ok
}

fn drive_tab(tab: &mut Tab, args: &[&str]) {
    assert!(run_tab(tab, args), "laura {args:?} exited non-zero");
}

/// Send one raw `Message` over the tab's socket, wait `settle` before the first `drain` (so the
/// request is queued before the tab catches up to disk), then pump `drain` until it's answered.
fn request_tab(tab: &mut Tab, msg: Message, settle: Duration) -> Response {
    let name = tab.socket.clone();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        tx.send(protocol::request(&name, &msg).unwrap()).unwrap();
    });
    thread::sleep(settle);
    let start = Instant::now();
    let resp = loop {
        tab.drain(Rect::new(0, 0, 120, 40));
        if let Ok(r) = rx.try_recv() {
            break r;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    resp
}

/// Open a comment on pane 1, then change the file under it so the pane freezes. The rewrite
/// changes byte length so the stat signature moves even within one mtime tick.
fn freeze(tab: &mut Tab, path: &str) -> Result<()> {
    drive_tab(tab, &["comment", "1", "x", "--pane", "1"]);
    let grown = format!(
        "{}CHANGED
",
        std::fs::read_to_string(path)?
    );
    std::fs::write(path, grown)?;
    tab.drain(Rect::new(0, 0, 120, 40));
    assert!(tab.panels[&1].source_changed, "pane 1 should be frozen");
    Ok(())
}

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

#[test]
fn replace_refused_while_threads_open() -> Result<()> {
    let a = tempfile::NamedTempFile::new()?;
    let b = tempfile::NamedTempFile::new()?;
    std::fs::write(a.path(), "alpha\n")?;
    std::fs::write(b.path(), "beta\n")?;
    let (a, b) = (a.path().to_str().unwrap(), b.path().to_str().unwrap());

    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", a]);
    drive_tab(&mut tab, &["comment", "1", "x", "--pane", "1"]);

    assert!(
        !run_tab(&mut tab, &["open", b, "--panel", "1"]),
        "replacing a pane under review should exit non-zero"
    );
    let panel = tab.panels.get(&1).expect("pane 1");
    assert_eq!(panel.path, a, "refused replace must keep the old file");
    assert_eq!(panel.thread_count(), 1, "refused replace must keep threads");
    Ok(())
}

#[test]
fn close_refused_while_threads_open() -> Result<()> {
    let a = tempfile::NamedTempFile::new()?;
    let b = tempfile::NamedTempFile::new()?;
    std::fs::write(a.path(), "alpha\n")?;
    std::fs::write(b.path(), "beta\n")?;
    let (a, b) = (a.path().to_str().unwrap(), b.path().to_str().unwrap());

    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", a]);
    drive_tab(&mut tab, &["open", b]);
    drive_tab(&mut tab, &["comment", "1", "x", "--pane", "1"]);

    assert!(
        !run_tab(&mut tab, &["close", "1"]),
        "closing a pane under review should exit non-zero"
    );
    assert!(
        !run_tab(&mut tab, &["close", "--all"]),
        "close --all with a pane under review should exit non-zero"
    );
    assert_eq!(
        tab.panels.get(&1).expect("pane 1 survives").thread_count(),
        1
    );
    assert!(
        tab.panels.contains_key(&2),
        "close --all is all-or-nothing: pane 2 survives"
    );

    // A pane without threads still closes.
    drive_tab(&mut tab, &["close", "2"]);
    assert!(!tab.panels.contains_key(&2), "pane 2 should be closed");
    Ok(())
}

#[test]
fn highlight_refused_on_frozen_pane() -> Result<()> {
    let f = tempfile::NamedTempFile::new()?;
    std::fs::write(
        f.path(),
        "one
two
three
",
    )?;
    let path = f.path().to_str().unwrap();

    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", path]);
    drive_tab(&mut tab, &["highlight", "1", "--pane", "1"]);
    freeze(&mut tab, path)?;
    let before = tab.panels[&1].highlight;

    assert!(
        !run_tab(&mut tab, &["highlight", "2", "--pane", "1"]),
        "highlight on a frozen pane should exit non-zero"
    );
    assert_eq!(
        tab.panels[&1].highlight, before,
        "refused highlight changes nothing"
    );

    drive_tab(&mut tab, &["highlight", "--off", "--pane", "1"]);
    assert_eq!(tab.panels[&1].highlight, None, "--off still clears");
    Ok(())
}

#[test]
fn message_after_edit_sees_new_content() -> Result<()> {
    let f = tempfile::NamedTempFile::new()?;
    std::fs::write(
        f.path(),
        "one
",
    )?;
    let path = f.path().to_str().unwrap();

    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", path]);
    let ten: String = (1..=10)
        .map(|i| {
            format!(
                "line {i}
"
            )
        })
        .collect();
    std::fs::write(path, ten)?;

    // ponytail: sleep-based ordering — can't observe the rx queue from outside; a tab-side
    // "queued" hook would make it exact.
    let resp = request_tab(
        &mut tab,
        Message::Highlight {
            pane: Some(1),
            range: Some((8, 8)),
        },
        Duration::from_millis(100),
    );
    assert!(matches!(resp, Response::Ok), "highlight failed: {resp:?}");
    assert_eq!(
        tab.panels[&1].highlight,
        Some((7, 7)),
        "highlight resolves against the edited file, not the stale snapshot"
    );
    Ok(())
}

#[test]
fn open_warns_when_file_already_open() -> Result<()> {
    let f = tempfile::NamedTempFile::new()?;
    std::fs::write(f.path(), "alpha\n")?;
    let path = f.path().to_str().unwrap();

    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", path]);

    // A different spelling of the same file: the raw message skips the CLI's `absolute`.
    let dir = f.path().parent().unwrap().to_str().unwrap();
    let name = f.path().file_name().unwrap().to_str().unwrap();
    let alias = format!("{dir}/./{name}");
    let msg: Message = serde_json::from_value(serde_json::json!({"type": "open", "path": alias}))?;
    match request_tab(&mut tab, msg, Duration::ZERO) {
        Response::Opened { warnings, .. } => {
            assert!(
                warnings
                    .iter()
                    .any(|w| w.contains("already open in pane #1")),
                "{warnings:?}"
            );
        }
        other => panic!("expected opened, got {other:?}"),
    }
    Ok(())
}

#[test]
fn replace_warns_when_file_already_open() -> Result<()> {
    let a = tempfile::NamedTempFile::new()?;
    let b = tempfile::NamedTempFile::new()?;
    std::fs::write(a.path(), "alpha\n")?;
    std::fs::write(b.path(), "beta\n")?;
    let (a, b) = (a.path().to_str().unwrap(), b.path().to_str().unwrap());

    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", a]);
    drive_tab(&mut tab, &["open", b]);

    let mut replace = |panel: u32| -> Result<Vec<String>> {
        let msg: Message =
            serde_json::from_value(serde_json::json!({"type": "open", "path": a, "panel": panel}))?;
        match request_tab(&mut tab, msg, Duration::ZERO) {
            Response::Opened { warnings, .. } => Ok(warnings),
            other => panic!("expected opened, got {other:?}"),
        }
    };
    // Re-opening a pane's own file into it isn't a duplicate.
    let warnings = replace(1)?;
    assert!(
        !warnings.iter().any(|w| w.contains("already open")),
        "{warnings:?}"
    );
    let warnings = replace(2)?;
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("already open in pane #1")),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn agent_request_waits_for_the_users_draft() -> Result<()> {
    let f = tempfile::NamedTempFile::new()?;
    std::fs::write(f.path(), "alpha\n")?;
    let path = f.path().to_str().unwrap();

    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", path]);
    // What the TUI sets while the user types a comment on the focused pane.
    tab.focus = 1;
    tab.hold = true;

    let name = tab.socket.clone();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let msg = Message::Close {
            pane: Some(1),
            all: false,
        };
        tx.send(protocol::request(&name, &msg).unwrap()).unwrap();
    });
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(200) {
        tab.drain(Rect::new(0, 0, 120, 40));
        thread::sleep(Duration::from_millis(5));
    }
    assert!(tab.panels.contains_key(&1), "held close must not apply");
    assert!(rx.try_recv().is_err(), "held request must not be answered");

    // The user presses Enter: the comment commits, then the hold lifts.
    let p = tab.panels.get_mut(&1).expect("pane 1");
    p.begin_comment();
    p.author_note("mine".into());
    tab.hold = false;
    let resp = loop {
        tab.drain(Rect::new(0, 0, 120, 40));
        if let Ok(r) = rx.try_recv() {
            break r;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    match resp {
        Response::Error { message } => {
            assert!(message.contains("unsubmitted inline review"), "{message}")
        }
        other => panic!("expected error, got {other:?}"),
    }
    assert_eq!(tab.panels[&1].thread_count(), 1, "pane 1 keeps its thread");
    Ok(())
}

#[test]
fn duplicate_open_of_a_pane_under_review_gives_no_reuse_advice() -> Result<()> {
    let f = tempfile::NamedTempFile::new()?;
    std::fs::write(f.path(), "alpha\n")?;
    let path = f.path().to_str().unwrap();

    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", path]);
    drive_tab(&mut tab, &["comment", "1", "x", "--pane", "1"]);

    let msg: Message = serde_json::from_value(serde_json::json!({"type": "open", "path": path}))?;
    match request_tab(&mut tab, msg, Duration::ZERO) {
        Response::Opened { warnings, .. } => {
            let w = warnings
                .iter()
                .find(|w| w.contains("already open in pane #1"))
                .unwrap_or_else(|| panic!("{warnings:?}"));
            assert!(w.contains("unsubmitted inline review"), "{w}");
            assert!(!w.contains("highlight --pane"), "{w}");
        }
        other => panic!("expected opened, got {other:?}"),
    }
    Ok(())
}
