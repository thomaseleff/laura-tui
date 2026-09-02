//! One workspace tab: a PTY, its panels, the split tree, and the socket that ties them together.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;

use anyhow::Result;
use portable_pty::CommandBuilder;
use ratatui::layout::Rect;

use serde_json::json;

use crate::journal::{Journal, is_runtime_temp};
use crate::layout::{Layout, rects};
use crate::panel::Panel;
use crate::protocol::{
    self, Dir, LayoutReport, Message, PTY_PANE, PaneId, PaneKind, PaneReport, Reply, Response, Side,
};
use crate::pty::PtyTab;

/// Walk `layout`'s rects and, for each panel, measure content vs. visible rows to fill a `LayoutReport`.
fn build_report(layout: &Layout, panels: &HashMap<PaneId, &Panel>, area: Rect) -> LayoutReport {
    let mut panes: Vec<PaneReport> = rects(layout, area)
        .into_iter()
        .map(|(id, rect)| {
            let visible_rows = rect.height.saturating_sub(2) as usize; // minus border
            let inner_w = rect.width.saturating_sub(2).max(1) as usize;
            let too_small = rect.width < 3 || rect.height < 3;
            let (kind, path, content_rows) = if id == PTY_PANE {
                (PaneKind::Pty, None, None)
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
            }
        })
        .collect();
    panes.sort_by_key(|p| p.id);
    LayoutReport {
        area: area.into(),
        panes,
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

/// One workspace tab: a PTY, its panel panes, the split tree, and its own `LAURA_TAB` socket. Per-tab sockets isolate tabs by addressing (protocol.rs).
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
    /// Display name shown in the tab bar; `None` renders the bare index.
    pub name: Option<String>,
    /// Set by `Open{focus}`; the run loop consumes it to focus the new pane once.
    pub pending_focus: Option<PaneId>,
    /// Tab hosts an agent (declared via `laura ready`); gates review injection.
    pub agent: bool,
    /// Per-session journal, created on `ready` (names + audits composition events).
    pub journal: Option<Journal>,
    /// Spawn sequence number, for the default session id.
    seq: u64,
    next_pane: PaneId,
    rx: Receiver<(Message, Reply)>,
    pty_size: (u16, u16),
}

impl Tab {
    /// Mint a unique socket, serve it, point `cmd`'s `LAURA_TAB` at it, spawn the PTY.
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
        let pty = PtyTab::spawn(cmd, rows, cols)?;
        Ok(Tab {
            pty,
            layout: Layout::Pane(PTY_PANE),
            panels: HashMap::new(),
            focus: PTY_PANE,
            socket,
            name: None,
            pending_focus: None,
            agent: false,
            journal: None,
            seq: n,
            next_pane: 1,
            rx,
            pty_size: (rows, cols),
        })
    }

    /// Append one event to this tab's journal, if a session has been named (`ready`). Best-effort.
    pub fn log_event(&self, event: serde_json::Value) {
        if let Some(j) = &self.journal {
            j.log(event);
        }
    }

    /// The focused panel, if a panel (not the PTY) has focus.
    pub fn focused_panel(&self) -> Option<&Panel> {
        (self.focus != PTY_PANE)
            .then_some(self.focus)
            .and_then(|id| self.panels.get(&id))
    }

    /// The focused panel, if a panel (not the PTY) has focus.
    pub fn focused_panel_mut(&mut self) -> Option<&mut Panel> {
        (self.focus != PTY_PANE)
            .then_some(self.focus)
            .and_then(|id| self.panels.get_mut(&id))
    }

    /// Borrow every panel keyed by id (for `build_report`).
    fn panel_refs(&self) -> HashMap<PaneId, &Panel> {
        self.panels.iter().map(|(k, v)| (*k, v)).collect()
    }

    /// Current geometry + overflow at draw area `area`.
    pub fn report(&self, area: Rect) -> LayoutReport {
        build_report(&self.layout, &self.panel_refs(), area)
    }

    /// Drain queued requests, applying each and replying, then live-reload panels. `area` is the current draw area (for `Layout`/dry-run reports).
    pub fn drain(&mut self, area: Rect) {
        while let Ok((msg, reply)) = self.rx.try_recv() {
            let resp = self.apply(msg, area);
            reply.send(&resp);
        }
        for p in self.panels.values_mut() {
            p.reload_if_changed();
        }
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
            } => {
                let target = split.unwrap_or(self.focus);
                if dry_run {
                    return self.dry_run_open(&path, target, dir, ratio, side, area);
                }
                let new = self.next_pane;
                match self.layout.split(target, dir, ratio, side, new) {
                    Ok(()) => {
                        self.next_pane += 1;
                        let mut panel = Panel::open(path.clone());
                        panel.set_follow(follow);
                        self.panels.insert(new, panel);
                        self.log_event(json!({"type": "open", "pane": new, "path": path}));
                        if focus {
                            self.pending_focus = Some(new);
                        }
                        let mut warnings = vec![];
                        if !self.agent {
                            warnings.push(
                                "panel shown, but run `laura ready` to enable review submission"
                                    .into(),
                            );
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
                    for p in self.panels.values() {
                        remove_if_temp(&p.path);
                    }
                    self.layout = Layout::Pane(PTY_PANE);
                    self.panels.clear();
                    self.focus = PTY_PANE;
                    self.log_event(json!({"type": "close", "all": true}));
                    return Response::Ok;
                }
                let target = pane.or((self.focus != PTY_PANE).then_some(self.focus));
                let Some(target) = target else {
                    return Response::Error {
                        message: "no panel focused to close".into(),
                    };
                };
                match self.layout.remove(target) {
                    Ok(()) => {
                        if let Some(p) = self.panels.remove(&target) {
                            remove_if_temp(&p.path);
                        }
                        if self.focus == target {
                            self.focus = PTY_PANE;
                        }
                        self.log_event(json!({"type": "close", "pane": target}));
                        Response::Ok
                    }
                    Err(message) => Response::Error { message },
                }
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
            Message::Layout => Response::Report(self.report(area)),
            Message::Ready { session, agent } => {
                self.agent = true;
                let session =
                    session.unwrap_or_else(|| format!("laura-{}-{}", std::process::id(), self.seq));
                let journal = Journal::open(&session, agent);
                let path = journal.path().to_string_lossy().into_owned();
                journal.log(json!({"type": "ready"}));
                self.journal = Some(journal);
                Response::Ready { journal: path }
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
        let temp = Panel::open(path.to_string());
        let mut refs = self.panel_refs();
        refs.insert(new, &temp);
        Response::Report(build_report(&layout, &refs, area))
    }

    /// Resize the PTY only when its draw area changed, else the shell wraps wrong.
    pub fn resize_to(&mut self, rows: u16, cols: u16) {
        if (rows, cols) != self.pty_size {
            self.pty_size = (rows, cols);
            self.pty.resize(rows, cols);
        }
    }
}
