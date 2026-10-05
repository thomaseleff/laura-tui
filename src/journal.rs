//! Persisted composition journal: one append-only NDJSON file per session, teeing every host
//! event so a session is auditable after it ends. `read_events` reads them back for `laura journal`.
//!
//! Files live at `~/.laura/sessions/<session>.ndjson`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

/// Laura's data dir: `~/.laura` on every OS.
pub fn data_dir() -> PathBuf {
    std::env::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".laura")
}

/// Where every session's journal lives.
pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

/// The per-session NDJSON path. `session` is sanitized to a safe file stem.
pub fn session_path(session: &str) -> PathBuf {
    sessions_dir().join(format!("{}.ndjson", sanitize(session)))
}

/// Scratch dir for `laura tail` spools: a per-user temp dir, so a crash's leftovers sit with the OS's temp files.
/// Always a `laura` subdir: `is_runtime_temp` is a direct-child check, and `close` deletes what it matches.
/// Inside a tab, `LAURA_RUNTIME` carries the host's choice, so a client with other temp env spools where `close` looks (#84).
/// A `LAURA_RUNTIME` the client can't create (sandbox) falls through; `close` can't find that spool, so it stays in the temp dir.
pub fn runtime_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("LAURA_RUNTIME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute() && d.ends_with("laura") && std::fs::create_dir_all(d).is_ok())
    {
        return d;
    }
    // An XDG_RUNTIME_DIR Laura can't create a dir in falls through to the per-OS dir.
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute() && std::fs::create_dir_all(d.join("laura")).is_ok())
    {
        return d.join("laura");
    }
    if cfg!(any(windows, target_os = "macos")) {
        std::env::temp_dir().join("laura") // per-user already
    } else {
        data_dir().join("runtime").join("laura") // a bare /tmp is shared between users
    }
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
    pub fn log(&self, event: Value) {
        let Value::Object(mut obj) = event else {
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
    let Ok(dir) = std::fs::read_dir(sessions_dir()) else {
        return vec![]; // no sessions yet
    };
    let cutoff = since_ms.map_or(0, |ms| now_ms().saturating_sub(ms));
    let mut events: Vec<Value> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ndjson"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
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
