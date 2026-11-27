//! The user's actions from the TUI and the web app (`UserAction`): `act` changes the workspace for
//! each one, and returns the notice to show or the error.

use std::time::Instant;

use anyhow::Result;

use crate::browser::{self, Page};
use crate::layout::{Layout, all_panes_fit, pane_fits, resize_fits, split_rect};
use crate::panel::{Panel, bracketed_paste};
use crate::protocol::{Dir, PTY_PANE, PaneId, Response};

use super::{
    DOCK, Draft, Error, NEEDS_READY, NO_COMMENT_IN_DIFF, NO_DIFF_IN_EDITOR, NO_ROOM_TO_DOCK,
    NO_ROOM_TO_RESIZE, NO_ROOM_TO_UNPIN, NO_SPLIT, Tab, UserAction, no_pane,
};

/// Whether `b` holds only reports of pointer motion with no button, SGR (`\e[<b;x;yM`) or X10
/// (`\e[M` and 3 bytes): a hover isn't the user looking, but a wheel step or a click is.
fn only_hover(mut b: &[u8]) -> bool {
    // Motion (32) with no button (3), below the wheel (64).
    let hover = |code: u32| code & 32 != 0 && code & 3 == 3 && code < 64;
    if b.is_empty() {
        return false;
    }
    while !b.is_empty() {
        if let Some(rest) = b.strip_prefix(b"\x1b[<") {
            let Some(end) = rest.iter().position(|c| !matches!(c, b'0'..=b'9' | b';')) else {
                return false;
            };
            let code = rest[..end].split(|&c| c == b';').next().unwrap_or_default();
            let code = std::str::from_utf8(code).ok().and_then(|c| c.parse().ok());
            if !matches!(rest[end], b'M' | b'm') || !code.is_some_and(hover) {
                return false;
            }
            b = &rest[end + 1..];
        } else if let Some(rest) = b.strip_prefix(b"\x1b[M")
            && rest.len() >= 3
            && hover(u32::from(rest[0].wrapping_sub(32)))
        {
            b = &rest[3..];
        } else {
            return false;
        }
    }
    true
}

impl Tab {
    /// Apply one user action. `Ok(Some(text))` only from `BeginComment`; `Err` carries the notice.
    /// Every field is untrusted.
    pub fn act(&mut self, action: UserAction) -> Result<Option<String>, Error> {
        use UserAction as A;
        match action {
            A::Focus { pane } => {
                if pane != PTY_PANE && !self.panels.contains_key(&pane) {
                    return Err(no_pane(pane).into());
                }
                self.focus = pane;
                self.forget_lock_from();
                // The user's move into the shell: the user sees the agent.
                if pane == PTY_PANE {
                    self.saw(self.pty.bells());
                }
            }
            A::MoveCursor { pane, delta } => self.panel_mut(pane)?.move_cursor(delta),
            A::ScrollH { pane, delta } => self.panel_mut(pane)?.scroll_h(delta),
            A::ToggleDiff { pane } => {
                if self.editors.contains_key(&pane) {
                    return Err(NO_DIFF_IN_EDITOR.into());
                }
                let p = self.panel_mut(pane)?;
                let want = !p.diff_view;
                p.set_diff_view(want)?;
            }
            A::TogglePause { pane } => {
                let p = self.panel_mut(pane)?;
                if p.pausable() {
                    let on = !p.paused;
                    p.set_paused(on);
                }
            }
            A::ToggleThread { pane, line } => {
                let p = self.panel_mut(pane)?;
                if let Some(l) = line {
                    p.set_cursor_line(l);
                }
                if p.cursor_thread().is_some() {
                    p.toggle_collapsed(p.cursor);
                } else {
                    let any_open = p.threads.iter().any(|t| !t.collapsed);
                    p.set_all_collapsed(any_open);
                }
            }
            A::JumpThread { pane, next } => {
                let p = self.panel_mut(pane)?;
                if let Some(row) = p.thread_jump(next) {
                    p.cursor = row;
                }
            }
            A::ClearHighlight { pane } => self.panel_mut(pane)?.clear_highlight(),
            A::SetCursor { pane, line } => self.panel_mut(pane)?.set_cursor_line(line),
            A::OpenLink { pane, href } => self.open_link(pane, &href)?,
            A::OpenPath { path, line, diff } => self.open_path(&path, line, diff)?,
            A::BeginComment { pane } => return self.begin_comment(pane).map(Some),
            A::Comment { pane, line, text } => self.comment(pane, line, text)?,
            A::BeginReview { pane } => self.begin_review(pane)?,
            A::Submit { pane, body } => self.submit(pane, &body)?,
            A::CancelDraft => self.close_draft(),
            A::Refresh { pane } if self.panels.get(&pane).is_some_and(|p| p.browser) => {
                match self.browsers.get(&pane) {
                    Some(page) => {
                        page.reload();
                        self.panel_mut(pane)?.discard_and_reload();
                    }
                    // Laura's browser exited: start it again, with every browser pane's page.
                    None => self.reopen(pane)?,
                }
            }
            A::Refresh { pane } => self.panel_mut(pane)?.discard_and_reload(),
            A::Close { pane } => {
                let from = self.focus;
                if let Response::Error { message } = self.close_pane(Some(pane)) {
                    return Err(message.into());
                }
                // The user's close that moves the focus to the shell: the user sees the agent.
                if from != PTY_PANE && self.focus == PTY_PANE {
                    self.saw(self.pty.bells());
                }
            }
            A::ToggleEditorView { pane } => {
                let e = self.editors.get_mut(&pane).ok_or_else(|| no_pane(pane))?;
                e.editor_view = !e.editor_view;
            }
            A::Dock => self.dock()?,
            A::Drawer => self.docked = false,
            A::Resize { a, b, ratio } => self.resize(a, b, Some(ratio))?,
            A::Unpin { a, b } => self.resize(a, b, None)?,
            A::DrawerHeight { .. } => {} // the web server saves it
            A::Seen { wait } => self.seen(wait),
            A::FlipDrawer => self.flip_drawer(),
            A::Replay { row, at } => self.replay(row, at)?,
            A::Input { pane, bytes } => {
                let bells = self.pty.bells();
                self.write_keys(pane, &bytes)?;
                if pane == PTY_PANE && !only_hover(&bytes) {
                    self.answered(bells);
                }
            }
            A::Paste { pane, text } => {
                let bells = self.pty.bells();
                self.write_keys(pane, &bracketed_paste(&text, false))?;
                if pane == PTY_PANE {
                    self.answered(bells);
                }
            }
            A::Quit => match self.unsaved_edits() {
                Some(message) => return Err(message.into()),
                None => self.ask_quit(),
            },
            A::Lock => self.toggle_lock(),
            A::Overlay { overlay } => self.set_overlay(overlay),
            A::Theme { laura } => self.answer_theme(laura),
            A::BrowserMouse { pane, mouse } => self.page(pane)?.mouse(&mouse, Instant::now()),
            A::BrowserKey { pane, key } => self.page(pane)?.key(&key),
            A::BrowserText { pane, text } => self.page(pane)?.insert_text(&text),
            A::BrowserBlur { pane } => self.page(pane)?.blur(),
            A::BrowserHistory { pane, delta } => self.page(pane)?.go(delta)?,
            A::BrowserDialog { pane, accept, text } => self.page(pane)?.answer(accept, &text),
            A::BrowserNavigate { pane, url } => {
                let url = browser::address(&url)?;
                self.navigate(pane, url)?;
            }
            A::BrowserPick { pane, x, y } => return self.pick(pane, x, y).map(Some),
            A::BrowserCommentMode { pane, on } => self.page(pane)?.comment = on,
            A::BrowserReveal { pane } => self.page(pane)?.reveal(),
        }
        Ok(None)
    }

    /// `c`: the comment's draft on the cursor's line, seeded with the user's own comment there.
    fn begin_comment(&mut self, pane: PaneId) -> Result<String, Error> {
        if !self.agent {
            return Err(NEEDS_READY.into());
        }
        let p = self.panel_mut(pane)?;
        if p.diff_view {
            return Err(NO_COMMENT_IN_DIFF.into());
        }
        p.set_cursor_line(p.cursor_line()); // snaps to a markdown block's first row
        Ok(self.open_comment(pane))
    }

    /// A click in comment mode: the comment's draft on the element at the point.
    fn pick(&mut self, pane: PaneId, x: f64, y: f64) -> Result<String, Error> {
        if !self.agent {
            return Err(NEEDS_READY.into());
        }
        let (selector, quote, url) = self.page(pane)?.pick(x, y)?.ok_or(browser::NO_ELEMENT)?;
        let p = self.panel_mut(pane)?;
        p.cursor = p.anchor(&selector, &quote, &url);
        Ok(self.open_comment(pane))
    }

    /// Open the comment's draft on pane `pane`'s cursor, which `panel_mut` found. Returns the seed.
    fn open_comment(&mut self, pane: PaneId) -> String {
        let p = self
            .panels
            .get_mut(&pane)
            .expect("the caller found the pane");
        let seed = p.begin_comment();
        let line = p.cursor_line();
        let edit = !seed.is_empty();
        self.open_draft(Draft::Comment { pane, line, edit });
        seed
    }

    /// `Enter` on a comment, or **Delete** on the user's own: `text` on `line`, then the draft closes.
    fn comment(&mut self, pane: PaneId, line: u32, text: String) -> Result<(), Error> {
        if !self.agent {
            return Err(NEEDS_READY.into());
        }
        let p = self.panel_mut(pane)?;
        if p.diff_view {
            return Err(NO_COMMENT_IN_DIFF.into());
        }
        p.set_cursor_line(line);
        p.author_note(text);
        self.close_draft();
        Ok(())
    }

    /// `Shift+S`: the review body's draft, past `review_gate`.
    fn begin_review(&mut self, pane: PaneId) -> Result<(), Error> {
        self.review_gate(pane)?;
        self.open_draft(Draft::Review { pane });
        Ok(())
    }

    /// **Show the shell as a pane**: the shell as the left-most pane, while every pane still fits
    /// beside it. The shell keeps the user's size while the panes fit, else takes Laura's. The TUI
    /// always shows the shell as a pane.
    fn dock(&mut self) -> Result<(), Error> {
        if !self.web {
            return Ok(());
        }
        let mut probe = self.layout.clone();
        probe.dock_left(PTY_PANE, DOCK);
        // Alone, the shell takes the left `DOCK`%, beside the canvas.
        let fits = |probe: &Layout| {
            if self.canvas_shown() {
                pane_fits(split_rect(self.area, Dir::Horizontal, DOCK).0)
            } else {
                all_panes_fit(probe, self.area)
            }
        };
        if !fits(&probe) {
            if let Some(b) = probe.order().into_iter().find(|&p| p != PTY_PANE) {
                probe.unpin(PTY_PANE, b);
            }
            if !fits(&probe) {
                return Err(NO_ROOM_TO_DOCK.into());
            }
        }
        self.layout = probe;
        self.docked = true;
        Ok(())
    }

    /// Pin the split between panes `a` and `b` at `ratio`, or give it back to Laura's ratio with
    /// `None`, while `resize_fits` allows it.
    fn resize(&mut self, a: PaneId, b: PaneId, ratio: Option<u16>) -> Result<(), Error> {
        let mut probe = self.layout.clone();
        let found = match ratio {
            Some(r) => probe.set_ratio(a, b, r),
            None => probe.unpin(a, b),
        };
        found.ok_or(NO_SPLIT)?;
        if !resize_fits(&self.shown(&self.layout), &self.shown(&probe), self.area) {
            return Err(match ratio {
                Some(_) => NO_ROOM_TO_RESIZE,
                None => NO_ROOM_TO_UNPIN,
            }
            .into());
        }
        self.layout = probe;
        if ratio.is_some() {
            self.dragged = Some(Instant::now());
        }
        Ok(())
    }

    /// Pane `pane`'s page in Laura's browser.
    fn page(&mut self, pane: PaneId) -> Result<&mut Page, String> {
        self.browsers.get_mut(&pane).ok_or_else(|| no_pane(pane))
    }

    /// The element that the agent's `selector` finds on browser pane `pane`'s page: its selector,
    /// quote and URL, for `Panel::anchor`.
    pub(super) fn element(
        &self,
        pane: PaneId,
        selector: &str,
    ) -> Result<(String, String, String), Error> {
        let page = self
            .browsers
            .get(&pane)
            .ok_or_else(|| Error::PageNotReady(browser::EXITED_AGENT.into()))?;
        if page.dialog_open() {
            return Err(Error::PageNotReady(format!(
                "pane #{pane}'s page is showing a dialog: answer it with your browser tool, \
                 or ask the user to answer it in the pane"
            )));
        }
        if page.loading() {
            return Err(Error::PageNotReady(format!(
                "pane #{pane}'s page is still loading: run the command again when the page has loaded"
            )));
        }
        page.find(selector)?.ok_or_else(|| Error::NoElement {
            selector: selector.into(),
            pane,
        })
    }

    /// Load `url` in browser pane `id`'s page, which keeps the pane's threads; open the page if
    /// Laura's browser exited.
    pub(super) fn navigate(&mut self, id: PaneId, url: String) -> Result<(), String> {
        let p = self
            .panels
            .get_mut(&id)
            .filter(|p| p.browser)
            .ok_or_else(|| no_pane(id))?;
        p.path = url.clone();
        match self.browsers.get_mut(&id) {
            Some(page) => page.navigate(&url),
            None => self.reopen(id)?,
        }
        Ok(())
    }

    /// The user's `Ctrl+R` or address field on pane `pane` with no page: start Laura's browser if
    /// it exited, which opens every browser pane's page once it starts, else open them now.
    fn reopen(&mut self, pane: PaneId) -> Result<(), String> {
        match self.browser {
            None => self.start_browser(Some(pane)),
            Some(_) => self.attach_pages(),
        }
    }

    /// Pane `pane`'s file pane, gated by `file_view`.
    pub(crate) fn panel_mut(&mut self, pane: PaneId) -> Result<&mut Panel, String> {
        self.file_view(pane)?;
        self.panels.get_mut(&pane).ok_or_else(|| no_pane(pane))
    }

    /// Neovim takes an editor pane's keys in its editor view, so its file pane takes no actions.
    pub(super) fn file_view(&self, pane: PaneId) -> Result<(), String> {
        if self.editors.get(&pane).is_some_and(|e| e.editor_view) {
            return Err(format!("pane #{pane} is in its editor view"));
        }
        Ok(())
    }

    /// Write `bytes` to pane `pane`'s terminal, snapping it to live.
    fn write_keys(&self, pane: PaneId, bytes: &[u8]) -> Result<(), String> {
        let pty = self
            .pane_pty(pane)
            .ok_or_else(|| format!("pane #{pane} doesn't take keystrokes"))?;
        pty.to_live();
        let _ = pty.write(bytes);
        Ok(())
    }
}
