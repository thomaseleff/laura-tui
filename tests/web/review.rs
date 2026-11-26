//! Threads and the inline review: threads in the layout, comment and submit, and a draft that holds
//! the agent.

use super::*;

#[test]
fn a_thread_shows_as_data() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready", "--agent", "claude"]);
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 4, "text": "user note"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.drive(&["comment", "--pane", "1", "2", "agent note"]);
    let mut ws = rig.connect();
    let frame = pane_frame(&mut rig, &mut ws, 1);
    assert_eq!(
        frame["threads"],
        json!([
            {"line": 2, "collapsed": false, "label": null, "notes": [{"author": "claude", "body": "agent note"}]},
            {"line": 4, "collapsed": false, "label": null, "notes": [{"author": null, "body": "user note"}]},
        ]),
        "{frame}"
    );
    let html = frame["html"].as_str().unwrap_or("");
    assert!(
        rows(html).iter().all(|(_, t)| !t.contains("note")),
        "{html}"
    );
    Ok(())
}

#[test]
fn a_collapsed_thread_says_so() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "2", "agent note"]);
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 2}));
    rig.until(
        &mut ws,
        "the cursor on the thread",
        |f| matches!(f, Frame::Json(v) if v["id"] == 1 && v["card"] == 2),
    );
    let acted = rig.act(&mut ws, json!({"act": "toggle_thread", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.until(&mut ws, "a collapsed thread", |f| {
        matches!(f, Frame::Json(v) if v["id"] == 1 && v["threads"][0]["collapsed"] == true && v["card"].is_null())
    });
    // The rows show no caret, and not the TUI's minimized card: the browser draws its own.
    let mut ws = rig.connect();
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(!html.contains('▸') && !html.contains('▾'), "{html}");
    assert!(!html.contains("1 comment"), "{html}");
    Ok(())
}

#[test]
fn a_thread_in_a_block_toggles_alone() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("q.md", "# T\n\n> one\n>\n> two\n>\n> three\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    for line in ["3", "5", "7"] {
        rig.drive(&["comment", "--pane", "1", line, "agent note"]);
    }
    let collapsed = |rig: &Rig| -> Vec<bool> {
        rig.tab.panels[&1]
            .threads
            .iter()
            .map(|t| t.collapsed)
            .collect()
    };
    // `r` on a thread off the block's first line toggles that thread, both ways.
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 5}));
    rig.act(&mut ws, json!({"act": "toggle_thread", "pane": 1}));
    assert_eq!(collapsed(&rig), [false, true, false]);
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 5}));
    rig.act(&mut ws, json!({"act": "toggle_thread", "pane": 1}));
    assert_eq!(collapsed(&rig), [false, false, false]);
    // A card's button toggles its thread alone, even on the block's first line.
    rig.act(
        &mut ws,
        json!({"act": "toggle_thread", "pane": 1, "line": 3}),
    );
    assert_eq!(collapsed(&rig), [true, false, false]);
    // `r` on the block toggles every thread in the block.
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 3}));
    rig.act(&mut ws, json!({"act": "toggle_thread", "pane": 1}));
    assert_eq!(collapsed(&rig), [true, true, true]);
    Ok(())
}

/// A click on a collapsed thread's row in the thread list sends `toggle_thread` with the row's
/// line (#125). This test pins what that act does: from a cursor on another line, the act moves
/// the cursor and expands that thread alone, also in a markdown block. The click itself is
/// JavaScript, which no test runs, so this test passes without the click's change; the manual
/// check covers the click.
#[test]
fn a_thread_list_click_expands_a_collapsed_thread() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "2", "agent note"]);
    rig.act(
        &mut ws,
        json!({"act": "toggle_thread", "pane": 1, "line": 2}),
    );
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    rig.until(
        &mut ws,
        "a collapsed thread",
        |f| matches!(f, Frame::Json(v) if v["id"] == 1 && v["threads"][0]["collapsed"] == true),
    );
    let acted = rig.act(
        &mut ws,
        json!({"act": "toggle_thread", "pane": 1, "line": 2}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.until(&mut ws, "the expanded thread at the cursor", |f| {
        matches!(f, Frame::Json(v) if v["id"] == 1 && v["threads"][0]["collapsed"] == false && v["card"] == 2)
    });

    // In a block, where `r` toggles each of the block's threads, the row's thread expands alone.
    let (_dir, path) = file("q.md", "# T\n\n> one\n>\n> two\n")?;
    rig.drive(&["open", &path]);
    for line in ["3", "5"] {
        rig.drive(&["comment", "--pane", "2", line, "agent note"]);
        rig.act(
            &mut ws,
            json!({"act": "toggle_thread", "pane": 2, "line": line.parse::<u32>()?}),
        );
    }
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 2, "line": 1}));
    rig.act(
        &mut ws,
        json!({"act": "toggle_thread", "pane": 2, "line": 3}),
    );
    let collapsed: Vec<bool> = rig.tab.panels[&2]
        .threads
        .iter()
        .map(|t| t.collapsed)
        .collect();
    assert_eq!(collapsed, [false, true]);
    assert_eq!(rig.tab.panels[&2].cursor_line(), 3);
    rig.until(&mut ws, "the block's first thread expanded", |f| {
        matches!(f, Frame::Json(v) if v["id"] == 2
            && v["threads"][0]["collapsed"] == false && v["threads"][1]["collapsed"] == true)
    });
    Ok(())
}

#[test]
fn an_unfocused_page_keeps_its_cursor_line() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "page.html",
        "<!doctype html>\n<html>\n<body>\n<p>four</p>\n</body>\n</html>\n",
    )?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "4", "agent note"]);
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    rig.act(&mut ws, json!({"act": "focus", "pane": 0}));
    // The browser shows the cursor's thread below the page; the frame's band follows the focus.
    page_frame(&mut rig, &mut ws, 1, |v| {
        v["cursor"].is_null() && v["line"] == 4
    });
    Ok(())
}

/// **Delete** on the user's own last comment saves it empty, which removes the comment and its thread.
#[test]
fn an_empty_comment_deletes_your_last_comment() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 2, "text": "user note"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    // A new view gets the pane's current frame.
    let mut ws = rig.connect();
    let frame = pane_frame(&mut rig, &mut ws, 1);
    assert_eq!(frame["threads"][0]["line"], 2, "{frame}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 2, "text": ""}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let mut ws = rig.connect();
    let frame = pane_frame(&mut rig, &mut ws, 1);
    assert_eq!(frame["threads"], json!([]), "{frame}");
    Ok(())
}

#[test]
fn comment_and_submit_from_the_browser() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": 1}),
    );
    let begin = json!({"act": "begin_comment", "pane": 1});
    let acted = rig.act(&mut ws, begin.clone());
    assert_eq!(acted["ok"], true, "{acted}");
    assert_eq!(acted["text"], "", "{acted}");
    assert_eq!(acted["line"], 2, "{acted}");
    let layout = layout_with(&mut rig, &mut ws, "the draft", |v| v["draft"]["line"] == 2);
    assert_eq!(layout["modes"], json!(["draft"]), "{layout}");
    let hint = pairs(&layout["hints"]["mode"]);
    assert!(
        hint.contains("L2") && hint.contains("Shift+Enter"),
        "{hint}"
    );
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 2, "text": "user note\nsecond line"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    assert_eq!(rig.tab.panels[&1].thread_count(), 1);
    // The comment keeps its newline, and the rows don't hold it.
    let mut ws = rig.connect();
    let frame = pane_frame(&mut rig, &mut ws, 1);
    assert_eq!(
        frame["threads"][0]["notes"][0]["body"], "user note\nsecond line",
        "{frame}"
    );
    let html = frame["html"].as_str().unwrap_or("");
    assert!(
        rows(html).iter().all(|(_, t)| !t.contains("user note")),
        "{html}"
    );
    // Again on the same line: the draft edits the user's comment.
    let acted = rig.act(&mut ws, begin);
    assert_eq!(acted["text"], "user note\nsecond line", "{acted}");
    let layout = layout_with(&mut rig, &mut ws, "the edit", |v| v["draft"]["line"] == 2);
    let hint = pairs(&layout["hints"]["mode"]);
    assert!(hint.contains("empty deletes"), "{hint}");
    let acted = rig.act(&mut ws, json!({"act": "begin_review", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    let layout = layout_with(&mut rig, &mut ws, "the review body", |v| {
        v["draft"]["kind"] == "review"
    });
    let hint = pairs(&layout["hints"]["mode"]);
    assert!(hint.contains("Review body"), "{hint}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "submit", "pane": 1, "body": "overall"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows("[laura review · pane #1 · ");
    Ok(())
}

#[test]
fn a_draft_holds_the_agent() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, a) = file("a.rs", &five_lines())?;
    let (_dir_b, b) = file("b.rs", &five_lines())?;
    rig.drive(&["open", &a]);
    rig.drive(&["ready"]);
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    // A failed begin, here a review with no comments yet, keeps the open draft's hold.
    let acted = rig.act(&mut ws, json!({"act": "begin_review", "pane": 1}));
    assert_eq!(acted["ok"], false, "{acted}");
    assert!(rig.tab.held());
    let open = rig.start(&["open", &b]);
    for _ in 0..20 {
        rig.pump();
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(rig.tab.focus, 1);
    assert!(!rig.tab.panels.contains_key(&2));
    assert!(open.try_recv().is_err(), "the agent's open didn't wait");
    let acted = rig.act(&mut ws, json!({"act": "cancel_draft"}));
    assert_eq!(acted["ok"], true, "{acted}");
    assert!(rig.wait(&open).status.success());
    assert!(rig.tab.panels.contains_key(&2));
    Ok(())
}

#[test]
fn a_takeover_or_a_closed_view_ends_the_draft() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut first = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let begin = json!({"act": "begin_comment", "pane": 1});
    assert_eq!(rig.act(&mut first, begin.clone())["ok"], true);
    assert!(rig.tab.held());
    let highlight = rig.start(&["highlight", "--pane", "1", "2"]);
    let mut second = rig.connect();
    rig.until(&mut second, "layout", |f| f.is("layout"));
    assert!(rig.wait(&highlight).status.success());
    assert_eq!(rig.act(&mut second, begin)["ok"], true);
    assert!(rig.tab.held());
    second.close(None)?;
    drop(second);
    let highlight = rig.start(&["highlight", "--pane", "1", "3"]);
    assert!(rig.wait(&highlight).status.success());
    Ok(())
}

#[test]
fn ready_lists_no_web_or_browser() -> Result<()> {
    let mut rig = Rig::new()?;
    let v: Value = serde_json::from_str(&rig.drive(&["ready"]))?;
    assert_eq!(v["experimental"], json!([]), "{v}");
    assert_eq!(v["web_app"], true, "{v}");
    assert!(v["browser_url"].is_null(), "{v}");
    Ok(())
}
