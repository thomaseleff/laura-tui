//! The History: what passed through Laura in the current session, as the drawer shows it. Each say,
//! each run of the agent's other commands folded into one row, each submitted inline review, and
//! each message from the message box, and each wait with no say since the user's last message.

use std::path::Path;

use serde::Serialize;

use crate::journal::Event;
use crate::protocol::{PTY_PANE, PaneId};

/// One row of the History. The web app paints the rows as they are.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Row {
    /// The agent's say, as markdown and as Laura's HTML of it.
    Say { body: String, html: String },
    /// A run of the agent's other commands, in one line. `pane`: the run's last pane, which a
    /// click focuses.
    Commands { text: String, pane: Option<PaneId> },
    /// The user's submitted inline review.
    Review {
        pane: PaneId,
        path: String,
        comments: usize,
        body: String,
    },
    /// The user's message from the message box.
    Message { body: String },
    /// The shell waits with no say since the user's last message. `done`: the wait ended.
    Wait { done: bool },
}

#[derive(Default)]
pub(super) struct History {
    rows: Vec<Row>,
    /// The events of the last row while it is `Commands`, which the fold reads.
    run: Vec<Event>,
    /// The says over the tab's life, so a front-end tells a new say from the last one it showed.
    says: u64,
    /// Counts each change, so the web app sends the History only when it changed.
    seq: u64,
    /// A message from the agent is being applied: only its commands fold into the History. The
    /// user's close or link click runs the same functions.
    agent: bool,
}

impl History {
    /// Add one journaled event.
    pub(super) fn push(&mut self, event: &Event) {
        let row = match event {
            Event::Say { body } => {
                self.says += 1;
                Row::Say {
                    body: body.clone(),
                    html: crate::render::markdown_html(body),
                }
            }
            Event::Open { .. }
            | Event::Close { .. }
            | Event::Focus { .. }
            | Event::Highlight { .. }
            | Event::Diff { .. }
            | Event::Comment { .. } => {
                if self.agent {
                    self.command(event);
                }
                return;
            }
            Event::Review {
                pane,
                path,
                comments,
                body,
                error: None,
            } => Row::Review {
                pane: *pane,
                path: path.clone(),
                comments: *comments,
                body: body.clone(),
            },
            Event::Message { body } => Row::Message { body: body.clone() },
            Event::Wait { said: false } => Row::Wait { done: false },
            _ => return,
        };
        self.run.clear();
        self.rows.push(row);
        self.seq += 1;
    }

    /// The agent's command joins the last row's run, or starts one.
    fn command(&mut self, event: &Event) {
        if self.run.is_empty() {
            self.rows.push(Row::Commands {
                text: String::new(),
                pane: None,
            });
        }
        self.run.push(event.clone());
        let (folded, last) = (fold(&self.run), run_pane(&self.run));
        if let Some(Row::Commands { text, pane }) = self.rows.last_mut() {
            (*text, *pane) = (folded, last);
        }
        self.seq += 1;
    }

    pub(super) fn set_agent(&mut self, on: bool) {
        self.agent = on;
    }

    /// The wait ended: its row loses its button.
    pub(super) fn end_wait(&mut self) {
        if let Some(Row::Wait { done }) = self
            .rows
            .iter_mut()
            .rev()
            .find(|r| matches!(r, Row::Wait { .. }))
            && !*done
        {
            *done = true;
            self.seq += 1;
        }
    }

    /// A new session (`ready`): an empty chat. The say count goes on.
    pub(super) fn new_session(&mut self) {
        self.rows.clear();
        self.run.clear();
        self.seq += 1;
    }

    /// The session's latest say, with its count.
    pub(super) fn latest_say(&self) -> Option<(u64, &str)> {
        self.rows.iter().rev().find_map(|r| match r {
            Row::Say { body, .. } => Some((self.says, body.as_str())),
            _ => None,
        })
    }

    pub(super) fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub(super) fn seq(&self) -> u64 {
        self.seq
    }
}

/// The run's last pane that is still open after it, for the row's click.
fn run_pane(run: &[Event]) -> Option<PaneId> {
    let mut closed = vec![];
    for e in run.iter().rev() {
        let pane = match e {
            Event::Close { all: true, .. } => return None,
            Event::Close { pane, .. } => {
                closed.extend(*pane);
                continue;
            }
            Event::Open { pane, .. }
            | Event::Focus { pane }
            | Event::Highlight { pane, .. }
            | Event::Diff { pane, .. }
            | Event::Comment { pane, .. } => *pane,
            _ => continue,
        };
        if !closed.contains(&pane) {
            return Some(pane);
        }
    }
    None
}

/// The run in one line: one phrase for each verb, in the order the agent first used it, such as
/// "opened notes.md, highlighted L40–52".
fn fold(run: &[Event]) -> String {
    let mut verbs: Vec<(std::mem::Discriminant<Event>, Vec<&Event>)> = vec![];
    for e in run {
        let d = std::mem::discriminant(e);
        match verbs.iter_mut().find(|(v, _)| *v == d) {
            Some((_, es)) => es.push(e),
            None => verbs.push((d, vec![e])),
        }
    }
    verbs
        .iter()
        .map(|(_, es)| phrase(es))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One verb's events, which share a variant, as a phrase. A verb that sets a state (a highlight,
/// the diff view, the focus) reads as its last event.
fn phrase(es: &[&Event]) -> String {
    let n = es.len();
    match es[n - 1] {
        Event::Open { path, .. } if n == 1 => format!("opened {}", file_name(path)),
        Event::Open { .. } => format!("opened {n} files"),
        Event::Close { .. }
            if es
                .iter()
                .any(|e| matches!(e, Event::Close { all: true, .. })) =>
        {
            "closed every pane".into()
        }
        Event::Close { pane, .. } if n == 1 => format!("closed pane #{}", pane.unwrap_or_default()),
        Event::Close { .. } => format!("closed {n} panes"),
        Event::Focus { pane } if *pane == PTY_PANE => "focused the shell".into(),
        Event::Focus { pane } => format!("focused pane #{pane}"),
        Event::Highlight { cleared: true, .. } => "cleared the highlight".into(),
        Event::Highlight {
            selector: Some(s), ..
        } => format!("highlighted {s}"),
        Event::Highlight { start, end, .. } => format!("highlighted {}", lines(*start, *end)),
        Event::Diff { pane, on: true } => format!("showed the diff of pane #{pane}"),
        Event::Diff { pane, .. } => format!("hid the diff of pane #{pane}"),
        Event::Comment { .. } if n > 1 => format!("left {n} comments"),
        Event::Comment {
            selector: Some(s), ..
        } => format!("commented on {s}"),
        Event::Comment { line, .. } => format!("commented on {}", lines(*line, *line)),
        _ => String::new(),
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
    if crate::browser::is_url(path) {
        return path.into();
    }
    Path::new(path)
        .file_name()
        .map_or_else(|| path.into(), |n| n.to_string_lossy().into_owned())
}
