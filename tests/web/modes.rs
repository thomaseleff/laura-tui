//! The input modes that Laura holds (#245): the help, the pane selector, the quit question, the
//! lock and the draft, as the layout frame's `modes` and `draft` show them after each `act`.

use super::*;

/// The workspace with `a.rs` in pane 1, which has the focus.
fn file_focused(rig: &mut Rig, ws: &mut Ws) -> Result<tempfile::TempDir> {
    let (dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    assert_eq!(rig.act(ws, json!({"act": "focus", "pane": 1}))["ok"], true);
    Ok(dir)
}

/// The next layout frame with `modes` and the focus on `focus`.
fn modes(rig: &mut Rig, ws: &mut Ws, modes: Value, focus: u64) -> Value {
    let what = format!("modes {modes} with the focus on {focus}");
    layout_with(rig, ws, &what, |v| {
        v["modes"] == modes && v["focus"] == focus
    })
}

#[test]
fn f12_from_a_file_pane_focuses_the_shell_and_back() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let _dir = file_focused(&mut rig, &mut ws)?;
    rig.act(&mut ws, json!({"act": "lock"}));
    let layout = modes(&mut rig, &mut ws, json!(["locked"]), 0);
    let hint = pairs(&layout["hints"]["mode"]);
    assert!(hint.contains("🔒 locked"), "{hint}");
    rig.act(&mut ws, json!({"act": "lock"}));
    let layout = modes(&mut rig, &mut ws, json!([]), 1);
    assert!(layout["hints"]["mode"].is_null(), "{layout}");
    Ok(())
}

#[test]
fn the_unlock_keeps_the_shell_after_a_focus_move() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let _dir = file_focused(&mut rig, &mut ws)?;
    let (_dir_b, b) = file("b.rs", &five_lines())?;
    rig.drive(&["open", &b]);
    rig.act(&mut ws, json!({"act": "focus", "pane": 1}));
    rig.act(&mut ws, json!({"act": "lock"}));
    modes(&mut rig, &mut ws, json!(["locked"]), 0);
    rig.act(&mut ws, json!({"act": "focus", "pane": 2}));
    rig.act(&mut ws, json!({"act": "focus", "pane": 0}));
    rig.act(&mut ws, json!({"act": "lock"}));
    modes(&mut rig, &mut ws, json!([]), 0);
    Ok(())
}

#[test]
fn f12_under_the_help_keeps_the_focus() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let _dir = file_focused(&mut rig, &mut ws)?;
    rig.act(&mut ws, json!({"act": "overlay", "overlay": "help"}));
    rig.act(&mut ws, json!({"act": "lock"}));
    let layout = modes(&mut rig, &mut ws, json!(["help", "locked"]), 1);
    // The help has no hint: the hint line shows the lock's.
    let hint = pairs(&layout["hints"]["mode"]);
    assert!(hint.contains("🔒 locked"), "{hint}");
    Ok(())
}

#[test]
fn the_help_and_the_pane_selector_open_and_close() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.act(&mut ws, json!({"act": "overlay", "overlay": "help"}));
    modes(&mut rig, &mut ws, json!(["help"]), 0);
    // One overlay at a time: the pane selector takes the help's place.
    rig.act(&mut ws, json!({"act": "overlay", "overlay": "selector"}));
    modes(&mut rig, &mut ws, json!(["selector"]), 0);
    rig.act(&mut ws, json!({"act": "overlay", "overlay": null}));
    modes(&mut rig, &mut ws, json!([]), 0);
    // The quit question opens only after `quit`.
    rig.act(&mut ws, json!({"act": "overlay", "overlay": "quit"}));
    assert!(rig.tab.modes().is_empty());
    Ok(())
}

#[test]
fn ctrl_q_asks_the_quit_question() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    assert_eq!(rig.act(&mut ws, json!({"act": "quit"}))["ok"], true);
    let layout = modes(&mut rig, &mut ws, json!(["quit"]), 0);
    let hint = pairs(&layout["hints"]["mode"]);
    assert!(hint.contains("quit? press y"), "{hint}");
    rig.act(&mut ws, json!({"act": "overlay", "overlay": null}));
    modes(&mut rig, &mut ws, json!([]), 0);
    Ok(())
}

#[test]
fn a_comment_ends_its_draft_and_the_wait_of_the_agent() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let _dir = file_focused(&mut rig, &mut ws)?;
    rig.drive(&["ready"]);
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let layout = modes(&mut rig, &mut ws, json!(["draft"]), 1);
    assert_eq!(
        layout["draft"],
        json!({"pane": 1, "kind": "comment", "line": 1, "anchor": null}),
        "{layout}"
    );
    let highlight = rig.start(&["highlight", "--pane", "1", "3"]);
    for _ in 0..20 {
        rig.pump();
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        highlight.try_recv().is_err(),
        "the agent's highlight didn't wait"
    );
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 1, "text": "mine"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    assert!(rig.wait(&highlight).status.success());
    let layout = modes(&mut rig, &mut ws, json!([]), 1);
    assert!(layout["draft"].is_null(), "{layout}");
    Ok(())
}

#[test]
fn esc_cancels_the_draft() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let _dir = file_focused(&mut rig, &mut ws)?;
    rig.drive(&["ready"]);
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    modes(&mut rig, &mut ws, json!(["draft"]), 1);
    rig.act(&mut ws, json!({"act": "cancel_draft"}));
    modes(&mut rig, &mut ws, json!([]), 1);
    assert!(!rig.tab.held());
    Ok(())
}

#[test]
fn a_new_view_ends_the_modes() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut first = rig.connect();
    let _dir = file_focused(&mut rig, &mut first)?;
    rig.drive(&["ready"]);
    rig.act(&mut first, json!({"act": "begin_comment", "pane": 1}));
    rig.act(&mut first, json!({"act": "lock"}));
    rig.act(&mut first, json!({"act": "overlay", "overlay": "help"}));
    modes(&mut rig, &mut first, json!(["help", "locked", "draft"]), 1);
    let mut second = rig.connect();
    let layout = modes(&mut rig, &mut second, json!([]), 1);
    assert!(layout["draft"].is_null(), "{layout}");
    Ok(())
}
