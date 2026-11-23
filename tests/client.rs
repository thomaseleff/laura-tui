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

use std::time::Instant;

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

/// Pane 1 holds a file, and the pane selector focused pane 1.
fn file_focused(r: &mut Rig, c: &Client) -> Result<tempfile::TempDir> {
    let dir = open_five(r)?;
    c.until(
        r,
        "the file's rows",
        "document.querySelector('#p1 .rows > [data-row]')",
    );
    c.focus(r, 1);
    Ok(dir)
}

#[test]
fn f12_from_a_file_pane_opens_the_drawer() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = file_focused(&mut r, &c)?;
    c.press(F12);
    c.until(
        &mut r,
        "the drawer, with the shell's focus",
        "document.getElementById('p0').classList.contains('open') && document.activeElement?.closest('#p0 .xterm') != null",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

#[test]
#[ignore = "#230: F12 doesn't tell Laura the focus"]
fn f12_from_a_file_pane_focuses_the_shell() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = file_focused(&mut r, &c)?;
    c.press(F12);
    let t0 = Instant::now();
    while r.tab.focus != 0 {
        assert!(
            t0.elapsed() < common::DEADLINE,
            "Laura's focus stays on pane {}",
            r.tab.focus
        );
        r.pump();
    }
    Ok(())
}

#[test]
fn c_then_shift_s_submits_an_inline_review() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    r.tab.agent = true;
    let c = Client::open(&mut r, &path);
    let _dir = file_focused(&mut r, &c)?;
    c.press(C);
    c.until(
        &mut r,
        "the comment's box",
        "document.activeElement?.matches('.draft textarea')",
    );
    c.type_text("Check this line");
    c.press(ENTER);
    c.until(
        &mut r,
        "the saved comment",
        "!document.querySelector('.draft textarea')",
    );
    assert_eq!(r.tab.panels[&1].thread_count(), 1);
    c.press(SHIFT_S);
    c.until(
        &mut r,
        "the review body's box",
        "document.activeElement?.matches('.pop.review textarea')",
    );
    c.type_text("Looks good");
    c.press(ENTER);
    // The drawer's few rows scroll the block's first line off Laura's screen, not off xterm's.
    c.until(
        &mut r,
        "the inline review in the shell",
        &format!("{SHELL_LINES}.some(l => l.includes('[laura review · pane #1'))"),
    );
    assert_eq!(r.tab.panels[&1].thread_count(), 0);
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

#[test]
fn ctrl_p_opens_the_pane_selector() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    c.press(CTRL_P);
    c.until(
        &mut r,
        "the pane selector, with the focus",
        "!document.getElementById('popup').hidden && document.activeElement?.id === 'pq'",
    );
    c.press(ESCAPE);
    c.until(
        &mut r,
        "the pane selector closes",
        "document.getElementById('popup').hidden",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}
