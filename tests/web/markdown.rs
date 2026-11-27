//! Markdown file panes: blocks as HTML, sanitizing, highlights and threads in blocks, and the units
//! the cursor steps through.

use super::*;

/// The web app's pane header takes the border's top two rows, so a pane shows its height minus three.
#[test]
fn the_web_border_is_three_rows() -> Result<()> {
    let mut rig = Rig::new()?;
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    let report: Value = serde_json::from_str(&rig.drive(&["layout"]))?;
    for p in report["panes"].as_array().expect("panes") {
        let h = p["rect"]["height"].as_u64().expect("height");
        assert_eq!(p["visible_rows"].as_u64(), Some(h - 3), "{p}");
    }
    let shell = &report["panes"][0]["rect"];
    let size = rig.tab.pty.with_screen(|s| s.size());
    let want = (
        shell["height"].as_u64().unwrap_or(0) as u16 - 3,
        shell["width"].as_u64().unwrap_or(0) as u16 - 2,
    );
    assert_eq!(size, want, "{shell}");
    Ok(())
}

#[test]
fn a_markdown_diff_view_tags_each_row_with_its_source_line() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file(
        "doc.md",
        "# Title\n\nalpha\nbeta\n",
        "# Title\n\nalpha\nbeta CHANGED\n",
    )?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let acted = rig.act(&mut ws, json!({"act": "toggle_diff", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let rows = rows(&html);
    let line_of = |text: &str| {
        rows.iter()
            .find(|(_, t)| t.contains(text))
            .map(|(l, _)| *l)
            .unwrap_or_else(|| panic!("no row with {text}: {rows:?}"))
    };
    assert_eq!(line_of("# Title"), 1);
    assert_eq!(line_of("alpha"), 3);
    assert_eq!(line_of("beta CHANGED"), 4);
    Ok(())
}

#[test]
fn a_markdown_pane_sends_each_block_as_html() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "# Title\n\nalpha\nbeta\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let blocks = blocks(&html);
    let lines: Vec<u32> = blocks.iter().map(|(l, _)| *l).collect();
    assert_eq!(lines, [1, 3, 6, 7, 10], "{html}");
    assert!(blocks[0].1.contains("<h1>Title</h1>"), "{html}");
    assert!(blocks[1].1.contains("alpha\nbeta"), "{html}");
    // A fenced block in a known language is syntax-highlighted.
    assert!(blocks[4].1.contains("<pre style="), "{html}");
    Ok(())
}

#[test]
fn a_comment_on_a_block_lands_on_its_first_line() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# Title\n\nalpha\nbeta\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    // `set_cursor` on a folded paragraph's second line lands on its first.
    let acted = rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let draft = rig.draft();
    assert_eq!(draft["line"], 3, "{draft}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 3, "text": "block note"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let acted = rig.act(&mut ws, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows("L3-4");
    Ok(())
}

#[test]
fn arrow_keys_reach_every_row_of_a_markdown_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "# T\n\n```rust\nfn a() {}\nfn b() {}\n```\n\nafter\n\n",
    )?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let acted = rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    assert_eq!(acted["ok"], true, "{acted}");
    let mut lines = vec![];
    for delta in [1, 1, 1, 1, 1, -1, -1, -1, -1] {
        let acted = rig.act(
            &mut ws,
            json!({"act": "move_cursor", "pane": 1, "delta": delta}),
        );
        assert_eq!(acted["ok"], true, "{acted}");
        rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
        let draft = rig.draft();
        lines.push(draft["line"].clone());
    }
    assert_eq!(lines, [5, 7, 8, 9, 9, 8, 7, 5, 4], "{lines:?}");
    Ok(())
}

#[test]
fn a_highlight_marks_markdown_blocks() -> Result<()> {
    let mut rig = Rig::new()?;
    let (_dir, path) = file("doc.md", "# T\n\nalpha\nbeta\n\nafter\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["highlight", "--pane", "1", "4"]);
    // A view that connects now gets the highlighted pane whole, not as a `rows` diff.
    let mut ws = rig.connect();
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let blocks = blocks(&html);
    let class = |line| {
        let (_, b) = blocks.iter().find(|(l, _)| *l == line).expect("block");
        b.split('"').next().unwrap_or("").to_string()
    };
    assert!(class(3).ends_with(" hl"), "{html}");
    assert_eq!(class(1), " dim", "{html}");
    assert_eq!(class(6), " dim", "{html}");
    Ok(())
}

#[test]
fn a_highlight_inside_a_block_lights_its_number() -> Result<()> {
    let mut rig = Rig::new()?;
    let (_dir, path) = file("doc.md", "# T\n\n```rust\nfn a() {}\nfn b() {}\n```\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["highlight", "--pane", "1", "5"]);
    let mut ws = rig.connect();
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let blocks = blocks(&html);
    let gutter = |line| {
        let (_, b) = blocks.iter().find(|(l, _)| *l == line).expect("block");
        let g = b.split(r#"<span class="g">"#).nth(1).unwrap_or("");
        g.split(r#"<div class="md">"#)
            .next()
            .unwrap_or("")
            .to_string()
    };
    assert!(gutter(4).contains("color:#1d1f24"), "{html}");
    assert!(!gutter(1).contains("color:#1d1f24"), "{html}");
    Ok(())
}

#[test]
fn a_markdown_code_block_shows_light_colors() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", &format!("# T\n\n```rust\n{RUST}```\n"))?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let (_, code) = blocks(&html)
        .into_iter()
        .find(|(_, b)| b.contains("<pre"))
        .expect("a code block");
    assert!(code.contains("color:#"), "{code}");
    for nord in NORD {
        assert!(!code.contains(nord), "{nord}: {code}");
    }
    Ok(())
}

#[test]
fn an_unclosed_fence_keeps_its_trailing_blank_lines_out_of_the_box() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "```\ncode\n\n\n")?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let (_, code) = blocks(&html)
        .into_iter()
        .find(|(_, b)| b.contains("<pre"))
        .expect("a code block");
    assert!(
        code.contains("<span data-line=\"2\">code</span>\n</code></pre>"),
        "{code}"
    );
    // The gap-fill rows show the blank lines.
    assert!(html.contains(r#"data-line="3""#), "{html}");
    assert!(html.contains(r#"data-line="4""#), "{html}");
    Ok(())
}

#[test]
fn footnotes_and_math_read_as_in_the_tui() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "> x[^q]\n>\n> [^q]: foot\n\n$a+b$\n")?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(html.contains("x[q]"), "{html}");
    assert!(html.contains("$a+b$"), "{html}");
    assert!(!html.contains(r#"class="math"#), "{html}");
    Ok(())
}

#[test]
fn a_reference_link_resolves_in_a_markdown_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "- see [x][r] and [y][s]\n\n```\n[r]\n```\n\n\
         [r]: <https://example.com/a b>\n[s]: https://example.com/?b=1&amp;amp;c=2\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let hrefs = |h: &str| -> Vec<String> {
        h.split(r#"href=""#)
            .skip(1)
            .filter_map(|s| s.split('"').next())
            .map(str::to_string)
            .collect()
    };
    // The entity decodes once, as in a render of the whole file.
    assert_eq!(
        hrefs(&html),
        [
            "https://example.com/a%20b",
            "https://example.com/?b=1&amp;amp;c=2"
        ],
        "{html}"
    );
    // The code block shows its own text, not the file's reference definitions.
    assert_eq!(html.matches("https://example.com").count(), 2, "{html}");
    Ok(())
}

#[test]
fn a_thread_on_an_inner_line_shows_under_its_block() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# Title\n\n> one\n>\n> two\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "5", "agent note"]);
    rig.until(
        &mut ws,
        "the thread",
        |f| matches!(f, Frame::Json(v) if v["id"] == 1 && v["threads"][0]["line"] == 5),
    );
    let mut ws = rig.connect();
    let frame = pane_frame(&mut rig, &mut ws, 1);
    assert_eq!(frame["cursor"], 0, "{frame}");
    let html = frame["html"].as_str().unwrap_or("");
    // The browser draws the thread; the rows don't hold it.
    assert!(!html.contains("agent note"), "{html}");
    // The quote's block shows no caret: the browser draws the thread's card.
    assert!(!blocks(html)[1].1.contains('▾'), "{html}");
    let quote = unit_of(html, 3);
    let acted = rig.act(
        &mut ws,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    // The cursor is on the thread's card, below the quote's block.
    rig.until(&mut ws, "the cursor on the card", |f| {
        matches!(f, Frame::Json(v) if (v["t"] == "rows" || v["t"] == "pane") && v["id"] == 1
            && v["card"] == 5 && v["cursor"] == quote)
    });
    let mut ws = rig.connect();
    let frame = pane_frame(&mut rig, &mut ws, 1);
    assert_eq!(frame["card"], 5, "{frame}");
    let html = frame["html"].as_str().unwrap_or("").to_string();
    assert!(!html.contains("agent note"), "{html}");
    let (_, quote) = blocks(&html)
        .into_iter()
        .find(|(l, _)| *l == 3)
        .expect("the quote's block");
    assert!(
        !quote.split('>').next().unwrap_or("").contains(" cur"),
        "{quote}"
    );
    let acted = rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": 1}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    // `↓` in the last block keeps the thread's row.
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let draft = rig.draft();
    assert_eq!(draft["line"], 5, "{draft}");
    // `↑` from the card goes to its block.
    let acted = rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": -1}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let draft = rig.draft();
    assert_eq!(draft["line"], 3, "{draft}");
    Ok(())
}

#[test]
fn a_comment_beside_a_collapsed_thread_goes_on_the_blocks_first_line() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# T\n\n> a\n>\n> b\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["comment", "--pane", "1", "5", "note"]);
    rig.drive(&["ready"]);
    for act in [
        json!({"act": "jump_thread", "pane": 1, "next": true}),
        json!({"act": "toggle_thread", "pane": 1}),
    ] {
        let acted = rig.act(&mut ws, act);
        assert_eq!(acted["ok"], true, "{acted}");
    }
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let draft = rig.draft();
    assert_eq!(draft["line"], 3, "{draft}");
    Ok(())
}

#[test]
fn a_highlight_deep_in_a_block_marks_its_place() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    // L1 opens the fence, L2-41 are code, L44 is a one-line paragraph.
    let code: String = (1..=40).map(|n| format!("c{n}\n")).collect();
    let (_dir, path) = file("doc.md", &format!("```\n{code}```\n\ntail\n"))?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let mut at = |rig: &mut Rig, line: &str| -> f64 {
        rig.drive(&["highlight", line, "--pane", "1"]);
        let frame = rig
            .until(&mut ws, "an hl block", |f| {
                matches!(f, Frame::Json(v) if v["id"] == 1 && v.to_string().contains("data-at"))
            })
            .json()
            .to_string();
        frame
            .split(r#"data-at=\""#)
            .nth(1)
            .and_then(|s| s.split('\\').next())
            .and_then(|s| s.parse().ok())
            .expect("data-at")
    };
    let deep = at(&mut rig, "30");
    assert!((0.6..0.8).contains(&deep), "{deep}");
    assert_eq!(at(&mut rig, "44"), 0.5);
    Ok(())
}

/// The index of the first row with `data-line` `line` among the units (blocks and rows) of a
/// `pane` frame's HTML.
fn unit_of(html: &str, line: u32) -> usize {
    let at = html
        .find(&format!(r#"data-line="{line}""#))
        .expect("a row on the line");
    let head = &html[..at];
    head.matches(r#"<div class="blk"#).count() + head.matches(r#"<div class="row"#).count() - 1
}

#[test]
fn jump_thread_between_threads_in_one_block_moves_to_its_card() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "x\n\n> a\n>\n> b\n>\n> c\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "5", "first"]);
    rig.drive(&["comment", "--pane", "1", "7", "second"]);
    rig.until(&mut ws, "both threads", |f| {
        matches!(f, Frame::Json(v) if v["id"] == 1 && v["threads"].as_array().map(Vec::len) == Some(2))
    });
    let mut ws = rig.connect();
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    // Both cards sit below the quote's block, so the cursor stays on its index.
    let want = unit_of(&html, 3);
    for line in [5, 7] {
        let acted = rig.act(
            &mut ws,
            json!({"act": "jump_thread", "pane": 1, "next": true}),
        );
        assert_eq!(acted["ok"], true, "{acted}");
        rig.until(&mut ws, "the cursor on the card", |f| {
            matches!(f, Frame::Json(v) if (v["t"] == "rows" || v["t"] == "pane") && v["id"] == 1
                && v["card"] == line && v["cursor"] == want)
        });
    }
    Ok(())
}

#[test]
fn r_on_a_block_toggles_only_its_threads() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\nafter\n",
    )?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "3", "t1"]);
    rig.drive(&["comment", "--pane", "1", "4", "t2"]);
    rig.drive(&["comment", "--pane", "1", "6", "out"]);
    pane_frame(&mut rig, &mut ws, 1);
    // A fresh view's pane frame: the table's block, and each thread's body and state.
    let view = |rig: &mut Rig, ws: &mut Ws| -> (String, Vec<(String, bool)>) {
        *ws = rig.connect();
        let frame = pane_frame(rig, ws, 1);
        let threads = frame["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                let body = t["notes"][0]["body"].as_str().unwrap_or("").to_string();
                (body, t["collapsed"] == true)
            })
            .collect();
        let table = blocks(frame["html"].as_str().unwrap_or(""))[0].1.clone();
        (table, threads)
    };
    let toggle = json!({"act": "toggle_thread", "pane": 1});

    // The cursor starts on the table's block.
    rig.act(&mut ws, toggle.clone());
    let (t, collapsed) = view(&mut rig, &mut ws);
    assert_eq!(
        collapsed,
        [
            ("t1".into(), true),
            ("t2".into(), true),
            ("out".into(), false)
        ]
    );
    // A stop with a thread carries no class, and no caret shows.
    assert!(t.contains(r#"<tr data-line="3">"#), "{t}");
    assert!(t.contains(r#"<tr data-line="4">"#), "{t}");
    assert!(!t.contains('▸') && !t.contains('▾'), "{t}");

    rig.act(&mut ws, toggle.clone());
    let (t, collapsed) = view(&mut rig, &mut ws);
    assert!(collapsed.iter().all(|(_, c)| !c));
    assert!(t.contains(r#"<tr data-line="3">"#), "{t}");
    assert!(t.contains(r#"<tr data-line="4">"#), "{t}");

    // On the second table thread's row, `r` toggles only that thread.
    for _ in 0..2 {
        rig.act(
            &mut ws,
            json!({"act": "jump_thread", "pane": 1, "next": true}),
        );
    }
    rig.act(&mut ws, toggle.clone());
    let (t, collapsed) = view(&mut rig, &mut ws);
    assert_eq!(
        collapsed,
        [
            ("t1".into(), false),
            ("t2".into(), true),
            ("out".into(), false)
        ]
    );
    assert!(t.contains(r#"<tr data-line="3">"#), "{t}");
    assert!(t.contains(r#"<tr data-line="4" class="cur">"#), "{t}");

    // On a blank line, `r` collapses every thread, as in the TUI.
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 5}));
    rig.act(&mut ws, toggle);
    assert!(view(&mut rig, &mut ws).1.iter().all(|(_, c)| *c));
    Ok(())
}

#[test]
fn a_comment_in_a_followed_markdown_pane_lands_on_the_blocks_first_line() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# T\n\n> a\n>\n> b\n")?;
    rig.drive(&["open", "--follow", &path]);
    rig.drive(&["ready"]);
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let draft = rig.draft();
    assert_eq!(draft["line"], 3, "{draft}");
    Ok(())
}

#[test]
fn a_thread_on_a_blank_line_shows_in_a_markdown_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# Title\n\nalpha\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "2", "gap note"]);
    let Frame::Json(v) = rig.until(
        &mut ws,
        "the thread",
        |f| matches!(f, Frame::Json(v) if v["id"] == 1 && v["threads"][0]["line"] == 2),
    ) else {
        unreachable!()
    };
    // The browser draws the card from `threads`; the rows hold no copy of it.
    assert!(!v["html"].to_string().contains("gap note"), "{v}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.until(&mut ws, "the cursor on the blank line", |f| {
        matches!(f, Frame::Json(v) if (v["t"] == "rows" || v["t"] == "pane") && v["id"] == 1 && v["cursor"] == 1)
    });
    Ok(())
}

#[test]
fn a_markdown_pane_keeps_the_files_text() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "---\ntitle: x\n---\n\nRun --web \"q\" it's...\n\n> [!NOTE]\n> hi\n\n    indented\n\n[^2]: - item\n\nend\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(html.contains(r#"<span data-line="2">title: x"#), "{html}");
    assert!(html.contains(r#"Run --web "q" it's..."#), "{html}");
    // The alert keeps the TUI's title; indented code stays code.
    assert!(html.contains("Note</strong>"), "{html}");
    assert!(
        html.contains(r#"<pre><code><span data-line="10">indented"#),
        "{html}"
    );
    // A footnote that opens with a list keeps its label out of the next paragraph.
    assert!(html.contains("[2]: "), "{html}");
    assert!(!html.contains("[2]: end"), "{html}");
    Ok(())
}

#[test]
fn a_code_block_that_ends_the_file_shows_its_removed_lines_above_it() -> Result<()> {
    // #99: no row comes after a closing fence on the file's last line.
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file("doc.md", "x\n\n```\na\nb\n```\n", "x\n\n```\na\n```\n")?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let gap = html.find("1 line removed");
    let pre = html.find("<pre").expect("code block");
    assert!(gap.is_some_and(|g| g < pre), "{html}");
    Ok(())
}

#[test]
fn an_end_of_file_deletion_shows_below_the_last_row() -> Result<()> {
    // #142: no line comes after the deleted ones, so the gap goes below the last row.
    let mut rig = Rig::new()?;
    let (_dir, txt) = changed_file("f.txt", "one\ntwo\nthree\n", "one\ntwo\n")?;
    let (_dir, md) = changed_file("doc.md", "one\n\ntwo\n\nthree\n", "one\n\ntwo\n")?;
    rig.drive(&["open", &txt]);
    rig.drive(&["open", &md]);
    rig.drive(&["comment", "--pane", "2", "3", "agent note"]);
    // A fresh view per pane: each view replays every pane, and `pane_frame` drops the other pane's.
    let html = |rig: &mut Rig, id| {
        let mut ws = rig.connect();
        pane_frame(rig, &mut ws, id)["html"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };

    let txt_html = html(&mut rig, 1);
    let rows = rows(&txt_html);
    let at = |s: &str| {
        rows.iter()
            .position(|(_, t)| t.contains(s))
            .unwrap_or_else(|| panic!("no {s}: {rows:?}"))
    };
    assert!(at("two") < at("1 line removed"), "{rows:?}");
    // The removed-lines row carries the last line's `data-line`; `gap` keeps a thread's tint off it.
    let class = |s: &str| {
        let row = txt_html
            .split(r#"<div class="row"#)
            .find(|r| r.contains(s))
            .unwrap_or_else(|| panic!("no {s}: {txt_html}"));
        row[..row.find('"').unwrap_or(0)].to_string()
    };
    assert_eq!(class("1 line removed"), " gap");
    assert!(!class("two").contains("gap"), "{txt_html}");

    let html = html(&mut rig, 2);
    let at = |s: &str| html.find(s).unwrap_or_else(|| panic!("no {s}: {html}"));
    assert!(at("<p>two") < at("2 lines removed"), "{html}");
    // The browser draws the card from `threads`; the rows hold no copy of it.
    assert!(!html.contains('╭'), "{html}");
    Ok(())
}

#[test]
fn a_markdown_block_shows_its_removed_lines_above_it() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file(
        "doc.md",
        "# T\n\na\nb\nc\nd\n\n```rust\nx\ny\nz\n```\n",
        "# T\n\nA\nb\nd\n\n```rust\nx\nz\n```\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    // One row above the modified paragraph, one above the code block it was inside.
    let gaps: Vec<usize> = html
        .match_indices("1 line removed")
        .map(|(i, _)| i)
        .collect();
    let at = |s: &str| html.find(s).unwrap_or_else(|| panic!("no {s}: {html}"));
    assert_eq!(gaps.len(), 2, "{html}");
    assert!(gaps[0] < at("<p>A") && at("<p>A") < gaps[1], "{html}");
    assert!(gaps[1] < at("<pre"), "{html}");
    // A block's `data-row` can't equal a row's, so copy keeps them on separate lines.
    assert!(
        html.split(r#"<div class="blk"#).skip(1).all(|b| b
            .split('>')
            .next()
            .unwrap_or("")
            .contains(r#"data-row="b"#)),
        "{html}"
    );
    Ok(())
}

#[test]
fn arrow_up_from_an_inner_row_leaves_the_block() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# T\n\n> a\n>\n> b\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    // The highlight puts the cursor on the quote's last row, which isn't a stop.
    rig.drive(&["highlight", "--pane", "1", "5"]);
    let acted = rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": -1}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let draft = rig.draft();
    assert_eq!(draft["line"], 2, "{draft}");
    Ok(())
}

#[test]
fn a_code_line_is_its_own_unit() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# T\n\n```rust\na\nb\nc\n```\n\nafter\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    for line in 4..=6 {
        let span = format!(r#"<span data-line="{line}">"#);
        assert!(blocks(&html)[1].1.contains(&span), "{html}");
    }
    let acted = rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    assert_eq!(acted["ok"], true, "{acted}");
    let mut lines = vec![];
    for delta in [1, 1, 1, -1] {
        rig.act(
            &mut ws,
            json!({"act": "move_cursor", "pane": 1, "delta": delta}),
        );
        rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
        let draft = rig.draft();
        lines.push(draft["line"].clone());
    }
    assert_eq!(lines, [5, 6, 8, 6], "{lines:?}");
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 5}));
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let draft = rig.draft();
    assert_eq!(draft["line"], 5, "{draft}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 5, "text": "line note"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 1}));
    rig.act(
        &mut ws,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    // The cursor shows on the code line, not on the block.
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let code = &blocks(&html)[1].1;
    assert!(
        code.contains(r#"<span data-line="5" class="cur">"#),
        "{code}"
    );
    assert!(
        !code.split('>').next().unwrap_or("").contains(" cur"),
        "{code}"
    );
    // The line keeps its cursor while another pane has focus, so the browser doesn't scroll.
    // A new view takes over from the old one and gets every pane fresh.
    rig.act(&mut view, json!({"act": "focus", "pane": 0}));
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        blocks(&html)[1]
            .1
            .contains(r#"<span data-line="5" class="cur">"#),
        "{html}"
    );
    let acted = rig.act(&mut view, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows("L5  b");
    Ok(())
}

#[test]
fn a_table_row_is_its_own_unit() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "| a | b |\n|---|:-:|\n| 1 | 2 |\n| 3 | 4 |\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let table = &blocks(&html)[0].1;
    let head = table.split("<tbody>").next().unwrap_or("");
    assert!(!head.contains("<tr data-line"), "{table}");
    assert!(table.contains(r#"<tr data-line="3">"#), "{table}");
    // The second row keeps the column's alignment.
    assert!(
        table.contains(r#"<tr data-line="4"><td>3</td><td style="text-align: center">4</td>"#),
        "{table}"
    );
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let draft = rig.draft();
    assert_eq!(draft["line"], 4, "{draft}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 4, "text": "row note"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let acted = rig.act(&mut ws, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows("L4  | 3 | 4 |");
    Ok(())
}

#[test]
fn a_nested_item_is_its_own_unit() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "- a\n  - b\n\n- c\n  - d\n    e\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "2", "note"]);
    rig.until(
        &mut ws,
        "the thread",
        |f| matches!(f, Frame::Json(v) if v["id"] == 1 && v["threads"][0]["line"] == 2),
    );
    let mut ws = rig.connect();
    let frame = pane_frame(&mut rig, &mut ws, 1);
    let html = frame["html"].as_str().unwrap_or("");
    // The item carries no class, and no caret shows.
    assert!(html.contains(r#"<li data-line="2">"#), "{html}");
    assert!(!html.contains('▾'), "{html}");
    // A hand-wrapped item renders on one row in the TUI, so it has no line of its own.
    assert!(!html.contains(r#"<li data-line="5""#), "{html}");
    rig.act(
        &mut ws,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(html.contains(r#"<li data-line="2" class="cur">"#), "{html}");
    rig.act(&mut view, json!({"act": "toggle_thread", "pane": 1}));
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(html.contains(r#"<li data-line="2" class="cur">"#), "{html}");
    Ok(())
}
