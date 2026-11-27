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
use laura::tab::Mode;
use serde_json::{Value, json};

use browser_rig::{bare, chrome};
use client_rig::*;
use rig::{Rig, fake_nvim};

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
    assert!(!r.tab.waiting(), "the wait");
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

/// #230: the user moves the focus to pane 2 and back to the shell under the lock, so the unlock
/// leaves the focus on the shell. The agent's moves come in a frame, as a click's do.
#[test]
fn the_unlock_keeps_the_shell_after_a_focus_move() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let dir = file_focused(&mut r, &c)?;
    let other = dir.path().join("b.rs");
    std::fs::write(&other, "fn b() {}\n")?;
    r.drive(&["open", "--no-focus", &other.to_string_lossy()]);
    c.until(
        &mut r,
        "pane 2",
        "layout.focus === 1 && layout.panes.length === 3",
    );
    c.press(F12);
    c.until(
        &mut r,
        "the lock, with the shell's focus",
        "layout.modes.includes('locked') && layout.focus === 0",
    );
    r.drive(&["focus", "2"]);
    c.until(&mut r, "pane 2's focus", "layout.focus === 2");
    r.drive(&["focus", "0"]);
    c.until(&mut r, "the shell's focus", "layout.focus === 0");
    c.press(F12);
    c.until(&mut r, "the unlock", "!layout.modes.includes('locked')");
    assert_eq!(r.tab.focus, 0, "the unlock gave pane 1 its focus back");
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

/// A say turns the drawer to the chat, which takes the keystrokes; `Ctrl+.` flips the drawer to the
/// shell, whose xterm takes them, and back.
#[test]
fn ctrl_period_flips_the_drawer() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    const CHAT: &str = concat!(
        "layout.drawer_view === 'chat' && !acts.length",
        " && document.querySelector('#p0 .chat').checkVisibility()",
        " && !document.querySelector('#p0 .term').checkVisibility({visibilityProperty: true})",
        " && document.activeElement === document.querySelector('#p0 .msgbox textarea')"
    );
    r.drive(&["say", "hello from the agent"]);
    c.until(&mut r, "the chat", CHAT);
    c.press(CTRL_PERIOD);
    c.until(
        &mut r,
        "the shell",
        concat!(
            "layout.drawer_view === 'shell' && !acts.length",
            " && !document.querySelector('#p0 .chat').checkVisibility()",
            " && document.querySelector('#p0 .term').checkVisibility({visibilityProperty: true})",
            " && !!document.activeElement.closest('#p0 .term')"
        ),
    );
    assert!(r.tab.drawer_shows_shell(), "Laura's drawer view");
    c.press(CTRL_PERIOD);
    c.until(&mut r, "the chat again", CHAT);
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// A say that comes while the drawer shows the shell is in view once the chat shows: Laura shows
/// the chat at the say, and the client scrolls the chat to its last row.
#[test]
fn a_say_while_the_shell_shows_is_in_view() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let tall = (1..=40)
        .map(|i| format!("p{i}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    r.drive(&["say", &tall]);
    c.until(
        &mut r,
        "the chat",
        "layout.drawer_view === 'chat' && !acts.length",
    );
    c.press(CTRL_PERIOD);
    c.until(
        &mut r,
        "the shell",
        "layout.drawer_view === 'shell' && !acts.length",
    );
    r.drive(&["say", "the-last-say"]);
    c.until(
        &mut r,
        "the last say in view",
        concat!(
            "layout.drawer_view === 'chat' && !acts.length",
            " && (l => l.textContent.includes('the-last-say') && l.scrollHeight > l.clientHeight",
            " && l.scrollTop + l.clientHeight >= l.scrollHeight - 1)(document.querySelector('#p0 .chat .list'))"
        ),
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// `F12` over the chat shows the shell, which takes the keystrokes, until the second `F12` brings
/// the chat back.
#[test]
fn f12_shows_the_shell_until_the_unlock() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    r.drive(&["say", "hello from the agent"]);
    c.until(
        &mut r,
        "the chat",
        "layout.drawer_view === 'chat' && !acts.length",
    );
    c.press(F12);
    c.until(
        &mut r,
        "the shell, with the keystrokes",
        "layout.drawer_view === 'shell' && !acts.length && !!document.activeElement.closest('#p0 .term')",
    );
    c.type_text("echo locked-key");
    c.press(ENTER);
    c.until(
        &mut r,
        "the echo in the shell",
        &format!("{SHELL_LINES}.some(l => l.trim() === 'locked-key')"),
    );
    c.press(F12);
    c.until(
        &mut r,
        "the chat again",
        "layout.drawer_view === 'chat' && !acts.length",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// A view that opens after the agent's say shows the say in the chat and in the shell button.
#[test]
fn a_view_that_opens_after_a_say_shows_it() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    r.drive(&["say", "said before the view"]);
    let c = Client::open(&mut r, &path);
    c.until(
        &mut r,
        "the say in the chat",
        "document.querySelector('#p0 .chat .te.say')?.textContent.includes('said before the view')",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// A click on a markdown link in a say opens its file, from the folder Laura started in (the
/// crate, the rig's folder).
#[test]
fn a_link_in_a_say_opens_its_file() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    r.drive(&["say", "Read [the CLI](docs/cli.md#commands)"]);
    let at = c.until(
        &mut r,
        "the say's link",
        "(() => { \
           const b = document.querySelector('#p0 .chat a[data-href]')?.getBoundingClientRect(); \
           return b?.width && [b.x + b.width / 2, b.y + b.height / 2]; })()",
    );
    c.click(at[0].as_f64().unwrap_or(0.0), at[1].as_f64().unwrap_or(0.0));
    c.until(
        &mut r,
        "cli.md in a focused file pane",
        "layout.panes.some(p => p.id === layout.focus && p.kind === 'file' && p.name === 'cli.md')",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// The chat's message box takes the user's keystrokes, and `Enter` writes the message into the
/// shell; the box empties once Laura wrote it. A second `Enter` before Laura's reply sends nothing.
#[test]
fn enter_in_the_message_box_sends_the_message() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    r.drive(&["say", "Ready when you are"]);
    c.until(
        &mut r,
        "the message box's focus",
        "layout.drawer_view === 'chat' && !acts.length && document.activeElement === document.querySelector('#p0 .msgbox textarea')",
    );
    c.type_text("hello-box");
    c.press(ENTER);
    c.press(ENTER);
    assert_eq!(
        c.eval("acts.length"),
        Ok(json!(1)),
        "the second Enter sent again"
    );
    c.until(
        &mut r,
        "the message in the shell, and an empty box",
        &format!(
            "{SHELL_LINES}.some(l => l.includes('hello-box')) && !acts.length && document.querySelector('#p0 .msgbox textarea').value === ''"
        ),
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// `Enter` under the help closes the help, and the message box keeps its text.
#[test]
fn enter_under_the_help_closes_the_help() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    r.drive(&["say", "Ready when you are"]);
    c.until(
        &mut r,
        "the message box's focus",
        "layout.drawer_view === 'chat' && !acts.length && document.activeElement === document.querySelector('#p0 .msgbox textarea')",
    );
    c.type_text("hi-help");
    c.press(CTRL_H);
    c.until(
        &mut r,
        "the help",
        "layout.modes.includes('help') && !acts.length",
    );
    c.press(ENTER);
    c.until(
        &mut r,
        "no help",
        "!layout.modes.includes('help') && !acts.length",
    );
    assert_eq!(
        c.eval("document.querySelector('#p0 .msgbox textarea').value"),
        Ok(json!("hi-help"))
    );
    let lines = c.eval(SHELL_LINES).unwrap_or_default();
    assert!(!lines.to_string().contains("hi-help"), "{lines}");
    assert_eq!(c.errors(), json!([]));
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

/// #245: Laura ends the draft when the socket drops. The web app begins the draft again on the
/// new socket and keeps the text in the comment's box.
#[test]
fn a_reconnect_begins_the_draft_again() -> Result<()> {
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
    c.type_text("Kept text");
    c.eval("globalThis.old = ws; ws.close()")
        .map_err(anyhow::Error::msg)?;
    c.until(
        &mut r,
        "the draft on the new socket",
        "ws !== globalThis.old && ws.readyState === 1 && layout.modes.includes('draft') \
         && document.querySelector('.draft textarea')?.value === 'Kept text'",
    );
    // The client shows `draft` in its copy of the modes before Laura begins the draft.
    let t0 = std::time::Instant::now();
    while !r.tab.held() {
        assert!(t0.elapsed().as_secs() < 10, "Laura begins the draft again");
        r.pump();
    }
    c.press(ENTER);
    c.until(
        &mut r,
        "the saved comment",
        "!document.querySelector('.draft textarea')",
    );
    let threads = &r.tab.panels[&1].threads;
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].root.body, "Kept text");
    Ok(())
}

/// #245: after a reconnect, a draft that Laura can't begin again closes, and Laura's notice says why.
#[test]
fn a_failed_reconnect_closes_the_draft() -> Result<()> {
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
    c.type_text("Lost text");
    c.eval("globalThis.old = ws; ws.close()")
        .map_err(anyhow::Error::msg)?;
    // `begin_comment` on the new socket fails with `NEEDS_READY`.
    r.tab.agent = false;
    c.until(
        &mut r,
        "the box closes on the new socket",
        "ws !== globalThis.old && ws.readyState === 1 && !layout.modes.includes('draft') \
         && !document.querySelector('.draft textarea')",
    );
    assert!(!r.tab.held());
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
    // #216: the comment's box shows no note about the agent's commands.
    assert_eq!(
        c.eval("!!document.querySelector('.draft .holdnote')"),
        Ok(json!(false))
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
    // #216: the review body's box is its text box and buttons, with no header or paragraph.
    assert_eq!(
        c.eval("[...document.querySelectorAll('.pop.review .holdnote, .pop.review .ph, .pop.review p')].length"),
        Ok(json!(0))
    );
    assert_eq!(
        c.eval("document.querySelector('.pop.review textarea').placeholder"),
        Ok(json!("Review body (optional)"))
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

/// #245: `Ctrl+H` under the pane selector shows the help in its place, as in the TUI.
#[test]
fn ctrl_h_under_the_pane_selector_shows_the_help() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    c.press(CTRL_P);
    c.until(
        &mut r,
        "the pane selector",
        "!document.getElementById('popup').hidden",
    );
    c.press(CTRL_H);
    c.until(
        &mut r,
        "the help in place of the pane selector",
        "!document.getElementById('help').hidden && document.getElementById('popup').hidden",
    );
    let t0 = std::time::Instant::now();
    while r.tab.modes() != [Mode::Help] {
        assert!(t0.elapsed().as_secs() < 10, "Laura holds the help");
        r.pump();
    }
    // The help hid the selector's input; the shell takes the keystrokes again once the help closes.
    c.press(ESCAPE);
    c.until(
        &mut r,
        "the shell with the focus",
        "document.getElementById('help').hidden && document.activeElement?.closest('#p0 .xterm')",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #245: a `layout` frame that Laura sent before it read `Ctrl+P` hides the pane selector for a
/// frame (#272); the input has the focus again when the selector shows.
#[test]
fn a_stale_layout_keeps_the_pane_selectors_focus() -> Result<()> {
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
    // Laura's frames, replayed: one sent before the act, then one sent after. Chrome moves the
    // focus off the hidden input at its next render, at no fixed time, so the blur stands in.
    let js = "(() => {
        const frame = modes => ws.onmessage({ data: JSON.stringify({ ...layout, t: 'layout', modes }) });
        frame([]);
        document.activeElement.blur();
        frame(['selector']);
        return [!document.getElementById('popup').hidden, document.activeElement?.id];
    })()";
    assert_eq!(c.eval(js), Ok(json!([true, "pq"])));
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #272: the user closes the pane selector before Laura's reply to `Ctrl+P`. Laura's frame from
/// before it read the close doesn't open the selector again, so `c` goes to the file pane.
#[test]
fn a_stale_layout_leaves_the_closed_pane_selector_closed() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    r.tab.agent = true;
    let c = Client::open(&mut r, &path);
    let _dir = file_focused(&mut r, &c)?;
    // No `until` between the keystrokes: Laura reads them only at the next pump.
    c.press(CTRL_P);
    c.type_text("1");
    let js = "(() => {
        ws.onmessage({ data: JSON.stringify({ ...layout, t: 'layout', modes: ['selector'] }) });
        return document.getElementById('popup').hidden;
    })()";
    assert_eq!(c.eval(js), Ok(json!(true)));
    c.press(C);
    c.until(
        &mut r,
        "the comment's box",
        "document.activeElement?.matches('.draft textarea')",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #272: the user cancels the comment before Laura's reply to `Esc`. Laura's frame from before it
/// read the cancel doesn't open the comment's box again.
#[test]
fn a_stale_layout_leaves_the_cancelled_draft_closed() -> Result<()> {
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
    assert!(c.eval("!!(window.staleDraft = layout.draft)") == Ok(json!(true)));
    c.press(ESCAPE);
    let js = "(() => {
        ws.onmessage({ data: JSON.stringify({ ...layout, t: 'layout', modes: ['draft'], draft: staleDraft }) });
        return !!document.querySelector('.draft textarea');
    })()";
    assert_eq!(c.eval(js), Ok(json!(false)));
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #245: the pane selector moves the focus before it closes, so the user, who never saw the
/// shell, keeps the wait.
#[test]
fn ctrl_p_to_a_file_pane_keeps_the_wait() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = open_five(&mut r)?;
    c.until(
        &mut r,
        "the file's rows",
        "document.querySelector('#p1 .rows > [data-row]')",
    );
    c.focus(&mut r, 0);
    ring(&mut r, &c)?;
    c.press(CTRL_P);
    c.until(
        &mut r,
        "the pane selector",
        "!document.getElementById('popup').hidden",
    );
    c.type_text("1");
    c.until(
        &mut r,
        "the file pane's focus",
        "document.getElementById('popup').hidden && layout.focus === 1",
    );
    if cfg!(unix) {
        assert!(r.tab.waiting(), "the wait");
    }
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// A click in the chat during a wait with no say keeps the wait: only the shell's own view ends it.
#[test]
fn a_click_in_the_chat_keeps_the_wait() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    if !cfg!(unix) {
        return Ok(()); // the shell rings the bell only on unix, as in `ring`
    }
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    r.drive(&["say", "hello from the agent"]);
    c.until(
        &mut r,
        "the chat",
        "layout.drawer_view === 'chat' && !acts.length",
    );
    c.press(CTRL_PERIOD);
    c.until(
        &mut r,
        "the shell",
        "layout.drawer_view === 'shell' && !acts.length && !!document.activeElement.closest('#p0 .term')",
    );
    // The user's write ends the say, so the bell begins a wait with no say.
    c.type_text("printf '\\007'");
    c.press(ENTER);
    c.until(
        &mut r,
        "the wait with no say",
        "layout.waiting && layout.unsaid && layout.drawer_view === 'shell'",
    );
    c.press(CTRL_PERIOD);
    let at = c.until(
        &mut r,
        "the chat",
        "layout.drawer_view === 'chat' && !acts.length && (() => { \
           const b = document.querySelector('#p0 .chat').getBoundingClientRect(); \
           return b.width && [b.x + b.width / 2, b.y + b.height / 2]; })()",
    );
    c.click(at[0].as_f64().unwrap_or(0.0), at[1].as_f64().unwrap_or(0.0));
    // A round trip after the click's `seen`, if any.
    c.press(CTRL_PERIOD);
    c.until(
        &mut r,
        "the shell again",
        "layout.drawer_view === 'shell' && !acts.length",
    );
    assert!(r.tab.waiting(), "the wait");
    assert_eq!(c.eval("layout.message_box"), Ok(json!(false)));
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #245: F12 under the workspace switcher's menu closes the menu, then locks. The user sees the
/// shell, and the wait ends.
#[test]
fn f12_closes_the_workspace_menu_and_locks() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = Client::open(&mut r, &path);
    let _dir = file_focused(&mut r, &c)?;
    ring(&mut r, &c)?;
    // The rig hosts one workspace and no Laura server, so the test enables the button as a second
    // workspace would.
    c.eval("const w = document.getElementById('wsw'); w.disabled = false; w.click()")
        .unwrap();
    c.until(
        &mut r,
        "the workspace switcher's menu",
        "!document.getElementById('menu').hidden",
    );
    c.press(F12);
    let waits = if cfg!(unix) {
        " && !layout.waiting"
    } else {
        ""
    };
    c.until(
        &mut r,
        "the menu closes, with the shell's focus",
        &format!("document.getElementById('menu').hidden && layout.focus === 0{waits}"),
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

/// The glance at 1280×870, with a session in the crate, the rig's folder, for the sessions card. The
/// home is the shared one, as in `a_click_on_the_glance_opens_the_file`.
fn open_glance(r: &mut Rig, path: &std::path::Path) -> Result<Client> {
    let sessions = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(".laura/sessions");
    std::fs::create_dir_all(&sessions)?;
    let root = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR"))?;
    // As Laura names its folder: without Windows' `\\?\`.
    let root = root.to_string_lossy();
    let root = std::path::Path::new(root.strip_prefix(r"\\?\").unwrap_or(&root));
    let ready = json!({"type": "ready", "ts": 1, "root": root, "title": "glance fit"});
    let review = json!({"type": "review", "ts": 2, "pane": 1, "path": root.join("README.md")});
    std::fs::write(
        sessions.join("client-glance-fit.ndjson"),
        format!("{ready}\n{review}\n"),
    )?;
    let c = Client::open(r, path);
    c.resize(1280, 870);
    c.until(
        r,
        "the glance's sessions card",
        "layout?.canvas === 'glance' && document.querySelector('#canvas .glance .sess')",
    );
    for _ in 0..10 {
        r.pump();
    }
    Ok(c)
}

/// #258: the glance shows README, Latest (or Changes) and Recently in this folder side by side, as
/// tall as the room above the drawer at its least height. The drawer covers the cards' lower part;
/// the user drags the drawer to its least height and sees each card in full, while the canvas holds
/// still. The crate's README is long, so the README card scrolls inside itself.
#[test]
fn the_glance_shows_in_full_over_the_least_drawer() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = open_glance(&mut r, &path)?;
    let measure = "(() => { const c = document.getElementById('canvas'), \
           cards = [...c.querySelectorAll('.gcard')], box = e => e.getBoundingClientRect(); \
         return { rects: cards.map(g => [box(g).left, box(g).top, box(g).width, box(g).height].map(Math.round)), \
           bottom: Math.max(...cards.map(g => box(g).bottom)), drawer: box(document.getElementById('p0')).top, \
           scrolls: c.scrollHeight > c.clientHeight + 1, \
           spill: cards.filter(g => [...g.children].some(k => box(k).bottom > box(g).bottom + 1)).length, \
           short: [...c.querySelectorAll('.md, .files, .sess')].filter(l => l.clientHeight < 20).map(l => l.className) }; })()";
    let before = c.eval(measure).map_err(anyhow::Error::msg)?;
    let rects = before["rects"].as_array().cloned().unwrap_or_default();
    let at = |i: usize, k: usize| rects[i][k].as_f64().unwrap_or(f64::NAN);
    assert_eq!(rects.len(), 3, "three cards: {before}");
    assert!(
        (0..3).all(|i| (at(i, 1) - at(0, 1)).abs() <= 1.0)
            && at(0, 0) < at(1, 0)
            && at(1, 0) < at(2, 0),
        "three cards side by side: {before}"
    );
    assert!(
        at(0, 2) > at(1, 2) && at(0, 2) > at(2, 2),
        "the README card is the widest: {before}"
    );
    assert_eq!(before["scrolls"], json!(false), "{before}");
    assert_eq!(before["spill"], json!(0), "{before}");
    assert_eq!(before["short"], json!([]), "{before}");
    let num = |v: &Value, k: &str| v[k].as_f64().unwrap_or(f64::NAN);
    assert!(
        num(&before, "drawer") < num(&before, "bottom"),
        "the drawer covers the cards: {before}"
    );
    // As in `a_drawer_step_holds_while_the_web_app_reconnects`: no socket, so the steps change the
    // drawer and save no height to the shared home.
    c.eval("WebSocket = function () { return { readyState: 0 }; }; ws.close()")
        .map_err(anyhow::Error::msg)?;
    for _ in 0..10 {
        c.press(CTRL_SHIFT_DOWN);
    }
    // A turn of the shell's `ResizeObserver`.
    let after = c
        .eval(&format!(
            "new Promise(r => setTimeout(() => r({measure}), 100))"
        ))
        .map_err(anyhow::Error::msg)?;
    assert_eq!(after["rects"], before["rects"], "the canvas holds still");
    assert!(
        num(&after, "bottom") <= num(&after, "drawer") + 1.0,
        "the whole glance shows: {after}"
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #251: the canvas that comes back shows three skeleton cards, marked busy for a screen reader,
/// also after a glance with one card, until Laura's glance replaces them.
#[test]
fn the_canvas_that_comes_back_shows_the_skeleton() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = open_glance(&mut r, &path)?;
    // Laura's frames, replayed: a glance with one card, the canvas hidden, then the glance again.
    let js = "(() => {
        const g = document.querySelector('#canvas .glance'), frame = m => ws.onmessage({ data: JSON.stringify(m) });
        const state = () => [g.querySelectorAll('.gcard.skel').length, g.classList.contains('one'), g.getAttribute('aria-busy')];
        const glance = { t: 'glance', readme: { name: 'README.md', html: '<p>Hi</p>' }, project: null, sessions: [] };
        frame(glance);
        const one = state();
        frame({ ...layout, t: 'layout', canvas: null });
        const hidden = state();
        frame({ ...layout, t: 'layout', canvas: 'glance' });
        frame(glance);
        return [one, hidden, state()];
    })()";
    assert_eq!(
        c.eval(js),
        Ok(json!([
            [0, true, null],
            [3, false, "true"],
            [0, true, null]
        ]))
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #258: beside the docked shell, the glance stacks its cards in line with the shell's card. The
/// README card takes half of the height, and the two lists share the rest.
#[test]
fn the_glance_stacks_beside_the_docked_shell() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let c = open_glance(&mut r, &path)?;
    c.eval("document.querySelector('#p0 [data-act=dock]').click()")
        .map_err(anyhow::Error::msg)?;
    c.until(
        &mut r,
        "the canvas beside the docked shell",
        "document.getElementById('canvas').classList.contains('docked')",
    );
    for _ in 0..10 {
        r.pump();
    }
    let fit = c.eval(
        "(() => { const c = document.getElementById('canvas'), box = e => e.getBoundingClientRect(), \
             cards = [...c.querySelectorAll('.gcard')].map(box), p0 = document.getElementById('p0'), \
             shell = box(p0), inset = parseFloat(getComputedStyle(p0, '::before').top); \
           return { stacked: cards.every((g, i) => !i || (Math.abs(g.left - cards[0].left) <= 1 && g.top >= cards[i - 1].bottom)), \
             top: cards[0].top - shell.top - inset, bottom: cards[2].bottom - shell.bottom + inset, \
             readme: cards[0].height / (cards[2].bottom - cards[0].top), lists: cards[1].height - cards[2].height, \
             scrolls: c.scrollHeight > c.clientHeight + 1, cards: cards.length }; })()",
    );
    let fit = fit.map_err(anyhow::Error::msg)?;
    let num = |k: &str| fit[k].as_f64().unwrap_or(f64::NAN);
    assert_eq!(fit["cards"], json!(3), "{fit}");
    assert_eq!(fit["stacked"], json!(true), "{fit}");
    assert!(
        num("top").abs() <= 1.0 && num("bottom").abs() <= 1.0,
        "in line with the shell's card: {fit}"
    );
    assert!(num("readme") >= 0.49, "the README card takes half: {fit}");
    assert!(num("lists").abs() <= 1.0, "the lists share the rest: {fit}");
    assert_eq!(fit["scrolls"], json!(false), "{fit}");
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

/// The center of the line between two panes, in CSS px, once the web app shows it at the page's
/// size: a layout from before the page's size (`--cols`) puts the line elsewhere, and the next
/// layout moves it from under the press.
fn split_center(r: &mut Rig, c: &Client) -> (f64, f64) {
    let at = c.until(
        r,
        "the line between the panes, at the page's size",
        "(() => { const s = layout.splits[0], b = document.querySelector('#ws > .split')?.getBoundingClientRect(); \
           return s?.x + s?.w === +document.documentElement.style.getPropertyValue('--cols') \
             && b?.height && [b.x + b.width / 2, b.y + b.height / 2]; })()",
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

/// #211: a pane that closes while the user drags a line ends the drag, so the shell fits again.
#[test]
fn a_pane_that_closes_during_a_drag_ends_the_drag() -> Result<()> {
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
    c.until(&mut r, "one split at 50%", "layout.splits[0]?.ratio === 50");
    let (x, y) = split_center(&mut r, &c);
    c.drag_hold((x, y), (x + 100.0, y));
    c.until(&mut r, "a drag", "ui.resizing");
    r.drive(&["close", "2"]);
    c.until(
        &mut r,
        "the drag's end",
        "!ui.resizing && !document.querySelector('#ws > .split')",
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// A stand-in Neovim in pane 2, beside pane 1, with one split at 50%: the client, the editor's
/// xterm's columns at its pane's size, and the folders of the two files.
fn editor_beside(
    r: &mut Rig,
    chrome: &std::path::Path,
) -> Result<(Client, Value, [tempfile::TempDir; 2])> {
    r.tab.editor = Ok(fake_nvim());
    let five = open_five(r)?;
    let dir = tempfile::tempdir()?;
    let b = dir.path().join("b.txt");
    std::fs::write(&b, "b\n")?;
    r.drive(&[
        "open",
        "--edit",
        &b.to_string_lossy(),
        "--split",
        "1",
        "--dir",
        "h",
    ]);
    let c = Client::open(r, chrome);
    // A layout from before the page's size (`--cols`) would resize the xterm after this returns.
    let cols = c.until(
        r,
        "one split at 50% at the page's size, with the editor's xterm at its pane's size",
        "(p => layout.splits[0]?.ratio === 50 \
           && p.x + p.w === +document.documentElement.style.getPropertyValue('--cols') \
           && terms.get(2)?.cols === p.w - 2 && terms.get(2).cols)(layout.panes.find(p => p.id === 2))",
    );
    Ok((c, cols, [five, dir]))
}

/// #211: while the user drags a line, an editor pane's xterm keeps Neovim's size, which Laura holds
/// until the drag ends; after the release, the xterm takes the pane's new size.
#[test]
fn an_editor_pane_keeps_its_size_during_a_drag() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let (c, cols, _dirs) = editor_beside(&mut r, &path)?;
    let (x, y) = split_center(&mut r, &c);
    c.drag_hold((x, y), (x + 100.0, y));
    c.until(
        &mut r,
        "a drag's layout",
        "ui.resizing && layout.splits[0].ratio > 50",
    );
    assert_eq!(
        c.eval("terms.get(2).cols"),
        Ok(cols.clone()),
        "the editor's xterm during the drag"
    );
    c.release((x + 100.0, y));
    let after = c.until(
        &mut r,
        "the editor's xterm at the pane's new size",
        "!ui.resizing && terms.get(2).cols === layout.panes.find(p => p.id === 2).w - 2 \
           && terms.get(2).cols",
    );
    assert_ne!(after, cols, "the editor's xterm after the drag");
    assert_eq!(c.errors(), json!([]));
    Ok(())
}

/// #211: after a keyboard step on a line, an editor pane's xterm keeps Neovim's size until Laura
/// fits the terminals (`RESIZE_SETTLE`, 200 ms), as after a drag. The times make the test
/// independent of the runner's speed.
#[test]
fn an_editor_pane_keeps_its_size_after_a_keyboard_step() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let mut r = bare()?;
    let (c, _cols, _dirs) = editor_beside(&mut r, &path)?;
    c.eval(
        "window.__resized = []; \
         terms.get(2).onResize(() => __resized.push(performance.now())); \
         document.querySelector('#ws > .split').focus(); \
         window.__key = performance.now()",
    )
    .map_err(anyhow::Error::msg)?;
    c.press(ARROW_RIGHT);
    c.until(
        &mut r,
        "the editor's xterm at the pane's new size",
        "layout.splits[0].ratio === 55 && __resized.length \
           && terms.get(2).cols === layout.panes.find(p => p.id === 2).w - 2",
    );
    let waited = c.eval("__resized[0] - __key").map_err(anyhow::Error::msg)?;
    assert!(
        waited.as_f64().is_some_and(|ms| ms >= 195.0),
        "the editor's xterm resized {waited} ms after the keystroke"
    );
    assert_eq!(c.errors(), json!([]));
    Ok(())
}
