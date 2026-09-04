//! Geometry/split-tree: a tab's recursive pane arrangement and the rects it draws to.

use std::collections::HashMap;

use ratatui::layout::Rect;

use crate::protocol::{Dir, PTY_PANE, PaneId, Side};

/// A tab's pane arrangement: a recursive binary split tree. Leaves are panes; one leaf is always the PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    Pane(PaneId),
    Split {
        dir: Dir,
        /// Percent of the split given to `first`, 1..=99.
        ratio: u16,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

impl Layout {
    /// Replace the `Pane(target)` leaf with a split placing `new` on `side`. Errors if `target` isn't a leaf here.
    pub fn split(
        &mut self,
        target: PaneId,
        dir: Dir,
        ratio: u16,
        side: Side,
        new: PaneId,
    ) -> Result<(), String> {
        match self {
            Layout::Pane(id) if *id == target => {
                let (existing, added) = (Layout::Pane(target), Layout::Pane(new));
                let (first, second) = match side {
                    Side::First => (added, existing),
                    Side::Second => (existing, added),
                };
                // `ratio` describes the NEW pane; the tree stores first's share.
                let first_ratio = match side {
                    Side::First => ratio,        // new pane is first
                    Side::Second => 100 - ratio, // new pane is second → first gets the rest
                };
                *self = Layout::Split {
                    dir,
                    ratio: first_ratio.clamp(1, 99),
                    first: Box::new(first),
                    second: Box::new(second),
                };
                Ok(())
            }
            Layout::Pane(_) => Err(format!("no pane #{target}")),
            Layout::Split { first, second, .. } => first
                .split(target, dir, ratio, side, new)
                .or_else(|e| second.split(target, dir, ratio, side, new).map_err(|_| e)),
        }
    }

    /// Remove `pane`, collapsing its parent split into the sibling subtree. Errors on the PTY or an absent pane.
    pub fn remove(&mut self, pane: PaneId) -> Result<(), String> {
        if pane == PTY_PANE {
            return Err("the shell pane can't be closed".into());
        }
        if !self.contains(pane) {
            return Err(format!("no pane #{pane}"));
        }
        // Root can only shrink if it's the split holding the pane as a direct leaf.
        self.collapse(pane);
        Ok(())
    }

    /// Replace any `Split` whose direct child is `Pane(pane)` with its other child; recurse otherwise.
    fn collapse(&mut self, pane: PaneId) {
        if let Layout::Split { first, second, .. } = self {
            match (first.as_ref(), second.as_ref()) {
                (Layout::Pane(id), _) if *id == pane => {
                    *self = *std::mem::replace(second, boxed_pty())
                }
                (_, Layout::Pane(id)) if *id == pane => {
                    *self = *std::mem::replace(first, boxed_pty())
                }
                _ => {
                    first.collapse(pane);
                    second.collapse(pane);
                }
            }
        }
    }

    /// True if `pane` is a leaf anywhere in this tree.
    pub fn contains(&self, pane: PaneId) -> bool {
        match self {
            Layout::Pane(id) => *id == pane,
            Layout::Split { first, second, .. } => first.contains(pane) || second.contains(pane),
        }
    }

    /// Panes in positional (in-order) sequence — the order the picker labels `1..N`.
    pub fn order(&self) -> Vec<PaneId> {
        let mut out = vec![];
        self.walk(&mut out);
        out
    }

    fn walk(&self, out: &mut Vec<PaneId>) {
        match self {
            Layout::Pane(id) => out.push(*id),
            Layout::Split { first, second, .. } => {
                first.walk(out);
                second.walk(out);
            }
        }
    }
}

/// A boxed `Pane(PTY)` used as a throwaway placeholder in `mem::replace`.
fn boxed_pty() -> Box<Layout> {
    Box::new(Layout::Pane(PTY_PANE))
}

/// Split `area` into (first, second) by `dir`/`ratio`, matching what the renderer draws.
fn split_rect(area: Rect, dir: Dir, ratio: u16) -> (Rect, Rect) {
    let ratio = ratio.clamp(1, 99);
    match dir {
        Dir::Horizontal => {
            let w = (area.width as u32 * ratio as u32 / 100) as u16;
            (
                Rect { width: w, ..area },
                Rect {
                    x: area.x + w,
                    width: area.width.saturating_sub(w),
                    ..area
                },
            )
        }
        Dir::Vertical => {
            let h = (area.height as u32 * ratio as u32 / 100) as u16;
            (
                Rect { height: h, ..area },
                Rect {
                    y: area.y + h,
                    height: area.height.saturating_sub(h),
                    ..area
                },
            )
        }
    }
}

/// Geometry for every pane. The single source both render and the overflow check read, so "what `check` measures" is "what gets drawn".
pub fn rects(layout: &Layout, area: Rect) -> HashMap<PaneId, Rect> {
    let mut map = HashMap::new();
    fill_rects(layout, area, &mut map);
    map
}

fn fill_rects(layout: &Layout, area: Rect, map: &mut HashMap<PaneId, Rect>) {
    match layout {
        Layout::Pane(id) => {
            map.insert(*id, area);
        }
        Layout::Split {
            dir,
            ratio,
            first,
            second,
        } => {
            let (a, b) = split_rect(area, *dir, *ratio);
            fill_rects(first, a, map);
            fill_rects(second, b, map);
        }
    }
}
