//! Per-tab local socket: typed request/response, NDJSON framing, bind/connect.
//!
//! `LAURA_TAB` holds a namespaced socket name, one per tab; a producer reaches a tab by connecting to it. Scoping is a consequence of addressing, not a security boundary.
//!
//! The socket is **one request → one response**: a client writes one `Message`, the run loop (which owns live layout state) writes back one `Response`. Dropping the reply unsent is a clean EOF the client reads as `Ok`.

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver};

use anyhow::Result;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, prelude::*};
use serde::{Deserialize, Serialize};

/// Per-tab pane handle. Minted on `open`, stable for the pane's lifetime; PTY is always `0`.
pub type PaneId = u64;

/// The shell pane: reserved, always present, never closed.
pub const PTY_PANE: PaneId = 0;

/// Split orientation. `Horizontal` = side-by-side, `Vertical` = stacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    #[default]
    #[value(alias = "h")]
    Horizontal,
    #[value(alias = "v")]
    Vertical,
}

/// Which side of a new split the *new* pane lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    First,
    #[default]
    Second,
}

/// Whether a pane hosts the shell or a file panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneKind {
    Pty,
    Panel,
}

/// A single request sent to a tab. Internally tagged so the JSON is a stable wire contract (`{"type":"open","path":"…"}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    /// Open `path` in a new panel, splitting `split` (or the focused pane). Back-compat: only `path` is required.
    Open {
        path: String,
        /// Pane to split; `None` splits the focused pane.
        #[serde(default)]
        split: Option<PaneId>,
        #[serde(default)]
        dir: Dir,
        /// Percent of the split given to the new panel, 1..=99.
        #[serde(default = "default_ratio")]
        ratio: u16,
        #[serde(default)]
        side: Side,
        /// Move focus into the new panel. Defaults true so an older `{"type":"open"}` still focuses.
        #[serde(default = "default_true")]
        focus: bool,
        /// Autoscroll: pin the cursor to the last line on open and every reload (tail-style).
        #[serde(default)]
        follow: bool,
        /// Report the would-be layout without committing.
        #[serde(default)]
        dry_run: bool,
        /// Highlight this line range (1-based, inclusive) once the panel is open.
        /// `None` = open with no highlight; the second value defaults to the first.
        #[serde(default)]
        highlight: Option<(u32, u32)>,
        /// Open straight into the inline diff view (vs HEAD). Ignored (with a warning)
        /// if there's nothing to diff — no `git`, or a clean/untracked file.
        #[serde(default)]
        diff: bool,
    },
    /// Close a pane. `None` = the focused panel; `all` returns to PTY-only. The PTY can't close.
    Close {
        #[serde(default)]
        pane: Option<PaneId>,
        #[serde(default)]
        all: bool,
    },
    /// Focus a pane by stable id.
    Focus { pane: PaneId },
    /// Highlight lines `start..=end` (1-based, inclusive) in a panel and scroll
    /// them into view. `pane` defaults to the focused panel; `end` defaults to `start`.
    Highlight {
        #[serde(default)]
        pane: Option<PaneId>,
        start: u32,
        #[serde(default)]
        end: Option<u32>,
    },
    /// Toggle (or set) the inline diff view on a panel. `pane` defaults to the
    /// focused panel; `on` = `None` toggles, `Some(b)` sets. Refused (error) when
    /// there's nothing to diff — no `git`, or a clean/untracked file.
    DiffView {
        #[serde(default)]
        pane: Option<PaneId>,
        #[serde(default)]
        on: Option<bool>,
    },
    /// Query the current layout + per-pane geometry/overflow.
    Layout,
    /// Mark the tab as hosting an agent; enables review submission. Optionally names the
    /// journal `session` and the `agent`; the reply carries the journal path.
    Ready {
        #[serde(default)]
        session: Option<String>,
        #[serde(default)]
        agent: Option<String>,
    },
    /// Append a meta-signal to the journal: how well Laura/the agent performed
    /// (layout, render quality, a missing tool) — not review content.
    Feedback {
        /// `"+"` positive, `"-"` negative.
        sentiment: String,
        #[serde(default)]
        body: Option<String>,
    },
    /// Reserved re-render nudge; not yet emitted.
    Update { path: String },
}

/// The tab's answer to a request. Internally tagged; a dropped (unsent) reply reads as `Ok`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Response {
    /// Mutation succeeded, nothing to report.
    Ok,
    /// `open` succeeded; carries the new pane id and any non-fatal nudges.
    Opened {
        pane: PaneId,
        #[serde(default)]
        warnings: Vec<String>,
    },
    /// Answers `Layout` and dry-run `Open`.
    Report(LayoutReport),
    /// `ready` succeeded; carries the session's journal path.
    Ready { journal: String },
    /// The request failed.
    Error { message: String },
}

/// Geometry + overflow for every pane at the current terminal size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutReport {
    pub area: RectDto,
    pub panes: Vec<PaneReport>,
}

/// One pane's placement and whether its content fits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneReport {
    pub id: PaneId,
    pub kind: PaneKind,
    pub path: Option<String>,
    pub rect: RectDto,
    /// Wrapped content height; `None` for the PTY (its grid always fits its rect).
    pub content_rows: Option<usize>,
    pub visible_rows: usize,
    /// `content_rows - visible_rows`, floored at 0.
    pub overflow_rows: usize,
    /// Content overflows, or the rect is too small to be usable.
    pub clipped: bool,
}

/// Serializable mirror of a ratatui `Rect` (which isn't `Serialize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RectDto {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl From<ratatui::layout::Rect> for RectDto {
    fn from(r: ratatui::layout::Rect) -> Self {
        RectDto {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_ratio() -> u16 {
    50
}

/// Client: connect to `tab`, write one request, read the one response line. A clean EOF (reply dropped unsent) reads as `Ok`.
pub fn request(tab: &str, msg: &Message) -> Result<Response> {
    let name = tab.to_ns_name::<GenericNamespaced>()?;
    let conn = Stream::connect(name)?;
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    let mut reader = BufReader::new(conn);
    reader.get_mut().write_all(line.as_bytes())?;
    let mut resp = String::new();
    if reader.read_line(&mut resp)? == 0 {
        return Ok(Response::Ok); // clean EOF
    }
    Ok(serde_json::from_str(resp.trim_end())?)
}

/// A held connection the run loop answers a request on. Sending (or dropping) it consumes it.
pub struct Reply {
    stream: Option<Stream>,
}

impl Reply {
    /// Write one `Response` line back to the client.
    pub fn send(mut self, resp: &Response) {
        if let Some(mut s) = self.stream.take()
            && let Ok(mut line) = serde_json::to_string(resp)
        {
            line.push('\n');
            let _ = s.write_all(line.as_bytes());
        }
    }
}

/// Server: bind `tab` synchronously (bind errors surface here), then accept on a background thread, forwarding each `(Message, Reply)` on the returned channel. One request per connection; the run loop holds the reply and answers.
pub fn serve(tab: &str) -> Result<Receiver<(Message, Reply)>> {
    let name = tab.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(name).create_sync()?;
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let mut reader = BufReader::new(conn);
            let mut line = String::new();
            if reader.read_line(&mut line).is_ok()
                && let Ok(msg) = serde_json::from_str::<Message>(line.trim_end())
            {
                let reply = Reply {
                    stream: Some(reader.into_inner()),
                };
                if tx.send((msg, reply)).is_err() {
                    return; // receiver dropped
                }
            }
        }
    });

    Ok(rx)
}
