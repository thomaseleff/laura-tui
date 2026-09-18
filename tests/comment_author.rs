//! An omitted `laura comment --author` defaults to the session's `ready --agent` name; an
//! explicit `--author` (the sub-agent case) overrides it. Drives the real seam
//! (binary → socket → `Tab::drain`) and reads the recorded author back off the panel.
//! Own test binary so the `LAURA_DATA_DIR` env set below isn't raced by a sibling test.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::protocol::{self, Message, Response};
use laura::{Author, Rect, Tab};

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

#[test]
fn comment_author_defaults_to_session_agent() -> Result<()> {
    let dir = tempfile::tempdir()?;
    // SAFETY: single test in this binary; nothing else reads the env concurrently.
    unsafe { std::env::set_var("LAURA_DATA_DIR", dir.path()) };

    let file = tempfile::NamedTempFile::new()?;
    std::fs::write(file.path(), "line one\nline two\n")?;
    let path = file.path().to_str().unwrap();

    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    let mut tab = Tab::spawn(cmd, 24, 80)?;

    drive_tab(
        &mut tab,
        &["ready", "--session", "author-test", "--agent", "claude"],
    );
    drive_tab(&mut tab, &["open", path]);
    // No --author: inherits the session agent.
    drive_tab(&mut tab, &["comment", "1", "inherited", "--pane", "1"]);
    // Explicit --author: the sub-agent case, does not inherit.
    drive_tab(
        &mut tab,
        &[
            "comment",
            "2",
            "explicit",
            "--author",
            "sub-agent",
            "--pane",
            "1",
        ],
    );

    // `drive_tab` only pumps `Tab::drain`, not the full run loop, so `open`'s pending focus
    // is never consumed — read pane 1 directly rather than via `focused_panel`.
    let panel = tab.panels.get(&1).expect("pane 1 opened");
    // Look threads up by note body — the row↔line mapping is internal.
    let author_of = |body: &str| {
        panel
            .threads
            .iter()
            .find(|t| t.root.body == body)
            .unwrap_or_else(|| panic!("no thread with note {body:?}"))
            .root
            .author
            .clone()
    };

    assert_eq!(
        author_of("inherited"),
        Author::Agent("claude".into()),
        "omitted --author should inherit the session agent"
    );
    assert_eq!(
        author_of("explicit"),
        Author::Agent("sub-agent".into()),
        "explicit --author should override the session agent"
    );
    Ok(())
}

/// Send one raw `Message` over the tab's socket while pumping `drain`, and return the response.
/// The CLI can't send a bodyless comment (clap requires `<body>`), so drive the protocol directly.
fn request_tab(tab: &mut Tab, msg: Message) -> Response {
    let name = tab.socket.clone();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        tx.send(protocol::request(&name, &msg).unwrap()).unwrap();
    });
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

#[test]
fn bodyless_comment_errors_and_places_no_thread() -> Result<()> {
    let dir = tempfile::tempdir()?;
    // SAFETY: single test in this binary; nothing else reads the env concurrently.
    unsafe { std::env::set_var("LAURA_DATA_DIR", dir.path()) };

    let file = tempfile::NamedTempFile::new()?;
    std::fs::write(file.path(), "line one\nline two\n")?;
    let path = file.path().to_str().unwrap();

    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    let mut tab = Tab::spawn(cmd, 24, 80)?;
    drive_tab(&mut tab, &["open", path]);

    // A whitespace-only body is empty too: the tab must answer Error and place no thread.
    let resp = request_tab(
        &mut tab,
        Message::Comment {
            pane: Some(1),
            line: 1,
            body: Some("   ".into()),
            author: None,
        },
    );
    assert!(
        matches!(resp, Response::Error { .. }),
        "bodyless comment must return an error, got {resp:?}"
    );
    assert_eq!(
        tab.panels.get(&1).expect("pane 1").thread_count(),
        0,
        "a rejected comment places no thread"
    );
    Ok(())
}

#[test]
fn comment_past_eof_exits_nonzero_and_places_no_thread() -> Result<()> {
    let dir = tempfile::tempdir()?;
    // SAFETY: single test in this binary; nothing else reads the env concurrently.
    unsafe { std::env::set_var("LAURA_DATA_DIR", dir.path()) };

    let file = tempfile::NamedTempFile::new()?;
    std::fs::write(file.path(), "line one\nline two\n")?;
    let path = file.path().to_str().unwrap();

    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    let mut tab = Tab::spawn(cmd, 24, 80)?;

    drive_tab(&mut tab, &["open", path]);
    // Line 999 is well past EOF: the client must exit non-zero and no thread is placed.
    let ok = run_tab(&mut tab, &["comment", "999", "x", "--pane", "1"]);
    assert!(!ok, "out-of-range comment should exit non-zero");

    let panel = tab.panels.get(&1).expect("pane 1 opened");
    assert_eq!(
        panel.thread_count(),
        0,
        "out-of-range comment must place no thread"
    );
    Ok(())
}
