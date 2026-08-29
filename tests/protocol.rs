//! A request sent by `laura open` from a separate process is framed, decoded, and answered by an in-process listener (request → response).

use std::thread;
use std::time::Duration;

use anyhow::Result;
use assert_cmd::Command;
use laura::protocol::{Dir, Side};
use laura::{Message, Response};

#[test]
fn open_round_trips_across_processes() -> Result<()> {
    let name = format!("laura-test-{}.sock", std::process::id());
    let rx = laura::protocol::serve(&name)?;

    // The client blocks for a response, so serve it while it's connected.
    let n = name.clone();
    let h = thread::spawn(move || {
        Command::cargo_bin("laura")
            .unwrap()
            .args(["open", "docs/spec.md"])
            .env("LAURA_TAB", &n)
            .assert()
            .success();
    });

    let (got, reply) = rx.recv_timeout(Duration::from_secs(5))?;
    reply.send(&Response::Opened {
        pane: 1,
        warnings: vec![],
    });
    h.join().unwrap();

    let Message::Open { path, .. } = got else {
        panic!("expected Open");
    };
    assert_eq!(path, "docs/spec.md");
    Ok(())
}

#[test]
fn open_wire_shape_is_stable() {
    let json = serde_json::to_string(&Message::Open {
        path: "docs/spec.md".into(),
        split: None,
        dir: Dir::Horizontal,
        ratio: 40,
        side: Side::Second,
        focus: true,
        dry_run: false,
    })
    .unwrap();
    assert_eq!(
        json,
        r#"{"type":"open","path":"docs/spec.md","split":null,"dir":"horizontal","ratio":40,"side":"second","focus":true,"dry_run":false}"#
    );
}

/// An older `{"type":"open","path":"x"}` (no split/dir/ratio/side/focus/dry_run) still decodes with sane defaults.
#[test]
fn open_back_compat_defaults() {
    let msg: Message = serde_json::from_str(r#"{"type":"open","path":"x"}"#).unwrap();
    assert_eq!(
        msg,
        Message::Open {
            path: "x".into(),
            split: None,
            dir: Dir::Horizontal,
            ratio: 50,
            side: Side::Second,
            focus: true,
            dry_run: false,
        }
    );
}
