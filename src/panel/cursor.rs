//! The web app's file pane cursor: `↑`/`↓` by rendered markdown block, stop and thread card, and
//! the block's threads that `r` collapses or expands. The TUI's cursor is `Panel::move_cursor` in
//! `panel`.

use super::Panel;
use crate::render::Block;

/// The markdown block holding the cursor; `None` in diff view or between blocks.
pub(crate) fn cursor_block(p: &Panel) -> Option<&Block> {
    if p.diff_view {
        return None;
    }
    p.blocks().iter().find(|b| b.rows.contains(&p.cursor))
}

/// Whether the cursor is on one of block `b`'s [`Block::stops`].
pub(crate) fn on_stop(p: &Panel, b: &Block) -> bool {
    b.stops().iter().any(|&(row, _)| row == p.cursor)
}

/// Whether the cursor shows on an open thread's card below block `b`, not on the block.
pub(crate) fn on_card(p: &Panel, b: &Block) -> bool {
    p.cursor != b.rows.start && !on_stop(p, b) && p.cursor_thread().is_some_and(|t| !t.collapsed)
}

/// The 1-based line of the open thread whose card holds the cursor. A pane without blocks shows
/// the cursor on the card of its line's open thread.
pub(crate) fn card_line(p: &Panel) -> Option<usize> {
    p.cursor_thread()
        .filter(|t| !t.collapsed && cursor_block(p).is_none_or(|b| on_card(p, b)))
        .map(|t| p.thread_source(t).0 + 1)
}

/// `↑`/`↓` in rendered markdown: one block, stop, card or blank row per keystroke. False in diff
/// view or for a file without blocks, so `Tab::act` moves the cursor.
pub(crate) fn step(p: &mut Panel, delta: isize) -> bool {
    if p.diff_view || p.blocks().is_empty() {
        return false;
    }
    let before = p.cursor;
    if let Some(b) = cursor_block(p) {
        let rows = b.rows.clone();
        let stops: Vec<usize> = b.stops().iter().map(|&(row, _)| row).collect();
        if on_card(p, b) {
            if delta < 0 {
                p.cursor = rows.start;
                return true;
            }
        } else {
            let at = if stops.contains(&p.cursor) {
                p.cursor
            } else {
                rows.start
            };
            let next = if delta > 0 {
                stops.iter().copied().find(|&r| r > at)
            } else {
                let start = (at > rows.start).then_some(rows.start);
                stops.iter().copied().rfind(|&r| r < at).or(start)
            };
            if let Some(r) = next {
                p.cursor = r;
                return true;
            }
        }
        p.cursor = if delta > 0 { rows.end - 1 } else { rows.start };
    }
    p.move_cursor(delta);
    // A move into a block goes to its first row, or `↑` to its last stop; at either end of the
    // file, the cursor keeps its row.
    if let Some(b) = cursor_block(p) {
        p.cursor = if b.rows.contains(&before) {
            before
        } else if delta < 0 {
            b.stops().last().map_or(b.rows.start, |&(row, _)| row)
        } else {
            b.rows.start
        };
    }
    true
}

/// Collapse the cursor block's threads if any is open, else expand them. False off a block, on a
/// thread off the block's first row, or with no threads in the block.
pub(crate) fn toggle_block_threads(p: &mut Panel) -> bool {
    let Some(b) = cursor_block(p) else {
        return false;
    };
    if (p.cursor != b.rows.start || on_stop(p, b)) && p.cursor_thread().is_some() {
        return false;
    }
    let b = b.rows.clone();
    let Some(open) = p
        .threads
        .iter()
        .filter(|t| b.contains(&t.line))
        .map(|t| !t.collapsed)
        .max()
    else {
        return false;
    };
    for t in p.threads.iter_mut().filter(|t| b.contains(&t.line)) {
        t.collapsed = open;
    }
    true
}
