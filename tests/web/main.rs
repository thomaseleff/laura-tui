//! The web app (#88) through its websocket, as a browser tab drives it: the auth gate, the frames a
//! view receives, the user's actions as `act` frames, and `laura --web` end to end. State and
//! frames, not pixels.

#[path = "../common/mod.rs"]
mod common;
#[path = "../common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::Command as StdCommand;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use assert_cmd::Command;
use laura::UserAction;
use laura::tab::{NO_DIFF_IN_EDITOR, NO_ROOM_TO_DOCK};
use serde_json::{Value, json};
use tungstenite::Message;

use rig::*;

mod file_panes;
mod layout;
mod shell;
mod views;

/// `body` as `name` in a fresh dir.
fn file(name: &str, body: &str) -> Result<(tempfile::TempDir, String)> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(name);
    std::fs::write(&path, body)?;
    Ok((dir, path.to_string_lossy().into_owned()))
}

fn five_lines() -> String {
    (1..=5).map(|n| format!("line {n}\n")).collect()
}

/// Run `git <args>` in `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=t@t", "-c", "user.name=t"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git spawns (test host needs git on PATH)");
    assert!(out.status.success(), "git {args:?} failed");
}

/// `name` committed with `committed`, then changed on disk to `now`.
fn changed_file(name: &str, committed: &str, now: &str) -> Result<(tempfile::TempDir, String)> {
    let (dir, path) = file(name, committed)?;
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", name]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    std::fs::write(&path, now)?;
    Ok((dir, path))
}

/// Each row in a `pane` frame's HTML: its `data-line` and its text, tags dropped.
fn rows(html: &str) -> Vec<(u32, String)> {
    html.split(r#"<div class="row"#)
        .skip(1)
        .map(|row| {
            let line = row
                .split(r#"data-line=""#)
                .nth(1)
                .and_then(|s| s.split('"').next())
                .and_then(|s| s.parse().ok())
                .expect("data-line");
            let mut text = String::new();
            let mut tag = false;
            for c in row.chars() {
                match c {
                    '<' => tag = true,
                    '>' => tag = false,
                    c if !tag => text.push(c),
                    _ => {}
                }
            }
            (line, text)
        })
        .collect()
}

/// Each rendered markdown block in a `pane` frame's HTML: its `data-line` and its HTML, up to the
/// next row.
fn blocks(html: &str) -> Vec<(u32, String)> {
    html.split(r#"<div class="blk"#)
        .skip(1)
        .map(|blk| {
            let blk = blk.split(r#"<div class="row"#).next().unwrap_or("");
            let line = blk
                .split(r#"data-line=""#)
                .nth(1)
                .and_then(|s| s.split('"').next())
                .and_then(|s| s.parse().ok())
                .expect("data-line");
            (line, blk.to_string())
        })
        .collect()
}

/// The first `pane` frame for pane `id`.
fn pane_frame(rig: &mut Rig, ws: &mut Ws, id: u64) -> Value {
    rig.until(
        ws,
        "pane frame",
        |f| matches!(f, Frame::Json(v) if v["t"] == "pane" && v["id"] == id),
    )
    .json()
}

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

/// The first `layout` frame that `want` accepts.
fn layout_when(rig: &mut Rig, ws: &mut Ws, what: &str, want: impl Fn(&Value) -> bool) -> Value {
    rig.until(
        ws,
        what,
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && want(v)),
    )
    .json()
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
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 3, "{acted}");
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
        let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
        lines.push(acted["line"].clone());
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

/// The Nord theme's code colors, which the light web app never shows.
const NORD: [&str; 7] = [
    "#81a1c1", "#a3be8c", "#8fbcbb", "#88c0d0", "#b48ead", "#616e88", "#d8dee9",
];

const RUST: &str = "fn main() {\n    let s = \"hi\";\n}\n";

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
    // The quote's block shows the thread's caret.
    assert!(blocks(html)[1].1.contains('▾'), "{html}");
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
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 5, "{acted}");
    // `↑` from the card goes to its block.
    let acted = rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": -1}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 3, "{acted}");
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
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 3, "{acted}");
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
    assert!(t.contains(r#"<tr data-line="3" class="th shut">"#), "{t}");
    assert!(t.contains(r#"<tr data-line="4" class="th shut">"#), "{t}");
    // A stop's caret shows on its row, not in the block's gutter.
    assert!(!t.contains('▸') && !t.contains('▾'), "{t}");

    rig.act(&mut ws, toggle.clone());
    let (t, collapsed) = view(&mut rig, &mut ws);
    assert!(collapsed.iter().all(|(_, c)| !c));
    assert!(t.contains(r#"<tr data-line="3" class="th">"#), "{t}");
    assert!(t.contains(r#"<tr data-line="4" class="th">"#), "{t}");

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
    assert!(t.contains(r#"<tr data-line="3" class="th">"#), "{t}");
    assert!(
        t.contains(r#"<tr data-line="4" class="cur th shut">"#),
        "{t}"
    );

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
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 3, "{acted}");
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
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 2, "{acted}");
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
        let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
        lines.push(acted["line"].clone());
    }
    assert_eq!(lines, [5, 6, 8, 6], "{lines:?}");
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 5}));
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 5, "{acted}");
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
    // The cursor and the thread show on the code line, not on the block.
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let code = &blocks(&html)[1].1;
    assert!(
        code.contains(r#"<span data-line="5" class="cur th">"#),
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
            .contains(r#"<span data-line="5" class="cur th">"#),
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
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 4, "{acted}");
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
    assert!(html.contains(r#"<li data-line="2" class="th">"#), "{html}");
    // The item's caret shows on the item, not in the block's gutter.
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
    assert!(
        html.contains(r#"<li data-line="2" class="cur th">"#),
        "{html}"
    );
    rig.act(&mut view, json!({"act": "toggle_thread", "pane": 1}));
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        html.contains(r#"<li data-line="2" class="cur th shut">"#),
        "{html}"
    );
    Ok(())
}

/// A stand-in editor that stays up and prints `fake` (after a bell on unix); a `--headless` push exits at once.
fn fake_nvim() -> std::path::PathBuf {
    static ONCE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        // Under the target dir: a `.cmd` whose path has a space can't be spawned.
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("web_fake_nvim");
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(windows)]
        let (name, body) = (
            "nvim.cmd",
            "@echo off\r\nif \"%~1\"==\"--headless\" exit /b 0\r\necho fake\r\nping -n 30 127.0.0.1 >nul\r\n",
        );
        #[cfg(not(windows))]
        let (name, body) = (
            "nvim",
            "#!/bin/sh\n[ \"$1\" = --headless ] && exit 0\nprintf '\\007'\necho fake\nsleep 30\n",
        );
        let f = dir.join(name);
        std::fs::write(&f, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        f
    })
    .clone()
}

/// `GET path` without the token; the raw response.
fn get(port: u16, path: &str) -> Result<String> {
    get_as(port, path, "")
}

/// [`get`] with `headers`, each line ending in `\r\n`.
fn get_as(port: u16, path: &str, headers: &str) -> Result<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{headers}\r\n")?;
    let mut resp = vec![];
    s.read_to_end(&mut resp)?;
    Ok(String::from_utf8_lossy(&resp).into_owned())
}

/// A fresh dir under the crate's `tmp/`, below the folder the tests run in, which Laura serves from.
fn served_dir() -> Result<tempfile::TempDir> {
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp");
    std::fs::create_dir_all(&tmp)?;
    Ok(tempfile::Builder::new().prefix("web").tempdir_in(tmp)?)
}

/// `docs/a.md` holding `md`, beside `img/s.png`, in a [`served_dir`].
fn image_doc(md: &str) -> Result<(tempfile::TempDir, String)> {
    let dir = served_dir()?;
    std::fs::create_dir_all(dir.path().join("docs"))?;
    std::fs::create_dir_all(dir.path().join("img"))?;
    std::fs::write(dir.path().join("img/s.png"), "PNGBYTES")?;
    let path = dir.path().join("docs/a.md");
    std::fs::write(&path, md)?;
    Ok((dir, path.to_string_lossy().into_owned()))
}

/// Pane `id`'s first frame's HTML, and its first `<img>`'s `src`.
fn img_src(rig: &mut Rig, ws: &mut Ws, id: u64) -> (String, String) {
    let html = pane_frame(rig, ws, id)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let src = html
        .split(r#"<img src=""#)
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap_or("")
        .to_string();
    (html, src)
}

#[test]
fn a_markdown_image_loads_through_the_file_route() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) =
        image_doc("![a\n<b>shot $$x$$](../img/s.png)\n\n![x](https://e.com/x.png)\n\na\0b\n")?;
    rig.drive(&["open", &path]);
    let (html, src) = img_src(&mut rig, &mut ws, 1);
    assert!(src.starts_with("/f/"), "{html}");
    assert!(src.ends_with("/docs/../img/s.png"), "{html}");
    assert!(!src.contains("t="), "{html}");
    assert!(html.contains(r#"alt="a &lt;b&gt;shot $$x$$""#), "{html}");
    // A remote image shows its alt text; a NUL in the file can't pose as the route.
    assert_eq!(html.matches("<img").count(), 1, "{html}");
    assert!(html.contains("<p>x</p>"), "{html}");
    assert!(html.contains("a\u{FFFD}b"), "{html}");
    let resp = get(rig.port, &src.replace("/docs/../", "/"))?;
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.contains("Content-Type: image/png\r\n"), "{resp}");
    assert!(
        resp.contains("X-Content-Type-Options: nosniff\r\n"),
        "{resp}"
    );
    assert!(
        resp.contains("Content-Security-Policy: sandbox\r\n"),
        "{resp}"
    );
    assert!(resp.ends_with("\r\n\r\nPNGBYTES"), "{resp}");
    Ok(())
}

#[test]
fn an_image_name_with_a_percent_sign_loads() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("![p](../img/100%.png)\n")?;
    std::fs::write(dir.path().join("img/100%.png"), "PCTBYTES")?;
    rig.drive(&["open", &path]);
    let (html, src) = img_src(&mut rig, &mut ws, 1);
    let resp = get(rig.port, &src.replace("/docs/../", "/"))?;
    assert!(resp.starts_with("HTTP/1.1 200"), "{html}\n{resp}");
    assert!(resp.ends_with("\r\n\r\nPCTBYTES"), "{resp}");
    Ok(())
}

#[test]
fn the_file_route_serves_only_images_in_the_root() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("![shot](../img/s.png)\n")?;
    std::fs::create_dir_all(dir.path().join(".git"))?;
    for f in ["x.png", ".x.png", ".git/x.png", "notes.txt", "x.css"] {
        std::fs::write(dir.path().join(f), "x")?;
    }
    rig.drive(&["open", &path]);
    let (html, src) = img_src(&mut rig, &mut ws, 1);
    // `/f/<nonce>/<the dir below the root>/`
    let base = src
        .strip_suffix("docs/../img/s.png")
        .expect(&html)
        .to_string();
    let port = rig.port;
    let status = |p: &str| get(port, &format!("{base}{p}")).map(|r| r[..12].to_string());
    assert_eq!(status("x.png")?, "HTTP/1.1 200");
    // Without the checks, each of these would reach a file.
    for p in [
        ".x.png",
        ".git/x.png",
        "docs/%2e%2e/x.png",
        "docs/..%2fx.png",
        "notes.txt",
        // A markdown pane's folder serves images only; a page's serves its stylesheets too.
        "x.css",
    ] {
        assert_eq!(status(p)?, "HTTP/1.1 404", "{p}");
    }
    let n = base.split('/').nth(2).expect("a nonce");
    let other = base.replacen(n, &"0".repeat(n.len()), 1);
    assert!(get(rig.port, &format!("{other}x.png"))?.starts_with("HTTP/1.1 404"));
    #[cfg(unix)]
    {
        let out = tempfile::NamedTempFile::with_suffix(".png")?;
        std::os::unix::fs::symlink(out.path(), dir.path().join("link.png"))?;
        assert_eq!(status("link.png")?, "HTTP/1.1 404");
        std::os::unix::fs::symlink(".git/x.png", dir.path().join("hid.png"))?;
        std::os::unix::fs::symlink("notes.txt", dir.path().join("txt.png"))?;
        for p in ["hid.png", "txt.png"] {
            assert_eq!(status(p)?, "HTTP/1.1 404", "{p}");
        }
    }
    rig.drive(&["close", "1"]);
    rig.idle(&mut ws, 2);
    assert_eq!(status("x.png")?, "HTTP/1.1 404");
    Ok(())
}

#[test]
fn a_markdown_file_in_a_hidden_folder_shows_images_beside_it() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, _) = image_doc("")?;
    std::fs::create_dir_all(dir.path().join(".hid"))?;
    std::fs::write(dir.path().join(".hid/s.png"), "PNGBYTES")?;
    let path = dir.path().join(".hid/a.md");
    std::fs::write(&path, "![s](s.png)\n")?;
    rig.drive(&["open", &path.to_string_lossy()]);
    let (html, src) = img_src(&mut rig, &mut ws, 1);
    // `/f/<nonce>/s.png`: the root is the file's own folder.
    assert_eq!(src.split('/').count(), 4, "{html}");
    assert!(get(rig.port, &src)?.ends_with("\r\n\r\nPNGBYTES"), "{html}");
    Ok(())
}

/// A page with each case Laura's tag scan must get right.
const PAGE: &str = "<!DOCTYPE html>
<html>
<body>
<button id=\"b\" title=\"a>b <i>\">Go</button>
<script>let s = \"<div>\";</script>
<!-- <p> -->
<p>a < b</p><DIV>x</DIV>
<xmp><b>x</b></xmp>
<p title=it's>x</p><em>y</em>
<!--><span>z</span>
</body>
</html>
";

/// The first `page` frame for pane `id` that `want` accepts.
fn page_frame(rig: &mut Rig, ws: &mut Ws, id: u64, want: impl Fn(&Value) -> bool) -> Value {
    rig.until(
        ws,
        "page frame",
        |f| matches!(f, Frame::Json(v) if v["t"] == "page" && v["id"] == id && want(v)),
    )
    .json()
}

/// A hint's `[keystroke, label]` pairs as one line: `key label · label`.
fn pairs(v: &Value) -> String {
    let items: Vec<String> = v
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let (k, l) = (p[0].as_str().unwrap_or(""), p[1].as_str().unwrap_or(""));
            if k.is_empty() {
                l.to_string()
            } else {
                format!("{k} {l}")
            }
        })
        .collect();
    items.join(" · ")
}

/// A `GET`'s body.
fn body(resp: &str) -> &str {
    resp.split_once("\r\n\r\n").map_or("", |(_, b)| b)
}

#[test]
fn an_html_pane_shows_its_page() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file("page.html", "<p>old</p>\n", PAGE)?;
    rig.drive(&["open", &path]);
    // A page doesn't take the arrows, so the hint line doesn't list them; rows do.
    let hint = |arrows: bool| {
        move |f: &Frame| {
            matches!(f, Frame::Json(v) if v["t"] == "layout" && v["focus"] == 1
                && pairs(&v["hints"]["pane"]).contains(if arrows { "↑ ↓" } else { "Click an element" }))
        }
    };
    rig.until(&mut ws, "hint without arrows", hint(false));
    let frame = page_frame(&mut rig, &mut ws, 1, |_| true);
    let url = frame["page"].as_str().unwrap_or("");
    assert!(url.starts_with("/f/"), "{frame}");
    assert!(url.contains("/page.html?v="), "{frame}");
    assert!(!url.contains("t="), "{frame}");
    let resp = get(rig.port, url)?;
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.contains("Content-Type: text/html"), "{resp}");
    assert!(
        resp.contains(
            "Content-Security-Policy: sandbox allow-scripts allow-forms; frame-ancestors 'self'\r\n"
        ),
        "{resp}"
    );
    assert!(
        resp.contains("Access-Control-Allow-Origin: *\r\n"),
        "{resp}"
    );
    let html = body(&resp);
    // The doctype stays first, so the page isn't in quirks mode.
    assert!(html.starts_with("<!DOCTYPE html>\n"), "{html}");
    for want in [
        r#"<html data-laura-line="2">"#,
        r#"<button data-laura-line="4" id="b" title="a>b <i>">"#,
        r#"<script data-laura-line="5">let s = "<div>";"#,
        "<!-- <p> -->",
        r#"<p data-laura-line="7">a < b</p><DIV data-laura-line="7">"#,
        r#"<em data-laura-line="9">"#,
        r#"<span data-laura-line="10">"#,
    ] {
        assert!(html.contains(want), "{want}\n{html}");
    }
    for not in [
        "<i data-laura-line",
        "<div data-laura-line",
        r#"<p data-laura-line="6""#,
        "<b data-laura-line",
    ] {
        assert!(!html.contains(not), "{not}\n{html}");
    }
    // `d` shows the diff as rows, and `d` again shows the page.
    let toggle = json!({"act": "toggle_diff", "pane": 1});
    rig.act(&mut ws, toggle.clone());
    // Laura sends `layout` before the pane's frame.
    rig.until(&mut ws, "hint with arrows", hint(true));
    pane_frame(&mut rig, &mut ws, 1);
    rig.act(&mut ws, toggle);
    page_frame(&mut rig, &mut ws, 1, |_| true);
    Ok(())
}

#[test]
fn a_page_keeps_lauras_script_before_unclosed_text() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let dir = tempfile::tempdir()?;
    for (i, (page, tail)) in [
        ("<p>a</p>\n<!-- x\n", "<!-- x\n"),
        (
            "<p>a</p>\n<plaintext>x\n",
            "<plaintext data-laura-line=\"2\">x\n",
        ),
        ("<p>a</p>\n<style>x\n", "<style data-laura-line=\"2\">x\n"),
        (
            "<p>a</p>\n<noscript>x\n",
            "<noscript data-laura-line=\"2\">x\n",
        ),
        (
            "<p>a</p>\n<p title=\"x>\n",
            "<p data-laura-line=\"2\" title=\"x>\n",
        ),
        (
            "<p>a</p>\n<template>x\n",
            "<template data-laura-line=\"2\">x\n",
        ),
        (
            "<p>a</p>\n<template><template>y</template>x\n",
            "<template data-laura-line=\"2\"><template>y</template>x\n",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("page{i}.html");
        let path = dir.path().join(&name);
        std::fs::write(&path, page)?;
        let path = path.to_string_lossy();
        if i == 0 {
            rig.drive(&["open", &path]);
        } else {
            rig.drive(&["open", &path, "--panel", "1"]);
        }
        let frame = page_frame(&mut rig, &mut ws, 1, |v| {
            v["page"]
                .as_str()
                .is_some_and(|u| u.contains(&format!("/{name}?")))
        });
        let resp = get(rig.port, frame["page"].as_str().unwrap_or(""))?;
        let html = body(&resp);
        assert!(html.ends_with(tail), "{page}\n{html}");
        let before = &html[..html.len() - tail.len()];
        assert!(before.contains("data-laura-cur"), "{page}\n{html}");
    }
    Ok(())
}

#[test]
fn a_missing_html_file_shows_rows_with_the_arrows() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("missing.html");
    rig.drive(&["open", &path.to_string_lossy()]);
    // No route, so the pane shows rows, which take the arrows.
    rig.until(&mut ws, "hint with arrows", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "layout" && v["focus"] == 1
            && pairs(&v["hints"]["pane"]).contains("↑ ↓"))
    });
    pane_frame(&mut rig, &mut ws, 1);
    Ok(())
}

#[test]
fn a_highlight_marks_a_page() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file("page.html", "<p>old</p>\n", PAGE)?;
    rig.drive(&["open", &path]);
    rig.drive(&["highlight", "--pane", "1", "4"]);
    page_frame(&mut rig, &mut ws, 1, |v| v["highlight"] == json!([4, 4]));
    // The page scrolls to the highlight, so `d` doesn't jump to it again.
    let toggle = json!({"act": "toggle_diff", "pane": 1});
    rig.act(&mut ws, toggle.clone());
    let frame = pane_frame(&mut rig, &mut ws, 1);
    assert_eq!(frame["recenter"], false, "{frame}");
    rig.act(&mut ws, toggle);
    page_frame(&mut rig, &mut ws, 1, |_| true);
    rig.drive(&["highlight", "--off", "--pane", "1"]);
    page_frame(&mut rig, &mut ws, 1, |v| v["highlight"].is_null());
    Ok(())
}

#[test]
fn a_page_loads_its_own_assets() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let dir = served_dir()?;
    std::fs::create_dir_all(dir.path().join("site/js"))?;
    for f in [
        "site/index.html",
        "site/style.css",
        "site/js/app.mjs",
        "site/notes.txt",
    ] {
        std::fs::write(dir.path().join(f), "x")?;
    }
    rig.drive(&[
        "open",
        &dir.path().join("site/index.html").to_string_lossy(),
    ]);
    let frame = page_frame(&mut rig, &mut ws, 1, |_| true);
    let base = frame["page"]
        .as_str()
        .and_then(|u| u.split("index.html").next())
        .unwrap_or("")
        .to_string();
    let port = rig.port;
    let resp = |p: &str| get(port, &format!("{base}{p}"));
    let css = resp("style.css")?;
    assert!(css.starts_with("HTTP/1.1 200"), "{css}");
    assert!(css.contains("Content-Type: text/css\r\n"), "{css}");
    let js = resp("js/app.mjs")?;
    assert!(js.starts_with("HTTP/1.1 200"), "{js}");
    assert!(js.contains("Content-Type: text/javascript\r\n"), "{js}");
    assert!(js.contains("Access-Control-Allow-Origin: *\r\n"), "{js}");
    // A page's route serves only its folder, so no URL reaches the folder above it.
    assert_eq!(base.matches('/').count(), 3, "{base}");
    let txt = resp("notes.txt")?;
    assert!(txt.starts_with("HTTP/1.1 404"), "{txt}");
    // A page's `/style.css` comes from its opaque origin, so it doesn't get the workspace's.
    let cross = "Sec-Fetch-Site: cross-site\r\n";
    let sub = get_as(
        port,
        "/style.css",
        &format!("{cross}Sec-Fetch-Mode: no-cors\r\n"),
    )?;
    assert!(sub.starts_with("HTTP/1.1 404"), "{sub}");
    let nav = get_as(port, "/", &format!("{cross}Sec-Fetch-Mode: navigate\r\n"))?;
    assert!(nav.starts_with("HTTP/1.1 200"), "{nav}");
    let own = get(port, "/style.css")?;
    assert!(own.starts_with("HTTP/1.1 200"), "{own}");
    Ok(())
}

#[test]
fn the_fonts_are_served() -> Result<()> {
    let rig = Rig::new()?;
    for font in ["/fonts/Inter.woff2", "/fonts/JetBrainsMono.woff2"] {
        let resp = get(rig.port, font)?;
        assert!(resp.starts_with("HTTP/1.1 200"), "{font}");
        assert!(resp.contains("Content-Type: font/woff2\r\n"), "{font}");
        assert!(body(&resp).starts_with("wOF2"), "{font}");
    }
    Ok(())
}

#[test]
fn the_web_app_shows_the_laura_logo() -> Result<()> {
    let rig = Rig::new()?;
    let logo = get(rig.port, "/logo.svg")?;
    assert!(logo.starts_with("HTTP/1.1 200"), "{logo}");
    assert!(logo.contains("Content-Type: image/svg+xml\r\n"), "{logo}");
    assert!(body(&logo).starts_with("<svg"), "{logo}");
    // The tab icon is the same mark, white in dark mode.
    let icon = get(rig.port, "/favicon.svg")?;
    assert!(icon.starts_with("HTTP/1.1 200"), "{icon}");
    assert!(icon.contains("Content-Type: image/svg+xml\r\n"), "{icon}");
    assert!(body(&icon).contains("prefers-color-scheme: dark"), "{icon}");
    let old = get(rig.port, "/laura.ico")?;
    assert!(old.starts_with("HTTP/1.1 404"), "{old}");
    let page = get(rig.port, "/")?;
    assert!(page.contains("<link rel=\"icon\" href=\"favicon.svg\" type=\"image/svg+xml\">"));
    assert!(page.contains("src=\"logo.svg\" alt=\"Laura\" width=\"17\" height=\"17\""));
    Ok(())
}

#[test]
fn a_frozen_page_serves_its_snapshot() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("page.html", PAGE)?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 4, "text": "q"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let url = page_frame(&mut rig, &mut ws, 1, |v| v["threads"][0]["line"] == 4)["page"]
        .as_str()
        .unwrap_or("")
        .to_string();
    std::fs::write(&path, "<p>new</p>\n")?;
    rig.until(
        &mut ws,
        "frozen layout",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["frozen"] == true),
    );
    let old = get(rig.port, &url)?;
    assert!(
        body(&old).contains(r#"<button data-laura-line="4""#),
        "{old}"
    );
    rig.act(&mut ws, json!({"act": "refresh", "pane": 1}));
    let frame = page_frame(&mut rig, &mut ws, 1, |v| v["page"] != url.as_str());
    let new = get(rig.port, frame["page"].as_str().unwrap_or(""))?;
    assert!(
        body(&new).starts_with(r#"<p data-laura-line="1">new</p>"#),
        "{new}"
    );
    Ok(())
}

#[test]
fn a_comment_on_an_element_quotes_its_tag() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("page.html", PAGE)?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    // A click on the button reports its tag's line.
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 4, "{acted}");
    rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 4, "text": "q"}),
    );
    let frame = page_frame(&mut rig, &mut ws, 1, |v| v["threads"][0]["line"] == 4);
    assert_eq!(frame["cursor"], 4, "{frame}");
    assert_eq!(frame["threads"][0]["notes"][0]["body"], "q", "{frame}");
    assert_eq!(frame["card"], 4, "{frame}");
    rig.act(&mut ws, json!({"act": "begin_review", "pane": 1}));
    let acted = rig.act(&mut ws, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows(r#"L4  <button id="b""#);
    Ok(())
}

#[test]
fn a_thread_names_its_block() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, md) = file("doc.md", "# Title\n\nSome prose.\n")?;
    rig.drive(&["open", &md]);
    rig.drive(&["comment", "--pane", "1", "3", "on prose"]);
    rig.until(
        &mut ws,
        "the paragraph's label",
        |f| matches!(f, Frame::Json(v) if v["id"] == 1 && v["threads"][0]["label"] == "paragraph"),
    );
    let (_dir2, page) = file("page.html", PAGE)?;
    rig.drive(&["open", &page]);
    rig.drive(&["comment", "--pane", "2", "4", "on the button"]);
    page_frame(&mut rig, &mut ws, 2, |v| {
        v["threads"][0]["label"] == "<button#b>"
    });
    // An attribute after a tab; `data-id` isn't `id`.
    let (_dir3, tabbed) = file("tabbed.html", "<p data-id=\"x\"\tclass=\"c\">x</p>\n")?;
    rig.drive(&["open", &tabbed]);
    rig.drive(&["comment", "--pane", "3", "1", "on the paragraph"]);
    page_frame(&mut rig, &mut ws, 3, |v| {
        v["threads"][0]["label"] == "<p.c>"
    });
    Ok(())
}

#[test]
fn a_silent_socket_does_not_block_a_second_connection() -> Result<()> {
    let mut rig = Rig::new()?;
    let _silent = TcpStream::connect(("127.0.0.1", rig.port))?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    Ok(())
}

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
    let hint = pairs(&acted["hint"]);
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
    let hint = pairs(&acted["hint"]);
    assert!(hint.contains("empty deletes"), "{hint}");
    let acted = rig.act(&mut ws, json!({"act": "begin_review", "pane": 1}));
    let hint = pairs(&acted["hint"]);
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
    assert!(rig.tab.hold);
    let open = rig.start(&["open", &b]);
    for _ in 0..20 {
        rig.pump();
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(rig.tab.focus, 1);
    assert!(!rig.tab.panels.contains_key(&2));
    assert!(open.try_recv().is_err(), "the agent's open didn't wait");
    send(&mut ws, json!({"hold": false}));
    assert!(rig.wait(&open).status.success());
    assert!(rig.tab.panels.contains_key(&2));
    Ok(())
}

#[test]
fn a_takeover_or_a_closed_view_releases_the_hold() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut first = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.until(&mut first, "layout", |f| f.is("layout"));
    send(&mut first, json!({"hold": true}));
    rig.idle(&mut first, 5);
    assert!(rig.tab.hold);
    let highlight = rig.start(&["highlight", "--pane", "1", "2"]);
    let mut second = rig.connect();
    rig.until(&mut second, "layout", |f| f.is("layout"));
    assert!(rig.wait(&highlight).status.success());
    send(&mut second, json!({"hold": true}));
    rig.idle(&mut second, 5);
    assert!(rig.tab.hold);
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

#[test]
fn the_shell_has_the_browser_url() -> Result<()> {
    isolate_home();
    let mut rig = Rig::host(laura::Tab::spawn_web(shell(), 24, 80, Ok("chrome".into()))?)?;
    let v: Value = serde_json::from_str(&rig.drive(&["ready"]))?;
    assert_eq!(v["experimental"], json!([]), "{v}");
    let url = v["browser_url"]
        .as_str()
        .expect("a browser URL")
        .to_string();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    let echo = if cfg!(windows) {
        "echo %LAURA_BROWSER_URL%\r"
    } else {
        "echo $LAURA_BROWSER_URL\r"
    };
    rig.tab.pty.write(echo.as_bytes())?;
    // The typed line names the variable; only the echo shows the URL.
    rig.shell_shows(&url);
    Ok(())
}

#[test]
fn the_strings_carry_the_root_path() -> Result<()> {
    isolate_home();
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let (_child, mut ws) = laura_web(dir.path(), &["--web"])?;
    let start = Instant::now();
    let strings = loop {
        if let Some(f) = read(&mut ws).filter(|f| f.is("strings")) {
            break f.json();
        }
        assert!(start.elapsed() < common::DEADLINE, "no strings");
    };
    let name = dir.path().file_name().expect("a named folder");
    assert_eq!(
        strings["root"],
        format!("~/{}", name.to_string_lossy()),
        "{strings}"
    );
    Ok(())
}

#[test]
fn the_strings_carry_the_web_failed_text() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let strings = rig.until(&mut ws, "strings", |f| f.is("strings")).json();
    // The web app's review popover names the keystrokes on its buttons, so its text names none.
    assert_eq!(
        strings["draft"]["failed"],
        "⚠ Laura did not submit the inline review, but kept its threads and review body."
    );
    Ok(())
}

#[test]
fn laura_web_serves_a_workspace_until_the_user_quits() -> Result<()> {
    let (mut child, mut ws) = laura_web(Path::new(env!("CARGO_MANIFEST_DIR")), &["--web"])?;
    let start = Instant::now();
    while !read(&mut ws).is_some_and(|f| f.is("layout")) {
        assert!(start.elapsed() < common::DEADLINE, "no layout");
    }
    send(&mut ws, json!({"quit": true}));
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.0.try_wait()? {
            break s;
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "laura --web kept running"
        );
        let _ = read(&mut ws);
    };
    assert!(status.success(), "{status}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn plain_laura_serves_the_web_app_and_opens_no_browser_from_a_pipe() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let stub = dir.path().join("browser.sh");
    std::fs::write(&stub, "#!/bin/sh\ntouch \"$(dirname \"$0\")/opened\"\n")?;
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))?;
    let (_child, mut ws) = laura_web(dir.path(), &[])?;
    let start = Instant::now();
    while !read(&mut ws).is_some_and(|f| f.is("layout")) {
        assert!(start.elapsed() < common::DEADLINE, "no layout");
    }
    // Nothing to wait for: the stub would run right after Laura prints the URL.
    thread::sleep(Duration::from_millis(500));
    assert!(
        !dir.path().join("opened").exists(),
        "Laura opened a browser"
    );
    Ok(())
}

/// Kills the stub browser whose pid is in `0`, also when an assert fails.
#[cfg(unix)]
struct KillPid(std::path::PathBuf);

#[cfg(unix)]
impl Drop for KillPid {
    fn drop(&mut self) {
        if let Ok(pid) = std::fs::read_to_string(&self.0) {
            let _ = StdCommand::new("kill").args(["-9", pid.trim()]).status();
        }
    }
}

/// Laura in a terminal: the process, the terminal and the terminal's output.
#[cfg(unix)]
type Terminal = (
    Box<dyn portable_pty::Child + Send + Sync>,
    Box<dyn portable_pty::MasterPty + Send>,
    mpsc::Receiver<Vec<u8>>,
);

/// `laura -- /bin/sh` in a terminal in `dir`, with `LAURA_BROWSER` = `stub`. See `laura_cmd`.
#[cfg(unix)]
fn laura_in_a_terminal(dir: &Path, stub: &str) -> Result<Terminal> {
    in_a_terminal(laura_cmd(dir, stub)?)
}

/// `laura -- /bin/sh` in `dir`, with `LAURA_BROWSER` = `stub`, as a first launch: `HOME` and
/// `TMPDIR` are `dir` and no runtime env is set, so the per-OS runtime folder, where Laura writes
/// the page that it opens the browser at, doesn't exist yet.
#[cfg(unix)]
fn laura_cmd(dir: &Path, stub: &str) -> Result<portable_pty::CommandBuilder> {
    use std::os::unix::fs::PermissionsExt;

    isolate_home();
    let browser = dir.join("browser.sh");
    std::fs::write(&browser, format!("#!/bin/sh\n{stub}"))?;
    std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o755))?;
    let mut cmd = portable_pty::CommandBuilder::new(env!("CARGO_BIN_EXE_laura"));
    cmd.args(["--", "/bin/sh"]);
    cmd.cwd(dir);
    cmd.env("HOME", dir);
    cmd.env("TMPDIR", dir);
    cmd.env("LAURA_BROWSER", &browser);
    cmd.env_remove("LAURA_RUNTIME");
    cmd.env_remove("XDG_RUNTIME_DIR");
    cmd.env_remove("LAURA_TAB");
    Ok(cmd)
}

#[cfg(unix)]
fn in_a_terminal(cmd: portable_pty::CommandBuilder) -> Result<Terminal> {
    let pty = portable_pty::native_pty_system().openpty(portable_pty::PtySize {
        rows: 24,
        cols: 200,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let laura = pty.slave.spawn_command(cmd)?;
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader()?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = reader.read(&mut buf) {
            let _ = tx.send(buf[..n].to_vec());
        }
    });
    Ok((laura, pty.master, rx))
}

#[cfg(unix)]
#[test]
fn plain_laura_in_a_terminal_opens_the_browser_and_keeps_it_on_ctrl_c() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    // The traps go in before `args`, so the test sends Ctrl+C only once they catch it.
    let (mut laura, master, rx) = laura_in_a_terminal(
        dir.path(),
        "d=\"$(dirname \"$0\")\"\necho $$ > \"$d/pid\"\n\
         trap 'echo INT >> \"$d/signals\"' INT\ntrap 'echo HUP >> \"$d/signals\"' HUP\n\
         echo \"$@\" > \"$d/args.tmp\" && mv \"$d/args.tmp\" \"$d/args\"\n\
         while :; do sleep 1; done\n",
    )?;
    let _stub = KillPid(dir.path().join("pid"));
    let mut out = String::new();
    let start = Instant::now();
    let url = loop {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        if let Some(line) = out.split('\n').find(|l| l.contains("/?t="))
            && line.ends_with('\r')
        {
            break line.trim().to_string();
        }
        assert!(start.elapsed() < common::DEADLINE, "no URL: {out:?}");
    };
    let token = port_token(&url).1;
    let args = dir.path().join("args");
    while !args.exists() {
        assert!(
            start.elapsed() < common::DEADLINE,
            "Laura opened no browser"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let args = std::fs::read_to_string(&args)?;
    let page = std::path::PathBuf::from(
        args.trim()
            .strip_prefix("file://")
            .unwrap_or_else(|| panic!("not a file page: {args}")),
    );
    assert!(page.starts_with(dir.path()), "{page:?}");
    assert!(
        page.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("open-")),
        "{page:?}"
    );
    assert!(
        !args.contains(&token),
        "the token is on the command line: {args}"
    );
    assert!(std::fs::read_to_string(&page)?.contains(&url));
    assert_eq!(
        std::fs::metadata(&page)?.permissions().mode() & 0o777,
        0o600,
        "other users can read the page"
    );
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    // Nothing to wait for: a signal would reach the stub right away.
    thread::sleep(Duration::from_millis(500));
    assert!(
        !dir.path().join("signals").exists(),
        "the browser got a signal from the terminal"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_browser_that_fails_shows_why() -> Result<()> {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let (mut laura, master, rx) = laura_in_a_terminal(dir.path(), "exit 3\n")?;
    let mut out = String::new();
    let start = Instant::now();
    while !out.contains("exited with") {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(start.elapsed() < common::DEADLINE, "no message: {out:?}");
    }
    assert!(
        out.contains("Laura could not open a web browser:"),
        "{out:?}"
    );
    while !out.contains("laura --tui") {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(start.elapsed() < common::DEADLINE, "no TUI hint: {out:?}");
    }
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn a_snap_browser_reads_the_page_from_its_own_folder() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    // `/snap/bin/chromium` links to `/usr/bin/snap`, which runs the snap that the link names.
    let snap = dir.path().join("usr/bin/snap");
    std::fs::create_dir_all(snap.parent().expect("a folder"))?;
    std::fs::write(
        &snap,
        "#!/bin/sh\nd=\"$(dirname \"$0\")\"\n\
         echo \"$@\" > \"$d/args.tmp\" && mv \"$d/args.tmp\" \"$d/args\"\n",
    )?;
    std::fs::set_permissions(&snap, std::fs::Permissions::from_mode(0o755))?;
    let chromium = dir.path().join("bin/chromium");
    std::fs::create_dir_all(chromium.parent().expect("a folder"))?;
    std::os::unix::fs::symlink(&snap, &chromium)?;
    let mut cmd = laura_cmd(dir.path(), "")?;
    cmd.env("LAURA_BROWSER", &chromium);
    let (mut laura, master, rx) = in_a_terminal(cmd)?;
    let args = dir.path().join("bin/args");
    let start = Instant::now();
    let mut out = String::new();
    while !args.exists() {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(50)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "Laura opened no browser: {out:?}"
        );
    }
    let args = std::fs::read_to_string(&args)?;
    let page = std::path::PathBuf::from(
        args.trim()
            .strip_prefix("file://")
            .unwrap_or_else(|| panic!("not a file page: {args}")),
    );
    assert_eq!(
        page.parent(),
        Some(dir.path().join("snap/chromium/common").as_path()),
        "{page:?}"
    );
    assert!(
        page.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("open-")),
        "{page:?}"
    );
    assert!(std::fs::read_to_string(&page)?.contains("/?t="));
    assert_eq!(
        std::fs::metadata(&page)?.permissions().mode() & 0o777,
        0o600,
        "other users can read the page"
    );
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn laura_opens_the_default_browser_without_laura_browser() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin)?;
    let opener = bin.join(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    });
    std::fs::write(
        &opener,
        "#!/bin/sh\nd=\"$(dirname \"$0\")\"\n\
         echo \"$@\" > \"$d/args.tmp\" && mv \"$d/args.tmp\" \"$d/args\"\n",
    )?;
    std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755))?;
    let mut cmd = laura_cmd(dir.path(), "")?;
    cmd.env_remove("LAURA_BROWSER");
    cmd.env_remove("WSL_DISTRO_NAME");
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![bin.clone()];
    dirs.extend(std::env::split_paths(&path));
    cmd.env("PATH", std::env::join_paths(dirs)?);
    let (mut laura, master, rx) = in_a_terminal(cmd)?;
    let args = bin.join("args");
    let start = Instant::now();
    let mut out = String::new();
    while !args.exists() {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(50)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "Laura opened no browser: {out:?}"
        );
    }
    let page = std::path::PathBuf::from(std::fs::read_to_string(&args)?.trim());
    assert!(
        page.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("open-") && n.ends_with(".html")),
        "{page:?}"
    );
    let html = std::fs::read_to_string(&page)?;
    assert!(html.contains("/?t="), "{html}");
    assert!(
        !page.to_string_lossy().contains("t="),
        "the token is on the command line: {page:?}"
    );
    assert_eq!(
        std::fs::metadata(&page)?.permissions().mode() & 0o777,
        0o600,
        "other users can read the page"
    );
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_browser_that_fails_later_shows_nothing() -> Result<()> {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    // A Chrome that opened the window, and then exited with an error when the user closed it.
    let (mut laura, master, rx) = laura_in_a_terminal(dir.path(), "sleep 6; exit 3\n")?;
    let mut out = String::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(7) {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
    }
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    assert!(!out.contains("could not open a web browser"), "{out:?}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_missing_browser_names_the_program_and_the_tui() -> Result<()> {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin)?;
    // No LAURA_BROWSER and an empty PATH: the OS opener is missing.
    let mut cmd = laura_cmd(dir.path(), "")?;
    cmd.env_remove("LAURA_BROWSER");
    cmd.env_remove("WSL_DISTRO_NAME");
    cmd.env("PATH", &bin);
    let (mut laura, master, rx) = in_a_terminal(cmd)?;
    let opener = if cfg!(target_os = "macos") {
        "open: "
    } else {
        "xdg-open: "
    };
    let mut out = String::new();
    let start = Instant::now();
    while !(out.contains(opener) && out.contains("laura --tui")) {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(start.elapsed() < common::DEADLINE, "no message: {out:?}");
    }
    master.take_writer()?.write_all(b"\x03")?;
    while laura.try_wait()?.is_none() {
        assert!(start.elapsed() < common::DEADLINE, "Laura kept running");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[test]
fn the_open_page_redirects_to_the_workspace_until_a_view_connects() -> Result<()> {
    let mut rig = Rig::new()?;
    let dir = tempfile::tempdir()?;
    let page = rig.web.open_page(dir.path())?;
    assert!(
        std::fs::read_to_string(&page)?.contains(rig.web.url()),
        "the page doesn't redirect to the workspace"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&page)?.permissions().mode() & 0o777,
            0o600,
            "other users can read the page"
        );
    }
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    assert!(!page.exists(), "the page outlived the view's connection");
    Ok(())
}

#[test]
fn a_relative_link_opens_its_file() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[b](sub/b.md#second-part)\n")?;
    std::fs::create_dir_all(dir.path().join("docs/sub"))?;
    std::fs::write(
        dir.path().join("docs/sub/b.md"),
        "# B\n\ntext\n\n## Second part\n\n![s](../../img/s.png)\n",
    )?;
    std::fs::write(dir.path().join("c.md"), "# C\n")?;
    let ready: Value = serde_json::from_str(&rig.drive(&["ready"]))?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "sub/b.md#second-part"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&2].cursor_line(), 5);
    let journal = std::fs::read_to_string(ready["journal"].as_str().unwrap_or(""))?;
    assert!(
        journal
            .lines()
            .any(|l| l.contains(r#""type":"open""#) && l.contains("b.md")),
        "{journal}"
    );
    let (html, src) = img_src(&mut rig, &mut ws, 2);
    // As the browser resolves the `..`s.
    let resp = get(rig.port, &src.replace("/docs/sub/../../", "/"))?;
    assert!(resp.starts_with("HTTP/1.1 200"), "{html}\n{resp}");
    // The same link again, with a query, moves to pane 2 instead of opening another.
    rig.tab.focus = 1;
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "sub/b.md?v=2#second-part"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert!(!rig.tab.panels.contains_key(&3));
    assert_eq!(rig.tab.focus, 2);
    // A `../` link names the file as `laura open` does.
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 2, "href": "../../c.md"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        rig.tab.panels[&3].path,
        std::path::absolute(dir.path().join("docs/sub/../../c.md"))?.to_string_lossy()
    );
    Ok(())
}

#[test]
fn a_heading_link_moves_the_cursor() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) =
        image_doc("# A\n\n[s](#setup)\n\n## Setup\n\nrun it\n\n## Two {#Install}\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#setup"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&1].cursor_line(), 5);
    rig.tab
        .panels
        .get_mut(&1)
        .expect("pane 1 is open")
        .set_cursor_line(1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#Setup"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&1].cursor_line(), 5);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#install"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&1].cursor_line(), 9);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#nope"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    rig.until(&mut ws, "heading notice", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "notice" && v["pane"] == 1
            && v["text"] == "⚠ The file a.md has no heading #nope.")
    });
    Ok(())
}

#[test]
fn a_link_to_a_missing_file_shows_a_notice() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = image_doc("[n](nope.md)\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "nope.md"}),
    );
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(
        r["text"], "⚠ The link target nope.md does not exist.",
        "{r}"
    );
    assert!(!rig.tab.panels.contains_key(&2));
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "../img/"}),
    );
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(r["text"], "⚠ The link target ../img/ is a folder.", "{r}");
    assert!(!rig.tab.panels.contains_key(&2));
    // A rooted path to a real file: the full path on Unix, drive-less on Windows.
    let rooted = &path[path.find(['/', '\\']).unwrap_or(0)..];
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": rooted}),
    );
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(
        r["text"],
        format!("⚠ The link target {rooted} does not exist."),
        "{r}"
    );
    assert!(!rig.tab.panels.contains_key(&2));
    Ok(())
}

#[test]
fn a_link_in_a_full_workspace_shows_a_notice() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[x](x.md)\n")?;
    std::fs::write(dir.path().join("docs/x.md"), "x\n")?;
    let y = dir.path().join("y.txt");
    std::fs::write(&y, "y\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let mut stderr = String::new();
    for n in 0..40 {
        let f = dir.path().join(format!("f{n}.txt"));
        std::fs::write(&f, "f\n")?;
        let out = rig.run(&["open", &f.to_string_lossy()]);
        if !out.status.success() {
            stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            break;
        }
    }
    // The agent keeps its own text.
    assert!(stderr.contains("lower --ratio"), "{stderr}");
    let panes = rig.tab.panels.len();
    let no_room = "⚠ No room for another pane. Close a pane, then click the link again.";
    let r = rig.act(
        &mut ws,
        json!({"act": "open_path", "pane": 0, "path": y.to_string_lossy()}),
    );
    assert_eq!(
        (r["ok"].clone(), r["text"].clone()),
        (json!(false), json!(no_room))
    );
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "x.md"}),
    );
    assert_eq!(
        (r["ok"].clone(), r["text"].clone()),
        (json!(false), json!(no_room))
    );
    assert_eq!(rig.tab.panels.len(), panes);
    Ok(())
}

#[test]
fn a_link_to_a_file_in_an_editor_pane_focuses_it() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.editor = Ok(fake_nvim());
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[b](b.md#two)\n")?;
    let b = dir.path().join("docs/b.md");
    std::fs::write(&b, "# B\n\n## Two\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["open", "--edit", &b.to_string_lossy()]);
    pane_frame(&mut rig, &mut ws, 1);
    rig.tab.focus = 1;
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "b.md#two"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.focus, 2);
    assert_eq!(rig.tab.panels[&2].cursor_line(), 3);
    Ok(())
}

#[test]
fn a_missing_heading_in_another_pane_shows_its_notice_there() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[b](b.md#nope)\n")?;
    let b = dir.path().join("docs/b.md");
    std::fs::write(&b, "# B\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["open", &b.to_string_lossy()]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "b.md#nope"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.focus, 2);
    rig.until(&mut ws, "heading notice in pane 2", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "notice" && v["pane"] == 2
            && v["text"] == "⚠ The file b.md has no heading #nope.")
    });
    Ok(())
}
