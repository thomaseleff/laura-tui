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
