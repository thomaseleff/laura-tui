//! Browser panes (#107): the threads and the highlight on a page's elements, from the user's pick
//! and from the agent's selectors, across reloads, routes and new views. Each test that needs Chrome or Edge skips when neither is
//! installed; under `CI`, it fails. State and frames, not pixels.

#[path = "common/browser.rs"]
#[allow(dead_code)] // each browser test crate uses part of the browser rig
mod browser_rig;
mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use std::time::{Duration, Instant};

use anyhow::Result;
use laura::browser;
use serde_json::json;

use browser_rig::*;
use rig::*;

#[test]
fn a_pick_names_the_element() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/pick");
    let pick = |r: &mut Rig, ws: &mut Ws, x: f64, y: f64| {
        r.act(
            ws,
            json!({"act": "browser_pick", "pane": id, "x": x, "y": y}),
        )
    };
    let acted = pick(&mut r, &mut ws, 50.0, 25.0);
    assert_eq!(acted["text"], laura::tab::NEEDS_READY, "{acted}");
    r.tab.agent = true;
    // A unique id names the element; the quote collapses its spaces.
    let acted = pick(&mut r, &mut ws, 50.0, 25.0);
    assert_eq!(acted["anchor"], json!(["#b", "Save draft"]), "{acted}");
    assert_eq!(acted["line"], 1, "{acted}");
    let acted = pick(&mut r, &mut ws, 50.0, 125.0);
    // The first of two paragraphs counts among its siblings.
    assert_eq!(
        acted["anchor"],
        json!(["p:nth-of-type(1)", "Some text"]),
        "{acted}"
    );
    let acted = pick(&mut r, &mut ws, 60.0, 230.0);
    assert_eq!(
        acted["anchor"],
        json!(["ul > li:nth-of-type(2)", "Two"]),
        "{acted}"
    );
    assert_eq!(acted["line"], 3, "{acted}");
    // A quote keeps the first 60 characters.
    let acted = pick(&mut r, &mut ws, 50.0, 325.0);
    let digits = "0123456789".repeat(6);
    assert_eq!(
        acted["anchor"],
        json!(["p:nth-of-type(2)", digits]),
        "{acted}"
    );
    // The same element again is the same row.
    let acted = pick(&mut r, &mut ws, 30.0, 10.0);
    assert_eq!(acted["line"], 1, "{acted}");
    // Another element at a thread's path is a new row.
    click(&mut ws, id, 50.0, 425.0);
    state_until(&mut r, &mut ws, "the added item", id, |v| {
        v["title"] == "Added"
    });
    let acted = pick(&mut r, &mut ws, 60.0, 230.0);
    assert_eq!(
        acted["anchor"],
        json!(["ul > li:nth-of-type(2)", "One"]),
        "{acted}"
    );
    assert_eq!(acted["line"], 5, "{acted}");
    let acted = pick(&mut r, &mut ws, 600.0, 500.0);
    assert_eq!(acted["text"], browser::NO_ELEMENT, "{acted}");
    Ok(())
}

#[test]
fn a_browser_review_quotes_selectors() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let (id, _) = load(&mut r, &mut ws, "/pick");
    comment_at(&mut r, &mut ws, id, (50.0, 25.0), "Rename");
    comment_at(&mut r, &mut ws, id, (50.0, 125.0), "Shorter");
    let shown = state_until(&mut r, &mut ws, "the threads", id, |v| {
        v["threads"].as_array().is_some_and(|t| t.len() == 2)
    });
    assert_eq!(shown["threads"][0]["anchor"], "#b", "{shown}");
    assert_eq!(shown["threads"][0]["label"], "Save draft", "{shown}");
    // After the page changes the element's text, a pick on the element opens the same thread.
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "the new text", id, |v| {
        v["title"] == "Saved"
    });
    let acted = r.act(
        &mut ws,
        json!({"act": "browser_pick", "pane": id, "x": 50.0, "y": 25.0}),
    );
    assert_eq!(acted["anchor"], json!(["#b", "Save draft"]), "{acted}");
    assert_eq!(acted["line"], 1, "{acted}");
    r.act(&mut ws, json!({"act": "begin_review", "pane": id}));
    let acted = r.act(
        &mut ws,
        json!({"act": "submit", "pane": id, "body": "Tidy up"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    // The header follows the prompt, where it can wrap; the lines after it don't.
    r.shell_shows("Tidy up");
    r.shell_shows(r##"#b  "Save draft""##);
    r.shell_shows("> [user] Rename");
    r.shell_shows(r#"p:nth-of-type(1)  "Some text""#);
    r.shell_shows("> [user] Shorter");
    Ok(())
}

#[test]
fn a_thread_survives_a_reload() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let (id, url) = keep(&mut r, &mut ws);
    comment_at(&mut r, &mut ws, id, (50.0, 25.0), "Keep me");
    outlined_until(&mut r, &mut ws, "the outline", id, "1:", (None, &[0]));
    // The same URL loads again, and Laura outlines the button again.
    r.act(
        &mut ws,
        json!({"act": "browser_navigate", "pane": id, "url": url}),
    );
    outlined_until(
        &mut r,
        &mut ws,
        "the outline after the reload",
        id,
        "2:",
        (None, &[0]),
    );
    // The page drops the button: the thread loses its element.
    click(&mut ws, id, 50.0, 25.0);
    // The notice and the lost thread's state can come in either order.
    let want = format!(
        "⚠ pane #{id}: The thread \"Keep me\" is only in the thread list, because the page lost its element."
    );
    let (start, mut noticed, mut lost) = (Instant::now(), false, false);
    while !(noticed && lost) {
        assert!(
            start.elapsed() < common::DEADLINE,
            "timed out: the lost thread"
        );
        for f in r.idle(&mut ws, 1) {
            let Frame::Json(v) = &f else { continue };
            noticed |= v["t"] == "notice" && v["text"] == want.as_str();
            if v["t"] == "browser" && v["id"] == id {
                lost = v["threads"][0]["lost"] == true && v["title"] == "2:";
            }
        }
    }
    outlined_until(&mut r, &mut ws, "no outline", id, "2:", (None, &[]));
    // The page puts it back: the outline returns, with no notice.
    click(&mut ws, id, 50.0, 125.0);
    let (start, mut back, mut shown) = (Instant::now(), None, false);
    while back.is_none_or(|t: Instant| t.elapsed() < Duration::from_secs(1)) {
        assert!(start.elapsed() < common::DEADLINE, "timed out: the outline");
        for f in r.idle(&mut ws, 1) {
            let Frame::Json(v) = &f else { continue };
            assert_ne!(v["t"], "notice", "{v}");
            if v["t"] == "browser" && v["id"] == id {
                shown = v["threads"][0]["lost"] == false;
            }
        }
        if shown && outlines(&r, id) == (None, vec![0]) {
            back.get_or_insert_with(Instant::now);
        }
    }
    Ok(())
}

/// A thread whose element the page lost shows only in the thread list: `n` and `N` skip it, and stop
/// on it again when the page shows the element again (#124).
#[test]
fn n_skips_a_lost_thread() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let (id, _) = keep(&mut r, &mut ws);
    comment_at(&mut r, &mut ws, id, (50.0, 125.0), "First");
    comment_at(&mut r, &mut ws, id, (50.0, 25.0), "Middle");
    comment_at(&mut r, &mut ws, id, (50.0, 225.0), "Last");
    let lost = |r: &mut Rig, ws: &mut Ws, what: &str, want: [bool; 3]| {
        state_until(r, ws, what, id, |v| {
            (0..3).all(|i| v["threads"][i]["lost"] == want[i])
        });
    };
    let jump = |r: &mut Rig, ws: &mut Ws, from: u32, next: bool| {
        r.act(ws, json!({"act": "set_cursor", "pane": id, "line": from}));
        r.act(ws, json!({"act": "jump_thread", "pane": id, "next": next}));
        r.tab.panels[&id].cursor_line()
    };
    lost(&mut r, &mut ws, "three threads", [false; 3]);
    assert_eq!(
        jump(&mut r, &mut ws, 1, true),
        2,
        "n stops on the middle thread"
    );
    // The page drops the middle thread's element.
    click(&mut ws, id, 50.0, 25.0);
    lost(
        &mut r,
        &mut ws,
        "the middle thread lost",
        [false, true, false],
    );
    assert_eq!(jump(&mut r, &mut ws, 1, true), 3, "n skips the lost thread");
    assert_eq!(
        jump(&mut r, &mut ws, 3, false),
        1,
        "N skips the lost thread"
    );
    // The page puts it back.
    click(&mut ws, id, 50.0, 125.0);
    lost(&mut r, &mut ws, "the middle thread back", [false; 3]);
    assert_eq!(
        jump(&mut r, &mut ws, 1, true),
        2,
        "n stops on the thread again"
    );
    // The page drops two elements at once: one notice counts the threads.
    click(&mut ws, id, 50.0, 325.0);
    let want = format!(
        "⚠ pane #{id}: 2 threads are only in the thread list, because the page lost their elements."
    );
    r.until(
        &mut ws,
        "the notice for two threads",
        |f| matches!(f, Frame::Json(v) if v["t"] == "notice" && v["text"] == want.as_str()),
    );
    Ok(())
}

#[test]
fn a_thread_on_another_page_is_not_lost() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let (id, url) = load(&mut r, &mut ws, "/one");
    let acted = comment_at(&mut r, &mut ws, id, (50.0, 25.0), "Link");
    assert_eq!(acted["anchor"][0], "a", "{acted}");
    click(&mut ws, id, 50.0, 25.0);
    let two = format!("{url}/two");
    let shown = state_until(&mut r, &mut ws, "the second page", id, |v| {
        v["url"] == two.as_str() && v["title"] == "Two"
    });
    assert_eq!(shown["threads"][0]["away"], "/one", "{shown}");
    // Past the check's 500 ms quiet time.
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1500) {
        for f in r.idle(&mut ws, 1) {
            assert!(!matches!(&f, Frame::Json(v) if v["t"] == "notice"), "{f:?}");
        }
    }
    // Back on its page, the thread names no other page, and Laura outlines its link again.
    r.act(
        &mut ws,
        json!({"act": "browser_history", "pane": id, "delta": -1}),
    );
    let one = format!("{url}/one");
    state_until(&mut r, &mut ws, "the first page again", id, |v| {
        v["url"] == one.as_str() && v["threads"][0]["away"].is_null()
    });
    outlined_until(
        &mut r,
        &mut ws,
        "the link's outline",
        id,
        "One",
        (None, &[0]),
    );
    // A review from another page names the thread's page.
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "the second page again", id, |v| {
        v["url"] == two.as_str()
    });
    r.act(&mut ws, json!({"act": "begin_review", "pane": id}));
    r.act(&mut ws, json!({"act": "submit", "pane": id, "body": ""}));
    r.shell_shows(&format!(r#"{one}  a  "Two""#));
    Ok(())
}

#[test]
fn a_hash_route_is_a_page() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let id = open(&mut r, &format!("{}/hash#/a", serve()));
    state_until(&mut r, &mut ws, "the first route", id, |v| {
        v["title"] == "#/a"
    });
    let acted = comment_at(&mut r, &mut ws, id, (50.0, 25.0), "Route A");
    assert_eq!(acted["anchor"][0], "#a", "{acted}");
    click(&mut ws, id, 50.0, 125.0);
    let shown = state_until(&mut r, &mut ws, "the second route", id, |v| {
        v["title"] == "#/b"
    });
    assert_eq!(shown["threads"][0]["away"], "/hash#/a", "{shown}");
    // Past the check's 500 ms quiet time.
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1500) {
        for f in r.idle(&mut ws, 1) {
            assert!(!matches!(&f, Frame::Json(v) if v["t"] == "notice"), "{f:?}");
        }
    }
    Ok(())
}

#[test]
fn a_late_element_is_not_lost() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let url = format!("{}/data", serve());
    let id = open(&mut r, &url);
    state_until(&mut r, &mut ws, "the button", id, |v| v["title"] == "1:");
    comment_at(&mut r, &mut ws, id, (50.0, 25.0), "Late");
    outlined_until(&mut r, &mut ws, "the outline", id, "1:", (None, &[0]));
    // After the reload, the button arrives late, and Laura outlines it with no notice.
    r.act(
        &mut ws,
        json!({"act": "browser_navigate", "pane": id, "url": url}),
    );
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < common::DEADLINE, "timed out: the outline");
        let frames = r.idle(&mut ws, 1);
        for f in &frames {
            assert!(!matches!(f, Frame::Json(v) if v["t"] == "notice"), "{f:?}");
        }
        let title = r.tab.browsers.get(&id).map(|p| p.title().to_string());
        if title.as_deref() == Some("2:") && outlines(&r, id) == (None, vec![0]) {
            break;
        }
    }
    Ok(())
}

#[test]
fn the_agent_annotates_by_selector() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let id = form(&mut r, &mut ws);
    let pane = id.to_string();
    r.drive(&["comment", "#email", "Needs a label", "--pane", &pane]);
    let shown = state_until(&mut r, &mut ws, "the agent's thread", id, |v| {
        v["threads"].as_array().is_some_and(|t| t.len() == 1)
    });
    outlined_until(
        &mut r,
        &mut ws,
        "the thread's outline",
        id,
        "Form",
        (None, &[100]),
    );
    let t = &shown["threads"][0];
    assert_eq!(t["anchor"], "#email", "{shown}");
    assert_eq!(t["notes"][0]["author"], "agent", "{shown}");
    // Another selector for the same element replies on its thread.
    r.drive(&["comment", "main > input", "Reply", "--pane", &pane]);
    let shown = state_until(&mut r, &mut ws, "the reply", id, |v| {
        v["threads"][0]["notes"]
            .as_array()
            .is_some_and(|n| n.len() == 2)
    });
    assert_eq!(
        shown["threads"].as_array().map(Vec::len),
        Some(1),
        "{shown}"
    );
    r.act(&mut ws, json!({"act": "begin_review", "pane": id}));
    let acted = r.act(&mut ws, json!({"act": "submit", "pane": id, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    r.shell_shows(r##"#email  """##);
    r.shell_shows("> [agent] Needs a label");
    Ok(())
}

#[test]
fn the_agent_highlights_by_selector() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let id = form(&mut r, &mut ws);
    let pane = id.to_string();
    r.drive(&["highlight", "main > h1", "--pane", &pane]);
    // The pane header's chip names the selector.
    r.until(&mut ws, "the highlight chip", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"].as_array().is_some_and(
            |ps| ps.iter().any(|p| p["id"] == id && p["highlight"] == "main > h1")))
    });
    outlined_until(&mut r, &mut ws, "the highlight", id, "Form", (Some(0), &[]));
    r.drive(&["highlight", "--off", "--pane", &pane]);
    outlined_until(&mut r, &mut ws, "no highlight", id, "Form", (None, &[]));
    // `h` clears it too.
    r.drive(&["highlight", "#h", "--pane", &pane]);
    outlined_until(
        &mut r,
        &mut ws,
        "the highlight again",
        id,
        "Form",
        (Some(0), &[]),
    );
    let acted = r.act(&mut ws, json!({"act": "clear_highlight", "pane": id}));
    assert_eq!(acted["ok"], true, "{acted}");
    outlined_until(
        &mut r,
        &mut ws,
        "no highlight again",
        id,
        "Form",
        (None, &[]),
    );
    Ok(())
}

#[test]
fn laura_draws_no_marks_in_the_page() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let id = form(&mut r, &mut ws);
    let pane = id.to_string();
    r.drive(&["comment", "#email", "Needs a label", "--pane", &pane]);
    r.drive(&["highlight", "main > h1", "--pane", &pane]);
    outlined_until(&mut r, &mut ws, "the marks", id, "Form", (Some(0), &[100]));
    // The agent's tools see the page as the user's own browser would: nothing of Laura's.
    let url = pane_of(&mut r, id)["path"]
        .as_str()
        .expect("a URL")
        .to_string();
    let mut tools = Tools::attach(&mut r, &url);
    let seen = tools.call(
        &mut r,
        "Runtime.evaluate",
        json!({"expression": "[document.documentElement.children.length, document.documentElement.outerHTML.includes('laura')]",
               "returnByValue": true}),
    );
    assert_eq!(seen["result"]["value"], json!([2, false]), "{seen}");
    Ok(())
}

#[test]
fn a_selector_must_match() -> Result<()> {
    let mut r = bare()?;
    let err = error(&mut r, &["highlight", "#a", "5"]);
    assert!(err.contains("a selector takes no end line"), "{err}");
    let err = error(&mut r, &["comment", "99999999999", "x"]);
    assert!(err.contains("invalid line number: 99999999999"), "{err}");
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let file = r.drive(&["open", "README.md"]);
    let err = error(&mut r, &["comment", "#a", "x", "--pane", &file]);
    assert!(
        err.contains(&format!("pane #{file} is a file pane: pass a line number")),
        "{err}"
    );
    let (id, _) = load(&mut r, &mut ws, "/dialog");
    let pane = id.to_string();
    let err = error(&mut r, &["highlight", "[", "--pane", &pane]);
    assert!(err.contains("invalid selector: ["), "{err}");
    let err = error(&mut r, &["comment", "#nope", "x", "--pane", &pane]);
    assert!(
        err.contains(&format!("no element matches #nope in pane #{id}")),
        "{err}"
    );
    let err = error(&mut r, &["comment", "#nope", "", "--pane", &pane]);
    assert!(err.contains("comment needs a body"), "{err}");
    let browser_pane = format!("pane #{id} is a browser pane: pass a CSS selector");
    let err = error(&mut r, &["comment", "1", "x", "--pane", &pane]);
    assert!(err.contains(&browser_pane), "{err}");
    let err = error(&mut r, &["highlight", "1", "--pane", &pane]);
    assert!(err.contains(&browser_pane), "{err}");
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "the confirm", id, |v| {
        v["dialog"]["type"] == "confirm"
    });
    let err = error(&mut r, &["highlight", "button", "--pane", &pane]);
    assert!(
        err.contains(&format!("pane #{id}'s page is showing a dialog")),
        "{err}"
    );
    Ok(())
}

#[test]
fn comment_mode_ends_with_its_view() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/type");
    send(
        &mut ws,
        json!({"act": "browser_comment_mode", "pane": id, "on": true}),
    );
    r.idle(&mut ws, 5);
    // The web app reloads: the new view starts with comment mode off, so the page gets the click.
    let mut ws2 = r.connect();
    r.until(&mut ws2, "layout", |f| f.is("layout"));
    r.idle(&mut ws2, 5);
    click(&mut ws2, id, 50.0, 25.0);
    type_key(&mut ws2, id, 'a');
    state_until(&mut r, &mut ws2, "the typed title", id, |v| {
        v["title"] == "a"
    });
    Ok(())
}

#[test]
fn comment_mode_turns_off() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/type");
    // `Esc` in the web app sends comment mode off, as **Done** does.
    for on in [true, false] {
        send(
            &mut ws,
            json!({"act": "browser_comment_mode", "pane": id, "on": on}),
        );
        r.idle(&mut ws, 5);
        assert_eq!(r.tab.browsers[&id].comment, on);
    }
    Ok(())
}

#[test]
fn a_new_view_gets_the_cards_place() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let (id, _) = keep(&mut r, &mut ws);
    comment_at(&mut r, &mut ws, id, (50.0, 25.0), "Here");
    // browser.js sends the box on a later frame, and `comment_at` can read past it: wait for Laura
    // to hold it. A new view takes over, so the first view is read no more after the connect.
    let start = Instant::now();
    while r.tab.browsers.get(&id).is_none_or(|p| p.rect().is_null()) {
        assert!(
            start.elapsed() < common::DEADLINE,
            "timed out: the card's place"
        );
        r.idle(&mut ws, 1);
    }
    let rect = r.tab.browsers[&id].rect().clone();
    let mut ws2 = r.connect();
    assert_eq!(event(&mut r, &mut ws2, "rect", id)["rect"], rect);
    Ok(())
}

#[test]
fn refresh_discards_and_reloads() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let (id, _) = keep(&mut r, &mut ws);
    comment_at(&mut r, &mut ws, id, (50.0, 25.0), "Gone soon");
    outlined_until(&mut r, &mut ws, "the outline", id, "1:", (None, &[0]));
    let acted = r.act(&mut ws, json!({"act": "refresh", "pane": id}));
    assert_eq!(acted["ok"], true, "{acted}");
    state_until(&mut r, &mut ws, "the next load, with no threads", id, |v| {
        v["title"] == "2:" && v["threads"] == json!([])
    });
    outlined_until(&mut r, &mut ws, "no outline", id, "2:", (None, &[]));
    Ok(())
}
