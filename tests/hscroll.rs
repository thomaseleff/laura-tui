//! Horizontal scroll for pre-formatted content: code / diffs / markdown code fences, tables, and
//! HTML blocks clip to a horizontal window and scroll sideways; prose still word-wraps. Asserted on
//! layout row structure (through `Panel::open` + `Panel::layout` + `Panel::scroll_h`), not pixels.

use std::io::Write;

use anyhow::Result;
use laura::Panel;
use tempfile::Builder;

fn open(suffix: &str, body: &str) -> Result<Panel> {
    let mut file = Builder::new().suffix(suffix).tempfile()?;
    write!(file, "{body}")?;
    file.flush()?;
    let panel = Panel::open(file.path().to_str().unwrap().to_string());
    drop(file);
    Ok(panel)
}

/// Visual row count for the source line whose plain text contains `needle`.
fn rows_for(panel: &Panel, width: usize, needle: &str) -> usize {
    let idx = panel
        .content
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no source line contains {needle:?}"));
    panel
        .layout(width)
        .rows
        .iter()
        .filter(|r| r.line == idx)
        .count()
}

#[test]
fn code_long_line_clips_scrolls_and_marks() -> Result<()> {
    // A code line wider than the panel stays one row (no wrap), scrolls, and shows the `›` clip mark.
    let long = "let x = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\";";
    let panel = open(".rs", &format!("fn main() {{\n    {long}\n}}\n"))?;

    assert_eq!(
        rows_for(&panel, 24, "aaaa"),
        1,
        "a wide code line must clip to one row, not wrap"
    );

    // Un-scrolled: content starts at column 0 and is clipped on the right.
    let idx = panel
        .content
        .lines()
        .position(|l| l.contains("aaaa"))
        .unwrap();
    let row = |p: &Panel| {
        p.layout(24)
            .rows
            .iter()
            .find(|r| r.line == idx)
            .expect("the wide line's row")
            .text()
    };
    assert!(
        row(&panel).contains('›'),
        "a clipped line marks the right margin with ›: {:?}",
        row(&panel)
    );
    assert!(
        row(&panel).starts_with("    let x"),
        "un-scrolled window starts at column 0: {:?}",
        row(&panel)
    );

    // Scrolling advances the window: the head chars leave, the tail comes in.
    let mut p = panel;
    for _ in 0..12 {
        p.scroll_h(1);
    }
    assert!(p.h_offset > 0, "scroll_h advances the offset");
    assert!(
        !row(&p).starts_with("    let x"),
        "after scrolling, the left of the line has moved off: {:?}",
        row(&p)
    );

    Ok(())
}

#[test]
fn markdown_fence_scrolls_prose_wraps() -> Result<()> {
    // A fenced code line stays one row (pre-formatted); a long prose line above it wraps.
    let md = "This is ordinary prose that is quite long and must wrap across a narrow panel width.\n\n```rust\nlet very_long_variable_name = some_function_call(argument_one, argument_two);\n```\n";
    let panel = open(".md", md)?;

    assert_eq!(
        rows_for(&panel, 30, "very_long_variable_name"),
        1,
        "a fenced code line clips to one row"
    );
    assert!(
        rows_for(&panel, 30, "ordinary prose") > 1,
        "long prose still wraps"
    );
    Ok(())
}

#[test]
fn markdown_table_row_stays_one_row() -> Result<()> {
    // A table row (box-drawing border) is pre-formatted: one row, alignment preserved in the window.
    let md = "| fruit | qty |\n|-------|-----|\n| apple | 3 |\n";
    let panel = open(".md", md)?;
    assert_eq!(
        rows_for(&panel, 30, "apple"),
        1,
        "a table row clips to one row rather than wrapping"
    );
    Ok(())
}

#[test]
fn inline_code_in_prose_still_wraps() -> Result<()> {
    // Guards the "all non-blank spans" rule: one inline `code` span must not flip the line to nowrap.
    let md =
        "Prose with an `inline` code span and enough trailing words to force a wrap here now.\n";
    let panel = open(".md", md)?;
    assert!(
        rows_for(&panel, 24, "Prose with") > 1,
        "a prose line with inline code still wraps"
    );
    Ok(())
}

#[test]
fn alert_block_stays_wrapped() -> Result<()> {
    // Guards the two-sided has-bg invariant: alerts are fg-only, so a long alert body wraps.
    let md =
        "> [!NOTE]\n> This is an alert body long enough that it must wrap across a narrow panel.\n";
    let panel = open(".md", md)?;
    assert!(
        rows_for(&panel, 24, "alert body") > 1,
        "an alert body is prose and must wrap, not clip"
    );
    Ok(())
}

#[test]
fn html_block_stays_one_row() -> Result<()> {
    // An HTML block line (DIM) is pre-formatted: one row.
    let md = "<div class=\"card\" data-attr=\"a very long attribute value that would wrap\">\ncontent\n</div>\n";
    let panel = open(".md", md)?;
    assert_eq!(
        rows_for(&panel, 24, "<div"),
        1,
        "an HTML block line clips to one row"
    );
    Ok(())
}
