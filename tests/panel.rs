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
    panel.author_note("note".into());
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

    // The thread on line 1 (index 1) renders a card of `comment` rows all tagged to that line,
    // one of which carries the note body.
    let card: Vec<_> = layout.rows.iter().filter(|r| r.comment).collect();
    assert!(!card.is_empty(), "expected comment card rows");
    assert!(card.iter().all(|r| r.line == 1), "card rows tag their line");
    assert!(
        card.iter().any(|r| r.text().contains("note")),
        "a card row holds the note body"
    );
    Ok(())
}

#[test]
fn card_labels_human_as_you_and_titlecases_unnamed_agent() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "line 0\nline 1\nline 2")?;
    file.flush()?;
    let mut panel = Panel::open(file.path().to_str().unwrap().to_string());
    panel.move_cursor(1);
    panel.author_note("hi".into()); // human note -> "You"
    panel.agent_note(1, Some("bot".into()), None).unwrap(); // unnamed fallback -> "Agent"
    panel
        .agent_note(3, Some("named".into()), Some("claude".into()))
        .unwrap(); // real name verbatim
    drop(file);

    let card = panel
        .layout(40)
        .rows
        .into_iter()
        .filter(|r| r.comment)
        .map(|r| r.text())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(card.contains("You · hi"), "human labelled 'You': {card}");
    assert!(
        card.contains("Agent · bot"),
        "unnamed agent title-cased to 'Agent': {card}"
    );
    assert!(
        card.contains("claude · named"),
        "named agent rendered verbatim: {card}"
    );
    Ok(())
}

#[test]
fn layout_carets_and_collapses_threads() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "line 0\nline 1\nline 2")?;
    file.flush()?;
    let mut panel = Panel::open(file.path().to_str().unwrap().to_string());
    panel.move_cursor(1);
    panel.author_note("note".into());
    drop(file);

    // Expanded: the line's body row carries an expanded caret and the card renders.
    let layout = panel.layout(40);
    let body = layout
        .rows
        .iter()
        .find(|r| r.gutter == Some(2))
        .expect("body row for line 1");
    assert_eq!(body.review, Some(false), "expanded");
    assert!(layout.rows.iter().any(|r| r.comment), "card is drawn");

    // Collapsed: caret flips and no card rows survive.
    panel.toggle_collapsed(1);
    let layout = panel.layout(40);
    let body = layout
        .rows
        .iter()
        .find(|r| r.gutter == Some(2))
        .expect("body row for line 1");
    assert_eq!(body.review, Some(true), "collapsed");
    assert!(
        !layout.rows.iter().any(|r| r.comment),
        "collapsed thread draws no card"
    );
    Ok(())
}

#[test]
fn thread_split_and_jump_track_the_cursor() -> Result<()> {
    // The title counter (`↑a ↓b`) and the `n`/`N` jump both read thread lines relative to the
    // cursor, so an off-screen comment shows in the count and jumps into view (#45).
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "l0\nl1\nl2\nl3\nl4\nl5")?;
    file.flush()?;
    let mut panel = Panel::open(file.path().to_str().unwrap().to_string());
    panel.cursor = 1;
    panel.author_note("a".into());
    panel.cursor = 4;
    panel.author_note("b".into());
    drop(file);

    // Split: at/above the cursor vs below it.
    panel.cursor = 0;
    assert_eq!(panel.thread_split(), (0, 2), "both threads below line 0");
    panel.cursor = 1;
    assert_eq!(panel.thread_split(), (1, 1), "on the first thread");
    panel.cursor = 5;
    assert_eq!(panel.thread_split(), (2, 0), "both at/above the last line");

    // Jump: nearest thread each way from between the two.
    panel.cursor = 2;
    assert_eq!(panel.thread_jump(true), Some(4), "next thread forward");
    assert_eq!(panel.thread_jump(false), Some(1), "previous thread back");
    // Wrap around the ends: next past the last lands on the first, prev before the first on the last.
    panel.cursor = 4;
    assert_eq!(
        panel.thread_jump(true),
        Some(1),
        "next past the last wraps to first"
    );
    panel.cursor = 1;
    assert_eq!(
        panel.thread_jump(false),
        Some(4),
        "prev before the first wraps to last"
    );
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
    let budget = inner_w - (layout.gutter_width + 5);
    for r in &layout.rows {
        assert!(
            r.text().chars().count() <= budget,
            "row overflows: {:?}",
            r.text()
        );
    }
    Ok(())
}

#[test]
fn table_rows_stay_aligned_with_inline_code() -> Result<()> {
    // #45: a table cell holding inline code must not gain `█` caps — they'd widen the cell and drift
    // the box-drawing borders out of alignment with the (code-free) separator row.
    let mut file = tempfile::Builder::new().suffix(".md").tempfile()?;
    write!(
        file,
        "# T\n\n| key | action |\n|-----|--------|\n| `Ctrl+P` | open |\n"
    )?;
    file.flush()?;
    let panel = Panel::open(file.path().to_str().unwrap().to_string());
    drop(file);

    let rows = panel.layout(80).rows;
    // No inline-code caps injected on the (nowrap) table.
    assert!(
        !rows
            .iter()
            .any(|r| r.spans.iter().any(|s| s.content.as_ref() == "\u{2588}")),
        "no `█` caps on pre-formatted table rows"
    );
    let width = |needle: &str| {
        rows.iter()
            .find(|r| r.text().contains(needle))
            .map(|r| r.text().chars().count())
            .unwrap_or_else(|| panic!("row containing {needle:?}"))
    };
    // The inline-code content row and a border row share one width — the columns line up.
    assert_eq!(
        width("Ctrl+P"),
        width("─"),
        "content row width matches the border row (no cap widening)"
    );
    Ok(())
}

#[test]
fn agent_comment_past_eof_is_dropped_not_misplaced() -> Result<()> {
    // Fixed contract: an agent `comment <line>` whose line is past the source must error, NOT
    // silently clamp onto the last rendered row (which would mislabel the note onto line 1).
    use std::fs;

    let file = tempfile::NamedTempFile::new()?;
    let path = file.path().to_str().unwrap().to_string();
    fs::write(file.path(), "l0\nl1\nl2")?;
    let mut panel = Panel::open(path.clone());

    // Line 99 is well past the 3-line file: must error and fork no thread (no clamp onto line 1).
    let err = panel.agent_note(99, Some("done".into()), Some("agent".into()));
    assert!(err.is_err(), "out-of-range comment must error, not clamp");
    assert_eq!(
        panel.thread_count(),
        0,
        "out-of-range comment must not fork a stray thread"
    );
    drop(file);
    Ok(())
}

#[test]
fn a_changed_file_freezes_the_pane_until_submit_or_refresh() -> Result<()> {
    // Call-and-response: while a thread is open, a change on disk must NOT reload out from under
    // the notes — the pane holds its snapshot and flags `source_changed`. Submit clears the notes
    // and the next reload catches up; `Ctrl+R` (discard_and_reload) drops the notes and refreshes.
    use std::fs;

    let file = tempfile::NamedTempFile::new()?;
    let path = file.path().to_str().unwrap().to_string();
    fs::write(file.path(), "l0\nl1\nl2")?;
    let mut panel = Panel::open(path.clone());
    panel.cursor = 1;
    panel.author_note("please fix".into());

    // File changes under an open note: the pane freezes on the reviewed snapshot.
    fs::write(file.path(), "l0\nCHANGED\nl2\nl3")?;
    assert!(
        !panel.reload_if_changed(),
        "frozen: no reload while notes open"
    );
    assert!(panel.source_changed, "the freeze is flagged");
    assert!(
        !panel
            .layout(80)
            .rows
            .iter()
            .any(|r| r.text().contains("CHANGED")),
        "pane still shows the reviewed snapshot, not disk"
    );
    // A frozen submit banners the mismatch; the note still ships.
    let review = panel.submit_review("");
    assert!(review.contains("please fix"), "note ships");
    assert!(
        review.contains("⚠ file changed"),
        "banner warns of the mismatch: {review}"
    );
    assert_eq!(panel.thread_count(), 0, "submit clears the threads");

    // Notes cleared → the next reload catches up to disk and clears the flag.
    assert!(panel.reload_if_changed(), "catches up once notes clear");
    assert!(!panel.source_changed, "flag clears after catch-up");
    assert!(
        panel
            .layout(80)
            .rows
            .iter()
            .any(|r| r.text().contains("CHANGED")),
        "pane now reflects disk"
    );
    Ok(())
}

#[test]
fn ctrl_r_discards_pending_notes_and_refreshes_to_disk() -> Result<()> {
    use std::fs;

    let file = tempfile::NamedTempFile::new()?;
    let path = file.path().to_str().unwrap().to_string();
    fs::write(file.path(), "l0\nl1\nl2")?;
    let mut panel = Panel::open(path.clone());
    panel.cursor = 1;
    panel.author_note("please fix".into());
    fs::write(file.path(), "l0\nCHANGED\nl2\nl3")?;
    assert!(!panel.reload_if_changed(), "frozen while notes open");

    panel.discard_and_reload();
    assert_eq!(panel.thread_count(), 0, "refresh discards the notes");
    assert!(!panel.source_changed, "flag cleared on refresh");
    assert!(
        panel
            .layout(80)
            .rows
            .iter()
            .any(|r| r.text().contains("CHANGED")),
        "pane refreshed to disk"
    );
    drop(file);
    Ok(())
}

#[test]
fn submit_clears_the_threads_and_a_clean_submit_has_no_banner() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "l0\nl1\nl2")?;
    file.flush()?;
    let mut panel = Panel::open(file.path().to_str().unwrap().to_string());
    drop(file);

    panel.cursor = 1;
    panel.author_note("please fix".into());
    let review = panel.submit_review("overall");
    assert!(
        review.contains("please fix"),
        "the note ships in the review"
    );
    assert!(
        !review.contains("⚠ file changed"),
        "clean submit carries no banner"
    );
    assert_eq!(panel.thread_count(), 0, "the panel clears on submit");
    Ok(())
}

#[test]
fn empty_author_note_creates_no_thread() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "l0\nl1")?;
    file.flush()?;
    let mut panel = Panel::open(file.path().to_str().unwrap().to_string());
    drop(file);

    panel.begin_comment();
    panel.author_note(String::new()); // stray c+Enter with no text
    panel.author_note("   ".into()); // whitespace-only is empty too
    assert_eq!(panel.thread_count(), 0, "empty comment pushes no thread");
    Ok(())
}

#[test]
fn comment_on_folded_block_row_joins_one_thread() -> Result<()> {
    // #45: a folded unit maps several rendered rows onto one source range. tui-markdown draws a
    // table's top-border and header rows both from the header source line (L3), so a human comment
    // from the non-first row and an agent reply on that source line must converge on ONE thread.
    let mut file = tempfile::Builder::new().suffix(".md").tempfile()?;
    write!(
        file,
        "# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n## Next\n"
    )?;
    file.flush()?;
    let mut panel = Panel::open(file.path().to_str().unwrap().to_string());
    drop(file);

    // The table's top-border and header rows both share source L3 but differ in rendered index.
    let block: Vec<usize> = panel
        .layout(80)
        .rows
        .iter()
        .filter(|r| r.gutter == Some(3))
        .map(|r| r.line)
        .collect();
    assert!(
        block.len() > 1,
        "table folds to multiple rendered rows: {block:?}"
    );
    let non_first = *block.last().unwrap();
    assert_ne!(
        non_first, block[0],
        "cursor lands off the block's first row"
    );

    // Human comments from the non-first row; agent replies on the folded source line (L3).
    panel.move_cursor(non_first as isize);
    panel.author_note("human here".into());
    panel
        .agent_note(3, Some("agent reply".into()), Some("agent".into()))
        .unwrap();

    assert_eq!(panel.thread_count(), 1, "one shared thread, no fork");
    let review = panel.assemble_review("");
    assert!(review.contains("L3"), "one folded-unit header: {review}");
    assert!(
        review.contains("human here") && review.contains("agent reply"),
        "both notes under one thread: {review}"
    );
    Ok(())
}

#[test]
fn review_header_carries_raw_source_not_rendered_glyphs() -> Result<()> {
    // A table thread's header must show the raw markdown row (`| 1 | 2 |`), not the rendered
    // box-drawing (`│ 1 │ 2 │`) the agent can't map back to source.
    let mut file = tempfile::Builder::new().suffix(".md").tempfile()?;
    write!(file, "# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n")?;
    file.flush()?;
    let mut panel = Panel::open(file.path().to_str().unwrap().to_string());
    drop(file);

    panel
        .agent_note(5, Some("check".into()), Some("agent".into()))
        .unwrap();
    let review = panel.assemble_review("");
    assert!(
        review.contains("| 1 | 2 |"),
        "header carries raw source: {review}"
    );
    assert!(
        !review.contains('│'),
        "no rendered box glyphs in the header: {review}"
    );
    Ok(())
}
