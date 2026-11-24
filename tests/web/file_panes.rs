//! File panes: rows, the cursor, keystrokes, jumps, diff rows, colors, editor panes, reloads and
//! tail panes.

use super::*;

#[test]
fn set_cursor_moves_the_cursor_and_sends_two_rows() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let acted = rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    assert_eq!(acted["ok"], true, "{acted}");
    assert_eq!(rig.tab.panels[&1].cursor, 3);
    let rows = rig.until(&mut ws, "rows", |f| f.is("rows")).json();
    assert_eq!(rows["rows"].as_array().map(Vec::len), Some(2), "{rows}");
    assert_eq!(rows["cursor"], 3);
    Ok(())
}

#[test]
fn a_jump_to_a_line_recenters_the_pane_and_a_click_does_not() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let body: String = (1..=400).map(|n| format!("line {n}\n")).collect();
    let (_dir, path) = file("long.txt", &body)?;
    let acted = rig.act(
        &mut ws,
        json!({"act": "open_path", "path": path, "line": 300}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let frame = pane_frame(&mut rig, &mut ws, 1);
    assert_eq!(
        (frame["cursor"].clone(), frame["recenter"].clone()),
        (json!(299), json!("line")),
        "{frame}"
    );
    rig.act(
        &mut ws,
        json!({"act": "set_cursor", "pane": 1, "line": 305}),
    );
    let rows = rig.until(&mut ws, "rows", |f| f.is("rows")).json();
    assert_eq!(rows["recenter"], false, "{rows}");
    // A highlight recenters on the whole span, not only on the cursor's line.
    rig.drive(&["highlight", "--pane", "1", "310"]);
    let rows = rig
        .until(
            &mut ws,
            "highlight",
            |f| matches!(f, Frame::Json(v) if v["cursor"] == 309),
        )
        .json();
    assert_eq!(rows["recenter"], "highlight", "{rows}");
    // A jump to the highlight's first line is a jump: it keeps the view when the line is on screen.
    let acted = rig.act(
        &mut ws,
        json!({"act": "open_path", "path": path, "line": 310}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let rows = rig
        .until(
            &mut ws,
            "jump",
            |f| matches!(f, Frame::Json(v) if v["cursor"] == 309 && v["recenter"] != false),
        )
        .json();
    assert_eq!(rows["recenter"], "line", "{rows}");
    Ok(())
}

#[test]
fn a_jump_to_a_line_in_an_html_file_tells_the_page_to_center_it() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let body: String = (1..=300).map(|n| format!("<p>line {n}</p>\n")).collect();
    let (_dir, path) = file("page.html", &body)?;
    let acted = rig.act(
        &mut ws,
        json!({"act": "open_path", "path": path, "line": 250}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let frame = page_frame(&mut rig, &mut ws, 1, |v| v["cursor"] == 250);
    assert_eq!(frame["jump"], true, "{frame}");
    // A click on the page moves the cursor, not the view.
    rig.act(
        &mut ws,
        json!({"act": "set_cursor", "pane": 1, "line": 260}),
    );
    let frame = page_frame(&mut rig, &mut ws, 1, |v| v["cursor"] == 260);
    assert_eq!(frame["jump"], false, "{frame}");
    Ok(())
}

#[test]
fn file_pane_keys_reach_the_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file(
        "a.rs",
        &five_lines(),
        "line 1\nline 2 CHANGED\nline 3\nline 4\nline 5\n",
    )?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "4", "agent note"]);
    rig.drive(&["highlight", "--pane", "1", "2", "3"]);
    let mut ok = |rig: &mut Rig, act: Value| {
        let acted = rig.act(&mut ws, act.clone());
        assert_eq!(acted["ok"], true, "{act} → {acted}");
    };
    ok(
        &mut rig,
        json!({"act": "move_cursor", "pane": 1, "delta": -10}),
    );
    assert_eq!(rig.tab.panels[&1].cursor, 0);
    ok(&mut rig, json!({"act": "scroll_h", "pane": 1, "delta": 1}));
    ok(
        &mut rig,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    assert_eq!(rig.tab.panels[&1].cursor_line(), 4);
    ok(&mut rig, json!({"act": "toggle_thread", "pane": 1}));
    assert!(rig.tab.panels[&1].threads[0].collapsed);
    ok(
        &mut rig,
        json!({"act": "move_cursor", "pane": 1, "delta": -10}),
    );
    ok(
        &mut rig,
        json!({"act": "jump_thread", "pane": 1, "next": false}),
    );
    assert_eq!(rig.tab.panels[&1].cursor_line(), 4);
    ok(&mut rig, json!({"act": "toggle_diff", "pane": 1}));
    assert!(rig.tab.panels[&1].diff_view);
    ok(&mut rig, json!({"act": "clear_highlight", "pane": 1}));
    assert_eq!(rig.tab.panels[&1].highlight, None);
    ok(&mut rig, json!({"act": "refresh", "pane": 1}));
    assert_eq!(rig.tab.panels[&1].thread_count(), 0);
    ok(&mut rig, json!({"act": "focus", "pane": 0}));
    assert_eq!(rig.tab.focus, 0);
    ok(&mut rig, json!({"act": "close", "pane": 1}));
    assert!(!rig.tab.panels.contains_key(&1));
    Ok(())
}

#[test]
fn a_diff_row_names_its_change() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file("a.txt", "keep\nold\n", "keep\nnew\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    rig.act(&mut ws, json!({"act": "toggle_diff", "pane": 1}));
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let row = |text: &str| {
        html.split("<div class=\"row")
            .find(|r| r.contains(text))
            .unwrap_or_else(|| panic!("no row with {text}: {html}"))
            .to_string()
    };
    assert!(row("+new").contains(r#"data-diff="add""#), "{html}");
    assert!(row("-old").contains(r#"data-diff="del""#), "{html}");
    assert!(!row("keep").contains("data-diff"), "{html}");
    Ok(())
}

#[test]
fn open_path_with_diff_opens_the_diff_view() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_a, a) = changed_file("a.txt", "keep\nold\n", "keep\nnew\n")?;
    let (_b, b) = changed_file("b.txt", "keep\nold\n", "keep\nnew\n")?;
    let acted = rig.act(
        &mut ws,
        json!({"act": "open_path", "path": a, "diff": true}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    assert!(rig.tab.panels[&1].diff_view);
    let acted = rig.act(&mut ws, json!({"act": "open_path", "path": b}));
    assert_eq!(acted["ok"], true, "{acted}");
    assert!(!rig.tab.panels[&2].diff_view);
    Ok(())
}

#[test]
fn a_code_pane_shows_light_colors() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", RUST)?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    // `fn` in the light keyword red.
    assert!(html.contains("color:#cf222e"), "{html}");
    for nord in NORD {
        assert!(!html.contains(nord), "{nord}: {html}");
    }
    Ok(())
}

#[test]
fn a_highlighted_code_row_keeps_its_colors() -> Result<()> {
    let mut rig = Rig::new()?;
    let (_dir, path) = file("a.rs", RUST)?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    // The cursor goes to line 1, so line 2 is a highlighted row off the cursor.
    rig.drive(&["highlight", "--pane", "1", "1", "2"]);
    let mut ws = rig.connect();
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let row = html
        .split(r#"<div class="row"#)
        .find(|r| r.starts_with(r#" hl" data-line="2""#))
        .expect("highlighted line 2");
    // `let` in the light keyword red, as without the highlight.
    assert!(row.contains("color:#cf222e"), "{row}");
    Ok(())
}

#[test]
fn the_cursor_keeps_the_highlights_band() -> Result<()> {
    let mut rig = Rig::new()?;
    let (_dir, path) = file("a.rs", RUST)?;
    rig.drive(&["open", &path]);
    rig.drive(&["highlight", "--pane", "1", "1", "2"]);
    assert_eq!(rig.tab.focus, 1);
    let mut ws = rig.connect();
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let tag = |line: u32| {
        html.split("<div ")
            .find(|r| r.contains(&format!(r#"data-line="{line}""#)))
            .and_then(|r| r.split('>').next())
            .unwrap_or_else(|| panic!("no row {line}: {html}"))
            .to_string()
    };
    // No inline background on the cursor's row, so the highlight's band shows through.
    let first = tag(1);
    assert!(first.starts_with(r#"class="row hl cur""#), "{first}");
    assert!(!first.contains("background:"), "{first}");
    assert!(tag(2).starts_with(r#"class="row hl""#), "{}", tag(2));
    Ok(())
}

#[test]
fn a_diff_hunk_shows_blue_in_light_colors() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.diff", "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n")?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let hunk = html
        .split(r#"<div class="row"#)
        .find(|r| r.contains("@@"))
        .unwrap_or_else(|| panic!("no hunk row: {html}"));
    assert!(hunk.contains("color:#0550ae"), "{hunk}");
    assert!(!hunk.contains("#cf222e"), "{hunk}");
    Ok(())
}

#[test]
fn the_layout_names_each_panes_icon() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_p, page) = file("p.html", "<p>hi</p>\n")?;
    let (_m, md) = file("m.md", "# hi\n")?;
    rig.drive(&["open", &page]);
    rig.drive(&["open", &md]);
    let out = rig.run(&["tail", "--title", "t"]);
    assert!(out.status.success());
    let layout = rig
        .until(&mut ws, "four panes", |f| {
            matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"].as_array().is_some_and(|p| p.len() == 4))
        })
        .json();
    let icons: Vec<_> = layout["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            (
                p["name"].as_str().unwrap_or("").to_string(),
                p["icon"].clone(),
            )
        })
        .collect();
    let icon = |name: &str| {
        icons
            .iter()
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, i)| i.clone())
    };
    assert_eq!(icon("p.html"), Some(json!("page")), "{icons:?}");
    assert_eq!(icon("m.md"), Some(Value::Null), "{icons:?}");
    assert_eq!(icon("shell"), Some(Value::Null), "{icons:?}");
    assert!(icons.iter().any(|(_, i)| i == "log"), "{icons:?}");
    Ok(())
}

#[test]
fn an_editor_pane_streams_its_terminal_and_refuses_diff() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.editor = Ok(fake_nvim());
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    assert_eq!(rig.drive(&["open", "--edit", &path]), "1");
    let layout = rig
        .until(&mut ws, "editor pane", |f| {
            matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"].as_array().is_some_and(|p| p.len() == 2))
        })
        .json();
    assert_eq!(layout["panes"][1]["kind"], "editor");
    assert_eq!(layout["panes"][1]["pty"], true);
    rig.until(&mut ws, "editor bytes", |f| matches!(f, Frame::Bytes(1, _)));
    let acted = rig.act(&mut ws, json!({"act": "toggle_editor_view", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    // In its file view the pane is drawn as a file pane, and keeps its terminal.
    let layout = rig
        .until(&mut ws, "file view", |f| {
            matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["kind"] == "file")
        })
        .json();
    assert_eq!(layout["panes"][1]["pty"], true);
    let acted = rig.act(&mut ws, json!({"act": "toggle_diff", "pane": 1}));
    assert_eq!(acted["ok"], false);
    assert_eq!(acted["text"], NO_DIFF_IN_EDITOR);
    Ok(())
}

#[test]
fn an_editor_panes_layout_marks_unsaved_edits() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.editor = Ok(fake_nvim());
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", "--edit", &path]);
    let unsaved = |want: bool| move |f: &Frame| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["unsaved"] == want);
    rig.until(&mut ws, "saved", unsaved(false));
    // Neovim's marker for unsaved edits (src/editor.rs `modified`).
    let dirty = std::env::temp_dir().join(format!("{}-1.dirty", rig.tab.socket));
    std::fs::write(&dirty, "")?;
    rig.until(&mut ws, "unsaved", unsaved(true));
    std::fs::remove_file(&dirty)?;
    Ok(())
}

#[test]
fn a_frozen_pane_shows_in_the_layout_without_a_notice() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.tab
        .act(UserAction::Comment {
            pane: 1,
            line: 2,
            text: "x".into(),
        })
        .map_err(|e| anyhow!(e))?;
    pane_frame(&mut rig, &mut ws, 1);
    std::fs::write(&path, "changed on disk\n")?;
    rig.until(
        &mut ws,
        "frozen layout",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["frozen"] == true),
    );
    // The web app shows a banner on the pane in place of the TUI's notice.
    let frames = rig.idle(&mut ws, 10);
    assert!(!frames.iter().any(|f| f.is("notice")));
    Ok(())
}

#[test]
fn a_reload_marks_the_pane_frame() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    // A cursor move rewrites rows, but the file didn't change.
    let acted = rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": 1}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let rows = rig
        .until(
            &mut ws,
            "rows frame",
            |f| matches!(f, Frame::Json(v) if v["t"] == "rows" && v["id"] == 1),
        )
        .json();
    assert_eq!(rows["reloaded"], false, "{rows}");
    std::fs::write(&path, "changed on disk\n")?;
    rig.until(&mut ws, "reloaded frame", |f| {
        matches!(f, Frame::Json(v) if (v["t"] == "pane" || v["t"] == "rows") && v["id"] == 1 && v["reloaded"] == true)
    });
    Ok(())
}

/// A `pane` or `rows` frame for pane `id`.
fn rows_frame(f: &Frame, id: u64) -> bool {
    matches!(f, Frame::Json(v) if (v["t"] == "pane" || v["t"] == "rows") && v["id"] == id)
}

#[test]
fn a_swapped_file_is_not_a_reload() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_a, a) = file("a.rs", &five_lines())?;
    let (_b, b) = file("b.rs", &five_lines().replace("line", "bee"))?;
    rig.drive(&["open", &a]);
    pane_frame(&mut rig, &mut ws, 1);
    rig.drive(&["open", &b, "--panel", "1"]);
    let swapped = rig
        .until(&mut ws, "b's rows", |f| {
            rows_frame(f, 1) && matches!(f, Frame::Json(v) if v.to_string().contains("bee"))
        })
        .json();
    assert_eq!(swapped["reloaded"], false, "{swapped}");
    std::fs::write(&b, "bee changed\n")?;
    rig.until(&mut ws, "b reloaded", |f| {
        rows_frame(f, 1) && matches!(f, Frame::Json(v) if v["reloaded"] == true)
    });
    Ok(())
}

#[test]
fn a_tail_pane_reloads_on_new_output() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    let name = rig.tab.socket.clone();
    let rx = {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let out = Command::cargo_bin("laura")
                .unwrap()
                .args(["tail", "--title", "t"])
                .env("LAURA_TAB", &name)
                .write_stdin("hello\n")
                .output()
                .unwrap();
            let _ = tx.send(out);
        });
        rx
    };
    assert!(rig.wait(&rx).status.success());
    rig.until(&mut ws, "tail rows", |f| rows_frame(f, 1));
    let spool = rig.tab.panels[&1].path.clone();
    let mut out = std::fs::OpenOptions::new().append(true).open(&spool)?;
    writeln!(out, "more output")?;
    drop(out);
    let mut frames = vec![];
    let start = Instant::now();
    while !frames
        .iter()
        .any(|f: &Value| f.to_string().contains("more output"))
    {
        assert!(start.elapsed() < common::DEADLINE, "no new output");
        frames.extend(
            rig.idle(&mut ws, 1)
                .into_iter()
                .filter(|f| rows_frame(f, 1))
                .map(Frame::json),
        );
    }
    assert!(frames.iter().any(|f| f["reloaded"] == true), "{frames:?}");
    Ok(())
}

#[test]
fn a_paused_tail_pane_sends_no_rows() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    let name = rig.tab.socket.clone();
    let rx = {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let out = Command::cargo_bin("laura")
                .unwrap()
                .args(["tail", "--title", "t"])
                .env("LAURA_TAB", &name)
                .write_stdin("hello\n")
                .output()
                .unwrap();
            let _ = tx.send(out);
        });
        rx
    };
    assert!(rig.wait(&rx).status.success());
    rig.until(&mut ws, "tail rows", |f| rows_frame(f, 1));

    let acted = rig.act(&mut ws, json!({"act": "toggle_pause", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    let layout = rig
        .until(
            &mut ws,
            "paused",
            |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["paused"] == true),
        )
        .json();
    assert_eq!(
        layout["panes"][0]["paused"],
        Value::Null,
        "the shell can't pause"
    );

    let spool = rig.tab.panels[&1].path.clone();
    let mut out = std::fs::OpenOptions::new().append(true).open(&spool)?;
    writeln!(out, "more output")?;
    drop(out);
    let frames: Vec<Value> = rig
        .idle(&mut ws, 20)
        .into_iter()
        .filter_map(|f| match f {
            Frame::Json(v) => Some(v),
            _ => None,
        })
        .collect();
    assert!(
        !frames.iter().any(|f| f.to_string().contains("more output")),
        "a paused pane sends no rows: {frames:?}"
    );

    rig.act(&mut ws, json!({"act": "toggle_pause", "pane": 1}));
    rig.until(
        &mut ws,
        "resumed",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["paused"] == false),
    );
    rig.until(&mut ws, "new rows", |f| {
        rows_frame(f, 1) && matches!(f, Frame::Json(v) if v.to_string().contains("more output"))
    });
    Ok(())
}

#[test]
fn a_tail_append_sends_only_the_new_rows() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    let name = rig.tab.socket.clone();
    let input: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    let rx = {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let out = Command::cargo_bin("laura")
                .unwrap()
                .args(["tail", "--title", "t"])
                .env("LAURA_TAB", &name)
                .write_stdin(input)
                .output()
                .unwrap();
            let _ = tx.send(out);
        });
        rx
    };
    assert!(rig.wait(&rx).status.success());
    // The 20 lines can arrive over more than one reload.
    rig.until(&mut ws, "line 20", |f| {
        rows_frame(f, 1) && matches!(f, Frame::Json(v) if v.to_string().contains("line 20"))
    });
    let spool = rig.tab.panels[&1].path.clone();
    let mut out = std::fs::OpenOptions::new().append(true).open(&spool)?;
    writeln!(out, "line 21")?;
    drop(out);
    let f = rig
        .until(&mut ws, "line 21", |f| {
            rows_frame(f, 1) && matches!(f, Frame::Json(v) if v.to_string().contains("line 21"))
        })
        .json();
    assert_eq!(f["t"], "rows", "{f}");
    assert_eq!(f["len"], 21, "{f}");
    let rows = f["rows"].as_array().expect("a rows frame has rows");
    assert_eq!(rows.len(), 1, "{f}");
    assert_eq!(rows[0][0], 20, "{f}");
    Ok(())
}

#[test]
fn a_binary_frame_reaches_the_shell() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    let mut frame = 0u64.to_be_bytes().to_vec();
    frame.extend(b"echo binmark\r");
    ws.send(Message::binary(frame))?;
    rig.shell_shows("binmark");
    Ok(())
}
