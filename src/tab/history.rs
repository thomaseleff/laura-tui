//! The History: what passed through Laura in the current session, as the drawer shows it. The
//! session's `ready`, one row for each `laura` command of the agent's that Laura journals, each
//! say, and each submitted inline review. A click on a command's row shows the command again
//! (`Tab::replay`), and never runs it again.

use std::path::Path;

use serde_json::{Value, json};

use crate::browser;
use crate::journal::{Event, is_runtime_temp, now_ms};
use crate::protocol::{PTY_PANE, PaneId, Response};

use super::{Error, Tab};

/// A highlight, diff, comment or focus row whose file no pane shows.
pub const NOT_OPEN: &str = "⚠ No pane shows {name}. Click its open row to open it again.";
/// An open row whose file is gone from the disk.
pub const GONE: &str = "⚠ {path} no longer exists.";
/// A highlight or comment row whose element is gone from the page.
pub const NO_ELEMENT: &str = "⚠ No element matches {selector} in pane #{n}.";
/// A highlight or comment row on a page that loads or shows a dialog.
pub const PAGE_NOT_READY: &str =
    "⚠ The page in pane #{n} isn't ready. Try again once it has loaded.";
/// A highlight row of lines, or a diff row, whose file an editor pane shows.
pub const IN_EDITOR: &str = "⚠ Pane #{n} is an editor pane, which shows no highlight or diff.";
/// A highlight row on a frozen pane.
pub const FROZEN: &str = "⚠ Pane #{n} is frozen. Submit or refresh its inline review first.";

/// One row of the History, with its time in ms since the epoch.
pub(crate) struct Row {
    at: u64,
    entry: Entry,
}

enum Entry {
    /// The session's `ready`, with its session label.
    Ready { title: Option<String> },
    /// One journaled command of the agent's. `path`: the file or URL of the pane that the event
    /// named when Laura journaled it; the replay finds its pane by it.
    Command { event: Event, path: Option<String> },
    /// The agent's say, as markdown and as Laura's HTML of it.
    Say { body: String, html: String },
    /// The user's submitted inline review.
    Review { path: String, comments: usize },
}

impl Row {
    /// The row as the `history` frame carries it, so the web app paints it as it is.
    pub(crate) fn json(&self) -> Value {
        let at = self.at;
        match &self.entry {
            Entry::Ready { title } => json!({"kind": "ready", "at": at, "title": title}),
            Entry::Command { event, path } => json!({
                "kind": "command", "at": at, "verb": verb(event),
                "target": target(event, path.as_deref()), "replay": self.replayable(),
            }),
            Entry::Say { html, .. } => json!({"kind": "say", "at": at, "html": html}),
            Entry::Review { path, comments } => json!({
                "kind": "review", "at": at, "path": file_name(path), "comments": comments,
            }),
        }
    }

    /// A click shows the row's command again: an open, a highlight that didn't clear, a diff
    /// view that turned on, a comment, or a focus on a pane.
    fn replayable(&self) -> bool {
        let Entry::Command {
            event,
            path: Some(_),
        } = &self.entry
        else {
            return false;
        };
        match event {
            // `laura tail`'s spool: Laura deletes it with its pane.
            Event::Open { path, .. } => !is_runtime_temp(path),
            Event::Comment { .. } => true,
            Event::Highlight { cleared, .. } => !cleared,
            Event::Diff { on, .. } => *on,
            Event::Focus { pane } => *pane != PTY_PANE,
            _ => false,
        }
    }
}

#[derive(Default)]
pub(super) struct History {
    rows: Vec<Row>,
    /// The says over the tab's life, so a front-end tells a new say from the last one it showed.
    says: u64,
    /// Counts each change, so the web app sends the History only when it changed.
    seq: u64,
    /// The `seq` of the last change that was not an append (`ready`): until the next one, a view
    /// that holds the rows needs only the new ones.
    full: u64,
    /// A message from the agent is being applied: only its commands get a row. The user's close,
    /// link click or replay runs the same functions.
    agent: bool,
}

impl History {
    /// Add one journaled event. `path_of` gives the path of the pane that the event names.
    pub(super) fn push(&mut self, event: &Event, path_of: impl FnOnce(PaneId) -> Option<String>) {
        let entry = match event {
            Event::Say { body } => {
                self.says += 1;
                Entry::Say {
                    body: body.clone(),
                    html: crate::render::markdown_html(body),
                }
            }
            Event::Review {
                path,
                comments,
                error: None,
                ..
            } => Entry::Review {
                path: path.clone(),
                comments: *comments,
            },
            Event::Open { path, .. } if self.agent => Entry::Command {
                event: event.clone(),
                path: Some(path.clone()),
            },
            Event::Close { .. }
            | Event::Focus { .. }
            | Event::Highlight { .. }
            | Event::Diff { .. }
            | Event::Comment { .. }
                if self.agent =>
            {
                Entry::Command {
                    event: event.clone(),
                    path: pane_of(event).and_then(path_of),
                }
            }
            _ => return,
        };
        self.rows.push(Row {
            at: now_ms(),
            entry,
        });
        self.seq += 1;
    }

    pub(super) fn set_agent(&mut self, on: bool) {
        self.agent = on;
    }

    /// `ready`: the ready row goes first. A new session after an earlier one starts an empty
    /// History; the first `ready` keeps the rows before it, which belong to the same work. The same
    /// session with a session label changes the ready row's label.
    pub(super) fn ready(&mut self, title: Option<String>, new_session: bool) {
        if new_session {
            self.rows.clear();
        }
        match self.rows.first_mut() {
            Some(Row {
                entry: Entry::Ready { title: label },
                ..
            }) => match title {
                Some(t) => *label = Some(t),
                None if !new_session => return,
                None => {}
            },
            _ => self.rows.insert(
                0,
                Row {
                    at: now_ms(),
                    entry: Entry::Ready { title },
                },
            ),
        }
        self.seq += 1;
        self.full = self.seq;
    }

    /// The session's latest say, with its count.
    pub(super) fn latest_say(&self) -> Option<(u64, &str)> {
        self.rows.iter().rev().find_map(|r| match &r.entry {
            Entry::Say { body, .. } => Some((self.says, body.as_str())),
            _ => None,
        })
    }

    /// Row `row`'s command and path, if a click shows it again. The rows shift only when a
    /// `ready` adds or clears them, so the index with the row's time `at` names the row the user saw.
    fn replayable(&self, row: usize, at: u64) -> Option<(Event, String)> {
        match self.rows.get(row) {
            Some(
                r @ Row {
                    entry:
                        Entry::Command {
                            event,
                            path: Some(path),
                        },
                    ..
                },
            ) if r.at == at && r.replayable() => Some((event.clone(), path.clone())),
            _ => None,
        }
    }

    pub(super) fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub(super) fn seq(&self) -> u64 {
        self.seq
    }

    pub(super) fn full(&self) -> u64 {
        self.full
    }
}

impl Tab {
    /// The user clicked History row `row`, with time `at`: show its command again, and focus its
    /// pane, which closes the drawer. Laura finds the pane by the row's path, and by its pane id
    /// only for a row of a page whose browser pane moved within the site (`same_site`). Laura
    /// journals what changes as the user's act. A row that no click shows again, or that a new
    /// session replaced, does nothing.
    pub(super) fn replay(&mut self, row: usize, at: u64) -> Result<(), Error> {
        let Some((event, path)) = self.history.replayable(row, at) else {
            return Ok(());
        };
        if let Event::Open { pane, .. } = event {
            return self.replay_open(&path, pane);
        }
        let pane = self
            .showing(&path, PTY_PANE)
            .or_else(|| pane_of(&event).filter(|&p| self.same_site(&path, p)))
            .ok_or_else(|| Error::Text(NOT_OPEN.replace("{name}", &file_name(&path))))?;
        let lines_or_diff = matches!(
            event,
            Event::Highlight { selector: None, .. } | Event::Diff { .. }
        );
        if lines_or_diff && self.editors.contains_key(&pane) {
            return Err(Error::Text(IN_EDITOR.replace("{n}", &pane.to_string())));
        }
        match event {
            Event::Highlight {
                start,
                end,
                selector,
                ..
            } => self.replay_highlight(pane, start.zip(end), selector)?,
            Event::Diff { .. } => done(self.diff_view(Some(pane), Some(true)))?,
            Event::Comment { line, selector, .. } => self.replay_comment(pane, line, selector)?,
            _ => {}
        }
        self.focus = pane;
        self.forget_lock_from();
        Ok(())
    }

    /// An open row: focus the pane that shows the file or URL, else the row's browser pane
    /// `pane` while it shows another page of the URL's site, else open it again as a link does.
    fn replay_open(&mut self, path: &str, pane: PaneId) -> Result<(), Error> {
        let shown = self.showing(path, PTY_PANE).is_some();
        if !shown && self.same_site(path, pane) {
            self.focus = pane;
            self.forget_lock_from();
            return Ok(());
        }
        if !shown && !browser::is_url(path) && !Path::new(path).is_file() {
            return Err(Error::Text(GONE.replace("{path}", path)));
        }
        self.open_file(path.into(), false)?;
        Ok(())
    }

    /// Browser pane `pane` shows another page of URL `path`'s site: a row of the page still
    /// finds the pane after the user followed a link.
    fn same_site(&self, path: &str, pane: PaneId) -> bool {
        browser::is_url(path)
            && self
                .panels
                .get(&pane)
                .is_some_and(|p| p.browser && browser::origin(&p.path) == browser::origin(path))
    }

    /// A highlight row: its lines `range` again, or its element.
    fn replay_highlight(
        &mut self,
        pane: PaneId,
        range: Option<(u32, u32)>,
        selector: Option<String>,
    ) -> Result<(), Error> {
        if let Some(sel) = selector {
            return self.replay_element(pane, &sel);
        }
        if self.panels.get(&pane).is_some_and(|p| p.source_changed) {
            return Err(Error::Text(FROZEN.replace("{n}", &pane.to_string())));
        }
        done(self.highlight_range(pane, range))
    }

    /// A comment row: the cursor on its line, or its element highlighted. No thread.
    fn replay_comment(
        &mut self,
        pane: PaneId,
        line: Option<u32>,
        selector: Option<String>,
    ) -> Result<(), Error> {
        if let Some(sel) = selector {
            return self.replay_element(pane, &sel);
        }
        if let (Some(l), Some(p)) = (line, self.panels.get_mut(&pane)) {
            p.jump_to_line(l);
        }
        Ok(())
    }

    /// Highlight element `sel` in browser pane `pane` again, with the user's notice for each cause.
    fn replay_element(&mut self, pane: PaneId, sel: &str) -> Result<(), Error> {
        self.highlight_element(pane, sel).map_err(|e| match e {
            Error::NoElement { selector, pane } => Error::Text(
                NO_ELEMENT
                    .replace("{selector}", &selector)
                    .replace("{n}", &pane.to_string()),
            ),
            Error::PageNotReady(_) => Error::Text(PAGE_NOT_READY.replace("{n}", &pane.to_string())),
            Error::BrowserExited => Error::Text(browser::EXITED.into()),
            e => e,
        })
    }
}

/// A `Response` from the agent's helpers as the user's result: its error's text, else nothing.
fn done(r: Response) -> Result<(), Error> {
    match r {
        Response::Error { message } => Err(Error::Text(message)),
        _ => Ok(()),
    }
}

/// The pane that a command's event names.
fn pane_of(event: &Event) -> Option<PaneId> {
    match event {
        Event::Close { pane, .. } => *pane,
        Event::Focus { pane }
        | Event::Highlight { pane, .. }
        | Event::Diff { pane, .. }
        | Event::Comment { pane, .. } => Some(*pane),
        _ => None,
    }
}

/// The command's name, as the agent types it.
fn verb(event: &Event) -> &'static str {
    match event {
        Event::Open { .. } => "open",
        Event::Close { .. } => "close",
        Event::Focus { .. } => "focus",
        Event::Highlight { .. } => "highlight",
        Event::Diff { .. } => "diff",
        Event::Comment { .. } => "comment",
        _ => "",
    }
}

/// What the command acted on: the file's name, with its lines, its element, or the state it set.
fn target(event: &Event, path: Option<&str>) -> String {
    let name = match (path, pane_of(event)) {
        (Some(p), _) => file_name(p),
        (None, Some(n)) => format!("pane #{n}"),
        (None, None) => String::new(),
    };
    match event {
        Event::Close { all: true, .. } => "every pane".into(),
        Event::Focus { pane } if *pane == PTY_PANE => "the shell".into(),
        Event::Highlight { cleared: true, .. } => format!("{name} · cleared"),
        Event::Highlight {
            selector: Some(s), ..
        }
        | Event::Comment {
            selector: Some(s), ..
        } => format!("{name} {s}"),
        Event::Highlight { start, end, .. } => format!("{name}:{}", lines(*start, *end)),
        Event::Comment { line, .. } => format!("{name}:{}", lines(*line, *line)),
        Event::Diff { on, .. } => format!("{name} · {}", if *on { "on" } else { "off" }),
        _ => name,
    }
}

/// `L40` or `L40–52`.
fn lines(start: Option<u32>, end: Option<u32>) -> String {
    let (s, e) = (start.unwrap_or_default(), end.or(start).unwrap_or_default());
    if e > s {
        format!("L{s}–{e}")
    } else {
        format!("L{s}")
    }
}

/// A file's name, or a URL as it is.
fn file_name(path: &str) -> String {
    if browser::is_url(path) {
        return path.into();
    }
    Path::new(path)
        .file_name()
        .map_or_else(|| path.into(), |n| n.to_string_lossy().into_owned())
}
