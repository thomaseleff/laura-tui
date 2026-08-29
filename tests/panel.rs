//! Panel layout is the load-bearing "line numbers stay factual" logic: the gutter a user reads must equal the review's `L<n>`, and wrapping must not overflow. Through `Panel::layout` + `wrap_line`.

use std::io::Write;

use anyhow::Result;
use laura::{Panel, wrap_line};

#[test]
fn wrap_line_covers_edges() {
    assert_eq!(wrap_line("abc de", 10), vec!["abc de"]); // fits on one row
    assert_eq!(wrap_line("abc def ghi", 7), vec!["abc def", "ghi"]); // word boundary
    assert_eq!(wrap_line("abcdefgh", 3), vec!["abc", "def", "gh"]); // over-long word splits
    assert_eq!(wrap_line("", 5), vec![""]); // blank source line still holds a row
    for row in wrap_line("the quick brown fox jumped over", 8) {
        assert!(row.chars().count() <= 8, "row too wide: {row:?}");
    }
}

#[test]
fn layout_gutter_matches_line_numbers_and_review() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "line 0\nline 1\nline 2")?;
    file.flush()?;
    let mut panel = Panel::open(file.path().to_str().unwrap().to_string());
    panel.move_cursor(1);
    panel.add_comment("note".into());
    drop(file);

    let layout = panel.layout(40);

    // Each source line's first row carries a 1-based gutter == its review `L<n>`.
    let mut expected_line = 0;
    for row in &layout.rows {
        if let Some(n) = row.gutter {
            assert_eq!(n, expected_line + 1, "gutter must be 1-based line number");
            expected_line += 1;
        }
    }
    assert_eq!(expected_line, 3, "one gutter number per source line");
    assert!(panel.assemble_review("").contains("L2  line 1"));

    // `starts[i]` points at the row whose gutter is `i+1` (the scroll anchor).
    for (i, &start) in layout.starts.iter().enumerate() {
        assert_eq!(layout.rows[start].gutter, Some(i + 1));
    }

    // The comment on line 1 (index 1) rides a dim `<-` row tagged to that line.
    let c = layout.rows.iter().find(|r| r.comment).expect("comment row");
    assert_eq!(c.line, 1);
    assert!(c.text().contains("note"));
    Ok(())
}

#[test]
fn layout_wraps_long_lines_within_width() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    // One long line that must wrap into several rows under a narrow panel.
    write!(file, "{}", "word ".repeat(40).trim())?;
    file.flush()?;
    let panel = Panel::open(file.path().to_str().unwrap().to_string());
    drop(file);

    let inner_w = 20usize;
    let layout = panel.layout(inner_w);

    // Only the first row is gutter-numbered; the rest are continuations.
    assert_eq!(layout.rows[0].gutter, Some(1));
    assert!(
        layout.rows.len() > 1,
        "long line should wrap to multiple rows"
    );
    assert!(layout.rows[1..].iter().all(|r| r.gutter.is_none()));

    // gutter (`gw` digits + space) + text never exceeds the text area.
    let budget = inner_w - (layout.gutter_width + 1);
    for r in &layout.rows {
        assert!(
            r.text().chars().count() <= budget,
            "row overflows: {:?}",
            r.text()
        );
    }
    Ok(())
}
