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
    // The client absolutizes relative paths against its cwd before framing; the round-trip
    // still carries the same path, now absolute against the test process's cwd.
    let expected = std::path::absolute("docs/spec.md")?;
    assert_eq!(std::path::Path::new(&path), expected);
    Ok(())
}

#[test]
fn open_wire_shape_is_stable() {
    let json = serde_json::to_string(&Message::Open {
        path: "docs/spec.md".into(),
        split: None,
        dir: Some(Dir::Horizontal),
        ratio: Some(40),
        side: Some(Side::Second),
        focus: true,
        follow: false,
        dry_run: false,
        highlight: None,
        diff: false,
        panel: None,
    })
    .unwrap();
    assert_eq!(
        json,
        r#"{"type":"open","path":"docs/spec.md","split":null,"dir":"horizontal","ratio":40,"side":"second","focus":true,"follow":false,"dry_run":false,"highlight":null,"diff":false,"panel":null}"#
    );
}

#[test]
fn comment_wire_shape_is_stable() {
    let json = serde_json::to_string(&Message::Comment {
        pane: Some(2),
        line: 42,
        body: Some("tighten this".into()),
        author: Some("agent".into()),
    })
    .unwrap();
    assert_eq!(
        json,
        r#"{"type":"comment","pane":2,"line":42,"body":"tighten this","author":"agent"}"#
    );
}

/// The bare form `{"type":"comment","line":N}` decodes: pane→focused, no body/author.
#[test]
fn comment_defaults_are_minimal() {
    let msg: Message = serde_json::from_str(r#"{"type":"comment","line":7}"#).unwrap();
    assert_eq!(
        msg,
        Message::Comment {
            pane: None,
            line: 7,
            body: None,
            author: None,
        }
    );
}

/// An older `{"type":"open","path":"x"}` (no split/dir/ratio/side/focus/dry_run) still decodes;
/// the split fields fill `None` so the server infers the layout.
#[test]
fn open_back_compat_defaults() {
    let msg: Message = serde_json::from_str(r#"{"type":"open","path":"x"}"#).unwrap();
    assert_eq!(
        msg,
        Message::Open {
            path: "x".into(),
            split: None,
            dir: None,
            ratio: None,
            side: None,
            focus: true,
            follow: false,
            dry_run: false,
            highlight: None,
            diff: false,
            panel: None,
        }
    );
}
