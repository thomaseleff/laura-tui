//! One workspace: a PTY, its panels, the split tree, and the socket that ties them together.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use anyhow::Result;
use portable_pty::CommandBuilder;
use ratatui::layout::{Margin, Rect};

use serde::Deserialize;
use serde_json::json;

use crate::browser::{self, Browser, Cdp, Page};
use crate::cdp_proxy;
use crate::editor::{self, Nvim};
use crate::journal::{Journal, is_runtime_temp, runtime_dir};
use crate::layout::{Layout, MIN_PANE, all_panes_fit, infer_split, rects};
use crate::panel::{Panel, bracketed_paste};
use crate::protocol::{
    self, Dir, LayoutReport, Message, PTY_PANE, PaneId, PaneKind, PaneReport, Reply, Response, Side,
};
use crate::pty::PtyTab;
use crate::web;

/// Walk `layout`'s rects and, for each panel, measure content vs. visible rows to fill a `LayoutReport`.
/// `web`: the border takes three rows (see [`Tab::inner`]), and the gutter's blank cell takes a
/// column (`web::GAP`).
fn build_report(
    layout: &Layout,
    panels: &HashMap<PaneId, &Panel>,
    editors: &HashMap<PaneId, Nvim>,
    browsers: &HashMap<PaneId, Page>,
    area: Rect,
    web: bool,
) -> LayoutReport {
    let mut panes: Vec<PaneReport> = rects(layout, area)
        .into_iter()
        .map(|(id, rect)| {
            let border = if web { 3 } else { 2 };
            let visible_rows = rect.height.saturating_sub(border) as usize;
            let inner_w = rect.width.saturating_sub(if web { 3 } else { 2 }).max(1) as usize;
            let too_small = rect.width < 3 || rect.height < border + 1;
            let mut title = None;
            let (kind, path, content_rows) = if id == PTY_PANE {
                (PaneKind::Pty, None, None)
            } else if let Some(p) = panels.get(&id).filter(|p| p.browser) {
                // Laura's browser fits the page to the pane, like the shell's grid.
                title = browsers.get(&id).map(|b| b.title().to_string());
                (PaneKind::Browser, Some(p.path.clone()), None)
            } else if editors.contains_key(&id) {
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
fn fit_rows(p: &Panel, width: u16, web: bool) -> usize {
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

/// `c` / `Shift+S` before `laura ready`. Trailing space: ⚠ renders width-2.
pub const NEEDS_READY: &str = "⚠ The agent must run `laura ready` before you can comment. ";
/// `Shift+S` on a pane without threads.
pub const NO_THREADS: &str = "⚠ Add a comment before you submit the inline review. ";
/// `d` on an editor pane.
pub const NO_DIFF_IN_EDITOR: &str = "⚠ An editor pane does not show the diff view. ";
/// `c` in diff view.
pub const NO_COMMENT_IN_DIFF: &str = "⚠ Press d to close the diff view before you comment. ";
pub const SUBMIT_FAILED: &str =
    "⚠ inline review submission failed — Enter to retry · Esc to cancel ";
/// `SUBMIT_FAILED` in the web app, whose review popover names the keystrokes on its buttons.
pub const SUBMIT_FAILED_WEB: &str =
    "⚠ Laura did not submit the inline review, but kept its threads and review body.";
/// The shell exited while Neovim has unsaved edits.
pub const SHELL_EXITED: &str = "⚠ To close Laura after the shell stopped, save or quit Neovim. ";
/// `Shift+S` / `Enter` after the shell exited, which would write the inline review nowhere.
pub const NO_SHELL: &str = "⚠ The shell stopped, so Laura cannot submit the inline review. ";

/// What the user did. Never a protocol `Message`: the agent can't send one. The web app sends
/// these as JSON, `{"act":"move_cursor","pane":1,"delta":-1}`.
#[derive(Deserialize)]
#[serde(tag = "act", rename_all = "snake_case")]
pub enum UserAction {
    Focus {
        pane: PaneId,
    },
    MoveCursor {
        pane: PaneId,
        delta: isize,
    },
    ScrollH {
        pane: PaneId,
        delta: isize,
    },
    ToggleDiff {
        pane: PaneId,
    },
    /// `p`: pause or resume a tail pane; any other pane does nothing.
    TogglePause {
        pane: PaneId,
    },
    /// `r`: the cursor's thread, else all. `line`: a card's 1-based line; the browser toggles that
    /// thread alone.
    ToggleThread {
        pane: PaneId,
        #[serde(default)]
        line: Option<u32>,
    },
    JumpThread {
        pane: PaneId,
        next: bool,
    },
    ClearHighlight {
        pane: PaneId,
    },
    /// `c` on the cursor's line; `Ok(Some(seed))`: the user's own comment to edit, else empty.
    BeginComment {
        pane: PaneId,
    },
    /// `Shift+S`: `Ok` means the caller may open the review body.
    BeginReview {
        pane: PaneId,
    },
    /// `line`: 1-based source line, as `L<n>`.
    Comment {
        pane: PaneId,
        line: u32,
        text: String,
    },
    Submit {
        pane: PaneId,
        body: String,
    },
    Refresh {
        pane: PaneId,
    },
    Close {
        pane: PaneId,
    },
    ToggleEditorView {
        pane: PaneId,
    },
    /// A click on a file pane's row. `line` is 1-based source.
    SetCursor {
        pane: PaneId,
        line: u32,
    },
    /// A relative link the user clicked in a rendered file.
    OpenLink {
        pane: PaneId,
        href: String,
    },
    /// Keys to the shell, or an editor pane's Neovim in its editor view. The web app sends these
    /// as binary frames, not JSON.
    #[serde(skip)]
    Input {
        pane: PaneId,
        bytes: Vec<u8>,
    },
    /// Always bracketed, whatever mode the child set.
    Paste {
        pane: PaneId,
        text: String,
    },
    /// `Ctrl+Q`: `Ok` means the caller may ask `y` and end the workspace.
    Quit,
    /// A browser pane's pointer, keystrokes and paste. The web app sends these without waiting
    /// for a reply: they change only the page's frames.
    BrowserMouse {
        pane: PaneId,
        #[serde(flatten)]
        mouse: browser::Mouse,
    },
    BrowserKey {
        pane: PaneId,
        #[serde(flatten)]
        key: browser::Key,
    },
    BrowserText {
        pane: PaneId,
        text: String,
    },
    /// `Esc` gave the keystrokes back to Laura: the page's field lets go of the focus.
    BrowserBlur {
        pane: PaneId,
    },
    /// Back (`-1`) or forward (`1`) in a browser pane's history.
    BrowserHistory {
        pane: PaneId,
        delta: isize,
    },
    /// The answer to a browser pane's dialog: OK or Cancel, and a `prompt`'s text.
    BrowserDialog {
        pane: PaneId,
        accept: bool,
        #[serde(default)]
        text: String,
    },
    /// The address field's text.
    BrowserNavigate {
        pane: PaneId,
        url: String,
    },
    /// Start a comment on the element at the point, in CSS px from the canvas's top left.
    BrowserPick {
        pane: PaneId,
        x: f64,
        y: f64,
    },
    /// Comment mode on a browser pane: a hover outlines the element under the pointer.
    BrowserCommentMode {
        pane: PaneId,
        on: bool,
    },
    /// The pane header's highlight chip: scroll the highlight's element into view.
    BrowserReveal {
        pane: PaneId,
    },
}

fn no_pane(pane: PaneId) -> String {
    format!("no pane #{pane}")
}

/// The notice for a browser pane's threads that just lost their element, by first comment.
fn lost_notice(gone: &[String]) -> Option<String> {
    match gone {
        [] => None,
        [one] => {
            let one = one.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut cut: String = one.chars().take(40).collect();
            if cut.len() < one.len() {
                cut.push('…');
            }
            Some(format!(
                "⚠ The thread \"{cut}\" is only in the thread list, because the page lost its element."
            ))
        }
        _ => Some(format!(
            "⚠ {} threads are only in the thread list, because the page lost their elements.",
            gone.len()
        )),
    }
}

/// Per-tab counter for unique socket names; no clock/rng needed.
static TAB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Per-process nonce so a reused PID can't re-mint a prior process's socket names.
/// ponytail: SystemTime-nanos nonce, no rng dep. A new process always starts *after* the old
/// one exited (that's what frees the PID), so its nonce differs. Upgrade to OS rng only if
/// same-nanosecond process starts ever prove real.
pub fn process_nonce() -> u64 {
    static NONCE: OnceLock<u64> = OnceLock::new();
    *NONCE.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    })
}

/// Delete a panel's backing file if it's a Laura-owned runtime temp (e.g. a `laura tail` spool).
fn remove_if_temp(path: &str) {
    if is_runtime_temp(path) {
        let _ = std::fs::remove_file(path);
    }
}

/// Resolve `.`/`..`, symlinks, Windows case and 8.3 names so two spellings
/// of one file compare equal; a missing file falls back to the raw path.
fn normalized(p: &str) -> PathBuf {
    // Laura's browser adds the trailing slash to a bare host.
    if browser::is_url(p) {
        return p.trim_end_matches('/').into();
    }
    std::fs::canonicalize(p).unwrap_or_else(|_| p.into())
}

/// One workspace: a PTY, its panel panes, the split tree, and its own `LAURA_TAB` socket. Each Laura process hosts one; per-workspace sockets isolate them by addressing (protocol.rs).
pub struct Tab {
    pub pty: PtyTab,
    /// The pane arrangement; starts as the bare PTY.
    pub layout: Layout,
    /// File panels by id; the PTY lives in `pty`, not here.
    pub panels: HashMap<PaneId, Panel>,
    /// Focused pane; `PTY_PANE` means the shell has focus.
    pub focus: PaneId,
    /// The tab's `.sock` name (its `LAURA_TAB` address).
    pub socket: String,
    /// Set by `Open{focus}`; the run loop consumes it to focus the new pane once.
    pub pending_focus: Option<PaneId>,
    /// Tab hosts an agent (declared via `laura ready`); gates review injection.
    pub agent: bool,
    /// Per-session journal, created on `ready` (names + audits composition events).
    pub journal: Option<Journal>,
    /// The user is typing a comment/review body or answering the theme wizard: leave requests queued, and panes unreloaded, until they finish.
    pub hold: bool,
    /// Editor panes' Neovim, by pane id. Each also has its file panel in `panels`.
    pub editors: HashMap<PaneId, Nvim>,
    /// The `nvim` to run for `open --edit`, or why editor panes are off. The TUI sets it from
    /// `editor::resolve()`; defaults to off.
    pub editor: Result<PathBuf, String>,
    /// Editor panes use Laura's theme (the startup wizard's answer). The TUI sets it.
    pub editor_theme: bool,
    /// Hosted by `laura --web`; `ready` lists `web` under `experimental`.
    pub web: bool,
    /// The browser to run for `open <url>`, or why browser panes are off. `spawn_tab` sets it
    /// from `browser::resolve()`; defaults to off.
    pub browser_path: Result<PathBuf, String>,
    /// Browser panes' pages, by pane id. Each also has its panel in `panels`, with the URL as its
    /// path. Declared before `browser`, so each page closes before Laura's browser does.
    pub browsers: HashMap<PaneId, Page>,
    /// Laura's browser, launched on the first browser pane.
    pub browser: Option<Browser>,
    /// The port in the shell's `LAURA_BROWSER_URL`, where `cdp_proxy` serves Laura's browser to
    /// the agent's browser tools.
    pub browser_port: Option<u16>,
    /// Laura's browser's own CDP port for `cdp_proxy`, `0` while it doesn't run.
    chrome_port: Arc<AtomicU16>,
    /// The agent's captures of Laura's browser's pages, from `cdp_proxy`.
    captures: Option<Receiver<cdp_proxy::Capture>>,
    /// The tool connections that trace the whole browser, so a page that opens records too.
    traces: HashSet<u64>,
    /// Laura's browser while it starts off the loop, with the pane for `START_FAILED` when the
    /// user started it.
    starting: Option<(browser::Start, Option<PaneId>)>,
    /// The agent's `open <url>`, which waits for Laura's browser to start.
    parked: Option<(Message, Reply)>,
    /// Each browser pane's last size from the view, for the page Laura opens there next.
    pub browser_sizes: HashMap<PaneId, browser::Size>,
    /// Notices for the view, which the web app shows and clears, each with the pane to show it in.
    pub notices: Vec<(Option<PaneId>, String)>,
    /// The editor pane `shell_done` last focused for `SHELL_EXITED`.
    shell_exit_pane: Option<PaneId>,
    /// Spawn sequence number, for the default session id.
    seq: u64,
    next_pane: PaneId,
    rx: Receiver<(Message, Reply)>,
    pty_size: (u16, u16),
    /// The last draw area `drain` saw; `OpenLink` splits against it.
    area: Rect,
}

impl Tab {
    /// Mint a unique socket, serve it, point `cmd`'s `LAURA_TAB` at it and `LAURA_RUNTIME` at
    /// the host's spool dir, spawn the PTY.
    pub fn spawn(mut cmd: CommandBuilder, rows: u16, cols: u16) -> Result<Tab> {
        let n = TAB_COUNTER.fetch_add(1, Ordering::Relaxed);
        let socket = format!(
            "laura-{}-{:x}-{}.sock",
            std::process::id(),
            process_nonce(),
            n
        );
        let rx = protocol::serve(&socket)?;
        cmd.env("LAURA_TAB", &socket);
        cmd.env("LAURA_RUNTIME", runtime_dir());
        let pty = PtyTab::spawn(cmd, rows, cols)?;
        Ok(Tab {
            pty,
            layout: Layout::Pane(PTY_PANE),
            panels: HashMap::new(),
            focus: PTY_PANE,
            socket,
            pending_focus: None,
            agent: false,
            journal: None,
            hold: false,
            editors: HashMap::new(),
            editor: Err(editor::FLAG_OFF.into()),
            editor_theme: false,
            web: false,
            browser_path: Err(browser::FLAG_OFF.into()),
            browsers: HashMap::new(),
            browser: None,
            browser_port: None,
            chrome_port: Arc::default(),
            captures: None,
            traces: HashSet::new(),
            starting: None,
            parked: None,
            browser_sizes: HashMap::new(),
            notices: Vec::new(),
            shell_exit_pane: None,
            seq: n,
            next_pane: 1,
            rx,
            pty_size: (rows, cols),
            area: Rect::default(),
        })
    }

    /// `spawn` for `laura --web`, with browser panes resolved to `browser_path`. With a browser to
    /// run, the shell's `LAURA_BROWSER_URL` names a free port, where `cdp_proxy` serves Laura's
    /// browser, so the agent's browser tools reach the panes' pages.
    pub fn spawn_web(
        mut cmd: CommandBuilder,
        rows: u16,
        cols: u16,
        browser_path: Result<PathBuf, String>,
    ) -> Result<Tab> {
        let proxy = match browser_path {
            Ok(_) => Some(std::net::TcpListener::bind(("127.0.0.1", 0))?),
            Err(_) => None,
        };
        let port = match &proxy {
            Some(l) => Some(l.local_addr()?.port()),
            None => None,
        };
        if let Some(port) = port {
            cmd.env("LAURA_BROWSER_URL", browser::endpoint(port));
        }
        let mut tab = Tab::spawn(cmd, rows, cols)?;
        if let Some(l) = proxy {
            tab.captures = Some(cdp_proxy::serve(l, tab.chrome_port.clone()));
        }
        (tab.web, tab.browser_path, tab.browser_port) = (true, browser_path, port);
        Ok(tab)
    }

    /// Append one event to this tab's journal, if a session has been named (`ready`). Best-effort.
    /// Field names and values are public: `laura journal` filters on them and `docs/cli.md` lists them; change both together.
    pub fn log_event(&self, event: serde_json::Value) {
        if let Some(j) = &self.journal {
            j.log(event);
        }
    }

    /// Apply one user action. `Ok(Some(text))` only from `BeginComment`; `Err` carries the notice
    /// text. Every field is untrusted.
    pub fn act(&mut self, action: UserAction) -> Result<Option<String>, String> {
        use UserAction as A;
        match action {
            A::Focus { pane } => {
                if pane != PTY_PANE && !self.panels.contains_key(&pane) {
                    return Err(no_pane(pane));
                }
                self.focus = pane;
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
            A::BeginComment { pane } => {
                if !self.agent {
                    return Err(NEEDS_READY.into());
                }
                let p = self.panel_mut(pane)?;
                if p.diff_view {
                    return Err(NO_COMMENT_IN_DIFF.into());
                }
                p.set_cursor_line(p.cursor_line()); // snaps to a markdown block's first row
                return Ok(Some(p.begin_comment()));
            }
            A::Comment { pane, line, text } => {
                if !self.agent {
                    return Err(NEEDS_READY.into());
                }
                let p = self.panel_mut(pane)?;
                if p.diff_view {
                    return Err(NO_COMMENT_IN_DIFF.into());
                }
                p.set_cursor_line(line);
                p.author_note(text);
            }
            A::BeginReview { pane } => self.review_gate(pane)?,
            A::Submit { pane, body } => self.submit(pane, &body)?,
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
                if let Response::Error { message } = self.close_pane(Some(pane)) {
                    return Err(message);
                }
            }
            A::ToggleEditorView { pane } => {
                let e = self.editors.get_mut(&pane).ok_or_else(|| no_pane(pane))?;
                e.editor_view = !e.editor_view;
            }
            A::Input { pane, bytes } => self.write_keys(pane, &bytes)?,
            A::Paste { pane, text } => self.write_keys(pane, &bracketed_paste(&text, false))?,
            A::Quit => {
                if let Some(message) = self.unsaved_edits() {
                    return Err(message);
                }
            }
            A::BrowserMouse { pane, mouse } => self.page(pane)?.mouse(&mouse, Instant::now()),
            A::BrowserKey { pane, key } => self.page(pane)?.key(&key, Instant::now()),
            A::BrowserText { pane, text } => self.page(pane)?.insert_text(&text, Instant::now()),
            A::BrowserBlur { pane } => self.page(pane)?.blur(),
            A::BrowserHistory { pane, delta } => self.page(pane)?.go(delta)?,
            A::BrowserDialog { pane, accept, text } => self.page(pane)?.answer(accept, &text),
            A::BrowserNavigate { pane, url } => {
                let url = browser::address(&url)?;
                self.navigate(pane, url)?;
            }
            A::BrowserPick { pane, x, y } => {
                if !self.agent {
                    return Err(NEEDS_READY.into());
                }
                let (selector, quote, url) =
                    self.page(pane)?.pick(x, y)?.ok_or(browser::NO_ELEMENT)?;
                let p = self.panel_mut(pane)?;
                p.cursor = p.anchor(&selector, &quote, &url);
                return Ok(Some(p.begin_comment()));
            }
            A::BrowserCommentMode { pane, on } => self.page(pane)?.comment = on,
            A::BrowserReveal { pane } => self.page(pane)?.reveal(),
        }
        Ok(None)
    }

    /// Pane `pane`'s page in Laura's browser.
    fn page(&mut self, pane: PaneId) -> Result<&mut Page, String> {
        self.browsers.get_mut(&pane).ok_or_else(|| no_pane(pane))
    }

    /// The element that the agent's `selector` finds on browser pane `pane`'s page: its selector,
    /// quote and URL, for `Panel::anchor`.
    fn element(&self, pane: PaneId, selector: &str) -> Result<(String, String, String), String> {
        let page = self
            .browsers
            .get(&pane)
            .ok_or_else(|| browser::EXITED_AGENT.to_string())?;
        if page.dialog_open() {
            return Err(format!(
                "pane #{pane}'s page is showing a dialog: answer it with your browser tool, \
                 or ask the user to answer it in the pane"
            ));
        }
        if page.loading() {
            return Err(format!(
                "pane #{pane}'s page is still loading: run the command again when the page has loaded"
            ));
        }
        page.find(selector)?
            .ok_or_else(|| format!("no element matches {selector} in pane #{pane}"))
    }

    /// Load `url` in browser pane `id`'s page, which keeps the pane's threads; open the page if
    /// Laura's browser exited.
    fn navigate(&mut self, id: PaneId, url: String) -> Result<(), String> {
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
    fn file_view(&self, pane: PaneId) -> Result<(), String> {
        if self.editors.get(&pane).is_some_and(|e| e.editor_view) {
            return Err(format!("pane #{pane} is in its editor view"));
        }
        Ok(())
    }

    /// The terminal that takes pane `pane`'s keys: the shell, or an editor pane's Neovim in its
    /// editor view. `None` for a file pane, or an editor pane in its file view.
    pub fn pane_pty(&self, pane: PaneId) -> Option<&PtyTab> {
        if pane == PTY_PANE {
            return Some(&self.pty);
        }
        self.editors
            .get(&pane)
            .filter(|e| e.editor_view)
            .map(|e| &e.pty)
    }

    /// Open `href`'s file, without its `?query`, beside pane `pane`'s, as `laura open` would, or move to the pane already
    /// showing it, and put the cursor on its `#heading`. A bare `#heading` moves pane `pane`'s
    /// cursor. A missing heading is a notice for the pane that shows the file.
    fn open_link(&mut self, pane: PaneId, href: &str) -> Result<(), String> {
        let missing = || format!("⚠ The link target {href} does not exist.");
        let (path, heading) = href.split_once('#').unwrap_or((href, ""));
        let path = path.split_once('?').map_or(path, |(p, _)| p);
        let (path, heading) = (
            web::pct_decode(path).ok_or_else(missing)?,
            web::pct_decode(heading).ok_or_else(missing)?,
        );
        let mut target = pane;
        let dir = Path::new(&self.panel_mut(pane)?.path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        if !path.is_empty() {
            // A root or drive (`/x`, `//host/share`, `C:x`) would replace the folder in `join`.
            let rel = Path::new(&path);
            if !rel.components().all(|c| {
                matches!(
                    c,
                    Component::Normal(_) | Component::CurDir | Component::ParentDir
                )
            }) {
                return Err(missing());
            }
            // As `laura open` names it, so `..` and `/` match the CLI's path on Windows.
            let file = std::path::absolute(dir.join(rel)).map_err(|_| missing())?;
            if file.is_dir() {
                return Err(format!("⚠ The link target {href} is a folder."));
            }
            if !file.is_file() {
                return Err(missing());
            }
            let file = file.to_string_lossy().into_owned();
            if let Some(id) = self.showing(&file, PTY_PANE) {
                self.focus = id;
                target = id;
            } else {
                let msg = serde_json::from_value(json!({"type": "open", "path": file}))
                    .expect("a path-only open always parses");
                match self.apply(msg, self.area) {
                    Response::Opened { pane, .. } => target = pane,
                    Response::Error { message } => return Err(message),
                    _ => {}
                }
            }
        }
        // Not `panel_mut`: an editor pane in its editor view keeps the heading for its file view.
        let p = self
            .panels
            .get_mut(&target)
            .ok_or_else(|| no_pane(target))?;
        if !heading.is_empty() && !p.blocks().is_empty() {
            match p.heading_line(&heading) {
                Some(line) => p.set_cursor_line(line),
                // A notice, not an error: an error shows in the link's pane, and `target` can be another.
                None => {
                    let name = Path::new(&p.path).file_name().unwrap_or_default();
                    let text = format!(
                        "⚠ The file {} has no heading #{heading}.",
                        name.to_string_lossy()
                    );
                    self.notices.push((Some(target), text));
                }
            }
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

    /// `Shift+S`'s gates, checked again on `Enter`.
    fn review_gate(&self, pane: PaneId) -> Result<(), String> {
        self.file_view(pane)?;
        let p = self.panels.get(&pane).ok_or_else(|| no_pane(pane))?;
        // A dead shell's PTY can still take the write, so a submit would clear the threads.
        if self.pty.has_exited() {
            return Err(NO_SHELL.into());
        }
        if !self.agent {
            return Err(NEEDS_READY.into());
        }
        if p.thread_count() == 0 {
            return Err(NO_THREADS.into());
        }
        Ok(())
    }

    /// `Shift+S` → `Enter`: write pane `pane`'s inline review with `body` into the shell and
    /// journal it. A sent review returns focus to the shell, and an editor pane to its editor
    /// view; a failed one keeps the threads for a retry.
    fn submit(&mut self, pane: PaneId, body: &str) -> Result<(), String> {
        self.review_gate(pane)?;
        let pty = &self.pty;
        let p = self
            .panels
            .get_mut(&pane)
            .expect("review_gate found the pane");
        // Read the count first: a sent review clears the threads.
        let (path, comments) = (p.path.clone(), p.thread_count());
        let res = p.submit_review(pane, body, |r| pty.write(&bracketed_paste(r, true)));
        let mut event = json!({"type": "review", "pane": pane, "path": path, "comments": comments, "body": body});
        if let Err(e) = &res {
            event["error"] = json!(e.to_string());
        }
        self.log_event(event);
        // The cause is in the journal; the notice stays fixed.
        res.map_err(|_| SUBMIT_FAILED.to_string())?;
        if let Some(e) = self.editors.get_mut(&pane) {
            e.editor_view = true;
        }
        self.focus = PTY_PANE;
        Ok(())
    }

    /// Per-frame upkeep before drawing `content`: take `pending_focus`, move focus off a closed
    /// pane, resize the shell and each editor pane's Neovim to its rect. Returns the rects.
    pub fn tick(&mut self, content: Rect) -> HashMap<PaneId, Rect> {
        if let Some(id) = self.pending_focus.take() {
            self.focus = id;
        }
        if self.focus != PTY_PANE && !self.panels.contains_key(&self.focus) {
            self.focus = PTY_PANE;
        }
        let rect_map = rects(&self.layout, content);
        let pty_inner = self.inner(rect_map.get(&PTY_PANE).copied().unwrap_or(content));
        self.resize_to(pty_inner.height, pty_inner.width);
        for (id, r) in &rect_map {
            let inner = self.inner(*r);
            if let Some(e) = self.editors.get_mut(id) {
                e.resize(inner.height, inner.width);
            }
        }
        rect_map
    }

    /// Inside pane rect `r`'s border. The web app's pane header takes two rows, so there the
    /// border is 2 rows on top and 1 below; the TUI's is 1 and 1.
    fn inner(&self, r: Rect) -> Rect {
        if self.web {
            Rect {
                x: r.x + 1,
                y: r.y + 2,
                width: r.width.saturating_sub(2),
                height: r.height.saturating_sub(3),
            }
        } else {
            r.inner(Margin::new(1, 1))
        }
    }

    /// The focused panel, if a panel (not the PTY) has focus.
    pub fn focused_panel(&self) -> Option<&Panel> {
        (self.focus != PTY_PANE)
            .then_some(self.focus)
            .and_then(|id| self.panels.get(&id))
    }

    /// Borrow every panel keyed by id (for `build_report`).
    fn panel_refs(&self) -> HashMap<PaneId, &Panel> {
        self.panels.iter().map(|(k, v)| (*k, v)).collect()
    }

    /// Current geometry + overflow at draw area `area`.
    pub fn report(&self, area: Rect) -> LayoutReport {
        build_report(
            &self.layout,
            &self.panel_refs(),
            &self.editors,
            &self.browsers,
            area,
            self.web,
        )
    }

    /// Drain queued requests, live-reloading panels before each so it resolves against current disk, then applying it and replying. `area` is the current draw area (for `Layout`/dry-run reports).
    /// Returns whether anything changed: a message applied, a file reloaded or froze, a Neovim exited.
    pub fn drain(&mut self, area: Rect) -> bool {
        self.area = area;
        // Before the hold: a frame or an exited browser doesn't wait for the user's draft.
        let browsed = self.poll_browsers();
        if self.hold {
            return browsed;
        }
        let started = self.poll_start(area);
        // An exited Neovim closes its pane, unless it holds threads: then it stays a file pane.
        let exited: Vec<PaneId> = self
            .editors
            .iter()
            .filter(|(_, e)| e.pty.has_exited())
            .map(|(id, _)| *id)
            .collect();
        let mut changed = browsed || started || !exited.is_empty();
        for id in exited {
            self.editors.remove(&id);
            if self.panels.get(&id).is_some_and(|p| p.thread_count() == 0) {
                self.close_pane(Some(id));
            }
        }
        loop {
            // Catch up to disk before each message: one sent right after an edit must see the new content.
            for p in self.panels.values_mut() {
                // A freeze returns false, so compare `source_changed` too.
                let frozen = p.source_changed;
                changed |= p.reload_if_changed() || p.source_changed != frozen;
            }
            // While Laura's browser starts, the agent's requests wait, as for a hold.
            if self.starting.is_some() {
                break;
            }
            let Ok((msg, reply)) = self.rx.try_recv() else {
                break;
            };
            // An `open <url>` that can open a page waits for Laura's browser to start. A gate error
            // falls through to `apply`, which returns it.
            let opens_page = matches!(
                &msg,
                Message::Open { path, dry_run: false, highlight: None, diff: false, edit: false, .. }
                    if browser::is_url(path)
            );
            if opens_page && self.browser.is_none() && self.start_browser(None).is_ok() {
                self.parked = Some((msg, reply));
                break;
            }
            let resp = self.apply(msg, area);
            reply.send(&resp);
            changed = true;
        }
        // Every change to a thread (comment, submit, `Ctrl+R`, freeze) reaches Neovim from here.
        for (id, e) in &mut self.editors {
            if let Some(p) = self.panels.get(id) {
                e.push(p);
            }
        }
        changed
    }

    /// Take each page's news: a new URL goes into its panel's path, a failed load into a notice, and
    /// the agent's capture of it to the view.
    /// When Laura's browser exits, drop it and its pages, and keep the panes for `Ctrl+R`.
    fn poll_browsers(&mut self) -> bool {
        if self.browser.as_ref().is_some_and(|b| b.cdp.closed()) {
            // The focused browser pane, else the first one: `Ctrl+R` there starts the browser.
            let browser_pane = |id: &PaneId| self.panels.get(id).is_some_and(|p| p.browser);
            let pane = Some(self.focus).filter(browser_pane).or_else(|| {
                self.panels
                    .keys()
                    .filter(|id| browser_pane(id))
                    .min()
                    .copied()
            });
            self.browsers.clear();
            self.browser = None;
            self.chrome_port.store(0, Ordering::SeqCst);
            self.notices.push((pane, browser::EXITED.into()));
            return true;
        }
        let (now, mut changed, mut closed, mut crashed) = (Instant::now(), false, vec![], vec![]);
        for (id, page) in &mut self.browsers {
            for event in page.poll(now) {
                changed = true;
                match event {
                    browser::Event::Closed => closed.push(*id),
                    browser::Event::Crashed => crashed.push(*id),
                    browser::Event::Url(url) => {
                        if let Some(p) = self.panels.get_mut(id) {
                            p.path = url;
                        }
                    }
                    browser::Event::Failed { url, error } => self.notices.push((
                        Some(*id),
                        format!("⚠ The page {url} did not load ({error})."),
                    )),
                    browser::Event::Download => self.notices.push((
                        Some(*id),
                        "⚠ Laura's browser does not support downloads.".into(),
                    )),
                    browser::Event::Lost { url, lines } => {
                        let gone = match self.panels.get_mut(id) {
                            Some(p) => p.set_lost(&url, &lines),
                            None => vec![],
                        };
                        if let Some(n) = lost_notice(&gone) {
                            self.notices.push((Some(*id), n));
                        }
                    }
                }
            }
            if let Some(p) = self.panels.get(id) {
                page.draw(p.scene(self.focus == *id, self.hold));
            }
        }
        // A capture of a page that no pane shows, such as one the tools opened, shows nothing.
        for c in self.captures.iter().flat_map(Receiver::try_iter) {
            let every = c.target == cdp_proxy::EVERY_PAGE;
            if every && let cdp_proxy::Kind::Recording(on) = c.kind {
                if on {
                    self.traces.insert(c.conn);
                } else {
                    self.traces.remove(&c.conn);
                }
            }
            for page in self
                .browsers
                .values_mut()
                .filter(|p| every || p.target() == c.target)
            {
                match c.kind {
                    cdp_proxy::Kind::Screenshot => page.screenshot(),
                    cdp_proxy::Kind::Recording(on) => page.record(c.conn, on),
                }
                changed = true;
            }
        }
        // No reopen here, so a page that crashes as it loads doesn't loop: `Ctrl+R` opens it.
        for id in crashed {
            self.browsers.remove(&id);
            self.notices.push((
                Some(id),
                "⚠ To load the page again after it stopped, press Ctrl+R.".into(),
            ));
        }
        // The agent's tools closed a pane's page: open it again, and `draw` draws its threads.
        if !closed.is_empty() {
            for id in &closed {
                self.browsers.remove(id);
            }
            // A browser that is exiting closes its pages first; the next poll shows `EXITED`.
            if let Err(e) = self.attach_pages()
                && !self.browser.as_ref().is_some_and(|b| b.cdp.closed())
            {
                self.notices.push((None, format!("⚠ {e}")));
            }
        }
        changed
    }

    /// Take Laura's browser once its start ends: apply the agent's parked `open <url>`, then open
    /// every other browser pane's page. On a failure, journal the cause and answer with the reason.
    fn poll_start(&mut self, area: Rect) -> bool {
        let Some((start, pane)) = &self.starting else {
            return false;
        };
        let Some(res) = start.poll() else {
            return false;
        };
        let pane = *pane;
        self.starting = None;
        let parked = self.parked.take();
        match res {
            Ok(b) => {
                self.chrome_port.store(b.port, Ordering::SeqCst);
                self.browser = Some(b);
                // The parked message first: an `open <url> --panel` opens its pane's page at the
                // new URL, and `attach_pages` doesn't open it at the old one.
                if let Some((msg, reply)) = parked {
                    let resp = self.apply(msg, area);
                    reply.send(&resp);
                }
                if let Err(e) = self.attach_pages() {
                    self.notices.push((pane, format!("⚠ {e}")));
                }
            }
            Err(e) => {
                let reason = format!("{:#}", e.reason);
                self.log_event(json!({"type": "browser", "error": reason, "output": e.output}));
                if let Some((_, reply)) = parked {
                    reply.send(&Response::Error {
                        message: format!("couldn't start Laura's browser: {reason}"),
                    });
                }
                // The cause is in the journal; the notice stays fixed.
                if pane.is_some() {
                    self.notices.push((pane, browser::START_FAILED.into()));
                }
            }
        }
        true
    }

    /// Why `open <url>` can't open a browser pane, in the order the user fixes them: the flag,
    /// then `--web`, then a browser to run.
    fn browser_gate(&self) -> Result<PathBuf, String> {
        match &self.browser_path {
            Err(e) if e == browser::FLAG_OFF => Err(e.clone()),
            _ if !self.web => Err(browser::NEED_WEB.into()),
            r => r.clone(),
        }
    }

    /// Start Laura's browser off the loop, unless it is starting; `poll_start` takes it. `pane`
    /// gets the notice if the start fails.
    fn start_browser(&mut self, pane: Option<PaneId>) -> Result<(), String> {
        let path = self.browser_gate()?;
        match &mut self.starting {
            Some((_, p)) => *p = p.or(pane),
            None => {
                let rx = Browser::launch(&path);
                self.starting = Some((rx, pane));
            }
        }
        Ok(())
    }

    /// Laura's browser's CDP client.
    fn cdp(&self) -> Result<Cdp, String> {
        self.browser
            .as_ref()
            .map(|b| b.cdp.clone())
            .ok_or_else(|| browser::EXITED_AGENT.to_string())
    }

    /// Open a page for every browser pane without one, at its URL. A page that fails to open
    /// doesn't stop the others.
    fn attach_pages(&mut self) -> Result<(), String> {
        self.cdp()?;
        let missing: Vec<(PaneId, String)> = self
            .panels
            .iter()
            .filter(|(id, p)| p.browser && !self.browsers.contains_key(id))
            .map(|(id, p)| (*id, p.path.clone()))
            .collect();
        let mut failures = vec![];
        for (id, url) in missing {
            match self.open_page(id, &url) {
                Ok(page) => {
                    self.browsers.insert(id, page);
                }
                Err(e) => failures.push(e),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    /// Open pane `id`'s page at `url`, at the pane's size.
    /// shortcut: `Page::open`'s setup calls still block the loop, up to 5 s each on a slow
    /// browser; move them off the loop, as `Browser::launch` is, if that is reported.
    fn open_page(&mut self, id: PaneId, url: &str) -> Result<Page, String> {
        let cdp = self.cdp()?;
        let size = self
            .browser_sizes
            .get(&id)
            .copied()
            .unwrap_or(browser::DEFAULT_SIZE);
        let mut page =
            Page::open(&cdp, url, size).map_err(|e| format!("couldn't open {url}: {e:#}"))?;
        for &conn in &self.traces {
            page.record(conn, true);
        }
        if let Some(b) = &mut self.browser {
            b.close_start();
        }
        Ok(page)
    }

    /// Close a pane (default: the focused panel). Shared by the `close` verb and the `x` key, so
    /// both refuse to drop unsaved Neovim edits.
    pub fn close_pane(&mut self, pane: Option<PaneId>) -> Response {
        let target = pane.or((self.focus != PTY_PANE).then_some(self.focus));
        let Some(target) = target else {
            return Response::Error {
                message: "no pane focused to close".into(),
            };
        };
        if let Err(e) = self.edits_guard(target) {
            return e;
        }
        match self.layout.remove(target) {
            Ok(()) => {
                if let Some(p) = self.panels.remove(&target) {
                    remove_if_temp(&p.path);
                }
                self.editors.remove(&target); // drop kills Neovim
                // Laura's browser keeps running, so the next browser pane opens fast.
                self.browsers.remove(&target);
                self.browser_sizes.remove(&target);
                if self.focus == target {
                    self.focus = PTY_PANE;
                }
                self.log_event(json!({"type": "close", "pane": target}));
                Response::Ok
            }
            Err(message) => Response::Error { message },
        }
    }

    /// Build a panel for `path` (follow → highlight → diff, order preserved), insert it at `id`,
    /// and collect the standard open warnings (ready / read-error / git / diff-refusal / overflow).
    /// Returns the warnings; logging stays with the caller (open-event differs per path).
    fn install_panel(
        &mut self,
        id: PaneId,
        path: &str,
        follow: bool,
        highlight: Option<(u32, u32)>,
        diff: bool,
        area: Rect,
    ) -> Vec<String> {
        // A file in a browser pane's place closes its page.
        self.browsers.remove(&id);
        let mut diff_warning = None;
        let panel = if browser::is_url(path) {
            Panel::page(path.to_string())
        } else {
            let mut panel = Panel::open(path.to_string());
            panel.set_follow(follow);
            // `set_highlight` after `set_follow` so an explicit highlight range
            // wins the *first* frame's viewport over tail-follow when both are set.
            if let Some((start, end)) = highlight {
                panel.set_highlight(start, end);
            }
            // Open-into-diff: refusal (no git / nothing to diff) becomes a warning.
            diff_warning = diff.then(|| panel.set_diff_view(true).err()).flatten();
            panel
        };
        self.panels.insert(id, panel);
        let mut warnings = vec![];
        if !self.agent {
            warnings.push(
                "pane shown, but run `laura ready` to enable inline review submission".into(),
            );
        }
        if let Some(e) = &self.panels[&id].read_error {
            warnings.push(e.clone());
        }
        if self.panels[&id].git_missing {
            warnings.push(
                "diff markers unavailable — install `git` to see changes in the gutter".into(),
            );
        }
        if let Some(w) = diff_warning {
            warnings.push(w);
        }
        let report = self.report(area);
        if let Some(w) = report
            .panes
            .iter()
            .find(|p| p.id == id)
            .and_then(overflow_warning)
        {
            warnings.push(w);
        }
        warnings
    }

    /// Refuse an agent verb that would drop pane `id`'s unsubmitted comment threads.
    fn threads_guard(&self, id: PaneId) -> Result<(), Response> {
        match self.panels.get(&id) {
            Some(p) if p.thread_count() > 0 => Err(Response::Error {
                message: format!(
                    "pane #{id} has an unsubmitted inline review — ask the user to submit (Shift+S) or refresh (Ctrl+R) first"
                ),
            }),
            _ => Ok(()),
        }
    }

    /// Refuse to kill pane `id`'s Neovim while it has unsaved edits.
    fn edits_guard(&self, id: PaneId) -> Result<(), Response> {
        match self.editors.get(&id) {
            Some(e) if e.modified() => Err(Response::Error {
                message: unsaved_message(id),
            }),
            _ => Ok(()),
        }
    }

    /// Why closing the tab would drop unsaved Neovim edits: the lowest-id editor pane with them,
    /// `(+N more)` for the rest. `None` when every Neovim is saved.
    pub fn unsaved_edits(&self) -> Option<String> {
        let ids: Vec<PaneId> = self
            .editors
            .iter()
            .filter(|(_, e)| e.modified())
            .map(|(&id, _)| id)
            .collect();
        let mut message = unsaved_message(*ids.iter().min()?);
        if ids.len() > 1 {
            message += &format!(" (+{} more)", ids.len() - 1);
        }
        Some(message)
    }

    /// The lowest-id editor pane with unsaved edits.
    fn unsaved_pane(&self) -> Option<PaneId> {
        self.editors
            .iter()
            .filter(|(_, e)| e.modified())
            .map(|(&id, _)| id)
            .min()
    }

    /// True when the shell exited and no Neovim has unsaved edits. Until then, show the first
    /// editor pane with unsaved edits in the editor view, focus it and show `SHELL_EXITED`, once
    /// per pane: again only after the user saves or quits that pane's Neovim.
    pub fn shell_done(&mut self) -> bool {
        if !self.pty.has_exited() {
            return false;
        }
        let Some(id) = self.unsaved_pane() else {
            return true;
        };
        let shown = self
            .shell_exit_pane
            .and_then(|p| self.editors.get(&p))
            .is_some_and(|e| e.modified());
        if !shown {
            self.shell_exit_pane = Some(id);
            if let Some(e) = self.editors.get_mut(&id) {
                e.editor_view = true;
            }
            self.focus = id;
            self.notices.push((Some(id), SHELL_EXITED.into()));
        }
        false
    }

    /// The lowest-id pane other than `except` that shows `path`.
    fn showing(&self, path: &str, except: PaneId) -> Option<PaneId> {
        let this = normalized(path);
        self.panels
            .iter()
            .filter(|(id, p)| **id != except && normalized(&p.path) == this)
            .map(|(id, _)| *id)
            .min()
    }

    /// Warn if `path` is already open in a pane other than `except` (lowest id wins).
    fn already_open(&self, path: &str, except: PaneId) -> Option<String> {
        let n = self.showing(path, except)?;
        // Suggest reuse verbs only where they work: not under review, nor on an editor pane (it refuses highlight; a bare --panel quits Neovim).
        Some(if self.panels[&n].thread_count() > 0 {
            format!("already open in pane #{n}, which has an unsubmitted inline review")
        } else if self.editors.contains_key(&n) {
            format!("already open in editor pane #{n}")
        } else if self.panels[&n].browser {
            format!("already open in pane #{n} — use `laura open <url> --panel {n}`")
        } else {
            format!(
                "already open in pane #{n} — use `laura highlight --pane {n}` \
                 or `laura open --panel {n}`"
            )
        })
    }

    /// Start Neovim on `path` in pane `id`, sized to the pane's inner rect.
    fn attach_editor(&mut self, id: PaneId, path: &str, area: Rect) -> Result<(), Response> {
        let nvim = self
            .editor
            .clone()
            .map_err(|message| Response::Error { message })?;
        // Drop the old Neovim first: its Drop deletes the same bootstrap files the new one writes.
        let was = self.editors.remove(&id).map(|e| e.editor_view);
        let inner = rects(&self.layout, area)
            .get(&id)
            .map(|r| self.inner(*r))
            .unwrap_or_default();
        let mut e = Nvim::spawn(
            &nvim,
            path,
            &self.socket,
            id,
            self.editor_theme,
            inner.height.max(1),
            inner.width.max(1),
        )
        .map_err(|e| Response::Error {
            message: format!("couldn't edit {path}: {e:#}"),
        })?;
        // A file pane the user is in opens in the file view, so their next keys don't land in Neovim.
        e.editor_view = was.unwrap_or(self.focus != id);
        self.editors.insert(id, e);
        Ok(())
    }

    /// Replace pane `id`'s content in place: same id, rect, and focus, no new split.
    /// `--edit` on the file the pane already shows attaches Neovim and keeps its threads.
    #[allow(clippy::too_many_arguments)] // one per `open` flag it honors
    fn replace_panel(
        &mut self,
        id: PaneId,
        path: String,
        follow: bool,
        highlight: Option<(u32, u32)>,
        diff: bool,
        edit: bool,
        area: Rect,
    ) -> Response {
        if id == PTY_PANE || !self.panels.contains_key(&id) {
            return Response::Error {
                message: format!("no pane #{id} to replace"),
            };
        }
        // A URL into a browser pane navigates its page, so the agent's browser tools keep their
        // page, and the pane keeps its threads.
        if self.panels[&id].browser && browser::is_url(&path) {
            let dup = self.already_open(&path, id);
            if let Err(message) = self.navigate(id, path.clone()) {
                return Response::Error { message };
            }
            self.log_event(json!({"type":"open","pane":id,"path":path,"replaced":true}));
            return Response::Opened {
                pane: id,
                warnings: dup.into_iter().collect(),
            };
        }
        if edit && normalized(&self.panels[&id].path) == normalized(&path) {
            if !self.editors.contains_key(&id) {
                if let Err(e) = self.attach_editor(id, &path, area) {
                    return e;
                }
                if let Some(p) = self.panels.get_mut(&id) {
                    p.clear_highlight();
                    p.diff_view = false;
                }
                self.log_event(
                    json!({"type":"open","pane":id,"path":path,"edit":true,"replaced":true}),
                );
            }
            return Response::Opened {
                pane: id,
                warnings: vec![],
            };
        }
        if let Err(e) = self.threads_guard(id).and(self.edits_guard(id)) {
            return e;
        }
        // Open the page before the file goes, so a page that won't open keeps the pane.
        let page = match browser::is_url(&path)
            .then(|| self.open_page(id, &path))
            .transpose()
        {
            Ok(page) => page,
            Err(message) => return Response::Error { message },
        };
        if edit {
            if let Err(e) = self.attach_editor(id, &path, area) {
                return e;
            }
        } else {
            self.editors.remove(&id);
        }
        remove_if_temp(&self.panels[&id].path); // clean an old tail spool, same as the close path
        let dup = self.already_open(&path, id);
        let mut warnings = self.install_panel(id, &path, follow, highlight, diff, area);
        if let Some(page) = page {
            self.browsers.insert(id, page);
        }
        warnings.extend(dup);
        let mut event = json!({"type":"open","pane":id,"path":path,"replaced":true});
        if edit {
            event["edit"] = true.into();
        }
        self.log_event(event);
        Response::Opened { pane: id, warnings }
    }

    /// Apply one request to layout/panel state and produce its response.
    fn apply(&mut self, msg: Message, area: Rect) -> Response {
        match msg {
            Message::Open {
                path,
                split,
                dir,
                ratio,
                side,
                focus,
                follow,
                dry_run,
                highlight,
                diff,
                panel,
                edit,
            } => {
                // A URL opens a browser pane. Its errors come first, one at a time; `--follow`
                // changes nothing, since the pane always shows the page live.
                let url = browser::is_url(&path);
                if url {
                    if let Err(message) = self.browser_gate() {
                        return Response::Error { message };
                    }
                    let flag = [
                        (highlight.is_some(), browser::NO_HIGHLIGHT),
                        (diff, browser::NO_DIFF),
                        (edit, browser::NO_EDIT),
                    ];
                    if let Some((_, message)) = flag.into_iter().find(|(on, _)| *on) {
                        return Response::Error {
                            message: message.into(),
                        };
                    }
                }
                if edit && let Err(message) = &self.editor {
                    return Response::Error {
                        message: message.clone(),
                    };
                }
                if edit && (highlight.is_some() || diff) {
                    return Response::Error {
                        message: editor::SHOWS_NEITHER.into(),
                    };
                }
                // ponytail: `--panel` short-circuits before the dry_run branch, so a wire-only
                // `{panel, dry_run:true}` would mutate. The CLI can't send it (--panel/--dry-run
                // don't conflict but replace ignores dry_run). Guard only if a replace preview is wanted.
                if let Some(id) = panel {
                    return self.replace_panel(id, path, follow, highlight, diff, edit, area);
                }
                let new = self.next_pane;
                let inferred =
                    (dir.is_none() && ratio.is_none() && side.is_none() && split.is_none())
                        .then(|| infer_split(&self.layout, area, new))
                        .flatten();
                let (target, dir, ratio, side) = match inferred {
                    Some(t) => t,
                    None => (
                        split.unwrap_or(self.focus),
                        dir.unwrap_or_default(),
                        ratio.unwrap_or(50),
                        side.unwrap_or_default(),
                    ),
                };
                // A stacked pane gets only the rows its file needs, up to half.
                let ratio = if inferred.is_some()
                    && dir == Dir::Vertical
                    && !(edit || diff || follow || url)
                {
                    self.fit_ratio(&path, target, side, new, area)
                } else {
                    ratio
                };
                if dry_run {
                    return self.dry_run_open(&path, target, dir, ratio, side, area);
                }
                let mut probe = self.layout.clone();
                if probe.split(target, dir, ratio, side, new).is_ok()
                    && !all_panes_fit(&probe, area)
                {
                    return Response::Error {
                        message: format!(
                            "split would shrink a pane below {}×{} — too small to render; \
                             close a pane or lower --ratio",
                            MIN_PANE.0, MIN_PANE.1
                        ),
                    };
                }
                // Open the page before the split, so a page that won't open adds no pane.
                let page = match url.then(|| self.open_page(new, &path)).transpose() {
                    Ok(page) => page,
                    Err(message) => return Response::Error { message },
                };
                match self.layout.split(target, dir, ratio, side, new) {
                    Ok(()) => {
                        self.next_pane += 1;
                        if edit && let Err(e) = self.attach_editor(new, &path, area) {
                            let _ = self.layout.remove(new);
                            return e;
                        }
                        let dup = self.already_open(&path, new);
                        let mut warnings =
                            self.install_panel(new, &path, follow, highlight, diff, area);
                        if let Some(page) = page {
                            self.browsers.insert(new, page);
                        }
                        warnings.extend(dup);
                        let mut event = json!({"type": "open", "pane": new, "path": path});
                        if edit {
                            event["edit"] = true.into();
                        }
                        self.log_event(event);
                        if let Some((start, end)) = highlight {
                            self.log_event(
                                json!({"type": "highlight", "pane": new, "start": start, "end": end}),
                            );
                        }
                        // An editor pane never takes focus: typing into the chat mustn't land in Neovim.
                        if focus && !edit {
                            self.pending_focus = Some(new);
                        }
                        Response::Opened {
                            pane: new,
                            warnings,
                        }
                    }
                    Err(message) => Response::Error { message },
                }
            }
            Message::Close { pane, all } => {
                if all {
                    // All-or-nothing: one pane under review refuses the whole close.
                    if let Some(e) = self
                        .panels
                        .keys()
                        .find_map(|&id| self.threads_guard(id).and(self.edits_guard(id)).err())
                    {
                        return e;
                    }
                    for p in self.panels.values() {
                        remove_if_temp(&p.path);
                    }
                    self.layout = Layout::Pane(PTY_PANE);
                    self.panels.clear();
                    self.editors.clear();
                    self.browsers.clear();
                    self.browser_sizes.clear();
                    self.focus = PTY_PANE;
                    self.log_event(json!({"type": "close", "all": true}));
                    return Response::Ok;
                }
                // Guarded here, not in `close_pane`: the user's `x` key closes through it too.
                if let Some(id) = pane.or((self.focus != PTY_PANE).then_some(self.focus))
                    && let Err(e) = self.threads_guard(id)
                {
                    return e;
                }
                self.close_pane(pane)
            }
            Message::Focus { pane } => {
                if pane == PTY_PANE || self.panels.contains_key(&pane) {
                    self.focus = pane;
                    self.log_event(json!({"type": "focus", "pane": pane}));
                    Response::Ok
                } else {
                    Response::Error {
                        message: format!("no pane #{pane}"),
                    }
                }
            }
            Message::Highlight {
                pane,
                range,
                selector,
            } => {
                let target = pane.or((self.focus != PTY_PANE).then_some(self.focus));
                let Some(target) = target else {
                    return Response::Error {
                        message: "no pane focused to highlight".into(),
                    };
                };
                if self.editors.contains_key(&target) {
                    return Response::Error {
                        message: format!("pane #{target}: {}", editor::SHOWS_NEITHER),
                    };
                }
                let Some(browser) = self.panels.get(&target).map(|p| p.browser) else {
                    return Response::Error {
                        message: format!("no pane #{target}"),
                    };
                };
                match (browser, &selector) {
                    (true, _) if range.is_some() => {
                        return Response::Error {
                            message: format!(
                                "pane #{target} is a browser pane: pass a CSS selector"
                            ),
                        };
                    }
                    (true, Some(sel)) => {
                        let (s, q, u) = match self.element(target, sel) {
                            Ok(e) => e,
                            Err(message) => return Response::Error { message },
                        };
                        if let Some(p) = self.panels.get_mut(&target) {
                            // Not `set_highlight`: the cursor stays where the user left it.
                            let row = p.anchor(&s, &q, &u);
                            p.highlight = Some((row, row));
                        }
                        self.log_event(
                            json!({"type": "highlight", "pane": target, "selector": sel}),
                        );
                        return Response::Ok;
                    }
                    (false, Some(_)) => {
                        return Response::Error {
                            message: format!("pane #{target} is a file pane: pass a line number"),
                        };
                    }
                    _ => {}
                }
                let Some(panel) = self.panels.get_mut(&target) else {
                    return Response::Error {
                        message: format!("no pane #{target}"),
                    };
                };
                match range {
                    Some((start, end)) => {
                        if panel.source_changed {
                            return Response::Error {
                                message: format!(
                                    "pane #{target} is frozen — its file changed on disk while it has an unsubmitted inline review; ask the user to submit (Shift+S) or refresh (Ctrl+R)"
                                ),
                            };
                        }
                        panel.set_highlight(start, end);
                        let paused = panel.paused;
                        self.log_event(
                            json!({"type":"highlight","pane":target,"start":start,"end":end}),
                        );
                        if paused {
                            return Response::Warning {
                                message: format!(
                                    "pane #{target} is paused — the user doesn't see new output, so Laura highlights the content that the pane holds"
                                ),
                            };
                        }
                    }
                    None => {
                        panel.clear_highlight();
                        self.log_event(json!({"type":"highlight","pane":target,"cleared":true}));
                    }
                }
                Response::Ok
            }
            Message::DiffView { pane, on } => {
                let target = pane.or((self.focus != PTY_PANE).then_some(self.focus));
                let Some(target) = target else {
                    return Response::Error {
                        message: "no pane focused for diff view".into(),
                    };
                };
                if self.editors.contains_key(&target) {
                    return Response::Error {
                        message: format!("pane #{target}: {}", editor::SHOWS_NEITHER),
                    };
                }
                let Some(panel) = self.panels.get_mut(&target) else {
                    return Response::Error {
                        message: format!("no pane #{target}"),
                    };
                };
                if panel.browser && on != Some(false) {
                    return Response::Error {
                        message: browser::NO_DIFF.into(),
                    };
                }
                let want = on.unwrap_or(!panel.diff_view);
                match panel.set_diff_view(want) {
                    Ok(()) => {
                        self.log_event(json!({"type":"diff","pane":target,"on":want}));
                        Response::Ok
                    }
                    // Refusal is a no-op with a warning surfaced as an error (CLI → stderr).
                    Err(message) => Response::Error { message },
                }
            }
            Message::Comment {
                pane,
                line,
                selector,
                body,
                author,
            } => {
                let target = pane.or((self.focus != PTY_PANE).then_some(self.focus));
                let Some(target) = target else {
                    return Response::Error {
                        message: "no pane focused to comment on".into(),
                    };
                };
                // Default to the session's agent name; `agent_note` falls back to `"agent"`.
                let author = author.or_else(|| self.journal.as_ref().and_then(|j| j.agent()));
                let Some(browser) = self.panels.get(&target).map(|p| p.browser) else {
                    return Response::Error {
                        message: format!("no pane #{target}"),
                    };
                };
                let (line, event) = match (browser, line, selector) {
                    // Before the anchor, which the empty body would leave on the pane.
                    (true, None, Some(_))
                        if body.as_deref().is_none_or(|b| b.trim().is_empty()) =>
                    {
                        return Response::Error {
                            message: "comment needs a body".into(),
                        };
                    }
                    (true, None, Some(sel)) => {
                        let (s, q, u) = match self.element(target, &sel) {
                            Ok(e) => e,
                            Err(message) => return Response::Error { message },
                        };
                        let row = self.panels.get_mut(&target).map(|p| p.anchor(&s, &q, &u));
                        let line = row.map_or(0, |r| r as u32 + 1);
                        (
                            line,
                            json!({"type": "comment", "pane": target, "selector": sel}),
                        )
                    }
                    (true, ..) => {
                        return Response::Error {
                            message: format!(
                                "pane #{target} is a browser pane: pass a CSS selector"
                            ),
                        };
                    }
                    (false, Some(line), None) => (
                        line,
                        json!({"type": "comment", "pane": target, "line": line}),
                    ),
                    (false, ..) => {
                        return Response::Error {
                            message: format!("pane #{target} is a file pane: pass a line number"),
                        };
                    }
                };
                let Some(panel) = self.panels.get_mut(&target) else {
                    return Response::Error {
                        message: format!("no pane #{target}"),
                    };
                };
                match panel.agent_note(line, body, author) {
                    Ok(()) => {
                        self.log_event(event);
                        Response::Ok
                    }
                    Err(message) => Response::Error { message },
                }
            }
            Message::Layout => Response::Report(self.report(area)),
            Message::Ready { session, agent } => {
                self.agent = true;
                let session =
                    session.unwrap_or_else(|| format!("laura-{}-{}", std::process::id(), self.seq));
                let journal = Journal::open(&session, agent);
                let path = journal.path().to_string_lossy().into_owned();
                journal.log(json!({"type": "ready"}));
                self.journal = Some(journal);
                let experimental = [
                    self.editor.is_ok().then_some("editor"),
                    self.web.then_some("web"),
                    (self.web && self.browser_path.is_ok()).then_some("browser"),
                ]
                .into_iter()
                .flatten()
                .map(String::from)
                .collect();
                Response::Ready {
                    journal: path,
                    experimental,
                    layout: Some(self.report(area)),
                    browser_url: self.browser_port.map(browser::endpoint),
                }
            }
            Message::Feedback { sentiment, body } => {
                let pane = (self.focus != PTY_PANE).then_some(self.focus);
                self.log_event(json!({
                    "type": "feedback",
                    "sentiment": sentiment,
                    "body": body,
                    "pane": pane,
                }));
                Response::Ok
            }
            Message::Update { .. } => Response::Ok, // reserved; not yet emitted
        }
    }

    /// Build the *would-be* report for an `open` without mutating: split a clone and measure a freshly-rendered panel.
    fn dry_run_open(
        &self,
        path: &str,
        target: PaneId,
        dir: Dir,
        ratio: u16,
        side: Side,
        area: Rect,
    ) -> Response {
        let new = self.next_pane;
        let mut layout = self.layout.clone();
        if let Err(message) = layout.split(target, dir, ratio, side, new) {
            return Response::Error { message };
        }
        let temp = if browser::is_url(path) {
            Panel::page(path.to_string())
        } else {
            Panel::open(path.to_string())
        };
        let mut refs = self.panel_refs();
        refs.insert(new, &temp);
        Response::Report(build_report(
            &layout,
            &refs,
            &self.editors,
            &self.browsers,
            area,
            self.web,
        ))
    }

    /// The smallest ratio, up to 50, at which a stacked split of `target` gives `new` the rows
    /// `path` needs. Brute force: `split_rect` floors and gives the second side `100 - ratio`.
    /// The browser draws a page, and markdown blocks, taller than their rows, so the web app fits neither.
    fn fit_ratio(&self, path: &str, target: PaneId, side: Side, new: PaneId, area: Rect) -> u16 {
        let temp = Panel::open(path.to_string());
        if self.web && (web::is_page(&temp) || !temp.blocks().is_empty()) {
            return 50;
        }
        let rect = |ratio| {
            let mut l = self.layout.clone();
            l.split(target, Dir::Vertical, ratio, side, new).ok()?;
            rects(&l, area).get(&new).copied()
        };
        let Some(half) = rect(50) else { return 50 };
        let need = fit_rows(&temp, half.width, self.web);
        (1..=50)
            .find(|&r| rect(r).is_some_and(|rc| rc.height as usize >= need))
            .unwrap_or(50)
    }

    /// Resize the PTY only when its draw area changed, else the shell wraps wrong.
    pub fn resize_to(&mut self, rows: u16, cols: u16) {
        if (rows, cols) != self.pty_size {
            self.pty_size = (rows, cols);
            self.pty.resize(rows, cols);
        }
    }
}

/// Delete the tab's `laura tail` spools and release its socket. The listener thread blocks in `accept` and only learns `rx` is gone
/// from a failed send, so the pipe outlives the tab until a request wakes it, and a stale
/// `LAURA_TAB` reads that request's dropped reply as success.
/// ponytail: a self-connect wakes the accept; a nonblocking listener with a stop flag if it's flaky.
impl Drop for Tab {
    fn drop(&mut self) {
        for p in self.panels.values() {
            remove_if_temp(&p.path);
        }
        self.chrome_port.store(0, Ordering::SeqCst);
        // `rx` first: alive, it would take the wake-up request and `request` would wait on a reply.
        self.rx = channel().1;
        let _ = protocol::request(&self.socket, &Message::Layout);
    }
}

/// The unsaved-edits refusal, shared by a pane's close and the workspace's quit.
fn unsaved_message(id: PaneId) -> String {
    format!("pane #{id} has unsaved edits in Neovim — save (:w) or quit Neovim first")
}
