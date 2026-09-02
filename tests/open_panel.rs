//! The full chain — real `laura open` → tab socket → decode → panel holds the file's content — without the render loop. State, not pixels.

use std::io::Write;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use assert_cmd::Command;
use laura::{Message, Panel, Response};

#[test]
fn open_renders_file_into_panel() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "# spec\nknown content\n")?;
    let path = file.path().to_str().unwrap().to_string();

    let name = format!("laura-test-{}.sock", std::process::id());
    let rx = laura::protocol::serve(&name)?;

    let n = name.clone();
    let p = path.clone();
    let h = thread::spawn(move || {
        Command::cargo_bin("laura")
            .unwrap()
            .args(["open", &p])
            .env("LAURA_TAB", &n)
            .assert()
            .success();
    });

    let (msg, reply) = rx.recv_timeout(Duration::from_secs(5))?;
    reply.send(&Response::Opened {
        pane: 1,
        warnings: vec![],
    });
    h.join().unwrap();

    let Message::Open { path, .. } = msg else {
        panic!("expected Open message");
    };
    let panel = Panel::open(path);

    assert_eq!(panel.path, file.path().to_str().unwrap());
    assert_eq!(panel.content, "# spec\nknown content\n");
    Ok(())
}

/// A panel over an unreadable path carries the read error as a signal (not just panel text),
/// so `Tab::apply` can warn on it; a readable file leaves it `None`.
#[test]
fn open_records_read_error_for_missing_file() -> Result<()> {
    let missing = std::env::temp_dir().join("laura-no-such-file-xyz.md");
    let panel = Panel::open(missing.to_string_lossy().into_owned());
    let e = panel.read_error.expect("missing file sets read_error");
    assert!(e.contains("cannot read"), "terse error, got: {e}");

    let file = tempfile::NamedTempFile::new()?;
    let panel = Panel::open(file.path().to_str().unwrap().to_string());
    assert!(
        panel.read_error.is_none(),
        "readable file leaves read_error None"
    );
    Ok(())
}

/// A relative path is absolutized against the *caller's* cwd before it hits the socket —
/// so the server never resolves it against its own cwd. State (the decoded message), not pixels.
#[test]
fn open_absolutizes_relative_path_against_caller_cwd() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("spec.md"), "# spec\n")?;
    let expected = std::path::absolute(dir.path().join("spec.md"))?;

    let name = format!("laura-abs-{}.sock", std::process::id());
    let rx = laura::protocol::serve(&name)?;

    let n = name.clone();
    let cwd = dir.path().to_path_buf();
    let h = thread::spawn(move || {
        Command::cargo_bin("laura")
            .unwrap()
            .args(["open", "spec.md"])
            .current_dir(&cwd)
            .env("LAURA_TAB", &n)
            .assert()
            .success();
    });

    let (msg, reply) = rx.recv_timeout(Duration::from_secs(5))?;
    reply.send(&Response::Opened {
        pane: 1,
        warnings: vec![],
    });
    h.join().unwrap();

    let Message::Open { path, .. } = msg else {
        panic!("expected Open message");
    };
    assert_eq!(std::path::Path::new(&path), expected);
    Ok(())
}
