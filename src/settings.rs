//! The user's settings, for every workspace: `~/.laura/settings.json`, one JSON object. A missing
//! key means the default.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Map, Value};

/// `~/.laura/settings.json`.
pub fn path() -> PathBuf {
    crate::journal::data_dir().join("settings.json")
}

/// The settings in the file. A missing file has none. A file that Laura can't read or parse is an
/// error, so a save doesn't drop the keys of a file that the user edits by hand.
fn read() -> Result<Map<String, Value>> {
    match std::fs::read_to_string(path()) {
        Ok(s) => Ok(serde_json::from_str(&s)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.into()),
    }
}

pub fn get(key: &str) -> Option<Value> {
    read().ok()?.remove(key)
}

/// Set `key`, or remove it for `None`. Keeps the other keys.
pub fn set(key: &str, value: Option<Value>) -> Result<()> {
    let dir = crate::journal::data_dir();
    std::fs::create_dir_all(&dir)?;
    // Another Laura's save waits here, so neither save drops the other's key. The OS ends the lock
    // when `lock` drops, or when a Laura crashes. Never truncate: another Laura may hold the lock.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dir.join("settings.lock"))?;
    lock.lock()?;
    let mut s = read()?;
    match value {
        Some(v) => s.insert(key.into(), v),
        None => s.remove(key),
    };
    crate::web::write_private(&path(), Value::Object(s).to_string().as_bytes())
}
