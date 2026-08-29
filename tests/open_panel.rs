//! The full chain — real `laura open` → tab socket → decode → panel holds the file's content — without the render loop. State, not pixels.

use std::io::Write;
use std::time::Duration;

use anyhow::Result;
use assert_cmd::Command;
use laura::{Message, Panel};

#[test]
fn open_renders_file_into_panel() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "# spec\nknown content\n")?;
    let path = file.path().to_str().unwrap().to_string();

    let name = format!("laura-test-{}.sock", std::process::id());
    let rx = laura::protocol::serve(&name)?;

    Command::cargo_bin("laura")?
        .args(["open", &path])
        .env("LAURA_TAB", &name)
        .assert()
        .success();

    let Message::Open { path, .. } = rx.recv_timeout(Duration::from_secs(5))? else {
        panic!("expected Open message");
    };
    let panel = Panel::open(path);

    assert_eq!(panel.path, file.path().to_str().unwrap());
    assert_eq!(panel.content, "# spec\nknown content\n");
    Ok(())
}
