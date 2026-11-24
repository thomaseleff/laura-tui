//! The layout report that `laura layout` and the agent's `open` replies carry: each pane's rect,
//! its content and visible rows, and its overflow, with the text for a pane that overflows.

use std::collections::HashMap;

use ratatui::layout::Rect;

use crate::layout::Layout;
use crate::panel::Panel;
use crate::protocol::{LayoutReport, PTY_PANE, PaneId, PaneKind, PaneReport};

use super::Tab;

/// For each pane of tree `layout` at `area`, measure content vs. visible rows to fill a
/// `LayoutReport`. `panels` may hold a probe pane the tab doesn't have yet (`dry_run_open`).
/// In the web app the border takes three rows (see [`Tab::inner`]), and the gutter's blank cell
/// takes a column (`web::html::GAP`). The shell's own rows from the web app differ from its rect
/// while the shell is in the drawer.
pub(super) fn build_report(
    tab: &Tab,
    layout: &Layout,
    panels: &HashMap<PaneId, &Panel>,
    area: Rect,
) -> LayoutReport {
    let (web, drawer) = (tab.web, tab.drawer_in_use());
    let shell_rows = tab.shell_size.map(|(rows, _)| rows);
    let mut panes: Vec<PaneReport> = tab
        .tree_rects(layout, area)
        .into_iter()
        .map(|(id, rect)| {
            let border = if web { 3 } else { 2 };
            let visible_rows = match shell_rows {
                Some(rows) if id == PTY_PANE => rows as usize,
                _ => rect.height.saturating_sub(border) as usize,
            };
            let inner_w = rect.width.saturating_sub(if web { 3 } else { 2 }).max(1) as usize;
            let too_small = rect.width < 3 || rect.height < border + 1;
            let mut title = None;
            let (kind, path, content_rows) = if id == PTY_PANE {
                (PaneKind::Pty, None, None)
            } else if let Some(p) = panels.get(&id).filter(|p| p.browser) {
                // Laura's browser fits the page to the pane, like the shell's grid.
                title = tab.browsers.get(&id).map(|b| b.title().to_string());
                (PaneKind::Browser, Some(p.path.clone()), None)
            } else if tab.editors.contains_key(&id) {
                // Neovim's grid always fits its rect, like the shell's.
                let path = panels.get(&id).map(|p| p.path.clone());
                (PaneKind::Editor, path, None)
            } else {
                let p = panels.get(&id);
                (
                    PaneKind::Panel,
                    p.map(|p| p.path.clone()),
                    p.map(|p| p.layout(inner_w).rows.len()),
                )
            };
            let overflow_rows = content_rows.map_or(0, |c| c.saturating_sub(visible_rows));
            PaneReport {
                id,
                kind,
                path,
                rect: rect.into(),
                content_rows,
                visible_rows,
                overflow_rows,
                clipped: overflow_rows > 0 || too_small,
                title,
                paused: panels.get(&id).is_some_and(|p| p.paused),
                drawer: drawer && id == PTY_PANE,
            }
        })
        .collect();
    panes.sort_by_key(|p| p.id);
    LayoutReport {
        area: area.into(),
        panes,
    }
}

/// Rows a file pane needs at `width`, border included, and at least one content row.
pub(super) fn fit_rows(p: &Panel, width: u16, web: bool) -> usize {
    let b = if web { 3 } else { 2 };
    p.layout(width.saturating_sub(b).max(1) as usize)
        .rows
        .len()
        .max(1)
        + b as usize
}

/// Terse open-time warning for a just-opened pane, or `None` if it fits vertically.
pub fn overflow_warning(p: &PaneReport) -> Option<String> {
    if p.overflow_rows > 0 {
        Some(format!(
            "pane #{} overflows: {} row(s) out of view — scroll, or grow the pane",
            p.id, p.overflow_rows
        ))
    } else if p.clipped {
        Some(format!(
            "pane #{} too small to render — lower --ratio or change --dir",
            p.id
        ))
    } else {
        None
    }
}

impl Tab {
    /// Borrow every panel keyed by id (for `build_report`).
    pub(super) fn panel_refs(&self) -> HashMap<PaneId, &Panel> {
        self.panels.iter().map(|(k, v)| (*k, v)).collect()
    }

    /// Current geometry + overflow at draw area `area`.
    pub fn report(&self, area: Rect) -> LayoutReport {
        build_report(self, &self.layout, &self.panel_refs(), area)
    }
}
