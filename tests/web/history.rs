//! The History in the drawer: one row for each `laura` command of the agent's, each say and each
//! submitted inline review; a click that shows a command again; and a drawer that keeps its view
//! through a say and a wait.

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

/// Each row's kind, or its verb for a command.
fn verbs(h: &Value) -> Vec<String> {
    h["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            r["verb"]
                .as_str()
                .or(r["kind"].as_str())
                .unwrap_or("")
                .into()
        })
        .collect()
}

/// The number of rows in the History frame `h`.
fn count(h: &Value) -> usize {
    h["rows"].as_array().map_or(0, Vec::len)
}

/// Click History row `row`; Laura's `acted` reply.
fn replay(rig: &mut Rig, ws: &mut Ws, row: usize) -> Value {
    rig.act(ws, json!({"act": "replay", "pane": 0, "row": row}))
}

/// One row for each command that the agent's `laura` journals, for each say and for each submitted
/// inline review. `layout`, `feedback` and a failed command add none. A new session starts an empty
/// History with its ready row.
#[test]
fn each_command_gets_a_row() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let name = session("history-rows");
    rig.drive(&["ready", "--session", &name]);
    let h = history_when(&mut rig, &mut ws, "the ready row", |c| {
        c["rows"][0]["kind"] == "ready"
    });
    assert_ne!(h["rows"][0]["replay"], true, "{h}");
    rig.drive(&["ready", "--session", &name, "--title", "flat profile"]);
    history_when(&mut rig, &mut ws, "the session label", |c| {
        c["rows"][0]["title"] == "flat profile" && count(c) == 1
    });

    let (_a, a) = file("a.rs", &five_lines())?;
    let (_b, b) = file("b.rs", &five_lines())?;
    let (_c, c) = changed_file("c.txt", "keep\nold\n", "keep\nnew\n")?;
    rig.drive(&["open", &a, "--no-focus"]);
    rig.drive(&["open", &b, "--no-focus", "--highlight", "2", "3"]);
    let pane = rig.drive(&["open", &c, "--no-focus"]);
    rig.drive(&["diff", "--pane", pane.trim()]);
    rig.drive(&["diff", "--pane", pane.trim(), "--off"]);
    rig.drive(&["layout"]);
    rig.drive(&["feedback", "--positive", "fine"]);
    assert!(
        !rig.run(&["highlight", "2", "--pane", "99"])
            .status
            .success()
    );
    rig.drive(&["say", "Look at **this**"]);
    let h = history_when(&mut rig, &mut ws, "the say", |c| {
        c["rows"]
            .as_array()
            .and_then(|r| r.last())
            .is_some_and(|r| r["kind"] == "say")
    });
    let want = [
        "ready",
        "open",
        "open",
        "highlight",
        "open",
        "diff",
        "diff",
        "say",
    ];
    assert_eq!(verbs(&h), want, "{h}");
    let replay: Vec<bool> = (0..want.len())
        .map(|i| h["rows"][i]["replay"] == true)
        .collect();
    assert_eq!(
        replay,
        [false, true, true, true, true, true, false, false],
        "{h}"
    );
    assert_eq!(h["rows"][1]["target"], "a.rs", "{h}");
    assert_eq!(h["rows"][3]["target"], "b.rs:L2–3", "{h}");
    assert_eq!(h["rows"][5]["target"], "c.txt · on", "{h}");
    assert_eq!(h["rows"][6]["target"], "c.txt · off", "{h}");
    let html = h["rows"][7]["html"].as_str().unwrap_or_default();
    assert!(html.contains("<strong>this</strong>"), "{h}");
    assert!(
        rig.tab.drawer_shows_shell(),
        "a say keeps the drawer's view"
    );

    rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 2, "text": "why"}),
    );
    let acted = rig.act(&mut ws, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    let h = history_when(&mut rig, &mut ws, "the review row", |c| count(c) == 9);
    assert_eq!(h["rows"][8]["kind"], "review", "{h}");
    assert_eq!(h["rows"][8]["path"], "a.rs", "{h}");
    assert_eq!(h["rows"][8]["comments"], 1, "{h}");

    rig.drive(&["ready", "--session", &session("history-other")]);
    history_when(&mut rig, &mut ws, "a new session", |c| {
        count(c) == 1 && c["rows"][0]["kind"] == "ready"
    });
    Ok(())
}

/// An open row whose pane closed opens the file again, as the user's act: Laura journals the open
/// and adds no row. A file that is gone shows `GONE`.
#[test]
fn an_open_row_opens_its_file_again() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let name = session("history-open");
    rig.drive(&["ready", "--session", &name]);
    let (dir, a) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &a, "--no-focus"]);
    rig.drive(&["close", "1"]);
    history_when(&mut rig, &mut ws, "the close row", |c| count(c) == 3);
    let acted = replay(&mut rig, &mut ws, 1);
    assert_eq!(acted["ok"], true, "{acted}");
    layout_when(&mut rig, &mut ws, "a.rs again, focused", |l| {
        l["focus"] == 2
            && l["panes"]
                .as_array()
                .is_some_and(|ps| ps.iter().any(|p| p["id"] == 2))
    });
    assert_eq!(events(&name, "open").len(), 2);
    rig.drive(&["say", "again"]);
    let h = history_when(&mut rig, &mut ws, "the say", |c| {
        c["rows"]
            .as_array()
            .and_then(|r| r.last())
            .is_some_and(|r| r["kind"] == "say")
    });
    assert_eq!(count(&h), 4, "the replay added a row: {h}");

    rig.act(&mut ws, json!({"act": "close", "pane": 2}));
    drop(dir);
    let acted = replay(&mut rig, &mut ws, 1);
    assert_eq!(
        acted["text"],
        laura::tab::GONE.replace("{path}", &a),
        "{acted}"
    );
    assert!(rig.tab.panels.is_empty(), "the replay opened a pane");
    Ok(())
}

/// A highlight row finds its pane by its file, never by its pane id: after `open --panel`, the row
/// shows `NOT_OPEN` and leaves the pane as it is. On a frozen pane, the row shows `FROZEN`.
#[test]
fn a_highlight_row_finds_its_pane_by_file() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.drive(&["ready", "--session", &session("history-highlight")]);
    let (_a, a) = file("a.rs", &five_lines())?;
    let (_b, b) = file("b.rs", &five_lines())?;
    rig.drive(&["open", &a, "--no-focus", "--highlight", "2", "3"]);
    rig.drive(&["open", &b, "--panel", "1", "--no-focus"]);
    history_when(&mut rig, &mut ws, "the replace", |c| count(c) == 4);
    let before = rig.tab.panels[&1].highlight_lines();
    let acted = replay(&mut rig, &mut ws, 2);
    assert_eq!(
        acted["text"],
        laura::tab::NOT_OPEN.replace("{name}", "a.rs"),
        "{acted}"
    );
    assert!(rig.tab.panels[&1].path.ends_with("b.rs"));
    assert_eq!(rig.tab.panels[&1].highlight_lines(), before);

    assert_eq!(replay(&mut rig, &mut ws, 1)["ok"], true);
    layout_when(&mut rig, &mut ws, "a.rs in pane 2", |l| l["focus"] == 2);
    rig.act(&mut ws, json!({"act": "focus", "pane": 0}));
    let acted = replay(&mut rig, &mut ws, 2);
    assert_eq!(acted["ok"], true, "{acted}");
    assert_eq!(rig.tab.panels[&2].highlight_lines(), Some([2, 3]));
    assert_eq!(rig.tab.focus, 2, "the replay focuses the pane");

    rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 2, "line": 1, "text": "x"}),
    );
    std::fs::write(&a, "changed on disk\n")?;
    layout_when(&mut rig, &mut ws, "frozen", |l| {
        l["panes"]
            .as_array()
            .is_some_and(|ps| ps.iter().any(|p| p["id"] == 2 && p["frozen"] == true))
    });
    let acted = replay(&mut rig, &mut ws, 2);
    assert_eq!(
        acted["text"],
        laura::tab::FROZEN.replace("{n}", "2"),
        "{acted}"
    );
    Ok(())
}

/// A diff row that turned the diff view on turns it on again; the row that turned it off shows nothing again.
#[test]
fn a_diff_row_turns_the_diff_view_on_again() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_c, c) = changed_file("c.txt", "keep\nold\n", "keep\nnew\n")?;
    rig.drive(&["open", &c, "--no-focus"]);
    rig.drive(&["diff", "--pane", "1"]);
    rig.drive(&["diff", "--pane", "1", "--off"]);
    history_when(&mut rig, &mut ws, "the diff rows", |c| count(c) == 3);
    assert_eq!(replay(&mut rig, &mut ws, 2)["ok"], true);
    assert!(
        !rig.tab.panels[&1].diff_view,
        "the off row turned the diff view on"
    );
    assert_eq!(replay(&mut rig, &mut ws, 1)["ok"], true);
    assert!(rig.tab.panels[&1].diff_view);
    Ok(())
}

/// A comment row moves the cursor to the comment's line and adds no thread.
#[test]
fn a_comment_row_moves_the_cursor() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.drive(&["ready", "--session", &session("history-comment")]);
    let (_a, a) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &a, "--no-focus"]);
    rig.drive(&["comment", "4", "look", "--pane", "1"]);
    history_when(&mut rig, &mut ws, "the comment row", |c| count(c) == 3);
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 1}));
    assert_eq!(replay(&mut rig, &mut ws, 2)["ok"], true);
    assert_eq!(rig.tab.panels[&1].cursor_line(), 4);
    assert_eq!(rig.tab.panels[&1].thread_count(), 1);
    assert_eq!(rig.tab.focus, 1);
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
        c["rows"][1]["kind"] == "say"
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
        c["rows"][2]["kind"] == "say"
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
