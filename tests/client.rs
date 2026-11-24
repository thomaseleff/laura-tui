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
use serde_json::{Value, json};

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

/// On unix, the shell rings the terminal bell, and the workspace waits. Elsewhere the shell can't
/// ring it, and the tests check the wait only on unix, as `tests/web/shell.rs` does.
fn ring(r: &mut Rig, c: &Client) -> Result<()> {
    if cfg!(unix) {
        r.tab.pty.write(b"printf '\\007'\r")?;
        c.until(r, "the wait", "layout.waiting");
    }
    Ok(())
}

/// #230: with a file pane focused and the shell waiting, `route` moves the focus to the shell.
/// Laura's focus moves to the shell, and the wait ends.
fn into_the_shell(
    r: &mut Rig,
    c: &Client,
    route: impl FnOnce(&mut Rig),
) -> Result<tempfile::TempDir> {
    let dir = file_focused(r, c)?;
    ring(r, c)?;
    route(r);
    c.until(
        r,
        "the shell's focus, with no wait",
        "layout.focus === 0 && !layout.waiting",
    );
    assert_eq!(r.tab.focus, 0, "Laura's focus");
    assert!(!r.tab.waiting, "the wait");
    assert_eq!(c.errors(), json!([]));
    Ok(dir)
}

#[test]
fn f12_from_a_file_pane_focuses_the_shell() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = into_the_shell(&mut r, &c, |_| c.press(F12))?;
    Ok(())
}

#[test]
fn a_second_f12_focuses_the_file_pane_again() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = into_the_shell(&mut r, &c, |_| c.press(F12))?;
    c.press(F12);
    // `press` doesn't pump Laura, so the reply hasn't come yet.
    assert_eq!(
        c.eval("document.activeElement?.closest('#p0') == null"),
        Ok(json!(true)),
        "the shell has the keystrokes before Laura's reply"
    );
    c.until(
        &mut r,
        "the file pane's focus, with the drawer closed",
        "layout.focus === 1 && !document.getElementById('p0').classList.contains('open')",
    );
    assert_eq!(r.tab.focus, 1, "Laura's focus");
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

#[test]
fn ctrl_backquote_focuses_the_shell() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = into_the_shell(&mut r, &c, |_| c.press(CTRL_BACKQUOTE))?;
    Ok(())
}

#[test]
fn the_shell_button_focuses_the_shell() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = into_the_shell(&mut r, &c, |_| {
        c.eval("document.getElementById('sbtn').click()").unwrap();
    })?;
    Ok(())
}

#[test]
fn esc_from_a_file_pane_focuses_the_shell() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = into_the_shell(&mut r, &c, |_| c.press(ESCAPE))?;
    Ok(())
}

#[test]
fn the_pane_selector_focuses_the_shell() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = into_the_shell(&mut r, &c, |r| c.focus(r, 0))?;
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

/// The hint line shows the quit question.
const QUIT: &str =
    "document.getElementById('hint').textContent.includes('quit? press y to confirm')";

/// #229: the quit question takes the keystroke after F12, as the help and the pane selector do.
/// Before, F12 turned on the lock under the question, and the question came back after the unlock.
#[test]
fn the_quit_question_takes_the_keystroke_after_f12() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    c.press(CTRL_Q);
    c.until(&mut r, "the quit question", QUIT);
    c.press(F12);
    c.press(N);
    c.press(F12);
    c.until(&mut r, "the quit question closes", &format!("!({QUIT})"));
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// The hint line's text.
const HINT: &str = "document.getElementById('hint').textContent";

/// Opens the help with `open`, resizes the window, which repaints the hint line, and closes the help.
/// The hint line shows `hint` all along.
fn help_keeps_the_hint(r: &mut Rig, c: &Client, open: impl Fn(), hint: &Value) {
    open();
    c.until(r, "the help", "!document.getElementById('help').hidden");
    c.eval("dispatchEvent(new Event('resize'))").unwrap();
    assert_eq!(&c.eval(HINT).unwrap(), hint, "the hint line under the help");
    c.press(ESCAPE);
    c.until(
        r,
        "the help closes",
        "document.getElementById('help').hidden",
    );
    assert_eq!(&c.eval(HINT).unwrap(), hint, "the hint line after the help");
}

#[test]
fn the_hint_line_keeps_the_modes_keystrokes_under_the_help() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    r.tab.agent = true;
    let c = Client::open(&mut r, &path);
    let _dir = file_focused(&mut r, &c)?;
    let pane = c.eval(HINT).unwrap();
    c.press(C);
    c.until(
        &mut r,
        "the comment's box",
        "document.querySelector('.draft textarea')",
    );
    let draft = c.eval(HINT).unwrap();
    assert_ne!(draft, pane, "the draft's hint");
    help_keeps_the_hint(&mut r, &c, || c.press(CTRL_H), &draft);

    c.press(ESCAPE);
    c.until(
        &mut r,
        "the draft closes",
        "!document.querySelector('.draft textarea')",
    );
    c.press(F12);
    let locked = c.until(
        &mut r,
        "the lock's hint",
        &format!("{HINT}.includes('🔒 locked') && {HINT}"),
    );
    let click = || {
        c.eval("document.getElementById('helpbtn').click()")
            .unwrap();
    };
    help_keeps_the_hint(&mut r, &c, click, &locked);
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// In a window `h` px high, once the shell's rows match `rows` (`>= 18`): the shell's rows, the
/// drawer's height and the workspace's height.
fn drawer_at(h: u32, rows: &str) -> String {
    format!(
        "innerHeight === {h} && terms.get(0).rows {rows} && [terms.get(0).rows, \
         document.getElementById('p0').offsetHeight, document.getElementById('ws').clientHeight]"
    )
}

/// #213: with no saved height, the shell starts in the drawer over the start canvas. The drawer
/// shows 18 rows of the shell, which hold an agent CLI's start screen, within 54% of the workspace.
/// In a laptop-high window (780 px), 54% holds fewer rows. The drawer stays open on `Ctrl+``, and
/// the help shows *Get started*'s steps.
#[test]
fn the_shell_starts_in_the_drawer_over_the_canvas() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    // The shared home can hold another test's inline review, so either canvas can show.
    c.until(
        &mut r,
        "the canvas's text",
        "!document.getElementById('canvas').hidden && document.getElementById('canvas').innerText\
         .includes({start: 'Welcome to Laura', glance: 'Open as a pane', calm: 'Ask your agent'}[layout.canvas])",
    );
    let share = |at: &Value| at[1].as_f64().unwrap_or(0.0) / at[2].as_f64().unwrap_or(1.0);
    c.resize(1280, 1100);
    let tall = c.until(&mut r, "18 rows", &drawer_at(1100, ">= 18"));
    assert!(share(&tall) < 0.54, "{tall}");
    c.resize(1280, 780);
    let laptop = c.until(&mut r, "the 54% cap", &drawer_at(780, "< 18"));
    assert!((share(&laptop) - 0.54).abs() < 0.01, "{laptop}");
    c.press(CTRL_BACKQUOTE);
    for _ in 0..10 {
        r.pump();
    }
    assert_eq!(
        c.eval("layout.focus === 0 && document.getElementById('p0').classList.contains('open')"),
        Ok(json!(true)),
        "Ctrl+` closed the drawer over the canvas"
    );
    c.press(CTRL_H);
    c.until(
        &mut r,
        "the help's steps",
        "!document.getElementById('help').hidden && document.querySelectorAll('#help .step').length === 3",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #214: after a submitted inline review, the canvas shows the project at a glance, and its
/// **Open as a pane** opens the README in a file pane, which takes the focus. The client rig's
/// folder is the crate, which has a README.
#[test]
fn a_click_on_the_glance_opens_the_file() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    // The shared home (`isolate_home`): before the rig, which picks the canvas at its first turn.
    let sessions = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(".laura/sessions");
    std::fs::create_dir_all(&sessions)?;
    let review = json!({"type": "review", "ts": 1, "pane": 1, "path": "x"});
    std::fs::write(sessions.join("client-glance.ndjson"), format!("{review}\n"))?;
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let at = c.until(
        &mut r,
        "the README card's button",
        "layout?.canvas === 'glance' && (() => { \
           const b = document.querySelector('#canvas .glance [data-path=\"README.md\"]')?.getBoundingClientRect(); \
           return b?.width && [b.x + b.width / 2, b.y + b.height / 2]; })()",
    );
    c.click(at[0].as_f64().unwrap_or(0.0), at[1].as_f64().unwrap_or(0.0));
    c.until(
        &mut r,
        "README.md in a focused file pane",
        "layout.panes.some(p => p.id === layout.focus && p.kind === 'file' && p.name === 'README.md')",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #213: the start canvas is not a pane, so a click on its text leaves the keystrokes in the shell.
#[test]
fn a_click_on_the_canvas_keeps_the_shell() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    // The center of the canvas's visible heading, which the drawer doesn't cover.
    let at = c.until(
        &mut r,
        "the canvas's heading",
        "!document.getElementById('canvas').hidden && (() => { \
           const h = [...document.querySelectorAll('#canvas h1')].find(h => h.offsetParent); \
           const b = h?.getBoundingClientRect(); \
           return b?.width && document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2)\
             ?.closest('#canvas') && [b.x + b.width / 2, b.y + b.height / 2]; })()",
    );
    c.click(at[0].as_f64().unwrap_or(0.0), at[1].as_f64().unwrap_or(0.0));
    c.until(
        &mut r,
        "the shell's DOM focus",
        "document.activeElement?.closest('#p0 .xterm')",
    );
    c.type_text("echo canvas-click");
    c.press(ENTER);
    c.until(
        &mut r,
        "the echo in the shell",
        &format!("{SHELL_LINES}.some(l => l.trim() === 'canvas-click')"),
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #213: a drawer step while the web app reconnects keeps its height. Until the new connection's
/// layout, the old layout's start height must not take the drawer back.
#[test]
fn a_drawer_step_holds_while_the_web_app_reconnects() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    c.resize(1280, 1100);
    c.until(&mut r, "the start height", &drawer_at(1100, ">= 18"));
    let drawer = "parseFloat(document.getElementById('ws').style.getPropertyValue('--drawer'))";
    let shown = c
        .eval(&format!("Math.round({drawer})"))
        .map_err(anyhow::Error::msg)?;
    // Laura is out of reach, as in a network outage: the reconnect's socket never opens, so a slow
    // runner can't reach the new connection before the check. `close` puts the socket in CLOSING.
    c.eval("WebSocket = function () { return { readyState: 0 }; }; ws.close()")
        .map_err(anyhow::Error::msg)?;
    c.press(CTRL_SHIFT_UP);
    // A turn of the shell's `ResizeObserver`.
    let after = c
        .eval(&format!(
            "new Promise(r => setTimeout(() => r({drawer}), 100))"
        ))
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        after.as_f64(),
        shown.as_f64().map(|h| h + 5.0),
        "{shown} → {after}"
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// Each line in the shell, and the links that the shell's link provider finds in it: each link's
/// text and first column. From `PATH`'s comment in `term.js`.
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
    ("routes/+page.svelte", &[("routes/+page.svelte", 1)]),
    ("Update(src/a.rs)", &[("src/a.rs", 8)]),
    ("\"src/a.rs\"", &[("src/a.rs", 2)]),
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

/// The center of the line between two panes, in CSS px, once the web app shows it.
fn split_center(r: &mut Rig, c: &Client) -> (f64, f64) {
    let at = c.until(
        r,
        "the line between the panes",
        "(() => { const b = document.querySelector('#ws > .split')?.getBoundingClientRect(); \
           return b?.height && [b.x + b.width / 2, b.y + b.height / 2]; })()",
    );
    (at[0].as_f64().unwrap_or(0.0), at[1].as_f64().unwrap_or(0.0))
}

/// #211: a drag on the line between two panes resizes them, and Laura keeps the size; a
/// double-click on the line gives the size back to Laura.
#[test]
fn a_drag_on_a_split_resizes_the_panes() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let _five = open_five(&mut r)?;
    let dir = tempfile::tempdir()?;
    let b = dir.path().join("b.txt");
    std::fs::write(&b, "b\n")?;
    r.drive(&["open", &b.to_string_lossy(), "--split", "1", "--dir", "h"]);
    let c = Client::open(&mut r, &path);
    // Ratios, not widths: the first layout can come before the page's size.
    c.until(&mut r, "one split at 50%", "layout.splits[0]?.ratio === 50");
    let (x, y) = split_center(&mut r, &c);
    c.drag((x, y), (x + 200.0, y));
    let w = c.until(
        &mut r,
        "a pinned split, wider for pane 1",
        "layout.splits[0].pinned && layout.splits[0].ratio > 50 \
           && layout.panes.find(p => p.id === 1).w",
    );
    let report: Value = serde_json::from_str(&r.drive(&["layout"]))?;
    assert_eq!(report["panes"][1]["pinned"], true, "{report}");
    assert_eq!(report["panes"][1]["rect"]["width"], w, "{report}");
    let (x, y) = split_center(&mut r, &c);
    c.double_click(x, y);
    c.until(
        &mut r,
        "Laura's ratio again",
        "!layout.splits[0].pinned && layout.splits[0].ratio === 50",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #211: a screen reader and the keyboard reach the line between two panes: `→` moves it 5 points,
/// and `Home` gives the size back to Laura.
#[test]
fn the_keyboard_moves_the_line_between_panes() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let _five = open_five(&mut r)?;
    let dir = tempfile::tempdir()?;
    let b = dir.path().join("b.txt");
    std::fs::write(&b, "b\n")?;
    r.drive(&["open", &b.to_string_lossy(), "--split", "1", "--dir", "h"]);
    let c = Client::open(&mut r, &path);
    c.until(
        &mut r,
        "a separator at 50%",
        "(() => { const s = document.querySelector('#ws > .split'); \
           return s?.getAttribute('role') === 'separator' \
             && s.getAttribute('aria-valuenow') === '50' \
             && s.getAttribute('aria-orientation') === 'vertical'; })()",
    );
    c.eval("document.querySelector('#ws > .split').focus()")
        .map_err(anyhow::Error::msg)?;
    c.press(ARROW_RIGHT);
    c.until(
        &mut r,
        "a pinned split at 55%",
        "layout.splits[0].pinned && layout.splits[0].ratio === 55 \
           && document.querySelector('#ws > .split').getAttribute('aria-valuenow') === '55'",
    );
    c.press(HOME);
    c.until(
        &mut r,
        "Laura's ratio again",
        "!layout.splits[0].pinned && layout.splits[0].ratio === 50",
    );
    // Three presses before the next layout: each one steps from the last, not from the layout.
    for _ in 0..3 {
        c.press(ARROW_RIGHT);
    }
    c.until(&mut r, "a split at 65%", "layout.splits[0].ratio === 65");
    assert_eq!(c.errors(), json!([]));
    Ok(())
}
