//! Browser panes (#107): the agent's browser tools on a pane's page, through a second CDP client
//! at `LAURA_BROWSER_URL` (Laura's proxy, #129): the page list, clicks and typing with their
//! marks, navigation, closed pages, emulation and dialogs. Each test that needs Chrome or Edge
//! skips when neither is installed; under `CI`, it fails. State and frames, not pixels.

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
use serde_json::json;

use browser_rig::*;
use rig::*;

#[test]
fn the_tools_see_only_the_panes() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (a, _) = load(&mut r, &mut ws, "/one");
    let (b, _) = load(&mut r, &mut ws, "/two");
    let mut want: Vec<(String, String)> = [a, b]
        .into_iter()
        .map(|id| {
            let p = pane_of(&mut r, id);
            let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
            (s("title"), s("path"))
        })
        .collect();
    want.sort();
    // Chrome closes its start tab a moment after Laura asks.
    let start = Instant::now();
    while listed(&r) != want {
        assert!(
            start.elapsed() < common::DEADLINE,
            "{:?} != {want:?}",
            listed(&r)
        );
        r.idle(&mut ws, 1);
    }
    Ok(())
}

#[test]
fn the_proxy_names_its_own_port() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    load(&mut r, &mut ws, "/one");
    let port = r.tab.browser_port.expect("a browser port");
    let chrome = r.tab.browser.as_ref().expect("Laura's browser").port;
    assert_ne!(port, chrome);
    let own = format!("ws://127.0.0.1:{port}/devtools/");
    let version = get(&r, "/json/version");
    let url = version["webSocketDebuggerUrl"].as_str().unwrap_or_default();
    assert!(url.starts_with(&own), "{version}");
    let list = get(&r, "/json/list");
    let pages = list.as_array().expect("a list");
    assert!(!pages.is_empty(), "{list}");
    for p in pages {
        let url = p["webSocketDebuggerUrl"].as_str().unwrap_or_default();
        assert!(url.starts_with(&own), "{p}");
        let front = p["devtoolsFrontendUrl"].as_str().unwrap_or_default();
        assert!(!front.contains(&format!(":{chrome}/")), "{p}");
    }
    Ok(())
}

#[test]
fn the_agents_tools_change_the_page_live() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/two");
    let mut tools = Tools::attach(&mut r, &format!("{url}/two"));
    let start = Instant::now();
    let before = loop {
        if let Some(f) = r.tab.browsers[&id].frame() {
            break f[8..].to_vec();
        }
        assert!(start.elapsed() < common::DEADLINE, "no frame");
        r.idle(&mut ws, 1);
    };
    tools.call(
        &mut r,
        "Runtime.evaluate",
        json!({"expression": "document.querySelector('h1').textContent = 'Changed'; document.title = 'Changed'"}),
    );
    let (start, mut titled, mut framed) = (Instant::now(), false, false);
    while !(titled && framed) {
        for f in r.idle(&mut ws, 1) {
            match f {
                Frame::Json(v) if v["t"] == "browser" && v["id"] == id => {
                    titled |= v["title"] == "Changed";
                }
                Frame::Bytes(n, b) if n == id => framed |= b[8..] != before[..],
                _ => {}
            }
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "titled {titled}, framed {framed}"
        );
    }
    Ok(())
}

#[test]
fn the_agents_tools_click_like_the_user() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/click");
    let mut tools = Tools::attach(&mut r, &format!("{url}/click"));
    for m in press_release(10.0, 10.0) {
        tools.call(&mut r, "Input.dispatchMouseEvent", m);
    }
    state_until(&mut r, &mut ws, "the click", id, |v| {
        v["title"] == "clicked"
    });
    Ok(())
}

#[test]
fn the_agents_click_shows_in_the_pane() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/click");
    let mut tools = Tools::attach(&mut r, &format!("{url}/click"));
    for m in press_release(10.0, 10.0) {
        tools.call(&mut r, "Input.dispatchMouseEvent", m);
    }
    assert_eq!(floats(&agent(&mut r, &mut ws, id)["click"]), [10.0, 10.0]);
    // A click while no view is open shows no mark in the next view.
    ws.close(None)?;
    drop(ws);
    for m in press_release(20.0, 20.0) {
        tools.call(&mut r, "Input.dispatchMouseEvent", m);
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
        r.pump();
        thread::sleep(Duration::from_millis(10));
    }
    let mut ws = r.connect();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
        let fs = r.idle(&mut ws, 1);
        assert!(
            !fs.iter().any(|f| f.is("agent")),
            "a mark from before the view"
        );
    }
    Ok(())
}

#[test]
fn a_script_click_marks_the_element() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/click");
    let mut tools = Tools::attach(&mut r, &format!("{url}/click"));
    tools.call(
        &mut r,
        "Runtime.evaluate",
        json!({"expression": "document.querySelector('button').click()"}),
    );
    // The `.at` button is 200×50 at the top left.
    assert_eq!(floats(&agent(&mut r, &mut ws, id)["click"]), [100.0, 25.0]);
    Ok(())
}

#[test]
fn the_agents_click_in_an_iframe_shows_in_the_pane() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/frame");
    let mut tools = Tools::attach(&mut r, &format!("{url}/frame"));
    for m in press_release(60.0, 30.0) {
        tools.call(&mut r, "Input.dispatchMouseEvent", m);
    }
    // The page's px, not the iframe's: the iframe is at (20, 10).
    assert_eq!(floats(&agent(&mut r, &mut ws, id)["click"]), [60.0, 30.0]);
    Ok(())
}

#[test]
fn the_agents_typing_shows_in_the_pane() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let id = form(&mut r, &mut ws);
    let url = pane_of(&mut r, id)["path"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let mut tools = Tools::attach(&mut r, &url);
    let rect = tools.call(
        &mut r,
        "Runtime.evaluate",
        json!({"expression": "email.focus(); const r = email.getBoundingClientRect(); [r.left, r.top, r.width, r.height]",
               "returnByValue": true}),
    );
    tools.call(&mut r, "Input.insertText", json!({"text": "a@b"}));
    assert_eq!(
        floats(&agent(&mut r, &mut ws, id)["type"]),
        floats(&rect["result"]["value"])
    );
    Ok(())
}

#[test]
fn the_users_input_shows_no_marker() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/type");
    click(&mut ws, id, 50.0, 25.0);
    type_key(&mut ws, id, 'h');
    send(
        &mut ws,
        json!({"act": "browser_text", "pane": id, "text": "i"}),
    );
    let mark = |f: &Frame| matches!(f, Frame::Json(v) if v["t"] == "agent");
    r.until(&mut ws, "the typed title", |f| {
        assert!(!mark(f), "a mark for the user's input: {f:?}");
        matches!(f, Frame::Json(v) if v["t"] == "browser" && v["id"] == id && v["title"] == "hi")
    });
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
        let fs = r.idle(&mut ws, 1);
        assert!(!fs.iter().any(mark), "a mark for the user's input");
    }
    Ok(())
}

#[test]
fn the_agents_tools_navigate_the_pane() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/one");
    let mut tools = Tools::attach(&mut r, &format!("{url}/one"));
    let two = format!("{url}/two");
    tools.call(&mut r, "Page.navigate", json!({"url": two}));
    state_until(&mut r, &mut ws, "the second page", id, |v| {
        v["url"] == two.as_str() && v["title"] == "Two"
    });
    assert_eq!(pane_of(&mut r, id)["path"], two.as_str());
    Ok(())
}

#[test]
fn a_closed_page_reopens_in_its_pane() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let id = form(&mut r, &mut ws);
    r.drive(&[
        "comment",
        "#email",
        "Needs a label",
        "--pane",
        &id.to_string(),
    ]);
    outlined_until(&mut r, &mut ws, "the thread", id, "Form", (None, &[100]));
    let url = pane_of(&mut r, id)["path"]
        .as_str()
        .expect("a URL")
        .to_string();
    let old = one_page(&mut r, &mut ws, &url);
    let mut tools = Tools::attach(&mut r, &url);
    tools.browser_call(&mut r, "Target.closeTarget", json!({"targetId": old}));
    // A new page at the same URL, where Laura outlines the thread's element again. Laura's own
    // state, since the view's state frame can come while `Tools` reads past it.
    let start = Instant::now();
    loop {
        let ps = pages(&r);
        let outlined = outlines(&r, id) == (None, vec![100]);
        if outlined && !ps.iter().any(|(t, _)| *t == old) && ps.iter().any(|(_, u)| *u == url) {
            break;
        }
        assert!(start.elapsed() < common::DEADLINE, "pages: {ps:?}");
        r.idle(&mut ws, 1);
    }
    for f in r.idle(&mut ws, 50) {
        assert!(!matches!(&f, Frame::Json(v) if v["t"] == "notice"), "{f:?}");
    }
    assert_eq!(pane_of(&mut r, id)["path"], url.as_str());
    Ok(())
}

#[test]
fn an_emulated_page_shows_at_its_size() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/click");
    send(
        &mut ws,
        json!({"size": {"id": id, "w": 400, "h": 300, "dpr": 1}}),
    );
    let pane_size = |r: &mut Rig, ws: &mut Ws| {
        let start = Instant::now();
        while png_size(&next_frame(r, ws, id).1) != (400, 300) {
            assert!(start.elapsed() < common::DEADLINE, "no 400×300 frame");
        }
    };
    pane_size(&mut r, &mut ws);
    let mut tools = Tools::attach(&mut r, &format!("{url}/click"));
    // The tool sets its size once, as chrome-devtools-mcp's `emulate` does.
    tools.call(
        &mut r,
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 393, "height": 852, "deviceScaleFactor": 3, "mobile": true}),
    );
    // Chrome sends a frame only when the page paints.
    let paint = |tools: &mut Tools, r: &mut Rig| {
        tools.call(
            r,
            "Runtime.evaluate",
            json!({"expression": "document.body.style.background = document.body.style.background ? '' : '#fffffe'"}),
        );
    };
    let start = Instant::now();
    'held: loop {
        paint(&mut tools, &mut r);
        let set = Instant::now();
        while set.elapsed() < Duration::from_millis(500) {
            for f in r.idle(&mut ws, 1) {
                if matches!(&f, Frame::Json(v) if v["t"] == "browser" && v["id"] == id
                    && v["emulated"] == json!({"w": 393, "h": 852}))
                {
                    break 'held;
                }
            }
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "timed out: the emulated size"
        );
    }
    // The user clicks in the page's own CSS px.
    click(&mut ws, id, 10.0, 10.0);
    state_until(&mut r, &mut ws, "the click", id, |v| {
        v["title"] == "clicked"
    });
    // A resize of the pane keeps the tool's emulation.
    send(
        &mut ws,
        json!({"size": {"id": id, "w": 500, "h": 400, "dpr": 1}}),
    );
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
        paint(&mut tools, &mut r);
        for f in r.idle(&mut ws, 5) {
            assert!(
                !matches!(&f, Frame::Json(v) if v["t"] == "browser" && v["id"] == id
                    && v["emulated"].is_null()),
                "the resize ended the emulation"
            );
        }
    }
    tools.call(&mut r, "Emulation.clearDeviceMetricsOverride", json!({}));
    // Laura sets the pane's size again after Chrome's frame at its window size, and the page then
    // sends a frame only when it paints again. The frames come back at the pane's new size with no
    // new `size` from the view.
    let (start, mut cleared, mut sized) = (Instant::now(), false, false);
    while !(cleared && sized) {
        paint(&mut tools, &mut r);
        let set = Instant::now();
        while set.elapsed() < Duration::from_millis(500) {
            for f in r.idle(&mut ws, 1) {
                match f {
                    Frame::Json(v) if v["t"] == "browser" && v["id"] == id => {
                        cleared = v["emulated"].is_null();
                    }
                    Frame::Bytes(n, b) if n == id => sized = png_size(&b[8..]) == (500, 400),
                    _ => {}
                }
            }
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "timed out: cleared {cleared}, sized {sized}"
        );
    }
    Ok(())
}

#[test]
fn the_agents_tools_answer_a_dialog() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/dialog");
    let mut tools = Tools::attach(&mut r, &format!("{url}/dialog"));
    tools.call(&mut r, "Page.enable", json!({}));
    // The release's reply waits on the dialog.
    for m in press_release(10.0, 10.0) {
        tools.send("Input.dispatchMouseEvent", m);
    }
    state_until(&mut r, &mut ws, "the confirm", id, |v| {
        v["dialog"]["type"] == "confirm"
    });
    tools.call(
        &mut r,
        "Page.handleJavaScriptDialog",
        json!({"accept": true}),
    );
    state_until(&mut r, &mut ws, "the answer", id, |v| {
        v["dialog"].is_null() && v["title"] == "true"
    });
    Ok(())
}

#[test]
fn a_page_the_tools_open_gets_no_pane() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (_, url) = load(&mut r, &mut ws, "/one");
    let before = layout(&mut r)["panes"].clone();
    let mut tools = Tools::attach(&mut r, &format!("{url}/one"));
    let two = format!("{url}/two");
    let made = tools.browser_call(&mut r, "Target.createTarget", json!({"url": two}));
    let target = made["targetId"].as_str().expect("a target").to_string();
    let start = Instant::now();
    while !pages(&r).iter().any(|(t, u)| *t == target && *u == two) {
        assert!(start.elapsed() < common::DEADLINE, "the page never loaded");
        r.idle(&mut ws, 1);
    }
    r.idle(&mut ws, 20);
    assert_eq!(layout(&mut r)["panes"], before);
    // It has no opener, so Laura leaves it open.
    assert!(pages(&r).iter().any(|(t, _)| *t == target));
    Ok(())
}
