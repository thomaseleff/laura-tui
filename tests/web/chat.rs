//! The chat in the drawer: the agent's says, its other commands folded into one row, a new
//! session's empty chat, and the drawer on the shell while the shell waits with no say.

use super::*;

/// The next `chat` frame on `ws` that `want` matches.
fn chat_when(rig: &mut Rig, ws: &mut Ws, what: &str, want: impl Fn(&Value) -> bool) -> Value {
    rig.until(
        ws,
        what,
        |f| matches!(f, Frame::Json(v) if v["t"] == "chat" && want(v)),
    )
    .json()
}

/// A session name no other test in this binary journals to.
fn session(name: &str) -> String {
    format!("{name}-{}", std::process::id())
}

/// The `wait` events in the test home's journal of `session`.
fn waits(session: &str) -> Vec<Value> {
    events(session, "wait")
}

/// The events of type `kind` in the test home's journal of `session`.
fn events(session: &str, kind: &str) -> Vec<Value> {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(".laura/sessions")
        .join(format!("{session}.ndjson"));
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|e| e["type"] == kind)
        .collect()
}

/// A say shows in the chat as Laura's HTML, the drawer turns to the chat, and the agent's next
/// commands fold into one row. A new session starts an empty chat.
#[test]
fn a_say_and_the_commands_after_it_fold_into_the_chat() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let first = layout_when(&mut rig, &mut ws, "layout", |_| true);
    assert_eq!(first["drawer_view"], "shell", "before a say: {first}");
    rig.drive(&["ready", "--session", &session("chat-fold")]);
    rig.drive(&["say", "Look at **this**"]);
    let chat = chat_when(&mut rig, &mut ws, "the say", |c| {
        c["rows"][0]["kind"] == "say"
    });
    let html = chat["rows"][0]["html"].as_str().unwrap_or_default();
    assert!(html.contains("<strong>this</strong>"), "{chat}");
    layout_when(&mut rig, &mut ws, "the chat", |l| {
        l["drawer_view"] == "chat"
    });

    let (_a, a) = file("a.rs", &five_lines())?;
    let (_b, b) = file("b.rs", &five_lines())?;
    rig.drive(&["open", &a, "--no-focus"]);
    let one = chat_when(&mut rig, &mut ws, "one open", |c| c["rows"][1].is_object());
    assert_eq!(one["rows"][1]["text"], "opened a.rs", "{one}");
    rig.drive(&["open", &b, "--no-focus"]);
    let two = chat_when(&mut rig, &mut ws, "two opens", |c| {
        c["rows"][1]["text"] == "opened 2 files"
    });
    assert_eq!(two["rows"].as_array().map(Vec::len), Some(2), "{two}");
    assert_eq!(two["rows"][1]["kind"], "commands", "{two}");
    assert_eq!(two["rows"][1]["pane"], 2, "the run's last pane: {two}");

    // The row's click focuses a pane still open after the run.
    rig.drive(&["close", "2"]);
    let closed = chat_when(&mut rig, &mut ws, "a close", |c| {
        c["rows"][1]["text"] == "opened 2 files, closed pane #2"
    });
    assert_eq!(closed["rows"][1]["pane"], 1, "pane 2 is closed: {closed}");
    // The user's own close stays out of the chat, and in the journal.
    rig.act(&mut ws, json!({"act": "close", "pane": 1}));
    rig.drive(&["focus", "0"]);
    let focused = chat_when(&mut rig, &mut ws, "a focus", |c| {
        c["rows"][1]["text"]
            .as_str()
            .is_some_and(|t| t.ends_with("focused the shell"))
    });
    assert_eq!(
        focused["rows"][1]["text"], "opened 2 files, closed pane #2, focused the shell",
        "{focused}"
    );
    assert_eq!(events(&session("chat-fold"), "close").len(), 2);

    rig.drive(&["ready", "--session", &session("chat-other")]);
    chat_when(&mut rig, &mut ws, "an empty chat", |c| {
        c["rows"] == json!([])
    });
    Ok(())
}

/// A bell with no say since the user's last message shows the shell in the drawer, adds a wait row
/// and journals `said: false`. A bell after a say journals `said: true`, and the drawer keeps the
/// chat.
#[cfg(unix)]
#[test]
fn a_bell_with_no_say_shows_the_shell() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    layout_when(&mut rig, &mut ws, "layout", |_| true);
    let name = session("chat-bell");
    rig.drive(&["ready", "--session", &name]);
    rig.drive(&["say", "hi"]);
    layout_when(&mut rig, &mut ws, "the chat", |l| {
        l["drawer_view"] == "chat"
    });
    // The user's keystrokes come after the say, so the bell comes with no say since.
    type_in(&mut ws, 0, b"printf '\\007'\r")?;
    // Laura sends the chat before the layout of the same turn.
    let chat = chat_when(&mut rig, &mut ws, "the wait row", |c| {
        c["rows"][1]["kind"] == "wait"
    });
    layout_when(&mut rig, &mut ws, "the shell", |l| {
        l["waiting"] == true && l["drawer_view"] == "shell"
    });
    assert_eq!(chat["rows"][1]["done"], false, "{chat}");
    assert_eq!(waits(&name)[0]["said"], false, "{:?}", waits(&name));

    // The shell rings once the test makes `go`, after the agent's say.
    let dir = tempfile::tempdir()?;
    let go = dir.path().join("go");
    let cmd = format!(
        "while [ ! -f {} ]; do sleep 0.05; done; printf '\\007'\r",
        go.display()
    );
    type_in(&mut ws, 0, cmd.as_bytes())?;
    chat_when(&mut rig, &mut ws, "the wait ended", |c| {
        c["rows"][1]["done"] == true
    });
    rig.drive(&["say", "Run the tests?"]);
    chat_when(&mut rig, &mut ws, "the second say", |c| {
        c["rows"][2]["kind"] == "say"
    });
    layout_when(&mut rig, &mut ws, "the chat again", |l| {
        l["waiting"] == false && l["drawer_view"] == "chat"
    });
    std::fs::write(&go, "")?;
    let l = layout_when(&mut rig, &mut ws, "the said wait", |l| l["waiting"] == true);
    assert_eq!(l["drawer_view"], "chat", "{l}");
    assert_eq!(waits(&name).len(), 2, "{:?}", waits(&name));
    assert_eq!(waits(&name)[1]["said"], true, "{:?}", waits(&name));
    let later = rig.idle(&mut ws, 5);
    assert!(
        !later.into_iter().any(|f| f.is("chat")),
        "a said wait added a row"
    );
    // A message from the message box ends the said wait, as the user's write to the shell.
    assert_eq!(
        rig.act(&mut ws, json!({"act": "message", "text": "go on"})),
        json!({"t": "acted", "ok": true, "text": null})
    );
    layout_when(&mut rig, &mut ws, "no wait", |l| l["waiting"] == false);
    Ok(())
}

/// A message from the message box reaches the shell as one paste, Laura journals it, and the chat
/// shows it. While the shell waits with no say, Laura turns the box off and the message waits.
#[cfg(unix)]
#[test]
fn a_message_reaches_the_shell() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    layout_when(&mut rig, &mut ws, "layout", |_| true);
    let name = session("chat-message");
    rig.drive(&["ready", "--session", &name]);
    rig.drive(&["say", "Ready?"]);
    let ok = rig.act(&mut ws, json!({"act": "message", "text": "hello-msg"}));
    assert_eq!(ok["ok"], true, "{ok}");
    rig.shell_shows("hello-msg");
    let chat = chat_when(&mut rig, &mut ws, "the message row", |c| {
        c["rows"][1]["kind"] == "message"
    });
    assert_eq!(chat["rows"][1]["body"], "hello-msg", "{chat}");
    assert_eq!(events(&name, "message")[0]["body"], "hello-msg");

    type_in(&mut ws, 0, b"printf '\\007'\r")?;
    let l = layout_when(&mut rig, &mut ws, "the unsaid wait", |l| {
        l["unsaid"] == true
    });
    assert_eq!(l["message_box"], false, "{l}");
    let refused = rig.act(&mut ws, json!({"act": "message", "text": "not-sent"}));
    assert_eq!(refused["text"], laura::tab::MESSAGE_WAITS, "{refused}");
    rig.idle(&mut ws, 5);
    assert!(
        !rig.tab
            .pty
            .with_screen(|s| s.contents())
            .contains("not-sent"),
        "the message reached the shell"
    );
    assert_eq!(
        events(&name, "message").len(),
        1,
        "{:?}",
        events(&name, "message")
    );
    Ok(())
}
