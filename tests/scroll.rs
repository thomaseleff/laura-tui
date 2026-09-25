//! #69: viewport stability. The view stays put while the cursor moves and scrolls only when the
//! cursor line leaves it. The render loop can't run in CI, so this asserts through the public
//! `scroll_offset` seam that render and copy share.

use std::io::Write;

use anyhow::Result;
use laura::Panel;

const VIEW: usize = 20;

/// An `n`-line `.txt` outside any repo (no diff gutter rows).
fn fixture(n: usize) -> Result<(tempfile::NamedTempFile, Panel)> {
    let mut f = tempfile::Builder::new().suffix(".txt").tempfile()?;
    for i in 1..=n {
        writeln!(f, "line {i}")?;
    }
    let panel = Panel::open(f.path().to_str().unwrap().to_string());
    Ok((f, panel))
}

/// Step the cursor `steps` times by `dir` (±1), checking after each step that the view only moved
/// in the cursor's direction, held still while the cursor line was already in view, and shows the
/// cursor line's first row.
fn trace(panel: &mut Panel, dir: isize, steps: usize) {
    let mut prev = panel.scroll_offset(&panel.layout(80), VIEW);
    for _ in 0..steps {
        panel.move_cursor(dir);
        let layout = panel.layout(80);
        let off = panel.scroll_offset(&layout, VIEW);
        let c = panel.cursor;
        if dir > 0 {
            assert!(
                off >= prev,
                "moving down scrolled up at L{c}: {prev} → {off}"
            );
        } else {
            assert!(
                off <= prev,
                "moving up scrolled down at L{c}: {prev} → {off}"
            );
        }
        let start = layout.starts[c];
        let end = layout.rows.iter().rposition(|r| r.line == c).unwrap();
        if start >= prev && end < prev + VIEW {
            assert_eq!(off, prev, "view moved though L{c} was in view");
        }
        assert!(
            (off..off + VIEW).contains(&start),
            "L{c} (row {start}) out of view at offset {off}"
        );
        prev = off;
    }
}

#[test]
fn plain_file_scrolls_only_at_edges() -> Result<()> {
    let (_f, mut panel) = fixture(80)?;
    trace(&mut panel, 1, 85);
    assert_eq!(panel.cursor, 79);
    trace(&mut panel, -1, 85);
    assert_eq!(panel.cursor, 0);
    Ok(())
}

#[test]
fn stepping_past_a_comment_card_holds_the_view() -> Result<()> {
    let (_f, mut panel) = fixture(80)?;
    panel.cursor = 29;
    panel.author_note("a comment long enough to make the card several rows tall ".repeat(4));
    panel.cursor = 0;
    trace(&mut panel, 1, 85);
    trace(&mut panel, -1, 85);
    Ok(())
}

#[test]
fn stepping_past_a_highlight_holds_the_view() -> Result<()> {
    let (_f, mut panel) = fixture(80)?;
    panel.set_highlight(50, 52);
    panel.scroll_offset(&panel.layout(80), VIEW); // consume the one-time recenter
    trace(&mut panel, 1, 40);
    trace(&mut panel, -1, 85);
    Ok(())
}

#[test]
fn reversing_at_the_bottom_edge_moves_the_cursor_not_the_view() -> Result<()> {
    let (_f, mut panel) = fixture(80)?;
    panel.move_cursor(30);
    let off = panel.scroll_offset(&panel.layout(80), VIEW);
    assert!(off > 0);
    panel.move_cursor(-1);
    assert_eq!(panel.scroll_offset(&panel.layout(80), VIEW), off);
    Ok(())
}

#[test]
fn wheel_scrolls_the_view() -> Result<()> {
    let (_f, mut panel) = fixture(80)?;
    panel.scroll_view(3);
    assert_eq!(panel.scroll_offset(&panel.layout(80), VIEW), 3);
    assert_eq!(panel.cursor, 3);
    Ok(())
}

/// A card opening or collapsing above the view leaves the same lines on screen.
#[test]
fn a_card_above_the_view_keeps_its_lines_on_screen() -> Result<()> {
    let (_f, mut panel) = fixture(200)?;
    panel.move_cursor(100);
    panel.scroll_offset(&panel.layout(80), VIEW);
    panel.move_cursor(-10); // mid-view
    let top = |p: &Panel| {
        let layout = p.layout(80);
        layout.rows[p.scroll_offset(&layout, VIEW)].line
    };
    let before = top(&panel);
    panel
        .agent_note(10, Some("x".into()), None)
        .map_err(anyhow::Error::msg)?;
    assert_eq!(top(&panel), before, "agent comment above the view");
    panel.set_all_collapsed(true);
    assert_eq!(top(&panel), before, "bulk collapse above the view");
    Ok(())
}
