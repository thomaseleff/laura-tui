//! The agent's socket messages (`Message`): `apply` changes the workspace and returns the
//! `Response`: opening a pane, replacing its content and fitting the split.

use anyhow::Result;
use ratatui::layout::Rect;

use serde_json::json;

use crate::browser;
use crate::editor::{self, Nvim};
use crate::journal::{Journal, session_path};
use crate::layout::{Layout, all_panes_fit, infer_split};
use crate::panel::Panel;
use crate::protocol::{Dir, Message, PTY_PANE, PaneId, Response, Side};

use super::report::{build_report, fit_rows, overflow_warning};
use super::{Error, Tab, normalized, remove_if_temp};

impl Tab {
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
        let nvim = self.editor.clone().map_err(|e| Response::Error {
            message: e.to_string(),
        })?;
        // Drop the old Neovim first: its Drop deletes the same bootstrap files the new one writes.
        let was = self.editors.remove(&id).map(|e| e.editor_view);
        let inner = self
            .rects(area)
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

    /// Open a file or URL in a new pane, or in pane `panel` in place. `Ok` holds the `Response`
    /// of a helper that answers the agent itself, an error included.
    pub(super) fn open(&mut self, msg: Message, area: Rect) -> Result<Response, Error> {
        let Message::Open {
            path,
            split,
            dir,
            ratio,
            side,
            focus,
            follow,
            tail,
            dry_run,
            highlight,
            diff,
            panel,
            edit,
        } = msg
        else {
            unreachable!("apply and open_file pass only Message::Open");
        };
        // A URL opens a browser pane. Its errors come first, one at a time; `--follow`
        // changes nothing, since the pane always shows the page live.
        let url = browser::is_url(&path);
        if url {
            self.browser_gate()?;
            let flag = [
                (highlight.is_some(), browser::NO_HIGHLIGHT),
                (diff, browser::NO_DIFF),
                (edit, browser::NO_EDIT),
            ];
            if let Some((_, message)) = flag.into_iter().find(|(on, _)| *on) {
                return Err(message.into());
            }
        }
        if edit && let Err(e) = &self.editor {
            return Err(e.to_string().into());
        }
        if edit && (highlight.is_some() || diff) {
            return Err(editor::SHOWS_NEITHER.into());
        }
        // ponytail: `--panel` short-circuits before the dry_run branch, so a wire-only
        // `{panel, dry_run:true}` would mutate. The CLI can't send it (--panel/--dry-run
        // don't conflict but replace ignores dry_run). Guard only if a replace preview is wanted.
        if let Some(id) = panel {
            return Ok(self.replace_panel(id, path, follow, highlight, diff, edit, area));
        }
        let new = self.next_pane;
        let inferred = (dir.is_none() && ratio.is_none() && side.is_none() && split.is_none())
            .then(|| infer_split(&self.shown(&self.layout), area, new))
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
        // A split never targets the hidden shell: the focus there is the open drawer.
        let target = if target == PTY_PANE && self.drawer_in_use() {
            self.fallback_focus()
        } else {
            target
        };
        // A stacked pane gets only the rows its file needs, up to half.
        let ratio =
            if inferred.is_some() && dir == Dir::Vertical && !(edit || diff || follow || url) {
                self.fit_ratio(&path, target, side, new, area)
            } else {
                ratio
            };
        if dry_run {
            return Ok(self.dry_run_open(&path, target, dir, ratio, side, area));
        }
        let mut probe = self.layout.clone();
        if probe.split(target, dir, ratio, side, new).is_ok()
            && !all_panes_fit(&self.shown(&probe), area)
        {
            return Err(Error::SplitTooSmall);
        }
        // Open the page before the split, so a page that won't open adds no pane.
        let page = url.then(|| self.open_page(new, &path)).transpose()?;
        match self.layout.split(target, dir, ratio, side, new) {
            Ok(()) => {
                self.next_pane += 1;
                if edit && let Err(e) = self.attach_editor(new, &path, area) {
                    let _ = self.layout.remove(new);
                    return Ok(e);
                }
                let dup = self.already_open(&path, new);
                let mut warnings = self.install_panel(new, &path, follow, highlight, diff, area);
                if let Some(p) = self.panels.get_mut(&new) {
                    p.tail = tail;
                }
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
                Ok(Response::Opened {
                    pane: new,
                    warnings,
                })
            }
            Err(message) => Err(message.into()),
        }
    }

    /// Apply one request to layout/panel state and produce its response.
    pub(super) fn apply(&mut self, msg: Message, area: Rect) -> Response {
        match msg {
            msg @ Message::Open { .. } => {
                self.open(msg, area).unwrap_or_else(|e| Response::Error {
                    message: e.to_string(),
                })
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
            Message::Ready {
                session,
                agent,
                title,
            } => {
                self.agent = true;
                // A blank `--title` would show an empty session label; treat it as no `--title`.
                let title = title
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());
                let session =
                    session.unwrap_or_else(|| format!("laura-{}-{}", std::process::id(), self.seq));
                // A re-`ready` of the same session (to set a title) keeps its agent name.
                let agent = agent.or_else(|| {
                    self.journal
                        .as_ref()
                        .filter(|j| j.path() == session_path(&session))
                        .and_then(Journal::agent)
                });
                let journal = Journal::open(&session, agent);
                let path = journal.path().to_string_lossy().into_owned();
                journal.log(json!({"type": "ready", "pid": std::process::id(), "title": title}));
                self.journal = Some(journal);
                if title.is_some() {
                    self.title = title;
                }
                let experimental = self
                    .editor
                    .is_ok()
                    .then(|| "editor".to_string())
                    .into_iter()
                    .collect();
                Response::Ready {
                    journal: path,
                    experimental,
                    layout: Some(self.report(area)),
                    browser_url: self.browser_port.map(browser::endpoint),
                    web_app: self.web,
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
            self.tree_rects(&layout, area),
            &refs,
            &self.editors,
            &self.browsers,
            area,
            self.web,
            self.web && !self.docked,
            self.shell_size.map(|(rows, _)| rows),
        ))
    }

    /// The smallest ratio, up to 50, at which a stacked split of `target` gives `new` the rows
    /// `path` needs. Brute force: `split_rect` floors and gives the second side `100 - ratio`.
    /// The browser draws a page, and markdown blocks, taller than their rows, so the web app fits neither.
    fn fit_ratio(&self, path: &str, target: PaneId, side: Side, new: PaneId, area: Rect) -> u16 {
        let temp = Panel::open(path.to_string());
        if self.web && (crate::panel::is_page(&temp) || !temp.blocks().is_empty()) {
            return 50;
        }
        let rect = |ratio| {
            let mut l = self.layout.clone();
            l.split(target, Dir::Vertical, ratio, side, new).ok()?;
            self.tree_rects(&l, area).get(&new).copied()
        };
        let Some(half) = rect(50) else { return 50 };
        let need = fit_rows(&temp, half.width, self.web);
        (1..=50)
            .find(|&r| rect(r).is_some_and(|rc| rc.height as usize >= need))
            .unwrap_or(50)
    }
}
