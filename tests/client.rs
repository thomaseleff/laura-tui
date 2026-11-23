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

/// Each line in the shell, and the links that the shell's link provider finds in it: each link's
/// text and first column. From `PATH`'s comment in `app.js`.
const LINKS: &[(&str, &[(&str, u64)])] = &[
    ("src/tab.rs:41", &[("src/tab.rs:41", 1)]),
    ("src/foo.test.ts", &[("src/foo.test.ts", 1)]),
    // No `/` or `\`: a word, not a path.
    ("foo.test.ts", &[]),
    ("app/[slug]/page.tsx", &[("app/[slug]/page.tsx", 1)]),
    ("app/(group)/page.tsx", &[("app/(group)/page.tsx", 1)]),
    (
        "packages/@scope/pkg/index.ts",
        &[("packages/@scope/pkg/index.ts", 1)],
    ),
    ("Update(src/a.rs)", &[("src/a.rs", 8)]),
    ("**src/a.rs**", &[("src/a.rs", 3)]),
    ("src/**/a.rs", &[]),
    ("**/a.rs", &[]),
    ("https://x.com/@a/b.js", &[]),
    ("~/a/b.rs", &[]),
    ("foo.test.js/index", &[]),
    ("logs/app.log.1", &[]),
    (r"C:\src\main.rs:12", &[(r"C:\src\main.rs:12", 1)]),
    ("./a/b.rs", &[("./a/b.rs", 1)]),
    ("../x/y.py:3", &[("../x/y.py:3", 1)]),
    ("see src/a.rs.", &[("src/a.rs", 5)]),
    (
        "src/a.rs and lib/b.py",
        &[("src/a.rs", 1), ("lib/b.py", 14)],
    ),
    // The emoji takes two columns.
    ("🚀 src/a.rs", &[("src/a.rs", 4)]),
];

/// Each of `lines`, alone in the shell's xterm, and its links from the shell's link provider. The
/// test writes into xterm, not into the shell. The shell's replay and prompt can reach xterm at any
/// time, so the JS turns off the shell's writes to xterm and lets xterm finish the bytes it holds
/// before the first row. xterm has no public way to ask a link provider, so the JS reads xterm's
/// private `_core`, where Laura's provider comes after xterm's own (OSC 8).
const PROVIDE_LINKS: &str = "async lines => {
  const t = terms.get(0), write = t.write.bind(t), p = t._core?._linkProviderService?.linkProviders?.at(-1);
  if (!p) return 'xterm has no term._core._linkProviderService.linkProviders';
  t.write = () => {};
  try {
    await new Promise(done => write('', done));
    const out = [];
    for (const line of lines) {
      t.reset();
      await new Promise(done => write(line, done));
      out.push(await new Promise(done => p.provideLinks(1, ls => done((ls ?? []).map(l => [l.text, l.range.start.x])))));
    }
    return out;
  } finally {
    delete t.write;
  }
}";

#[test]
fn the_shell_links_these_paths() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let lines: Vec<&str> = LINKS.iter().map(|(l, _)| *l).collect();
    let got = c
        .eval(&format!("({PROVIDE_LINKS})({})", json!(lines)))
        .unwrap_or_else(|e| panic!("the links: {e}"));
    let want: Vec<_> = LINKS.iter().map(|(_, links)| json!(links)).collect();
    for ((line, want), got) in lines
        .iter()
        .zip(&want)
        .zip(got.as_array().into_iter().flatten())
    {
        assert_eq!(got, want, "{line}");
    }
    assert_eq!(got.as_array().map(Vec::len), Some(lines.len()), "{got}");
    Ok(())
}
