//! One workspace tab: a PTY, its panels, the split tree, and the socket that ties them together.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};

use anyhow::Result;
use portable_pty::CommandBuilder;
use ratatui::layout::{Margin, Rect};

use serde_json::json;

use crate::editor::{self, Nvim};
use crate::journal::{Journal, is_runtime_temp};
use crate::layout::{Layout, MIN_PANE, all_panes_fit, infer_split, rects};
use crate::panel::{Panel, bracketed_paste};
use crate::protocol::{
    self, Dir, LayoutReport, Message, PTY_PANE, PaneId, PaneKind, PaneReport, Reply, Response, Side,
};
use crate::pty::PtyTab;

/// Walk `layout`'s rects and, for each panel, measure content vs. visible rows to fill a `LayoutReport`.
fn build_report(
    layout: &Layout,
    panels: &HashMap<PaneId, &Panel>,
    editors: &HashMap<PaneId, Nvim>,
    area: Rect,
) -> LayoutReport {
    let mut panes: Vec<PaneReport> = rects(layout, area)
        .into_iter()
        .map(|(id, rect)| {
            let visible_rows = rect.height.saturating_sub(2) as usize; // minus border
            let inner_w = rect.width.saturating_sub(2).max(1) as usize;
            let too_small = rect.width < 3 || rect.height < 3;
            let (kind, path, content_rows) = if id == PTY_PANE {
                (PaneKind::Pty, None, None)
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
            }
        })
        .collect();
    panes.sort_by_key(|p| p.id);
    LayoutReport {
        area: area.into(),
        panes,
    }
}

/// Terse open-time warning for a just-opened pane, or `None` if it fits vertically.
pub fn overflow_warning(p: &PaneReport) -> Option<String> {
    if p.overflow_rows > 0 {
        Some(format!(
            "pane #{} overflows: {} row(s) below the fold — scroll, or grow the pane",
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
    std::fs::canonicalize(p).unwrap_or_else(|_| p.into())
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
    /// The user is typing a comment/review body or answering the theme wizard: leave requests queued, and panes unreloaded, until they finish.
    pub hold: bool,
    /// Editor panes' Neovim, by pane id. Each also has its file panel in `panels`.
    pub editors: HashMap<PaneId, Nvim>,
    /// The `nvim` to run for `open --edit`, or why editor panes are off. The TUI sets it from
    /// `editor::resolve()`; defaults to off.
    pub editor: Result<PathBuf, String>,
    /// Editor panes use Laura's theme (the startup wizard's answer). The TUI sets it.
    pub editor_theme: bool,
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
            hold: false,
            editors: HashMap::new(),
            editor: Err(editor::FLAG_OFF.into()),
            editor_theme: false,
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

    /// `Shift+S` → `Enter`: write the focused pane's inline review with `body` into the shell and
    /// journal it. `None` without a focused file pane. A sent review returns focus to the shell,
    /// and an editor pane to its editor view; a failed one keeps the threads for a retry (#72).
    pub fn submit_focused(&mut self, body: &str) -> Option<std::io::Result<()>> {
        let pty = &self.pty;
        let p = self.panels.get_mut(&self.focus)?;
        // Read the count first: a sent review clears the threads.
        let (path, comments) = (p.path.clone(), p.thread_count());
        let res = p.submit_review(body, |r| pty.write(&bracketed_paste(r, true)));
        let mut event = json!({"type": "review", "path": path, "comments": comments, "body": body});
        if let Err(e) = &res {
            event["error"] = json!(e.to_string());
        }
        self.log_event(event);
        if res.is_ok() {
            if let Some(e) = self.editors.get_mut(&self.focus) {
                e.editor_view = true;
            }
            self.focus = PTY_PANE;
        }
        Some(res)
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
        build_report(&self.layout, &self.panel_refs(), &self.editors, area)
    }

    /// Drain queued requests, live-reloading panels before each so it resolves against current disk, then applying it and replying. `area` is the current draw area (for `Layout`/dry-run reports).
    pub fn drain(&mut self, area: Rect) {
        if self.hold {
            return;
        }
        // An exited Neovim closes its pane, unless it holds threads: then it stays a file pane.
        let exited: Vec<PaneId> = self
            .editors
            .iter()
            .filter(|(_, e)| e.pty.has_exited())
            .map(|(id, _)| *id)
            .collect();
        for id in exited {
            self.editors.remove(&id);
            if self.panels.get(&id).is_some_and(|p| p.thread_count() == 0) {
                self.close_pane(Some(id));
            }
        }
        loop {
            // Catch up to disk before each message: one sent right after an edit must see the new content.
            for p in self.panels.values_mut() {
                p.reload_if_changed();
            }
            let Ok((msg, reply)) = self.rx.try_recv() else {
                break;
            };
            let resp = self.apply(msg, area);
            reply.send(&resp);
        }
        // Every change to a thread (comment, submit, `Ctrl+R`, freeze) reaches Neovim from here.
        for (id, e) in &mut self.editors {
            if let Some(p) = self.panels.get(id) {
                e.push(p);
            }
        }
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
        let mut panel = Panel::open(path.to_string());
        panel.set_follow(follow);
        // `set_highlight` after `set_follow` so an explicit highlight range
        // wins the *first* frame's viewport over tail-follow when both are set.
        if let Some((start, end)) = highlight {
            panel.set_highlight(start, end);
        }
        // Open-into-diff: refusal (no git / nothing to diff) becomes a warning.
        let diff_warning = diff.then(|| panel.set_diff_view(true).err()).flatten();
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

    /// Warn if `path` is already open in a pane other than `except` (lowest id wins).
    fn already_open(&self, path: &str, except: PaneId) -> Option<String> {
        let this = normalized(path);
        let n = self
            .panels
            .iter()
            .filter(|(id, p)| **id != except && normalized(&p.path) == this)
            .map(|(id, _)| *id)
            .min()?;
        // Suggest reuse verbs only where they work: not under review, nor on an editor pane (it refuses highlight; a bare --panel quits Neovim).
        Some(if self.panels[&n].thread_count() > 0 {
            format!("already open in pane #{n}, which has an unsubmitted inline review")
        } else if self.editors.contains_key(&n) {
            format!("already open in editor pane #{n}")
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
            .map(|r| r.inner(Margin::new(1, 1)))
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
            Message::Highlight { pane, range } => {
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
                        self.log_event(
                            json!({"type":"highlight","pane":target,"start":start,"end":end}),
                        );
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
                let Some(panel) = self.panels.get_mut(&target) else {
                    return Response::Error {
                        message: format!("no pane #{target}"),
                    };
                };
                match panel.agent_note(line, body, author) {
                    Ok(()) => {
                        self.log_event(json!({"type":"comment","pane":target,"line":line}));
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
                let experimental = self.editor.iter().map(|_| "editor".to_string()).collect();
                Response::Ready {
                    journal: path,
                    experimental,
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
        let temp = Panel::open(path.to_string());
        let mut refs = self.panel_refs();
        refs.insert(new, &temp);
        Response::Report(build_report(&layout, &refs, &self.editors, area))
    }

    /// Resize the PTY only when its draw area changed, else the shell wraps wrong.
    pub fn resize_to(&mut self, rows: u16, cols: u16) {
        if (rows, cols) != self.pty_size {
            self.pty_size = (rows, cols);
            self.pty.resize(rows, cols);
        }
    }
}

/// Release the tab's socket. The listener thread blocks in `accept` and only learns `rx` is gone
/// from a failed send, so the pipe outlives the tab until a request wakes it, and a stale
/// `LAURA_TAB` reads that request's dropped reply as success.
/// ponytail: a self-connect wakes the accept; a nonblocking listener with a stop flag if it's flaky.
impl Drop for Tab {
    fn drop(&mut self) {
        // `rx` first: alive, it would take the wake-up request and `request` would wait on a reply.
        self.rx = channel().1;
        let _ = protocol::request(&self.socket, &Message::Layout);
    }
}

/// The unsaved-edits refusal, shared by a pane's close and the tab's.
fn unsaved_message(id: PaneId) -> String {
    format!("pane #{id} has unsaved edits in Neovim — save (:w) or quit Neovim first")
}
