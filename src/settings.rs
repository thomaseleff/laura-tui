//! The user's settings, for every workspace: `~/.laura/settings.json`, one JSON object. A missing
//! key means the default.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Map, Value};

/// `~/.laura/settings.json`.
pub fn path() -> PathBuf {
    crate::journal::data_dir().join("settings.json")
}

fn all() -> Map<String, Value> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn get(key: &str) -> Option<Value> {
    all().remove(key)
}

/// Set `key`, or remove it for `None`. Keeps the other keys.
pub fn set(key: &str, value: Option<Value>) -> Result<()> {
    let mut s = all();
    match value {
        Some(v) => s.insert(key.into(), v),
        None => s.remove(key),
    };
    std::fs::create_dir_all(crate::journal::data_dir())?;
    crate::web::write_private(&path(), Value::Object(s).to_string().as_bytes())
}
