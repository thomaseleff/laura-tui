//! The web app's client (#228): the real page in Chrome, driven by the user's keys, with Laura's
//! state and the page's DOM checked after each step. Each test skips when neither Chrome nor Edge
//! is installed; under `CI`, it fails.

#[path = "common/browser.rs"]
#[allow(dead_code)] // each browser test crate uses part of the browser rig
mod browser_rig;
#[path = "common/client.rs"]
mod client_rig;
mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use anyhow::Result;
use serde_json::json;

use browser_rig::{bare, chrome};
use client_rig::*;
use rig::Rig;

/// A file of five lines, `line 1` to `line 5`, open in pane 1.
fn open_five(r: &mut Rig) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("five.txt");
    std::fs::write(
        &path,
        (1..=5).map(|n| format!("line {n}\n")).collect::<String>(),
    )?;
    assert_eq!(r.drive(&["open", &path.to_string_lossy()]), "1");
    Ok(dir)
}

#[test]
fn the_web_app_loads_and_paints() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    assert_eq!(c.errors(), json!([]));
    c.type_text("echo client-smoke");
    c.press(ENTER);
    c.until(
        &mut r,
        "the echo in the shell",
        &format!("{SHELL_LINES}.some(l => l.trim() === 'client-smoke')"),
    );
    let _dir = open_five(&mut r)?;
    let text = c.until(
        &mut r,
        "the file's rows",
        "document.querySelector('#p1 .rows')?.textContent.includes('line 5') && document.querySelector('#p1 .rows').textContent",
    );
    for n in 1..=5 {
        assert!(text.to_string().contains(&format!("line {n}")), "{text}");
    }
    assert_eq!(c.errors(), json!([]));
    Ok(())
}
