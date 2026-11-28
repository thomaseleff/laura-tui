//! The client tests' rig (#228): the web app's own page in a headless Chrome of the test's, so a
//! test presses the user's keys and reads what the web app shows. The page is the workspace's only
//! view: a second view would take the workspace over. Included with `#[path]` by `tests/client.rs`.
//!
//! The rig's JS reads the client's top-level names: see the note at the end of `state.js`.

use std::path::Path;
use std::time::{Duration, Instant};

use laura::browser::Browser;
use serde_json::{Value, json};

use crate::browser_rig::Keystroke;
use crate::common;
use crate::rig::Rig;

pub const F12: Keystroke = ("F12", "F12", 123, 0, "");
pub const ENTER: Keystroke = ("Enter", "Enter", 13, 0, "\r");
pub const TAB: Keystroke = ("Tab", "Tab", 9, 0, "");
pub const ESCAPE: Keystroke = ("Escape", "Escape", 27, 0, "");
pub const CTRL_H: Keystroke = ("h", "KeyH", 72, 2, "");
pub const CTRL_BACKQUOTE: Keystroke = ("`", "Backquote", 192, 2, "");
pub const CTRL_PERIOD: Keystroke = (".", "Period", 190, 2, "");
pub const CTRL_SHIFT_UP: Keystroke = ("ArrowUp", "ArrowUp", 38, 10, "");
pub const CTRL_SHIFT_DOWN: Keystroke = ("ArrowDown", "ArrowDown", 40, 10, "");
pub const CTRL_P: Keystroke = ("p", "KeyP", 80, 2, "");
pub const CTRL_Q: Keystroke = ("q", "KeyQ", 81, 2, "");
pub const SHIFT_S: Keystroke = ("S", "KeyS", 83, 8, "S");
pub const C: Keystroke = ("c", "KeyC", 67, 0, "c");
pub const N: Keystroke = ("n", "KeyN", 78, 0, "n");
pub const ARROW_RIGHT: Keystroke = ("ArrowRight", "ArrowRight", 39, 0, "");
pub const HOME: Keystroke = ("Home", "Home", 36, 0, "");

/// The rig's init script, run before the page's own scripts:
/// - each error, unhandled rejection and `console.error` of the page, in `window.__errors`;
/// - the page's last 100 events, in `window.__events` as `<ms> <type> <detail>` lines, which
///   `Client::until` prints on a timeout: the pointer, the keys, the window's focus, the frames'
///   messages and the websocket's text frames both ways.
const INIT: &str = "(() => {
  const errors = window.__errors = [], error = console.error;
  addEventListener('error', e => errors.push(String(e.message)));
  addEventListener('unhandledrejection', e => errors.push('unhandled rejection: ' + e.reason));
  console.error = (...a) => (errors.push(a.join(' ')), error(...a));
  const events = window.__events = [];
  const log = (type, detail) => {
    events.push(Math.round(performance.now()) + ' ' + type + ' ' + detail);
    if (events.length > 100) events.shift();
  };
  const el = t => t instanceof Element
    ? t.tagName.toLowerCase() + (t.id ? '#' + t.id : '') + ' in ' + (t.closest('.pane')?.id ?? '-')
    : String(t);
  for (const type of ['pointerdown', 'pointerup', 'click', 'keydown'])
    addEventListener(type, e => log(type, el(e.target) + (e.key ? ' ' + e.key : '')), true);
  for (const type of ['blur', 'focus'])
    addEventListener(type, e => e.target === window && log(type, el(document.activeElement)));
  addEventListener('message', e => e.data?.laura && log('message', e.data.laura));
  // Laura's frames name themselves in `t`; the view's, by their first key (`act`, `size`, `view`).
  const field = d => { try { const m = JSON.parse(d); return m.t ?? (m.act ? 'act ' + m.act : Object.keys(m)[0]); } catch { return '?'; } };
  window.WebSocket = class extends WebSocket {
    constructor(...a) {
      super(...a);
      this.addEventListener('message', e => typeof e.data === 'string' && log('ws<', field(e.data)));
    }
    send(d) {
      if (typeof d === 'string') log('ws>', field(d));
      super.send(d);
    }
  };
})()";

/// The web app has its layout and the shell's pane.
const PAINTED: &str = "layout !== null && document.getElementById('p0') !== null";

/// The shell's xterm buffer, one string per line.
pub const SHELL_LINES: &str = "(() => { const b = terms.get(0).buffer.active;
  return Array.from({length: b.length}, (_, i) => b.getLine(i).translateToString(true)); })()";

/// The web app's page in a headless Chrome. Drop kills Chrome and deletes its profile.
pub struct Client {
    browser: Browser,
    session: String,
}

impl Client {
    /// `r`'s web app in a new headless Chrome at 1280×800, once the client paints its layout.
    #[track_caller]
    pub fn open(r: &mut Rig, chrome: &Path) -> Client {
        // Chrome can route a fast press and its release to different processes when an HTML
        // page's sandboxed frame runs in its own; the user's own browser runs the web app, and
        // Laura relies only on `postMessage` across the frame.
        let start = Browser::launch(chrome, &["IsolateSandboxedIframes"]);
        let t0 = Instant::now();
        let browser = loop {
            if let Some(res) = start.poll() {
                break res.unwrap_or_else(|e| {
                    panic!("Chrome didn't start: {:#}\n{}", e.reason, e.output)
                });
            }
            // Past Laura's own 60 s limit, so a failed start reports its cause and Chrome's output.
            assert!(
                t0.elapsed() < Duration::from_secs(75),
                "Chrome didn't start"
            );
            r.pump();
        };
        let cdp = &browser.cdp;
        let target = cdp
            .call(
                "Target.createTarget",
                json!({"url": "about:blank", "newWindow": true, "width": 1280, "height": 800}),
                None,
            )
            .expect("a page");
        let attached = cdp
            .call(
                "Target.attachToTarget",
                json!({"targetId": target["targetId"], "flatten": true}),
                None,
            )
            .expect("a session");
        let session = attached["sessionId"].as_str().expect("a session id").into();
        let c = Client { browser, session };
        // A background page has no focused element for the keys to reach.
        c.call(
            "Emulation.setFocusEmulationEnabled",
            json!({"enabled": true}),
        );
        c.call("Page.enable", json!({}));
        c.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": INIT}),
        );
        // No wait for the reply: Laura serves the page only while the rig pumps.
        c.browser.cdp.send(
            "Page.navigate",
            json!({"url": r.web.url()}),
            Some(&c.session),
        );
        c.until(r, "the layout", PAINTED);
        c
    }

    /// `method` on the page's session: its result. Only for a call that doesn't wait on Laura.
    #[track_caller]
    fn call(&self, method: &str, params: Value) -> Value {
        self.browser
            .cdp
            .call(method, params, Some(&self.session))
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// `js`'s value, awaited if `js` is a promise; the exception's text when `js` throws, or CDP's
    /// error while the page loads.
    pub fn eval(&self, js: &str) -> Result<Value, String> {
        let v = self
            .browser
            .cdp
            .call(
                "Runtime.evaluate",
                json!({"expression": js, "returnByValue": true, "awaitPromise": true}),
                Some(&self.session),
            )
            .map_err(|e| e.to_string())?;
        if let Some(x) = v.get("exceptionDetails") {
            let text = x["exception"]["description"]
                .as_str()
                .or(x["text"].as_str());
            return Err(text.unwrap_or("an exception").into());
        }
        Ok(v["result"]["value"].clone())
    }

    /// Pump `r` until `js` is truthy, and return its value; fail after `common::DEADLINE` with
    /// `what`, `js`'s last error, the page's errors and its last events (see `INIT`).
    #[track_caller]
    pub fn until(&self, r: &mut Rig, what: &str, js: &str) -> Value {
        let t0 = Instant::now();
        let mut last = String::new();
        loop {
            r.pump();
            match self.eval(js) {
                Ok(v)
                    if !matches!(&v, Value::Null | Value::Bool(false))
                        && v != json!(0)
                        && v != json!("") =>
                {
                    return v;
                }
                Ok(_) => {}
                Err(e) => last = e,
            }
            assert!(
                t0.elapsed() < common::DEADLINE,
                "timed out: {what}; the last error: {last}; the page's errors: {}; the page's last events:\n{}",
                self.errors(),
                self.events()
            );
        }
    }

    /// The page's errors so far (see `INIT`).
    pub fn errors(&self) -> Value {
        self.eval("window.__errors").unwrap_or(Value::Null)
    }

    /// The page's last events, one per line (see `INIT`); read only after a deadline, since each
    /// eval changes the timing.
    fn events(&self) -> String {
        self.eval("window.__events.join('\\n')")
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default()
    }

    /// `k`'s press and release: `key`, `code`, the virtual key code, CDP's modifiers and the text.
    pub fn press(&self, (key, code, key_code, mods, text): Keystroke) {
        let down = if text.is_empty() {
            "rawKeyDown"
        } else {
            "keyDown"
        };
        for kind in [down, "keyUp"] {
            self.call(
                "Input.dispatchKeyEvent",
                json!({"type": kind, "key": key, "code": code, "windowsVirtualKeyCode": key_code,
                       "modifiers": mods, "text": if kind == "keyUp" { "" } else { text }}),
            );
        }
    }

    /// A left click at `x`, `y` CSS px from the viewport's top left: press and release, so the
    /// click moves the DOM focus as the user's click does.
    pub fn click(&self, x: f64, y: f64) {
        self.mouse("mousePressed", (x, y), 1, 1);
        self.mouse("mouseReleased", (x, y), 0, 1);
    }

    /// A left drag from `from` to `to`, in CSS px from the viewport's top left, in one move: a
    /// resize the client sends during the drag can't land after the release's.
    pub fn drag(&self, from: (f64, f64), to: (f64, f64)) {
        self.drag_hold(from, to);
        self.release(to);
    }

    /// `drag` without the release: the user still holds the button at `to`.
    pub fn drag_hold(&self, from: (f64, f64), to: (f64, f64)) {
        // The pointer comes to the point first, as the user's does.
        self.mouse("mouseMoved", from, 0, 1);
        self.mouse("mousePressed", from, 1, 1);
        self.mouse("mouseMoved", to, 1, 1);
    }

    /// The pointer moves to `x`, `y` CSS px from the viewport's top left, with no button held.
    pub fn hover(&self, x: f64, y: f64) {
        self.mouse("mouseMoved", (x, y), 0, 1);
    }

    /// The release of a [`Client::drag_hold`] at `to`.
    pub fn release(&self, to: (f64, f64)) {
        self.mouse("mouseReleased", to, 0, 1);
    }

    /// The rig's one pointer sender, as a mouse sends: `buttons` holds the left button from the
    /// press to the release.
    fn mouse(&self, kind: &str, (x, y): (f64, f64), buttons: u8, count: u8) {
        self.call(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": x, "y": y, "button": "left", "buttons": buttons, "clickCount": count}),
        );
    }

    /// A left double-click at `x`, `y` CSS px from the viewport's top left.
    pub fn double_click(&self, x: f64, y: f64) {
        self.hover(x, y);
        for count in [1, 2] {
            self.mouse("mousePressed", (x, y), 1, count);
            self.mouse("mouseReleased", (x, y), 0, count);
        }
    }

    /// The page's viewport at `width`×`height` CSS px, as a window of that size shows it.
    pub fn resize(&self, width: u32, height: u32) {
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": width, "height": height, "deviceScaleFactor": 0, "mobile": false}),
        );
        // Chrome fires `resize` at its next rendering step, which can come after the test's next
        // check under load; that step runs before the second frame's callbacks.
        let frames = "new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))";
        self.eval(frames).expect("the page's resize");
    }

    /// The page's locale, as the user's system sets it: the clock and the date formats follow it.
    pub fn set_locale(&self, locale: &str) {
        self.call("Emulation.setLocaleOverride", json!({ "locale": locale }));
    }

    /// `s`, typed into the focused element as an input method types it.
    pub fn type_text(&self, s: &str) {
        self.call("Input.insertText", json!({"text": s}));
    }

    /// `Ctrl+P`, then pane `id`: the pane selector focuses the pane, in Laura and in the web app.
    #[track_caller]
    pub fn focus(&self, r: &mut Rig, id: u64) {
        self.press(CTRL_P);
        self.until(
            r,
            "the pane selector",
            "!document.getElementById('popup').hidden",
        );
        self.type_text(&id.to_string());
        self.until(
            r,
            "the focus",
            &format!(
                "document.getElementById('popup').hidden && layout.focus === {id} && !acts.length"
            ),
        );
        assert_eq!(r.tab.focus, id, "Laura's focus");
    }
}
