//! Browser panes (#107): the user's clicks, keystrokes, scrolling, copying, dialogs, history and
//! address field, the spinner, and a page that keeps the workspace waiting. Each test that needs Chrome or Edge skips when neither is
//! installed; under `CI`, it fails. State and frames, not pixels.

#[path = "common/browser.rs"]
#[allow(dead_code)] // each browser test crate uses part of the browser rig
mod browser_rig;
mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use laura::browser;
use serde_json::json;

use browser_rig::*;
use rig::*;

#[test]
fn a_click_reaches_the_page() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/click");
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "the click's title", id, |v| {
        v["title"] == "clicked"
    });
    Ok(())
}

#[test]
fn typing_reaches_a_focused_field() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/type");
    click(&mut ws, id, 50.0, 25.0);
    assert_eq!(event(&mut r, &mut ws, "editable", id)["editable"], true);
    type_key(&mut ws, id, 'h');
    type_key(&mut ws, id, 'i');
    state_until(&mut r, &mut ws, "the typed title", id, |v| {
        v["title"] == "hi"
    });
    // A paste types too.
    send(
        &mut ws,
        json!({"act": "browser_text", "pane": id, "text": " there"}),
    );
    state_until(&mut r, &mut ws, "the pasted title", id, |v| {
        v["title"] == "hi there"
    });
    click(&mut ws, id, 400.0, 400.0);
    assert_eq!(event(&mut r, &mut ws, "editable", id)["editable"], false);
    Ok(())
}

#[test]
fn a_field_the_page_focuses_after_a_click_takes_the_keystrokes() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/late");
    click(&mut ws, id, 50.0, 25.0);
    assert_eq!(event(&mut r, &mut ws, "editable", id)["editable"], true);
    Ok(())
}

#[test]
fn typing_reaches_a_field_in_an_iframe() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/frame");
    click(&mut ws, id, 50.0, 25.0);
    assert_eq!(event(&mut r, &mut ws, "editable", id)["editable"], true);
    type_key(&mut ws, id, 'h');
    state_until(&mut r, &mut ws, "the typed title", id, |v| {
        v["title"] == "h"
    });
    Ok(())
}

#[test]
fn typing_reaches_a_field_in_a_shadow_root() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/shadow");
    click(&mut ws, id, 50.0, 25.0);
    assert_eq!(event(&mut r, &mut ws, "editable", id)["editable"], true);
    type_key(&mut ws, id, 'h');
    state_until(&mut r, &mut ws, "the typed title", id, |v| {
        v["title"] == "h"
    });
    Ok(())
}

#[test]
fn a_new_tab_opens_in_the_same_pane() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/blank");
    let (blank, two) = (format!("{url}/blank"), format!("{url}/two"));
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "the link's page", id, |v| {
        v["url"] == two.as_str() && v["title"] == "Two" && v["back"] == true
    });
    let acted = r.act(
        &mut ws,
        json!({"act": "browser_history", "pane": id, "delta": -1}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    state_until(&mut r, &mut ws, "back to the links", id, |v| {
        v["url"] == blank.as_str() && v["title"] == "Blank"
    });
    click(&mut ws, id, 50.0, 125.0);
    state_until(&mut r, &mut ws, "window.open's page", id, |v| {
        v["url"] == two.as_str() && v["title"] == "Two"
    });
    Ok(())
}

/// Clicks the dropdown list on `page`, presses `before`, then `↓`, and waits for the choice `want`.
fn arrow_down_picks(page: &str, before: Option<Keystroke>, want: &str) -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, page);
    click(&mut ws, id, 50.0, 25.0);
    assert_eq!(event(&mut r, &mut ws, "editable", id)["editable"], true);
    if let Some(k) = before {
        press_key(&mut ws, id, k);
    }
    press(&mut ws, id, "ArrowDown", 40);
    let what = format!("the next choice after {before:?}");
    state_until(&mut r, &mut ws, &what, id, |v| v["title"] == want);
    Ok(())
}

#[test]
fn a_dropdown_list_takes_the_arrow_keys() -> Result<()> {
    arrow_down_picks("/select", None, "b")
}

#[test]
fn a_dropdown_list_in_a_web_component_takes_the_arrow_keys() -> Result<()> {
    arrow_down_picks("/shadow-select", None, "b")
}

#[test]
fn a_dropdown_list_s_arrow_keys_skip_hidden_choices() -> Result<()> {
    arrow_down_picks("/hidden-select", None, "d")
}

/// A page that cancels `↓` keeps its choice, as Chrome does it on Linux and Windows.
#[test]
fn a_page_that_cancels_a_key_keeps_its_dropdown_choice() -> Result<()> {
    arrow_down_picks("/cancel-select", None, "kept a")
}

/// #157: a popup that opened would take the `↓`, and the user can't see the popup.
#[test]
fn a_dropdown_list_opens_no_popup() -> Result<()> {
    for k in [
        (" ", "Space", 32, 0, " "),
        ("Enter", "Enter", 13, 0, ""),
        ("ArrowUp", "ArrowUp", 38, 1, ""),
        ("ArrowDown", "ArrowDown", 40, 1, ""),
        ("F4", "F4", 115, 0, ""),
    ] {
        arrow_down_picks("/select", Some(k), "b")?;
    }
    Ok(())
}

#[test]
fn a_move_off_the_page_ends_the_hover() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/hover");
    mouse(&mut ws, id, "move", 50.0, 25.0);
    state_until(&mut r, &mut ws, "the hover", id, |v| v["title"] == "in");
    // The web app's move when the pointer leaves the pane.
    mouse(&mut ws, id, "move", -1.0, -1.0);
    state_until(&mut r, &mut ws, "the hover's end", id, |v| {
        v["title"] == "out"
    });
    Ok(())
}

#[test]
fn a_drag_copies_the_selection() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/drag");
    // Past the line's end, inside the paragraph, so the selection ends at the last letter.
    mouse(&mut ws, id, "down", 1.0, 12.0);
    send(
        &mut ws,
        json!({"act": "browser_mouse", "pane": id, "kind": "move", "x": 300, "y": 12, "buttons": 1}),
    );
    mouse(&mut ws, id, "up", 300.0, 12.0);
    assert_eq!(event(&mut r, &mut ws, "copy", id)["text"], "hello world");
    Ok(())
}

#[test]
fn the_address_field_navigates() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/click");
    let host = url.trim_start_matches("http://");
    let acted = r.act(
        &mut ws,
        json!({"act": "browser_navigate", "pane": id, "url": format!("{host}/two")}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let two = format!("{url}/two");
    state_until(&mut r, &mut ws, "the typed URL", id, |v| {
        v["url"] == two.as_str() && v["title"] == "Two"
    });
    let path_of = |r: &mut Rig| {
        layout(r)["panes"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| p["id"] == id).cloned())
            .expect("the pane")["path"]
            .clone()
    };
    assert_eq!(path_of(&mut r), two.as_str());
    for bad in ["file:///etc/hosts", "javascript:alert(1)"] {
        let acted = r.act(
            &mut ws,
            json!({"act": "browser_navigate", "pane": id, "url": bad}),
        );
        assert_eq!(acted["ok"], false, "{bad}: {acted}");
        assert_eq!(acted["text"], browser::ONLY_HTTP, "{bad}");
    }
    assert_eq!(path_of(&mut r), two.as_str());
    // As in Chrome: http for an IP address or a local name, and a `:` in the path is no scheme.
    for (typed, want) in [
        ("0.0.0.0:9/x", "http://0.0.0.0:9/x"),
        ("[::1]:9/x", "http://[::1]:9/x"),
        ("example.invalid/a:b", "https://example.invalid/a:b"),
        ("myhost:9/x", "http://myhost:9/x"),
    ] {
        let acted = r.act(
            &mut ws,
            json!({"act": "browser_navigate", "pane": id, "url": typed}),
        );
        assert_eq!(acted["ok"], true, "{typed}: {acted}");
        // The URL event of the last failed load can still arrive, so wait for this one.
        state_until(&mut r, &mut ws, want, id, |v| v["url"] == want);
    }
    // A LAN name gets http too; Chrome may add a `/` once it tries the host.
    let acted = r.act(
        &mut ws,
        json!({"act": "browser_navigate", "pane": id, "url": "myapp.local:3000"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    state_until(&mut r, &mut ws, "the LAN name", id, |v| {
        v["url"]
            .as_str()
            .is_some_and(|u| u.starts_with("http://myapp.local:3000"))
    });
    Ok(())
}

#[test]
fn back_and_forward_navigate() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/one");
    let at = |p: &str| format!("{url}{p}");
    let (one, two, three) = (at("/one"), at("/two"), at("/three"));
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "the second page", id, |v| {
        v["url"] == two.as_str() && v["back"] == true && v["forward"] == false
    });
    let go = |r: &mut Rig, ws: &mut Ws, delta: i32| {
        let acted = r.act(
            ws,
            json!({"act": "browser_history", "pane": id, "delta": delta}),
        );
        assert_eq!(acted["ok"], true, "{acted}");
    };
    // The page's first entry, `about:blank`, isn't a page to go back to.
    go(&mut r, &mut ws, -1);
    state_until(&mut r, &mut ws, "back to the first page", id, |v| {
        v["url"] == one.as_str() && v["back"] == false && v["forward"] == true
    });
    go(&mut r, &mut ws, 1);
    state_until(&mut r, &mut ws, "forward to the second page", id, |v| {
        v["url"] == two.as_str() && v["back"] == true
    });
    // A single-page app's route counts as a page.
    go(&mut r, &mut ws, -1);
    state_until(&mut r, &mut ws, "the first page again", id, |v| {
        v["url"] == one.as_str() && v["title"] == "One"
    });
    click(&mut ws, id, 50.0, 125.0);
    state_until(&mut r, &mut ws, "the route", id, |v| {
        v["url"] == three.as_str() && v["back"] == true && v["forward"] == false
    });
    go(&mut r, &mut ws, -1);
    state_until(&mut r, &mut ws, "back from the route", id, |v| {
        v["url"] == one.as_str() && v["forward"] == true
    });
    Ok(())
}

#[test]
fn a_new_view_keeps_the_pages_field() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/type");
    click(&mut ws, id, 50.0, 25.0);
    assert_eq!(event(&mut r, &mut ws, "editable", id)["editable"], true);
    let mut ws2 = r.connect();
    assert_eq!(event(&mut r, &mut ws2, "editable", id)["editable"], true);
    type_key(&mut ws2, id, 'h');
    state_until(&mut r, &mut ws2, "the typed title", id, |v| {
        v["title"] == "h"
    });
    Ok(())
}

#[test]
fn a_busy_page_doesnt_stall_the_workspace() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/busy");
    click(&mut ws, id, 50.0, 25.0);
    // The page's 3 s loop has the click; a move waits behind it.
    r.idle(&mut ws, 5);
    thread::sleep(Duration::from_millis(200));
    mouse(&mut ws, id, "move", 60.0, 30.0);
    let start = Instant::now();
    layout(&mut r);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "layout waited {:?} on the busy page",
        start.elapsed()
    );
    Ok(())
}

#[test]
fn the_wheel_scrolls_the_page() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/wheel");
    send(
        &mut ws,
        json!({"act": "browser_mouse", "pane": id, "kind": "wheel", "x": 100, "y": 100, "dy": 300}),
    );
    state_until(&mut r, &mut ws, "the scrolled title", id, |v| {
        v["title"]
            .as_str()
            .and_then(|t| t.strip_prefix('y'))
            .and_then(|y| y.parse::<f64>().ok())
            .is_some_and(|y| y > 0.0)
    });
    Ok(())
}

#[test]
fn a_scroll_key_scrolls_the_page() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/wheel");
    press(&mut ws, id, "PageDown", 34);
    state_until(&mut r, &mut ws, "the scrolled title", id, |v| {
        v["title"]
            .as_str()
            .and_then(|t| t.strip_prefix('y'))
            .and_then(|y| y.parse::<f64>().ok())
            .is_some_and(|y| y > 0.0)
    });
    Ok(())
}

#[test]
fn an_autofocused_field_takes_the_keystrokes() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let id = open(&mut r, &format!("{}/auto", serve()));
    r.until(&mut ws, "the autofocused field", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "editable" && v["id"] == id && v["editable"] == true)
    });
    type_key(&mut ws, id, 'h');
    state_until(&mut r, &mut ws, "the typed title", id, |v| {
        v["title"] == "h"
    });
    Ok(())
}

#[test]
fn esc_gives_the_scroll_keys_back_to_the_page() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/type");
    click(&mut ws, id, 50.0, 25.0);
    assert_eq!(event(&mut r, &mut ws, "editable", id)["editable"], true);
    type_key(&mut ws, id, 'h');
    state_until(&mut r, &mut ws, "the typed title", id, |v| {
        v["title"] == "h"
    });
    // As the web app sends `Esc`: the field lets go, so `x` reaches no field.
    send(&mut ws, json!({"act": "browser_blur", "pane": id}));
    // The blur (a script) and the keys (input) reach the page by different routes; a person's
    // next keystroke comes later than this.
    r.idle(&mut ws, 5);
    thread::sleep(Duration::from_millis(200));
    type_key(&mut ws, id, 'x');
    click(&mut ws, id, 50.0, 25.0);
    type_key(&mut ws, id, 'i');
    state_until(&mut r, &mut ws, "the title after Esc", id, |v| {
        v["title"] == "hi"
    });
    Ok(())
}

#[test]
fn the_cursor_follows_the_page() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/cursor");
    // Each step changes the cursor, so each sends a frame.
    for (x, y, want) in [
        (20.0, 20.0, "pointer"),
        (20.0, 58.0, "text"),
        (600.0, 500.0, "default"),
        (50.0, 125.0, "text"),
        (50.0, 175.0, "grab"),
        (20.0, 20.0, "pointer"),
    ] {
        mouse(&mut ws, id, "move", x, y);
        state_until(&mut r, &mut ws, want, id, |v| v["cursor"] == want);
    }
    // The same cursor again sends nothing.
    mouse(&mut ws, id, "move", 30.0, 20.0);
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        for f in r.idle(&mut ws, 1) {
            assert!(
                !matches!(&f, Frame::Json(v) if v["t"] == "browser" && v["id"] == id),
                "{f:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_dialog_waits_for_an_answer() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/dialog");
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "the confirm", id, |v| {
        v["dialog"]["type"] == "confirm" && v["dialog"]["message"] == "Discard?"
    });
    let acted = r.act(
        &mut ws,
        json!({"act": "browser_dialog", "pane": id, "accept": false}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    state_until(&mut r, &mut ws, "the answer", id, |v| {
        v["title"] == "false" && v["dialog"].is_null()
    });
    click(&mut ws, id, 50.0, 125.0);
    state_until(&mut r, &mut ws, "the prompt", id, |v| {
        v["dialog"]["type"] == "prompt" && v["dialog"]["default"] == "x"
    });
    r.act(
        &mut ws,
        json!({"act": "browser_dialog", "pane": id, "accept": true, "text": "Ada"}),
    );
    state_until(&mut r, &mut ws, "the typed answer", id, |v| {
        v["title"] == "Ada" && v["dialog"].is_null()
    });
    Ok(())
}

#[test]
fn a_click_shows_busy_only_while_the_page_handles_it() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/busy");
    // The page's 3 s loop has the click.
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "busy: true", id, |v| v["busy"] == true);
    state_until(&mut r, &mut ws, "busy: false", id, |v| v["busy"] == false);
    // A click that the page handles at once shows no spinner.
    let (id, _) = load(&mut r, &mut ws, "/click");
    click(&mut ws, id, 50.0, 25.0);
    let (start, mut clicked) = (Instant::now(), None);
    while clicked.is_none_or(|t: Instant| t.elapsed() < Duration::from_millis(500)) {
        assert!(start.elapsed() < common::DEADLINE, "timed out: the click");
        for f in r.idle(&mut ws, 1) {
            if let Frame::Json(v) = &f
                && v["t"] == "browser"
                && v["id"] == id
            {
                assert_ne!(v["busy"], true, "{v}");
                if v["title"] == "clicked" {
                    clicked.get_or_insert_with(Instant::now);
                }
            }
        }
    }
    Ok(())
}

#[test]
fn a_slow_click_shows_busy() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/fetch");
    let busy = |r: &mut Rig, ws: &mut Ws, on: bool| {
        state_until(r, ws, &format!("busy: {on}"), id, |v| v["busy"] == on);
    };
    // A fetch that answers in 1.5 s.
    click(&mut ws, id, 50.0, 25.0);
    busy(&mut r, &mut ws, true);
    busy(&mut r, &mut ws, false);
    // A fetch that never answers stops counting after 10 s.
    click(&mut ws, id, 50.0, 225.0);
    busy(&mut r, &mut ws, true);
    let start = Instant::now();
    busy(&mut r, &mut ws, false);
    assert!(
        start.elapsed() > Duration::from_secs(8),
        "{:?}",
        start.elapsed()
    );
    // A response that streams its body stops the spinner when the server answers.
    click(&mut ws, id, 50.0, 325.0);
    let start = Instant::now();
    state_until(&mut r, &mut ws, "the streaming title", id, |v| {
        v["title"] == "streaming" && v["busy"] == false
    });
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    // A link to a page that loads in 3 s.
    click(&mut ws, id, 50.0, 125.0);
    busy(&mut r, &mut ws, true);
    let slow = format!("{url}/slow");
    state_until(&mut r, &mut ws, "the slow page", id, |v| {
        v["busy"] == false && v["url"] == slow.as_str()
    });
    Ok(())
}
