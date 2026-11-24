//! Geometry/split-tree: a tab's recursive pane arrangement, the rects it draws to, and the lines
//! between panes that the user drags to resize them (a pinned split).

use std::collections::HashMap;

use ratatui::layout::Rect;

use crate::protocol::{Dir, PTY_PANE, PaneId, Side};

/// A tab's pane arrangement: a recursive binary split tree. Leaves are panes; one leaf is always the PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    Pane(PaneId),
    Split {
        dir: Dir,
        /// Percent of the split given to `first`, 1..=99, as Laura set it.
        ratio: u16,
        /// The user's percent for `first`, from a drag. It overrides `ratio` until the user gives
        /// the size back to Laura, who then draws `ratio` again.
        pinned: Option<u16>,
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
                    pinned: None,
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

    /// The tree with `pane`'s leaf collapsed; `None` when the leaf is the whole tree.
    pub fn without(&self, pane: PaneId) -> Option<Layout> {
        if *self == Layout::Pane(pane) {
            return None;
        }
        let mut rest = self.clone();
        rest.collapse(pane);
        Some(rest)
    }

    /// Move `pane`'s leaf to the left of the other panes, at the full height, with `ratio` percent
    /// of the width. With one leaf, nothing changes.
    pub fn dock_left(&mut self, pane: PaneId, ratio: u16) {
        if let Some(rest) = self.without(pane) {
            *self = Layout::Split {
                dir: Dir::Horizontal,
                ratio,
                pinned: None,
                first: Box::new(Layout::Pane(pane)),
                second: Box::new(rest),
            };
        }
    }

    /// The user's ratio of the split between panes `a` and `b`: the lowest split with `a` under
    /// `first` and `b` under `second`.
    fn between(&mut self, a: PaneId, b: PaneId) -> Option<&mut Option<u16>> {
        let Layout::Split {
            pinned,
            first,
            second,
            ..
        } = self
        else {
            return None;
        };
        match (first.contains(a), second.contains(b)) {
            (true, true) => Some(pinned),
            (true, false) if first.contains(b) => first.between(a, b),
            (false, true) if second.contains(a) => second.between(a, b),
            _ => None,
        }
    }

    /// Pin the split between panes `a` and `b` at `ratio` percent for `a`'s side, 1..=99.
    pub(crate) fn set_ratio(&mut self, a: PaneId, b: PaneId, ratio: u16) -> Result<(), String> {
        let pinned = self.between(a, b).ok_or_else(|| no_split(a, b))?;
        *pinned = Some(ratio.clamp(1, 99));
        Ok(())
    }

    /// Give the split between panes `a` and `b` back to Laura's ratio.
    pub(crate) fn unpin(&mut self, a: PaneId, b: PaneId) -> Result<(), String> {
        *self.between(a, b).ok_or_else(|| no_split(a, b))? = None;
        Ok(())
    }

    /// Each split's line at `area`, outer splits first, with the ratios at which every pane still
    /// clears `MIN_PANE`. The front-ends clamp a drag to that range.
    pub(crate) fn handles(&self, area: Rect) -> Vec<Handle> {
        let mut out = vec![];
        self.fill_handles(area, &mut out);
        for h in &mut out {
            // Brute force, as `Tab::fit_ratio`: `split_rect` floors, so the range isn't a formula.
            let fits = |r| {
                let mut probe = self.clone();
                probe.set_ratio(h.first[0], h.second[0], r).is_ok() && all_panes_fit(&probe, area)
            };
            let min = (1..=99).find(|&r| fits(r));
            let max = (1..=99).rev().find(|&r| fits(r));
            (h.min, h.max) = min.zip(max).unwrap_or((h.ratio, h.ratio));
        }
        out
    }

    fn fill_handles(&self, area: Rect, out: &mut Vec<Handle>) {
        let Layout::Split {
            dir,
            ratio,
            pinned,
            first,
            second,
        } = self
        else {
            return;
        };
        let drawn = pinned.unwrap_or(*ratio);
        let (a, b) = split_rect(area, *dir, drawn);
        out.push(Handle {
            dir: *dir,
            area,
            at: match dir {
                Dir::Horizontal => a.width,
                Dir::Vertical => a.height,
            },
            ratio: drawn,
            pinned: pinned.is_some(),
            first: first.order(),
            second: second.order(),
            min: drawn,
            max: drawn,
        });
        first.fill_handles(a, out);
        second.fill_handles(b, out);
    }

    /// Every pane under a split the user pinned.
    pub(crate) fn pinned_panes(&self) -> Vec<PaneId> {
        match self {
            Layout::Pane(_) => vec![],
            Layout::Split {
                pinned: Some(_), ..
            } => self.order(),
            Layout::Split { first, second, .. } => {
                let mut out = first.pinned_panes();
                out.extend(second.pinned_panes());
                out
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

/// The line between a split's two sides, which the user drags to resize them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handle {
    pub dir: Dir,
    /// The split's whole rect.
    pub area: Rect,
    /// `first`'s width (horizontal) or height (vertical) in cells: the line's offset in `area`.
    pub at: u16,
    /// The percent for `first` that Laura draws: the user's while pinned, else Laura's.
    pub ratio: u16,
    pub pinned: bool,
    /// The panes on each side, in order.
    pub first: Vec<PaneId>,
    pub second: Vec<PaneId>,
    /// The lowest and highest ratio at which every pane clears `MIN_PANE`.
    pub min: u16,
    pub max: u16,
}

fn no_split(a: PaneId, b: PaneId) -> String {
    format!("no split between pane #{a} and pane #{b}")
}

/// A boxed `Pane(PTY)` used as a throwaway placeholder in `mem::replace`.
fn boxed_pty() -> Box<Layout> {
    Box::new(Layout::Pane(PTY_PANE))
}

/// Split `area` into (first, second) by `dir`/`ratio`, matching what the renderer draws.
pub(crate) fn split_rect(area: Rect, dir: Dir, ratio: u16) -> (Rect, Rect) {
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

/// Smallest rect that can render a bordered pane: 3×3 (1 content cell inside a 1-cell border).
pub const MIN_PANE: (u16, u16) = (3, 3);

/// Smallest terminal (columns, rows) that shows the workspace; below it the TUI shows one line.
pub const MIN_WORKSPACE: (u16, u16) = (40, 10);

/// True if a terminal of `area` clears `MIN_WORKSPACE`.
pub fn workspace_fits(area: Rect) -> bool {
    area.width >= MIN_WORKSPACE.0 && area.height >= MIN_WORKSPACE.1
}

/// True if every pane in `layout` at `area` clears `MIN_PANE`.
pub fn all_panes_fit(layout: &Layout, area: Rect) -> bool {
    rects(layout, area).values().all(|r| pane_fits(*r))
}

/// True if a pane at `r` clears `MIN_PANE`.
pub(crate) fn pane_fits(r: Rect) -> bool {
    r.width >= MIN_PANE.0 && r.height >= MIN_PANE.1
}

/// Choose (target, dir, ratio, side) for an unspecified `open` — a dwindle: split the
/// newest pane (highest live id; the shell/PTY if none yet), alternating orientation by its
/// tree depth so repeated opens split off the newest pane. Deterministic, topology-only.
/// `new` is the id the caller will assign; the probe measures the real post-split geometry.
/// Returns None if the newest pane can't be split without collapsing (caller then errors via the gate).
///
/// ponytail: dwindle only ever targets the newest pane, so a full spiral corner errors even when
/// a larger pane exists elsewhere — the upgrade path is the later relayout feature, not a target-hunt.
pub fn infer_split(layout: &Layout, area: Rect, new: PaneId) -> Option<(PaneId, Dir, u16, Side)> {
    // Newest live pane = highest non-PTY id (ids mint monotonically, never reused); PTY if none.
    let target = layout
        .order()
        .into_iter()
        .filter(|&id| id != PTY_PANE)
        .max()
        .unwrap_or(PTY_PANE);
    // Alternate H/V by the target leaf's depth: root leaf (depth 0) → H, then V, H, … as the
    // spiral deepens. Measuring real depth (not a stored toggle) stays correct across closes.
    let dir = if depth_of(layout, target).unwrap_or(0).is_multiple_of(2) {
        Dir::Horizontal
    } else {
        Dir::Vertical
    };
    let (ratio, side) = (50, Side::Second); // new pane goes down/right; the caller fits a stacked split's ratio
    // Only return a split Part A's gate accepts.
    let mut probe = layout.clone();
    probe.split(target, dir, ratio, side, new).ok()?;
    all_panes_fit(&probe, area).then_some((target, dir, ratio, side))
}

/// Depth of `target`'s leaf (root leaf = 0); None if absent.
fn depth_of(layout: &Layout, target: PaneId) -> Option<u16> {
    match layout {
        Layout::Pane(id) => (*id == target).then_some(0),
        Layout::Split { first, second, .. } => depth_of(first, target)
            .or_else(|| depth_of(second, target))
            .map(|d| d + 1),
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
            pinned,
            first,
            second,
        } => {
            let (a, b) = split_rect(area, *dir, pinned.unwrap_or(*ratio));
            fill_rects(first, a, map);
            fill_rects(second, b, map);
        }
    }
}
