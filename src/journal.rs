//! Persisted composition journal: one append-only NDJSON file per session, teeing
//! `open`/`close`/`focus`/`review`/`feedback` events so a session is auditable after it ends.
//!
//! Files live under the OS data dir (`%APPDATA%` / `$XDG_DATA_HOME` / `~/Library/Application Support`),
//! overridable via `LAURA_DATA_DIR`. Auditing is just files: `cat $(ls -t <dir>/*.ndjson | head) | jq …`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

/// The OS data dir Laura writes under, honoring `LAURA_DATA_DIR` first.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("LAURA_DATA_DIR") {
        return PathBuf::from(d);
    }
    #[cfg(windows)]
    if let Ok(d) = std::env::var("APPDATA") {
        return PathBuf::from(d);
    }
    #[cfg(target_os = "macos")]
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h).join("Library/Application Support");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(d) = std::env::var("XDG_DATA_HOME") {
            return PathBuf::from(d);
        }
        if let Ok(h) = std::env::var("HOME") {
            return PathBuf::from(h).join(".local/share");
        }
    }
    std::env::temp_dir()
}

/// The per-session NDJSON path. `session` is sanitized to a safe file stem.
pub fn session_path(session: &str) -> PathBuf {
    data_dir()
        .join("laura")
        .join("sessions")
        .join(format!("{}.ndjson", sanitize(session)))
}

/// Runtime scratch dir for internal, auto-removed files (e.g. `laura tail` spools).
pub fn runtime_dir() -> PathBuf {
    data_dir().join("laura").join("runtime")
}

/// Whether `path` is a file Laura owns under `runtime_dir` (so `close` may delete it).
pub fn is_runtime_temp(path: &str) -> bool {
    let rt = runtime_dir();
    Path::new(path).starts_with(&rt)
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
