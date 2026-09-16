//! Bug #32: markdown panel line numbers match the *source* file. A hand-wrapped paragraph
//! collapses N source lines into one rendered row, and gutter/`highlight`/`L<n>` all key off the
//! source line, not the rendered index. State through the real seam (binary → socket → `Tab::drain`)
//! for the `highlight` verb; direct `Panel` for the review-payload format, per CLAUDE.md.

use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Panel, Rect, Tab};

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a real tab, draining (and replying) until it exits; return trimmed stdout.
fn drive(tab: &mut Tab, args: &[&str]) -> String {
    let name = tab.socket.clone();
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let out = Command::cargo_bin("laura")
            .unwrap()
            .args(&a)
            .env("LAURA_TAB", &name)
            .output()
            .unwrap();
        tx.send(out).unwrap();
    });
    let start = Instant::now();
    let out = loop {
        tab.drain(area());
        if let Ok(out) = rx.try_recv() {
            break out;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    assert!(
        out.status.success(),
        "laura {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A doc whose L3-L4 paragraph collapses to one rendered row:
/// ```text
/// # Title         (L1)
///                 (L2)
/// para line one   (L3)
/// para line two   (L4)
///                 (L5)
/// ## Next         (L6)
/// ```
const DOC: &str = "# Title\n\npara line one\npara line two\n\n## Next\n";

fn write_md() -> Result<(tempfile::NamedTempFile, String)> {
    let mut f = tempfile::Builder::new().suffix(".md").tempfile()?;
    write!(f, "{DOC}")?;
    f.flush()?;
    let p = f.path().to_str().unwrap().to_string();
    Ok((f, p))
}

#[test]
fn highlight_maps_source_line_into_the_collapsed_paragraph_row() -> Result<()> {
    let (_f, p) = write_md()?;
    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &p, "--no-focus"]).parse()?;
    let id_s = id.to_string();

    // The paragraph's rendered row: both source lines joined onto one row, gutter = source L3.
    let para_line = {
        let layout = tab.panels[&id].layout(80);
        let para = layout
            .rows
            .iter()
            .find(|r| r.text().contains("para line one"))
            .expect("paragraph row");
        assert!(
            para.text().contains("para line two"),
            "hand-wrapped paragraph collapses to one row: {:?}",
            para.text()
        );
        assert_eq!(
            para.gutter,
            Some(3),
            "gutter shows the paragraph's source line"
        );
        para.line
    };

    // Source L4 (mid-paragraph) and L3 both land on that one rendered row — the block is the unit.
    drive(&mut tab, &["highlight", "4", "--pane", &id_s]);
    assert_eq!(tab.panels[&id].highlight, Some((para_line, para_line)));
    drive(&mut tab, &["highlight", "3", "--pane", &id_s]);
    assert_eq!(tab.panels[&id].highlight, Some((para_line, para_line)));

    // Span endpoints stay addressable by their own source line.
    let gutter_of = |tab: &Tab, needle: &str| {
        tab.panels[&id]
            .layout(80)
            .rows
            .iter()
            .find(|r| r.text().contains(needle))
            .and_then(|r| r.gutter)
    };
    drive(&mut tab, &["highlight", "1", "--pane", &id_s]);
    let (lo, _) = tab.panels[&id].highlight.unwrap();
    assert_eq!(gutter_of(&tab, "Title"), Some(1));
    assert_eq!(tab.panels[&id].layout(80).rows[lo].line, lo);

    drive(&mut tab, &["highlight", "6", "--pane", &id_s]);
    assert_eq!(
        gutter_of(&tab, "Next"),
        Some(6),
        "last heading keeps source L6"
    );
    Ok(())
}

#[test]
fn review_header_is_the_blocks_source_range() -> Result<()> {
    let (_f, p) = write_md()?;
    let mut panel = Panel::open(p);

    // Pin a comment on the collapsed paragraph row, then assemble the review.
    let para_line = panel
        .layout(80)
        .rows
        .iter()
        .find(|r| r.text().contains("para line one"))
        .expect("paragraph row")
        .line;
    panel.move_cursor(para_line as isize);
    panel.add_comment("tighten this".into());

    let review = panel.assemble_review("");
    assert!(
        review.contains("L3-4"),
        "collapsed block emits its source range, not a single line: {review}"
    );
    assert!(
        !review.contains("L3  "),
        "single-line header would be wrong here: {review}"
    );
    Ok(())
}

/// A fenced code block renders one row per source line, so each row owns its own real line:
/// ```text
/// # Title       (L1)
///               (L2)
/// ```rust       (L3)  fence open (hidden)
/// let a = 1;    (L4)
/// let b = 2;    (L5)
/// let c = 3;    (L6)
/// ```           (L7)  fence close (hidden)
///               (L8)
/// ## Next       (L9)
/// ```
const CODE_DOC: &str = "# Title\n\n```rust\nlet a = 1;\nlet b = 2;\nlet c = 3;\n```\n\n## Next\n";

fn write_doc(doc: &str) -> Result<(tempfile::NamedTempFile, String)> {
    let mut f = tempfile::Builder::new().suffix(".md").tempfile()?;
    write!(f, "{doc}")?;
    f.flush()?;
    let p = f.path().to_str().unwrap().to_string();
    Ok((f, p))
}

#[test]
fn fenced_code_maps_per_line() -> Result<()> {
    let (_f, p) = write_doc(CODE_DOC)?;
    let mut panel = Panel::open(p);

    let gutter_of = |panel: &Panel, needle: &str| {
        panel
            .layout(80)
            .rows
            .iter()
            .find(|r| r.text().contains(needle))
            .and_then(|r| r.gutter)
    };
    // Each content row's gutter is its real source line, not the fence line.
    assert_eq!(gutter_of(&panel, "let a = 1;"), Some(4));
    assert_eq!(gutter_of(&panel, "let b = 2;"), Some(5));
    assert_eq!(gutter_of(&panel, "let c = 3;"), Some(6));

    // A comment on the middle line emits a single L<n>, not a block range.
    let mid = panel
        .layout(80)
        .rows
        .iter()
        .find(|r| r.text().contains("let b = 2;"))
        .expect("code row")
        .line;
    panel.move_cursor(mid as isize);
    panel.add_comment("this one".into());
    let review = panel.assemble_review("");
    assert!(review.contains("L5  "), "single source line: {review}");
    assert!(!review.contains("L4-"), "not a block range: {review}");
    Ok(())
}

#[test]
fn html_block_maps_per_line() -> Result<()> {
    // <div>(L3) <span>x</span>(L4) </div>(L5) — no fence, offset 0.
    let doc = "# Title\n\n<div>\n<span>x</span>\n</div>\n\n## Next\n";
    let (_f, p) = write_doc(doc)?;
    let mut panel = Panel::open(p);

    let gutter_of = |panel: &Panel, needle: &str| {
        panel
            .layout(80)
            .rows
            .iter()
            .find(|r| r.text().contains(needle))
            .and_then(|r| r.gutter)
    };
    assert_eq!(gutter_of(&panel, "<div>"), Some(3));
    assert_eq!(gutter_of(&panel, "<span>x</span>"), Some(4));
    assert_eq!(gutter_of(&panel, "</div>"), Some(5));

    let mid = panel
        .layout(80)
        .rows
        .iter()
        .find(|r| r.text().contains("<span>x</span>"))
        .expect("html row")
        .line;
    panel.move_cursor(mid as isize);
    panel.add_comment("nested".into());
    let review = panel.assemble_review("");
    assert!(review.contains("L4  "), "single source line: {review}");
    assert!(!review.contains("L3-"), "not a block range: {review}");
    Ok(())
}

#[test]
fn table_stays_whole_block() -> Result<()> {
    // tui-markdown injects synthetic border rows, so a table is deliberately NOT 1:1: every rendered
    // row shares the table's source range, and a comment emits `L<a>-<b>`.
    let doc = "# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n## Next\n";
    let (_f, p) = write_doc(doc)?;
    let mut panel = Panel::open(p);

    // Every table row (border or content) shares one gutter — the table's first source line (L3).
    let gutters: Vec<Option<usize>> = panel
        .layout(80)
        .rows
        .iter()
        .filter(|r| {
            let t = r.text();
            t.contains('a') || t.contains('1') || t.contains('3') || t.contains('─')
        })
        .filter(|r| !r.text().contains("Title") && !r.text().contains("Next"))
        .map(|r| r.gutter)
        .collect();
    assert!(!gutters.is_empty(), "found table rows");
    assert!(
        gutters.iter().all(|g| *g == Some(3)),
        "table rows share the block's first source line: {gutters:?}"
    );

    let first = panel
        .layout(80)
        .rows
        .iter()
        .find(|r| r.gutter == Some(3))
        .expect("table row")
        .line;
    panel.move_cursor(first as isize);
    panel.add_comment("fix header".into());
    let review = panel.assemble_review("");
    assert!(
        review.contains("L3-6"),
        "table emits its whole source range: {review}"
    );
    Ok(())
}

#[test]
fn blank_line_before_heading_survives() -> Result<()> {
    // #39: a list block's byte range swallows its trailing blank line, which used to
    // consume the row for the blank before the next heading. It must get its own row.
    let (_f, p) = write_doc("- a\n- b\n\n## H\n")?;
    let panel = Panel::open(p);
    let rows = panel.layout(80).rows;
    let b = rows
        .iter()
        .position(|r| r.text().contains('b'))
        .expect("list row b");
    let h = rows
        .iter()
        .position(|r| r.text().contains('H'))
        .expect("heading row");
    assert!(
        rows[b + 1..h].iter().any(|r| r.text().trim().is_empty()),
        "a blank row sits between the list and the heading: {:?}",
        rows.iter().map(|r| r.text()).collect::<Vec<_>>()
    );
    Ok(())
}

/// #47: highlight into a fenced block must reverse-video the code rows, not a rendered row well
/// below. The distinct-from-#32 case is *reflow before the block* — prose that collapses several
/// source lines into fewer rendered rows, so rendered-row-index ≠ source-line at the fence.
/// L1 `# Title`, L2 blank, L3-8 one long paragraph (reflows), L9 blank,
/// L10 ```` ```rust ````, L11 `let a = 1;`, L12 `let b = 2;`, L13 ```` ``` ````.
const REFLOW_CODE_DOC: &str = "# Title\n\nword word word word word word word word word word word\nword word word word word word word word word word word\nword word word word word word word word word word word\nword word word word word word word word word word word\nword word word word word word word word word word word\nword word word word word word word word word word word\n\n```rust\nlet a = 1;\nlet b = 2;\n```\n";

#[test]
fn highlight_fenced_block_lands_on_code_rows_after_reflow() -> Result<()> {
    let (_f, p) = write_doc(REFLOW_CODE_DOC)?;
    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &p, "--no-focus"]).parse()?;
    let id_s = id.to_string();

    // Highlight source lines 11..=12 (the two `let …;` lines).
    drive(&mut tab, &["highlight", "11", "12", "--pane", &id_s]);
    let panel = &tab.panels[&id];
    let (lo, hi) = panel.highlight.expect("highlight set");

    // The highlighted styled-lines are the code rows — by text, not by index.
    let layout = panel.layout(80);
    let text = |line: usize| layout.rows.iter().find(|r| r.line == line).unwrap().text();
    assert!(
        text(lo).contains("let a = 1;"),
        "highlight lo row is the first code line, got {:?}",
        text(lo)
    );
    assert!(
        text(hi).contains("let b = 2;"),
        "highlight hi row is the second code line, got {:?}",
        text(hi)
    );
    Ok(())
}

#[test]
fn code_files_keep_identity_line_mapping() -> Result<()> {
    // A .rs file has no collapsing: highlight 4 → rendered row 3, gutter 4. Non-markdown untouched.
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile()?;
    for i in 1..=6 {
        writeln!(f, "let x{i} = {i};")?;
    }
    f.flush()?;
    let p = f.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &p, "--no-focus"]).parse()?;
    drive(&mut tab, &["highlight", "4", "--pane", &id.to_string()]);
    assert_eq!(
        tab.panels[&id].highlight,
        Some((3, 3)),
        "identity source→row"
    );

    let row = tab.panels[&id]
        .layout(80)
        .rows
        .into_iter()
        .find(|r| r.line == 3)
        .unwrap();
    assert_eq!(
        row.gutter,
        Some(4),
        "gutter equals the source line for code"
    );
    Ok(())
}
