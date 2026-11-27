//! Persisted composition journal: one append-only NDJSON file per session, teeing every host
//! event so a session is auditable after it ends. `read_events` reads them back for `laura journal`.
//!
//! Files live at `~/.laura/sessions/<session>.ndjson`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::platform::{data_dir, runtime_dir, strip_verbatim};
use crate::protocol::PaneId;

/// Where every session's journal lives.
pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

/// The per-session NDJSON path. `session` is sanitized to a safe file stem.
pub fn session_path(session: &str) -> PathBuf {
    sessions_dir().join(format!("{}.ndjson", sanitize(session)))
}

/// Whether `path` is a spool directly in `runtime_dir` (so `close` may delete it).
/// An exact parent match: a prefix check would also match `<runtime>/../x`, since Unix `absolute` keeps `..`.
pub fn is_runtime_temp(path: &str) -> bool {
    Path::new(path).parent() == Some(runtime_dir().as_path())
}

/// Keep session ids to a safe file stem: alphanumerics, `-`, `_`, `.`; everything else → `_`.
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "session".into()
    } else {
        cleaned
    }
}

/// One journaled event, written as `{"type": "<variant>", …}`. The field names and values are
/// public: `laura journal` filters on them and `docs/cli.md` lists them; change both together.
/// An absent flag or optional value leaves its key out, except where a `null` says "none".
#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum Event {
    /// The agent named its session. `title` and `root` are `null` when unknown.
    Ready {
        pid: u32,
        title: Option<String>,
        root: Option<String>,
    },
    Open {
        pane: PaneId,
        path: String,
        #[serde(skip_serializing_if = "is_false")]
        replaced: bool,
        #[serde(skip_serializing_if = "is_false")]
        edit: bool,
    },
    /// One pane, or every pane with `all`.
    Close {
        #[serde(skip_serializing_if = "Option::is_none")]
        pane: Option<PaneId>,
        #[serde(skip_serializing_if = "is_false")]
        all: bool,
    },
    Focus {
        pane: PaneId,
    },
    /// Lines (`start`, `end`), an element (`selector`), or no highlight (`cleared`).
    Highlight {
        pane: PaneId,
        #[serde(skip_serializing_if = "Option::is_none")]
        start: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        #[serde(skip_serializing_if = "is_false")]
        cleared: bool,
    },
    Diff {
        pane: PaneId,
        on: bool,
    },
    /// The agent's comment, on a line or an element.
    Comment {
        pane: PaneId,
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// The agent's words to the user (`laura say`).
    Say {
        body: String,
    },
    /// The user's message from the chat's message box, which Laura wrote into the shell.
    Message {
        body: String,
    },
    /// The shell began a wait: it rang the bell or sent a notification. `said`: a say came since
    /// the user's last message.
    Wait {
        said: bool,
    },
    /// `body` and `pane` are `null` when absent.
    Feedback {
        sentiment: String,
        body: Option<String>,
        pane: Option<PaneId>,
    },
    /// A submitted inline review; `error` when it didn't reach the shell.
    Review {
        pane: PaneId,
        path: String,
        comments: usize,
        body: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Laura's browser didn't start.
    Browser {
        error: String,
        output: String,
    },
}

fn is_false(b: &bool) -> bool {
    !b
}

/// An append-only session log. The file is created lazily on the first `log`, so naming a
/// session (which returns its path) never writes until there's an event to record.
pub struct Journal {
    path: PathBuf,
    session: String,
    agent: Option<String>,
}

impl Journal {
    /// Name a journal for `session` (defaulting a blank one) and optional `agent`. No file yet.
    pub fn open(session: &str, agent: Option<String>) -> Journal {
        Journal {
            path: session_path(session),
            session: sanitize(session),
            agent,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The agent name this session was `ready`'d with, if any.
    pub fn agent(&self) -> Option<String> {
        self.agent.clone()
    }

    /// Append one event, stamping `ts` (unix ms), `session`, and `agent`. Best-effort:
    /// a write error is dropped — journaling must never crash the run loop.
    pub(crate) fn log(&self, event: &Event) {
        let Ok(Value::Object(mut obj)) = serde_json::to_value(event) else {
            return;
        };
        stamp(&mut obj, "ts", Value::from(now_ms()));
        stamp(&mut obj, "version", Value::from(build_version()));
        stamp(&mut obj, "session", Value::from(self.session.clone()));
        if let Some(a) = &self.agent {
            stamp(&mut obj, "agent", Value::from(a.clone()));
        }
        let Ok(mut line) = serde_json::to_string(&Value::Object(obj)) else {
            return;
        };
        line.push('\n');
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Every journaled event across sessions matching `filters` and newer than `since_ms`, oldest
/// first. A repeated field ORs, different fields AND. Unreadable files and bad lines are skipped:
/// the host may be mid-write.
// ponytail: reads every journal in full per call; read the newest files first if session count makes it slow.
pub fn read_events(filters: &[(String, String)], since_ms: Option<u64>) -> Vec<Value> {
    let cutoff = since_ms.map_or(0, |ms| now_ms().saturating_sub(ms));
    let mut events: Vec<Value> = journals()
        .flat_map(|s| {
            s.lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(Value::is_object) // a bare `5` parses but isn't an event
                .collect::<Vec<_>>()
        })
        .filter(|v| ts(v) >= cutoff && matches(v, filters))
        .collect();
    events.sort_by_key(ts);
    events
}

/// Whether the user ever submitted an inline review, in any folder: a `review` event without an
/// `error`.
/// Stops at the first one, and parses only the lines that can hold one.
pub(crate) fn reviewed() -> bool {
    journals().any(|s| {
        s.lines()
            .filter(|l| l.contains("\"review\""))
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .any(|e| e["type"] == "review" && e.get("error").is_none())
    })
}

/// A session in a folder, for the start canvas: its session label, its newest event in Unix ms,
/// the files it opened or reviewed that still exist, newest first, and its submitted inline reviews.
#[derive(Serialize)]
pub(crate) struct Recent {
    label: Option<String>,
    ts: u64,
    files: Vec<String>,
    reviews: usize,
}

/// The most files a recent session lists.
const RECENT_FILES: usize = 4;

/// The `n` newest sessions in the canonical folder `root`. A session belongs to `root` when its
/// last `ready` names `root`, or, from an older Laura without `root`, when it opened or reviewed a
/// file below `root`.
// shortcut: reads every journal in full, as `read_events` does; read the newest files first if session count makes it slow.
pub(crate) fn recent(root: &Path, n: usize) -> Vec<Recent> {
    let root = strip_verbatim(root.to_path_buf());
    let mut sessions: Vec<Recent> = journals().filter_map(|s| session_in(&s, &root)).collect();
    sessions.sort_by_key(|r| std::cmp::Reverse(r.ts));
    sessions.truncate(n);
    sessions
}

/// One journal's session, when it belongs to `root`.
fn session_in(text: &str, root: &Path) -> Option<Recent> {
    let events: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(Value::is_object)
        .collect();
    let kind = |e: &Value, t: &[&str]| t.iter().any(|t| e["type"] == *t);
    let ready = events.iter().rev().find(|e| kind(e, &["ready"]))?;
    // Before any `canonicalize`: a session in another folder costs no file system call.
    if ready["root"].as_str().is_some_and(|r| Path::new(r) != root) {
        return None;
    }
    // Each file below `root`, relative, newest first, and whether it still exists. A file that is
    // gone since is below `root` when its folder is, so the session still belongs to `root`.
    let below = |e: &Value| {
        let p = Path::new(e["path"].as_str()?);
        let full = p
            .canonicalize()
            .ok()
            .or_else(|| Some(p.parent()?.canonicalize().ok()?.join(p.file_name()?)))?;
        let rel = strip_verbatim(full).strip_prefix(root).ok()?.to_path_buf();
        Some((rel.to_string_lossy().replace('\\', "/"), p.exists()))
    };
    let opened: Vec<(String, bool)> = events
        .iter()
        .rev()
        .filter(|e| kind(e, &["open", "review"]))
        .filter_map(below)
        .collect();
    // A journal from before `root` belongs to the folder where it opened a file.
    if ready["root"].as_str().is_none() && opened.is_empty() {
        return None;
    }
    let mut files: Vec<String> = vec![];
    for (f, _) in opened.into_iter().filter(|(_, exists)| *exists) {
        if !files.contains(&f) && files.len() < RECENT_FILES {
            files.push(f);
        }
    }
    Some(Recent {
        label: events
            .iter()
            .rev()
            .find_map(|e| kind(e, &["ready"]).then(|| e["title"].as_str()).flatten())
            .map(str::to_string),
        ts: events.iter().map(ts).max().unwrap_or(0),
        files,
        reviews: events
            .iter()
            .filter(|e| kind(e, &["review"]) && e.get("error").is_none())
            .count(),
    })
}

/// The text of each session's journal. An unreadable file is skipped: the host may be mid-write.
fn journals() -> impl Iterator<Item = String> {
    std::fs::read_dir(sessions_dir())
        .into_iter() // no sessions yet: no journals
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ndjson"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
}

fn ts(v: &Value) -> u64 {
    v["ts"].as_u64().unwrap_or(0)
}

fn matches(v: &Value, filters: &[(String, String)]) -> bool {
    filters.iter().all(|(field, _)| {
        filters
            .iter()
            .filter(|(f, _)| f == field)
            .any(|(_, want)| match &v[field] {
                Value::String(s) => s == want,
                other => serde_json::from_str::<Value>(want).is_ok_and(|w| w == *other),
            })
    })
}

/// `X.Y.Z+abc1234` off a git checkout (commit set by build.rs), bare `X.Y.Z` off a tarball.
fn build_version() -> String {
    let commit = env!("LAURA_COMMIT");
    if commit.is_empty() {
        env!("CARGO_PKG_VERSION").to_string()
    } else {
        format!("{}+{}", env!("CARGO_PKG_VERSION"), commit)
    }
}

fn stamp(obj: &mut Map<String, Value>, key: &str, val: Value) {
    obj.entry(key.to_string()).or_insert(val);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
