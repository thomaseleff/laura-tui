//! One workspace: a PTY, its panels, the split tree, the input modes, the wait, the History, and the
//! socket that ties them together.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use portable_pty::CommandBuilder;
use ratatui::layout::{Margin, Rect};

use serde::Deserialize;

use crate::browser::{self, Browser, Page};
use crate::cdp_proxy;
use crate::editor::{self, Nvim};
use crate::journal::{Event, Journal, is_runtime_temp};
use crate::layout::{Handle, Layout, MIN_PANE, rects, split_rect};
use crate::panel::{Panel, bracketed_paste};
use crate::platform::runtime_dir;
use crate::protocol::{self, Dir, Message, PTY_PANE, PaneId, Reply, Response};
use crate::pty::PtyTab;

mod act;
mod apply;
mod browsers;
mod history;
mod links;
mod mode;
mod report;
mod wait;
pub use history::{FROZEN, GONE, IN_EDITOR, NO_ELEMENT, NOT_OPEN, PAGE_NOT_READY};
use mode::DrawerView;
pub use mode::{Draft, Mode, Overlay};
pub use report::overflow_warning;

/// A link that can't open a pane, in place of `Error::SplitTooSmall`.
pub const NO_ROOM: &str = "⚠ No room for another pane. Close a pane, then click the link again.";
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
/// **Show the shell as a pane** when a pane would get too small beside the shell.
pub const NO_ROOM_TO_DOCK: &str =
    "⚠ The shell doesn't fit as a pane. Close a pane or make the window larger.";
/// A drag of the line between two panes would make a pane too small.
pub const NO_ROOM_TO_RESIZE: &str = "⚠ A pane can't get any smaller.";
/// A double-click on the line between two panes, when Laura's size would make a pane too small.
pub const NO_ROOM_TO_UNPIN: &str = "⚠ Laura's size doesn't fit in this window. Make the window larger, then double-click the line again.";
/// A resize of a line that a pane's close took away while the user dragged it.
pub const NO_SPLIT: &str = "⚠ A pane closed while you moved the line.";
/// The user answered the theme wizard.
pub const THEME_SET: &str = "✅ Experimental · Editor panes setup complete";
/// The shell exited while Neovim has unsaved edits.
pub const SHELL_EXITED: &str = "⚠ To close Laura after the shell stopped, save or quit Neovim. ";
/// `Shift+S` / `Enter` after the shell exited, which would write the inline review nowhere.
pub const NO_SHELL: &str = "⚠ The shell stopped, so Laura cannot submit the inline review. ";

/// An error from `Tab::act` or the agent's `open`. A caller matches a variant; `Text` is shown as is.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum Error {
    /// The agent's split would leave a pane below `MIN_PANE`.
    #[error(
        "split would shrink a pane below {}×{} — too small to render; close a pane or lower --ratio",
        MIN_PANE.0,
        MIN_PANE.1
    )]
    SplitTooSmall,
    /// The inline review didn't reach the shell; the cause is in the journal.
    #[error("{}", SUBMIT_FAILED)]
    SubmitFailed,
    /// The agent's selector finds no element on the browser pane's page.
    #[error("no element matches {selector} in pane #{pane}")]
    NoElement { selector: String, pane: PaneId },
    /// The browser pane's page can't be searched now: it loads or shows a dialog. The text is the
    /// agent's.
    #[error("{0}")]
    PageNotReady(String),
    /// Laura's browser exited, and the browser pane has no page.
    #[error("{}", browser::EXITED_AGENT)]
    BrowserExited,
    #[error("{0}")]
    Text(String),
}

impl From<String> for Error {
    fn from(text: String) -> Self {
        Error::Text(text)
    }
}

impl From<&str> for Error {
    fn from(text: &str) -> Self {
        Error::Text(text.into())
    }
}

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
    /// `c` on the cursor's line opens the comment's draft; `Ok(Some(seed))`: the user's own
    /// comment to edit, else empty.
    BeginComment {
        pane: PaneId,
    },
    /// `Shift+S`: `Ok` opens the review body's draft.
    BeginReview {
        pane: PaneId,
    },
    /// `line`: 1-based source line, as `L<n>`. `Ok` closes the draft.
    Comment {
        pane: PaneId,
        line: u32,
        text: String,
    },
    /// `Ok` closes the draft; a failure keeps it for a retry.
    Submit {
        pane: PaneId,
        body: String,
    },
    /// `Esc` on the draft.
    CancelDraft,
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
    /// A file path the user clicked in the shell or on the start canvas. `line` is 1-based
    /// source; `diff` opens a changed file in the diff view.
    OpenPath {
        path: String,
        #[serde(default)]
        line: Option<u32>,
        #[serde(default)]
        diff: bool,
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
    /// `Ctrl+Q`: `Ok` opens the quit question, after which the caller may end the workspace.
    Quit,
    /// `F12`: the lock on or off (`Tab::toggle_lock`).
    Lock,
    /// Open the help (`Ctrl+H`) or the pane selector (`Ctrl+P`), or close the overlay (`None`).
    Overlay {
        #[serde(default)]
        overlay: Option<Overlay>,
    },
    /// The theme wizard's answer: Laura's theme, the user's own, or not now (`None`).
    Theme {
        #[serde(default)]
        laura: Option<bool>,
    },
    /// **Show the shell as a pane**: the shell as the left-most pane, at the full height. The TUI ignores it.
    Dock,
    /// **Show the shell in a drawer**: the shell back in the drawer.
    Drawer,
    /// The user dragged the line between panes `a` and `b`: `ratio` percent for `a`'s side, which
    /// Laura keeps (pins) until the user unpins it.
    Resize {
        a: PaneId,
        b: PaneId,
        ratio: u16,
    },
    /// The user double-clicked the line between panes `a` and `b`: Laura's own ratio again.
    Unpin {
        a: PaneId,
        b: PaneId,
    },
    /// The drawer's height in % of the workspace's, which the web app saves for the user; `None`
    /// resets it. The TUI ignores it.
    DrawerHeight {
        #[serde(default)]
        percent: Option<f64>,
    },
    /// What the web app keeps from the shell and sends no bytes for: a keystroke or a click after
    /// which the shell has the keystrokes, or a wheel step over the shell. `wait` is the layout
    /// frame's `wait` at the press, so a wait that began after the press stays. Focus stays where it is.
    Seen {
        wait: u64,
    },
    /// A click on History row `row`, which shows its command again (`Tab::replay`). `at`: the
    /// row's time, as the user saw it.
    Replay {
        row: usize,
        at: u64,
    },
    /// `Ctrl+.`, the drawer's **History** and **Shell** tabs, and **Show the shell**: the drawer
    /// flips between the History and the shell (`Tab::flip_drawer`). The TUI never sends it.
    FlipDrawer,
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

/// The docked shell's share of the workspace's width, in %.
const DOCK: u16 = 40;

/// How long after the user's last drag step Laura fits the terminals to their panes.
const RESIZE_SETTLE: Duration = Duration::from_millis(200);

fn no_pane(pane: PaneId) -> String {
    format!("no pane #{pane}")
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
    /// The session label from the latest `laura ready --title`.
    pub title: Option<String>,
    /// Whether the agent waits for the user (`wait.rs`).
    wait: wait::Wait,
    /// What passed through Laura in the current session (`history.rs`).
    history: history::History,
    /// The shell's size (rows, cols) from the web app, which fits xterm.js to the shell's own
    /// type; `None` sizes the shell to its pane, as the TUI always does.
    pub shell_size: Option<(u16, u16)>,
    /// The user showed the shell as a pane (**Show the shell as a pane**) in the web app, in place of the drawer.
    docked: bool,
    /// The overlay, the lock and the draft (`mode.rs`).
    modes: mode::Modes,
    /// Editor panes' Neovim, by pane id. Each also has its file panel in `panels`.
    pub editors: HashMap<PaneId, Nvim>,
    /// The `nvim` to run for `open --edit`, or why editor panes are off. The TUI sets it from
    /// `editor::resolve()`; defaults to off.
    pub editor: Result<PathBuf, editor::Error>,
    /// Editor panes use Laura's theme (the startup wizard's answer). The TUI sets it.
    pub editor_theme: bool,
    /// Hosted by the web app; browser panes need it.
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
    /// Notices for the view, which the web app or the TUI shows and clears, each with the pane to
    /// show it in.
    pub notices: Vec<(Option<PaneId>, String)>,
    /// The editor pane `shell_done` last focused for `SHELL_EXITED`.
    shell_exit_pane: Option<PaneId>,
    /// Spawn sequence number, for the default session id.
    seq: u64,
    next_pane: PaneId,
    rx: Receiver<(Message, Reply)>,
    /// The socket's listener thread, which `Drop` joins.
    listener: Option<JoinHandle<()>>,
    pty_size: (u16, u16),
    /// The last draw area `drain` saw; `OpenLink` splits against it.
    area: Rect,
    /// The user's last drag step on a line between panes: until `RESIZE_SETTLE` after it, the
    /// shell and editor panes keep their terminal size, so a program doesn't redraw at each step.
    dragged: Option<Instant>,
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
        let (rx, listener) = protocol::serve(&socket)?;
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
            title: None,
            wait: wait::Wait::default(),
            history: history::History::default(),
            shell_size: None,
            docked: false,
            modes: mode::Modes::default(),
            editors: HashMap::new(),
            editor: Err(editor::Error::FlagOff),
            editor_theme: false,
            web: false,
            browser_path: Err(browser::NO_BROWSER.into()),
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
            listener: Some(listener),
            pty_size: (rows, cols),
            area: Rect::default(),
            dragged: None,
        })
    }

    /// `spawn` for the web app, with browser panes resolved to `browser_path`. With a browser to
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
    /// The one place that journals an event, so the History holds each journaled event.
    pub(super) fn log_event(&mut self, event: Event) {
        if let Some(j) = &self.journal {
            j.log(&event);
        }
        let panels = &self.panels;
        self.history
            .push(&event, |pane| panels.get(&pane).map(|p| p.path.clone()));
    }

    /// The agent's latest `say` in this session, with a count that names it over the tab's life.
    pub fn latest_say(&self) -> Option<(u64, &str)> {
        self.history.latest_say()
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
    /// journal it. A sent review closes the draft and returns focus to the shell, and an editor
    /// pane to its editor view; a failed one keeps the threads and the draft for a retry.
    fn submit(&mut self, pane: PaneId, body: &str) -> Result<(), Error> {
        self.review_gate(pane)?;
        let bells = self.pty.bells();
        let pty = &self.pty;
        let p = self
            .panels
            .get_mut(&pane)
            .expect("review_gate found the pane");
        // Read the count first: a sent review clears the threads.
        let (path, comments) = (p.path.clone(), p.thread_count());
        let res = p.submit_review(pane, body, |r| paste_into_shell(pty, r));
        self.log_event(Event::Review {
            pane,
            path,
            comments,
            body: body.into(),
            error: res.as_ref().err().map(|e| e.to_string()),
        });
        // The cause is in the journal; the notice stays fixed.
        res.map_err(|_| Error::SubmitFailed)?;
        if let Some(e) = self.editors.get_mut(&pane) {
            e.editor_view = true;
        }
        self.focus = PTY_PANE;
        self.close_draft();
        self.answered(bells);
        Ok(())
    }

    /// A say came since the user last wrote to the shell (`Wait::said`): the shell button shows it.
    pub(crate) fn said(&self) -> bool {
        self.wait.said()
    }

    /// The shell waits with no say since the user last wrote to it (`Wait::unsaid`).
    pub(crate) fn unsaid(&self) -> bool {
        self.wait.unsaid()
    }

    /// Per-frame upkeep before drawing `content`: take `pending_focus`, move focus off a closed
    /// pane, resize the shell and each editor pane's Neovim to its rect once the user's drag
    /// settled. Returns the rects.
    pub fn tick(&mut self, content: Rect) -> HashMap<PaneId, Rect> {
        if let Some(id) = self.pending_focus.take() {
            self.focus = id;
        }
        if self.focus != PTY_PANE && !self.panels.contains_key(&self.focus) {
            self.focus = self.fallback_focus();
        }
        let rect_map = self.rects(content);
        if !self.settled() {
            return rect_map;
        }
        let pty_inner = self.inner(rect_map.get(&PTY_PANE).copied().unwrap_or(content));
        let (rows, cols) = self
            .shell_size
            .unwrap_or((pty_inner.height, pty_inner.width));
        self.resize_to(rows, cols);
        for (id, r) in &rect_map {
            let inner = self.inner(*r);
            if let Some(e) = self.editors.get_mut(id) {
                e.resize(inner.height, inner.width);
            }
        }
        rect_map
    }

    /// No drag step on a line between panes within `RESIZE_SETTLE`.
    fn settled(&self) -> bool {
        self.dragged.is_none_or(|t| t.elapsed() >= RESIZE_SETTLE)
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

    /// The web app shows the shell in the drawer, over the other panes or the start canvas: the
    /// user didn't dock it. The TUI always shows the shell as a pane.
    pub fn drawer_in_use(&self) -> bool {
        self.web && !self.docked
    }

    /// The web app shows the start canvas: no pane but the shell is open. The TUI has no canvas.
    pub(crate) fn canvas_shown(&self) -> bool {
        self.canvas_in(&self.layout)
    }

    /// Whether tree `l` leaves the canvas showing; `dry_run_open` and `fit_ratio` draw a probe tree.
    fn canvas_in(&self, l: &Layout) -> bool {
        self.web && *l == Layout::Pane(PTY_PANE)
    }

    /// The tree that `l` draws: without the shell's leaf while the shell is in the drawer. The
    /// shell's leaf stays in `layout`, so its rules hold.
    fn shown<'a>(&self, l: &'a Layout) -> Cow<'a, Layout> {
        match self.drawer_in_use().then(|| l.without(PTY_PANE)).flatten() {
            Some(rest) => Cow::Owned(rest),
            None => Cow::Borrowed(l),
        }
    }

    /// Each pane's rect in `l` at `area`. The drawer floats over the panes, so the shell gets the
    /// whole area. A docked shell alone takes its left `DOCK`%, beside the canvas.
    fn tree_rects(&self, l: &Layout, area: Rect) -> HashMap<PaneId, Rect> {
        if self.docked && self.canvas_in(l) {
            return HashMap::from([(PTY_PANE, split_rect(area, Dir::Horizontal, DOCK).0)]);
        }
        let mut map = rects(&self.shown(l), area);
        map.entry(PTY_PANE).or_insert(area);
        map
    }

    /// The lines between the panes that the workspace shows at `area`, which the user drags. None
    /// beside the start canvas, which isn't a pane.
    pub fn handles(&self, area: Rect) -> Vec<Handle> {
        if self.canvas_shown() {
            return vec![];
        }
        self.shown(&self.layout).handles(area)
    }

    /// Each pane's rect at `area`: the one source of geometry for drawing and for overflow.
    pub fn rects(&self, area: Rect) -> HashMap<PaneId, Rect> {
        self.tree_rects(&self.layout, area)
    }

    /// The pane that takes the focus when the focused pane goes: the first other pane while the
    /// drawer is in use, so a close doesn't open the drawer, else the shell.
    fn fallback_focus(&self) -> PaneId {
        let other = self
            .layout
            .order()
            .into_iter()
            .find(|id| self.panels.contains_key(id));
        match other {
            Some(id) if self.drawer_in_use() => id,
            _ => PTY_PANE,
        }
    }

    /// The shell's name: the `--agent` name of the latest `laura ready`, else `shell`.
    pub fn shell_name(&self) -> String {
        self.journal
            .as_ref()
            .and_then(Journal::agent)
            .unwrap_or_else(|| "shell".into())
    }

    /// The focused panel, if a panel (not the PTY) has focus.
    pub fn focused_panel(&self) -> Option<&Panel> {
        (self.focus != PTY_PANE)
            .then_some(self.focus)
            .and_then(|id| self.panels.get(&id))
    }

    /// The user wrote to the shell (input, a paste, an inline review): the wait ends. `bells` is
    /// the count the answer covers, read before the write reaches the shell, so a bell after the
    /// answer is a new wait.
    fn answered(&mut self, bells: u64) {
        self.wait.answered(bells);
    }

    /// The user saw the shell: a move into the shell (`Focus`, `Close`, the lock, an overlay's
    /// close) or `Seen`, with `bells` read at arrival. The wait ends; a say since the user's last
    /// write still counts.
    fn saw(&mut self, bells: u64) {
        self.wait.saw(bells);
    }

    /// The web app's `seen` of wait `wait` (`Wait::seen`).
    fn seen(&mut self, wait: u64) {
        self.wait.seen(wait, self.pty.bells());
    }

    /// A new wait began: Laura journals it. The drawer keeps its view.
    fn wait_began(&mut self) {
        self.log_event(Event::Wait {
            said: self.wait.said(),
        });
    }

    /// The drawer shows the shell, not the History: until the user flips it (`Ctrl+.`), and while
    /// the lock is on, which sends the keystrokes to the shell.
    pub fn drawer_shows_shell(&self) -> bool {
        self.modes.drawer == DrawerView::Shell || self.locked()
    }

    /// `Ctrl+.`: away from the shell, the focus moves to the shell, which opens the drawer as it
    /// is; on the shell, the drawer flips between the History and the shell.
    pub(super) fn flip_drawer(&mut self) {
        if self.focus != PTY_PANE {
            self.focus = PTY_PANE;
            self.forget_lock_from();
            return self.saw(self.pty.bells());
        }
        self.modes.drawer = match self.modes.drawer {
            DrawerView::History => DrawerView::Shell,
            DrawerView::Shell => DrawerView::History,
        };
    }

    /// The History's rows, a count that changes with them, and that count at the last change
    /// that was not an append (`history.rs`).
    pub(crate) fn history(&self) -> (&[history::Row], u64, u64) {
        (self.history.rows(), self.history.seq(), self.history.full())
    }

    /// The agent waits for the user (`wait.rs`).
    pub fn waiting(&self) -> bool {
        self.wait.waiting()
    }

    /// The bell count that names the current wait, for the web app's `seen`.
    pub(crate) fn wait_id(&self) -> u64 {
        self.wait.id()
    }

    /// Drain queued requests, live-reloading panels before each so it resolves against current disk, then applying it and replying. `area` is the current draw area (for `Layout`/dry-run reports).
    /// Returns whether anything changed: a message applied, a file reloaded or froze, a Neovim exited.
    pub fn drain(&mut self, area: Rect) -> bool {
        self.area = area;
        self.forget_lock_from();
        // Before the hold: a bell while the user types a comment still counts.
        if self.wait.bell(self.pty.bells()) {
            self.wait_began();
        }
        // Before the hold: a frame or an exited browser doesn't wait for the user's draft.
        let browsed = self.poll_browsers();
        if self.held() {
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
            // A move away and back within one drain still forgets the unlock's pane.
            self.forget_lock_from();
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
                // Before the remove, so the History's row names the pane's file.
                self.log_event(Event::Close {
                    pane: Some(target),
                    all: false,
                });
                if let Some(p) = self.panels.remove(&target) {
                    remove_if_temp(&p.path);
                }
                self.editors.remove(&target); // drop kills Neovim
                // Laura's browser keeps running, so the next browser pane opens fast.
                self.browsers.remove(&target);
                self.browser_sizes.remove(&target);
                if self.focus == target {
                    self.focus = self.fallback_focus();
                }
                Response::Ok
            }
            Err(message) => Response::Error { message },
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

    /// The PTY's size (rows, cols) now, which is the size of the shell's saved screen.
    pub fn pty_size(&self) -> (u16, u16) {
        self.pty_size
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
/// `LAURA_TAB` reads that request's dropped reply as success. A self-connect wakes the accept,
/// and the drop joins the thread: the request's reply comes before the thread drops the
/// listener, so without the join a client could still connect to the closed tab.
impl Drop for Tab {
    fn drop(&mut self) {
        for p in self.panels.values() {
            remove_if_temp(&p.path);
        }
        self.chrome_port.store(0, Ordering::SeqCst);
        // `rx` first: alive, it would take the wake-up request and `request` would wait on a reply.
        self.rx = channel().1;
        // A failed wake-up leaves the thread in `accept`, where a join would block for good.
        if protocol::request(&self.socket, &Message::Layout).is_ok()
            && let Some(thread) = self.listener.take()
        {
            let _ = thread.join();
        }
    }
}

/// Write `text` into the shell as one bracketed paste and press `Enter`, so the agent CLI reads it
/// as one message. An inline review and the message box write this way.
fn paste_into_shell(pty: &PtyTab, text: &str) -> std::io::Result<()> {
    pty.write(&bracketed_paste(text, true))
}

/// The unsaved-edits refusal, shared by a pane's close and the workspace's quit.
fn unsaved_message(id: PaneId) -> String {
    format!("pane #{id} has unsaved edits in Neovim — save (:w) or quit Neovim first")
}
