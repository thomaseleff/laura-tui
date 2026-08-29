//! A message sent by `laura open` from a separate process is framed and decoded by an in-process listener.

use std::time::Duration;

use anyhow::Result;
use assert_cmd::Command;
use laura::Message;

#[test]
fn open_round_trips_across_processes() -> Result<()> {
    let name = format!("laura-test-{}.sock", std::process::id());
    let rx = laura::protocol::serve(&name)?;

    Command::cargo_bin("laura")?
        .args(["open", "docs/spec.md"])
        .env("LAURA_TAB", &name)
        .assert()
        .success();

    let got = rx.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(
        got,
        Message::Open {
            path: "docs/spec.md".into(),
            focus: true,
        }
    );
    Ok(())
}

#[test]
fn open_wire_shape_is_stable() {
    let json = serde_json::to_string(&Message::Open {
        path: "docs/spec.md".into(),
        focus: true,
    })
    .unwrap();
    assert_eq!(
        json,
        r#"{"type":"open","path":"docs/spec.md","focus":true}"#
    );
}

/// An older `{"type":"open"}` with no `focus` field still decodes, defaulting to focus = true.
#[test]
fn open_without_focus_field_defaults_true() {
    let msg: Message = serde_json::from_str(r#"{"type":"open","path":"x"}"#).unwrap();
    assert_eq!(
        msg,
        Message::Open {
            path: "x".into(),
            focus: true,
        }
    );
}
