//! The History in the drawer: the agent's says, its other commands folded into one row, a new
//! session's empty History, and a drawer that keeps its view through a say and a wait.

use super::*;

/// The next `history` frame on `ws` that `want` matches.
fn history_when(rig: &mut Rig, ws: &mut Ws, what: &str, want: impl Fn(&Value) -> bool) -> Value {
    rig.until(
        ws,
        what,
        |f| matches!(f, Frame::Json(v) if v["t"] == "history" && want(v)),
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

/// A say shows in the History as Laura's HTML, the drawer stays on the shell, and the agent's next
/// commands fold into one row. A new session starts an empty History.
#[test]
fn a_say_and_the_commands_after_it_fold_into_the_history() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let first = layout_when(&mut rig, &mut ws, "layout", |_| true);
    assert_eq!(first["drawer_view"], "shell", "before a say: {first}");
    rig.drive(&["ready", "--session", &session("chat-fold")]);
    rig.drive(&["say", "Look at **this**"]);
    let chat = history_when(&mut rig, &mut ws, "the say", |c| {
        c["rows"][0]["kind"] == "say"
    });
    let html = chat["rows"][0]["html"].as_str().unwrap_or_default();
    assert!(html.contains("<strong>this</strong>"), "{chat}");
    assert!(
        rig.tab.drawer_shows_shell(),
        "a say keeps the drawer's view"
    );

    let (_a, a) = file("a.rs", &five_lines())?;
    let (_b, b) = file("b.rs", &five_lines())?;
    rig.drive(&["open", &a, "--no-focus"]);
    let one = history_when(&mut rig, &mut ws, "one open", |c| c["rows"][1].is_object());
    assert_eq!(one["rows"][1]["text"], "opened a.rs", "{one}");
    rig.drive(&["open", &b, "--no-focus"]);
    let two = history_when(&mut rig, &mut ws, "two opens", |c| {
        c["rows"][1]["text"] == "opened 2 files"
    });
    assert_eq!(two["rows"].as_array().map(Vec::len), Some(2), "{two}");
    assert_eq!(two["rows"][1]["kind"], "commands", "{two}");
    assert_eq!(two["rows"][1]["pane"], 2, "the run's last pane: {two}");

    // The row's click focuses a pane still open after the run.
    rig.drive(&["close", "2"]);
    let closed = history_when(&mut rig, &mut ws, "a close", |c| {
        c["rows"][1]["text"] == "opened 2 files, closed pane #2"
    });
    assert_eq!(closed["rows"][1]["pane"], 1, "pane 2 is closed: {closed}");
    // The user's own close stays out of the History, and in the journal.
    rig.act(&mut ws, json!({"act": "close", "pane": 1}));
    rig.drive(&["focus", "0"]);
    let focused = history_when(&mut rig, &mut ws, "a focus", |c| {
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
    history_when(&mut rig, &mut ws, "an empty History", |c| {
        c["rows"] == json!([])
    });
    Ok(())
}

/// A wait keeps the drawer's view, adds no row, and journals whether a say came since the user
/// last wrote to the shell.
#[cfg(unix)]
#[test]
fn a_wait_keeps_the_drawer_view() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    layout_when(&mut rig, &mut ws, "layout", |_| true);
    let name = session("history-bell");
    rig.drive(&["ready", "--session", &name]);
    rig.drive(&["say", "hi"]);
    history_when(&mut rig, &mut ws, "the say", |c| {
        c["rows"][0]["kind"] == "say"
    });
    rig.act(&mut ws, json!({"act": "flip_drawer"}));
    layout_when(&mut rig, &mut ws, "the History", |l| {
        l["drawer_view"] == "history"
    });
    // The user's keystrokes come after the say, so the bell comes with no say since.
    type_in(&mut ws, 0, b"printf '\\007'\r")?;
    let l = layout_when(&mut rig, &mut ws, "the unsaid wait", |l| {
        l["unsaid"] == true
    });
    assert_eq!(l["drawer_view"], "history", "{l}");
    assert_eq!(waits(&name)[0]["said"], false, "{:?}", waits(&name));

    // The shell rings once the test makes `go`, after the agent's say.
    let dir = tempfile::tempdir()?;
    let go = dir.path().join("go");
    let cmd = format!(
        "while [ ! -f {} ]; do sleep 0.05; done; printf '\\007'\r",
        go.display()
    );
    type_in(&mut ws, 0, cmd.as_bytes())?;
    layout_when(&mut rig, &mut ws, "the wait ended", |l| {
        l["waiting"] == false
    });
    rig.drive(&["say", "Run the tests?"]);
    history_when(&mut rig, &mut ws, "the second say", |c| {
        c["rows"][1]["kind"] == "say"
    });
    std::fs::write(&go, "")?;
    let l = layout_when(&mut rig, &mut ws, "the said wait", |l| l["waiting"] == true);
    assert_eq!(l["drawer_view"], "history", "{l}");
    assert_eq!(waits(&name).len(), 2, "{:?}", waits(&name));
    assert_eq!(waits(&name)[1]["said"], true, "{:?}", waits(&name));
    let later = rig.idle(&mut ws, 5);
    assert!(
        !later.into_iter().any(|f| f.is("history")),
        "a wait added a row"
    );
    Ok(())
}
