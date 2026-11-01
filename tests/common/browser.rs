//! The browser tests' shared rig (#107): the test pages and their server, Laura with Laura's
//! browser, and a second CDP client that stands in for the agent's browser tools. Included with
//! `#[path]` by each `browser*` test crate.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use laura::browser;
use serde_json::{Value, json};
use tungstenite::Message;

use crate::common;
use crate::rig::*;

/// The installed Chrome or Edge, or `None` (the test skips; under `CI`, it fails). The lock runs
/// one test's browser at a time; CI runs each browser test crate in its own job.
/// ponytail: one Chrome at a time keeps CI's memory low; a lock per test if the suite gets slow.
pub fn chrome() -> Option<(PathBuf, MutexGuard<'static, ()>)> {
    // CI's `app` job leaves the browser tests to the browser jobs.
    if std::env::var_os("LAURA_SKIP_BROWSER_TESTS").is_some() {
        return None;
    }
    static ONE: Mutex<()> = Mutex::new(());
    // SAFETY: set before any test reads it, under the lock; std's env lock serializes it with std's
    // own readers.
    let guard = ONE.lock().unwrap_or_else(PoisonError::into_inner);
    unsafe { std::env::set_var("LAURA_EXPERIMENTAL_BROWSER", "1") };
    match browser::resolve() {
        Ok(path) => Some((path, guard)),
        Err(e) => {
            // CI must run these: a runner without a browser fails instead of passing empty.
            if std::env::var_os("CI").is_some() {
                panic!("CI needs Chrome or Edge: {e}");
            }
            eprintln!("skipped: {e}");
            None
        }
    }
}

/// Every test page's style: `.at` is a 200×50 box at the left edge, so a test knows where to click.
pub const CSS: &str =
    "body { margin: 0 } .at { position: fixed; left: 0; width: 200px; height: 50px; margin: 0 }";

/// A route's delay in ms, title and body. Any other path is a page titled "Fast page".
pub fn route(path: &str) -> (u64, &'static str, &'static str) {
    match path {
        "/slow" => (3000, "Fast page", "<h1>Hello</h1>"),
        "/fetch-slow" => (1500, "", ""),
        "/hang" => (30_000, "", ""),
        "/stream" => (30_000, "", ""),
        "/click" => (
            0,
            "Click",
            r#"<button class=at style=top:0 onclick="document.title='clicked'">Go</button>"#,
        ),
        "/type" => (
            0,
            "Type",
            r#"<input class=at style=top:0 oninput="document.title=this.value">"#,
        ),
        "/auto" => (
            0,
            "Auto",
            r#"<input class=at style=top:0 autofocus oninput="document.title=this.value">"#,
        ),
        "/late" => (
            0,
            "Late",
            r#"<button class=at style=top:0 onclick="setTimeout(() => f.focus(), 50)">Edit</button>
               <input id=f class=at style=top:100px>"#,
        ),
        "/frame" => (
            0,
            "Frame",
            "<iframe src=/inner style='position:fixed; top:10px; left:20px; width:400px; height:200px; border:0'></iframe>",
        ),
        "/inner" => (
            0,
            "",
            r#"<input class=at style=top:0 oninput="parent.document.title=this.value">"#,
        ),
        // A web component whose open shadow root holds the field.
        "/shadow" => (
            0,
            "Shadow",
            r#"<x-in class=at style=top:0></x-in>
               <script>customElements.define('x-in', class extends HTMLElement { constructor() { super();
                 this.attachShadow({mode: 'open'}).innerHTML = '<input style="width:200px; height:50px; margin:0" oninput="document.title=this.value">';
               } })</script>"#,
        ),
        // A web component whose open shadow root holds the dropdown list.
        "/shadow-select" => (
            0,
            "Shadow select",
            r#"<x-sel class=at style=top:0></x-sel>
               <script>customElements.define('x-sel', class extends HTMLElement { constructor() { super();
                 this.attachShadow({mode: 'open'}).innerHTML = '<select style="width:200px; height:50px; margin:0"><option>a<option>b</select>';
               } })</script>
               <script>document.addEventListener('input', e => document.title = e.composedPath()[0].value)</script>"#,
        ),
        "/blank" => (
            0,
            "Blank",
            r#"<a class=at style=top:0 href=/two target=_blank>Two</a>
               <button class=at style=top:100px onclick="window.open('/two')">Open</button>"#,
        ),
        "/select" => (
            0,
            "Select",
            r#"<select class=at style=top:0 onchange="document.title=this.value"><option>a<option>b</select>"#,
        ),
        // `b` is hidden, and `c` is in a hidden group: `↓` from `a` picks `d`.
        "/hidden-select" => (
            0,
            "Hidden select",
            r#"<select class=at style=top:0 onchange="document.title=this.value"><option>a<option hidden>b<optgroup label=g style=display:none><option>c</optgroup><option>d</select>"#,
        ),
        "/hover" => (
            0,
            "Hover",
            r#"<div class=at style=top:0 onmouseenter="document.title='in'" onmouseleave="document.title='out'"></div>"#,
        ),
        "/drag" => (
            0,
            "Drag",
            "<p class=at style='top:0; width:400px; font:20px monospace'>hello world</p>",
        ),
        "/one" => (
            0,
            "One",
            r#"<a class=at style=top:0 href=/two>Two</a>
               <button class=at style=top:100px onclick="history.pushState(null, '', '/three')">Route</button>"#,
        ),
        "/two" => (0, "Two", "<h1>Two</h1>"),
        "/pick" => (
            0,
            "Pick",
            "<button id=b class=at style=top:0 onclick=\"this.textContent = 'Saved'; document.title = 'Saved'\">Save   draft</button><p class=at style=top:100px>Some text</p>
             <ul class=at style=top:200px><li>One<li>Two</ul>
             <p class=at style=top:300px>0123456789012345678901234567890123456789012345678901234567890123456789</p>
             <button class=at style=top:400px onclick=\"const li = document.createElement('li'); li.textContent = 'Zero'; document.querySelector('ul').prepend(li); document.title = 'Added'\">Add</button>",
        ),
        // The title counts the tab's loads. No other element's text starts with "Drop", the
        // button's quote, so Laura doesn't find the button in another element.
        "/keep" => (
            0,
            "Keep",
            "<button id=b class=at style=top:0>Drop</button><button id=r class=at style=top:100px>Restore</button>
             <p id=s class=at style=top:200px>Text</p><button id=d class=at style=top:300px>Remove two</button>
             <script>
             const b = document.getElementById('b'), n = sessionStorage.n = +(sessionStorage.n ?? 0) + 1;
             b.onclick = () => b.remove();
             r.onclick = () => document.body.prepend(b);
             d.onclick = () => (b.remove(), s.remove());
             document.title = n + ':';
             </script>",
        ),
        // A hash-routed app: the title names the route.
        "/hash" => (
            0,
            "",
            "<div id=v></div><a class=at style=top:100px href=#/b>B</a>
             <script>
             const show = () => {
               v.innerHTML = location.hash == '#/b' ? '<p id=b class=at style=top:0>B page</p>' : '<p id=a class=at style=top:0>A page</p>';
               document.title = location.hash;
             };
             show();
             onhashchange = show;
             </script>",
        ),
        // The button arrives 1.5 s after the load, as after a fetch. Then the title counts the
        // tab's loads, as `/keep`'s does.
        "/data" => (
            0,
            "Data",
            "<script>
             const n = sessionStorage.n = +(sessionStorage.n ?? 0) + 1;
             setTimeout(() => {
               document.body.insertAdjacentHTML('afterbegin', '<button id=b class=at style=top:0>Late</button>');
               document.title = n + ':';
             }, 1500);
             </script>",
        ),
        "/form" => (
            0,
            "Form",
            "<main><h1 id=h class=at style=top:0>Sign up</h1><input id=email class=at style=top:100px></main>",
        ),
        "/busy" => (
            0,
            "Busy",
            r#"<button class=at style=top:0 onclick="const t = Date.now(); while (Date.now() - t < 3000) {}">Busy</button>"#,
        ),
        "/wheel" => (
            0,
            "Wheel",
            "<div style=height:5000px></div><script>onscroll = () => document.title = 'y' + scrollY</script>",
        ),
        "/cursor" => (
            0,
            "Cursor",
            "<a class=at style=top:0 href=#>Link</a><p class=at style=top:50px>Some text</p>
             <input class=at style=top:100px><div class=at style='top:150px; cursor:url(x.png), grab'></div>",
        ),
        "/dialog" => (
            0,
            "Dialog",
            r#"<button class=at style=top:0 onclick="document.title = String(confirm('Discard?'))">Confirm</button>
               <button class=at style=top:100px onclick="document.title = prompt('Name?', 'x')">Prompt</button>"#,
        ),
        "/fetch" => (
            0,
            "Fetch",
            r#"<button class=at style=top:0 onclick="fetch('/fetch-slow')">Save</button>
               <a class=at style=top:100px href=/slow>Slow</a>
               <button class=at style=top:200px onclick="fetch('/hang')">Hang</button>
               <button class=at style=top:300px onclick="fetch('/stream').then(() => document.title = 'streaming')">Stream</button>"#,
        ),
        "/deny" => (0, "Deny", "<h1>No frames</h1>"),
        "/download" => (
            0,
            "Download",
            "<a class=at style=top:0 href=/file.txt>File</a>",
        ),
        "/ask" => (
            0,
            "Ask",
            "<button class=at style=top:0 onclick=\"Notification.requestPermission().then(p => document.title = p)\">Ask</button>",
        ),
        // A named target, then a plain link for a middle-click.
        "/named" => (
            0,
            "Named",
            "<a class=at style=top:0 href=/two target=other>Other</a>
             <a class=at style=top:100px href=/two>Two</a>",
        ),
        _ => (0, "Fast page", "<h1>Hello</h1>"),
    }
}

/// The server sent `/file.txt`, the download.
pub static DOWNLOADED: AtomicBool = AtomicBool::new(false);
pub const DOWNLOAD: &str = "laura-download-test.txt";

/// A route's extra headers.
pub fn headers(path: &str) -> String {
    match path {
        "/deny" => {
            "X-Frame-Options: DENY\r\nContent-Security-Policy: frame-ancestors 'none'\r\n".into()
        }
        "/file.txt" => {
            DOWNLOADED.store(true, Ordering::SeqCst);
            format!("Content-Disposition: attachment; filename={DOWNLOAD}\r\n")
        }
        _ => String::new(),
    }
}

/// A local web server for `route`'s pages.
pub fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("port").port();
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("/");
                let (delay, title, body) = route(path);
                let body = format!(
                    "<!doctype html><title>{title}</title><style>{CSS}</style><body>{body}</body>"
                );
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                    body.len(),
                    headers(path)
                );
                // `/stream` sends its headers at once and its body after the delay.
                let early = path == "/stream";
                if early {
                    let _ = stream.write_all(head.as_bytes());
                }
                thread::sleep(Duration::from_millis(delay));
                if !early {
                    let _ = stream.write_all(head.as_bytes());
                }
                let _ = stream.write_all(body.as_bytes());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// `Rig::new` without its isolated home: Chrome on Windows doesn't start with `USERPROFILE` moved.
/// No test here runs `ready`, so nothing writes the journal.
pub fn bare() -> Result<Rig> {
    hosted(Err(browser::FLAG_OFF.into()))
}

/// `bare`, with browser panes resolved to `path` as `laura --web` resolves them.
pub fn hosted(path: Result<PathBuf, String>) -> Result<Rig> {
    let tab = laura::Tab::spawn_web(shell(), 24, 80, path)?;
    let web = laura::web::WebServer::bind()?;
    let (port, token) = port_token(web.url());
    Ok(Rig {
        tab,
        web,
        port,
        token,
    })
}

/// A workspace under `laura --web` whose browser panes run `path`, and a view of it.
pub fn rig(path: PathBuf) -> Result<(Rig, Ws)> {
    let mut r = hosted(Ok(path))?;
    let mut ws = r.connect();
    r.until(&mut ws, "layout", |f| f.is("layout"));
    Ok((r, ws))
}

/// `laura open <url>`: the new pane's id.
pub fn open(r: &mut Rig, url: &str) -> u64 {
    r.drive(&["open", url]).parse().expect("a pane id")
}

/// A frame for `id`: its paint time (ms since the epoch) and its PNG.
pub fn next_frame(r: &mut Rig, ws: &mut Ws, id: u64) -> (f64, Vec<u8>) {
    let f = r.until(
        ws,
        "a frame",
        |f| matches!(f, Frame::Bytes(n, _) if *n == id),
    );
    let Frame::Bytes(_, b) = f else {
        unreachable!()
    };
    let (t, png) = b.split_first_chunk::<8>().expect("a paint time");
    (f64::from_be_bytes(*t), png.to_vec())
}

/// A PNG's width and height, from its IHDR.
pub fn png_size(png: &[u8]) -> (u32, u32) {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "a PNG");
    let n = |i: usize| u32::from_be_bytes(png[i..i + 4].try_into().expect("4 bytes"));
    (n(16), n(20))
}

pub fn layout(r: &mut Rig) -> Value {
    serde_json::from_str(&r.drive(&["layout"])).expect("layout JSON")
}

pub fn error(r: &mut Rig, args: &[&str]) -> String {
    let out = r.run(args);
    assert!(!out.status.success(), "laura {args:?} succeeded");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The next `browser` frame for `id` that `want` matches.
pub fn state_until(
    r: &mut Rig,
    ws: &mut Ws,
    what: &str,
    id: u64,
    want: impl Fn(&Value) -> bool,
) -> Value {
    r.until(
        ws,
        what,
        |f| matches!(f, Frame::Json(v) if v["t"] == "browser" && v["id"] == id && want(v)),
    )
    .json()
}

/// What the web app outlines over pane `id`'s page, from Laura's own state: the top of the
/// highlight's box, and of each thread's. The fixtures' elements sit at known tops.
pub fn outlines(r: &Rig, id: u64) -> (Option<i64>, Vec<i64>) {
    let Some(p) = r.tab.browsers.get(&id) else {
        return (None, vec![]);
    };
    let top = |v: &Value| v.as_f64().map(|t| t.round() as i64);
    let m = p.marks();
    let th = m["th"].as_array().into_iter().flatten();
    (top(&m["hl"][0]), th.filter_map(|t| top(&t[1])).collect())
}

/// Wait until pane `id`'s page has `title` and the web app outlines `want` (see `outlines`).
pub fn outlined_until(
    r: &mut Rig,
    ws: &mut Ws,
    what: &str,
    id: u64,
    title: &str,
    want: (Option<i64>, &[i64]),
) {
    let want = (want.0, want.1.to_vec());
    let start = Instant::now();
    while !(r.tab.browsers.get(&id).is_some_and(|p| p.title() == title) && outlines(r, id) == want)
    {
        assert!(
            start.elapsed() < common::DEADLINE,
            "timed out: {what}: {:?}",
            outlines(r, id)
        );
        r.idle(ws, 1);
    }
}

/// The next frame of type `t` for `id`.
pub fn event(r: &mut Rig, ws: &mut Ws, t: &str, id: u64) -> Value {
    r.until(
        ws,
        t,
        |f| matches!(f, Frame::Json(v) if v["t"] == t && v["id"] == id),
    )
    .json()
}

/// Open `path` from a new server and wait until it loads: the pane's id and the server's URL.
pub fn load(r: &mut Rig, ws: &mut Ws, path: &str) -> (u64, String) {
    let url = serve();
    let id = open(r, &format!("{url}{path}"));
    let title = route(path).1;
    state_until(r, ws, "the page's title", id, |v| {
        v["title"] == title && v["busy"] == false
    });
    (id, url)
}

/// A pointer event on pane `id`'s page, sent as the web app sends it: with no reply.
pub fn mouse(ws: &mut Ws, id: u64, kind: &str, x: f64, y: f64) {
    send(
        ws,
        json!({"act": "browser_mouse", "pane": id, "kind": kind, "x": x, "y": y,
               "button": 0, "buttons": u8::from(kind == "down"), "clicks": 1}),
    );
}

pub fn click(ws: &mut Ws, id: u64, x: f64, y: f64) {
    mouse(ws, id, "down", x, y);
    mouse(ws, id, "up", x, y);
}

/// A letter's press and release, as the web app sends them.
pub fn type_key(ws: &mut Ws, id: u64, c: char) {
    let up = c.to_ascii_uppercase();
    for kind in ["down", "up"] {
        send(
            ws,
            json!({"act": "browser_key", "pane": id, "kind": kind, "key": c.to_string(),
                   "code": format!("Key{up}"), "text": c.to_string(), "key_code": up as u32}),
        );
    }
}

/// A key's press and release with no text, such as `ArrowDown`, as the web app sends them.
pub fn press(ws: &mut Ws, id: u64, key: &str, key_code: u32) {
    for kind in ["down", "up"] {
        send(
            ws,
            json!({"act": "browser_key", "pane": id, "kind": kind, "key": key, "code": key,
                   "key_code": key_code}),
        );
    }
}

/// Pick the element at the point in pane `id` and comment `text` on it, as the web app does.
pub fn comment_at(r: &mut Rig, ws: &mut Ws, id: u64, (x, y): (f64, f64), text: &str) -> Value {
    let acted = r.act(
        ws,
        json!({"act": "browser_pick", "pane": id, "x": x, "y": y}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let done = r.act(
        ws,
        json!({"act": "comment", "pane": id, "line": acted["line"], "text": text}),
    );
    assert_eq!(done["ok"], true, "{done}");
    send(ws, json!({"hold": false}));
    acted
}

/// Open `/keep` and wait for its first load's title: the pane's id and the page's URL.
pub fn keep(r: &mut Rig, ws: &mut Ws) -> (u64, String) {
    let url = format!("{}/keep", serve());
    let id = open(r, &url);
    state_until(r, ws, "the first load", id, |v| v["title"] == "1:");
    (id, url)
}

/// Open `/form` and wait for it to load: the pane's id.
pub fn form(r: &mut Rig, ws: &mut Ws) -> u64 {
    let id = open(r, &format!("{}/form", serve()));
    state_until(r, ws, "the form", id, |v| v["title"] == "Form");
    id
}

/// Laura's browser's pages, `(targetId, url)`, from a second CDP client.
pub fn pages(r: &Rig) -> Vec<(String, String)> {
    let url = r
        .tab
        .browser
        .as_ref()
        .expect("Laura's browser")
        .ws_url
        .clone();
    let (mut cdp, _) = tungstenite::connect(url).expect("a CDP client");
    cdp.send(Message::text(
        json!({"id": 1, "method": "Target.getTargets"}).to_string(),
    ))
    .expect("getTargets");
    loop {
        let Message::Text(t) = cdp.read().expect("a reply") else {
            continue;
        };
        let v: Value = serde_json::from_str(&t).expect("CDP JSON");
        if v["id"] != 1 {
            continue;
        }
        return v["result"]["targetInfos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| t["type"] == "page")
            .map(|t| {
                let s = |k: &str| t[k].as_str().unwrap_or_default().to_string();
                (s("targetId"), s("url"))
            })
            .collect();
    }
}

/// Pump until Laura's browser has one web page, which loaded `url`, and return its target id.
/// Chrome's own start tab, `chrome://newtab/`, doesn't count.
pub fn one_page(r: &mut Rig, ws: &mut Ws, url: &str) -> String {
    let start = Instant::now();
    loop {
        let mut ps = pages(r);
        ps.retain(|(_, u)| browser::is_url(u));
        if let [(id, u)] = ps.as_slice()
            && u == url
        {
            return id.clone();
        }
        assert!(start.elapsed() < common::DEADLINE, "pages: {ps:?}");
        r.idle(ws, 1);
    }
}

/// A GET to Laura's browser at `LAURA_BROWSER_URL`, as the agent's browser tools start: its JSON.
pub fn get(r: &Rig, path: &str) -> Value {
    let port = r.tab.browser_port.expect("a browser port");
    browser::get_json(port, path).expect("Laura's browser answers")
}

/// `/json/list`'s pages, `(title, url)`, sorted.
pub fn listed(r: &Rig) -> Vec<(String, String)> {
    let mut out: Vec<_> = get(r, "/json/list")
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["type"] == "page")
        .map(|t| {
            let s = |k: &str| t[k].as_str().unwrap_or_default().to_string();
            (s("title"), s("url"))
        })
        .collect();
    out.sort();
    out
}

/// The agent's browser tools: a second CDP client from `LAURA_BROWSER_URL`, attached to the page
/// whose URL is `url`.
pub struct Tools {
    ws: Ws,
    session: String,
    next: u64,
}

impl Tools {
    pub fn attach(r: &mut Rig, url: &str) -> Tools {
        let ws_url = get(r, "/json/version")["webSocketDebuggerUrl"]
            .as_str()
            .expect("a websocket URL")
            .to_string();
        let port = r.tab.browser_port.expect("a browser port");
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("Laura's browser listens");
        let (ws, _) = tungstenite::client(ws_url, stream).expect("the tools connect");
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(5)))
            .expect("a timeout");
        let mut t = Tools {
            ws,
            session: String::new(),
            next: 0,
        };
        let start = Instant::now();
        let target = loop {
            let ts = t.browser_call(r, "Target.getTargets", json!({}));
            let found = ts["targetInfos"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|i| i["type"] == "page" && i["url"] == url)
                .map(|i| i["targetId"].clone());
            if let Some(id) = found {
                break id;
            }
            assert!(start.elapsed() < common::DEADLINE, "no page at {url}: {ts}");
        };
        let attached = t.browser_call(
            r,
            "Target.attachToTarget",
            json!({"targetId": target, "flatten": true}),
        );
        t.session = attached["sessionId"].as_str().expect("a session").into();
        t
    }

    pub fn write(&mut self, method: &str, params: Value, session: bool) -> u64 {
        self.next += 1;
        let mut m = json!({"id": self.next, "method": method, "params": params});
        if session {
            m["sessionId"] = self.session.clone().into();
        }
        self.ws.send(Message::text(m.to_string())).expect("send");
        self.next
    }

    /// `method` on the page's session with no wait, as for a click that opens a dialog.
    pub fn send(&mut self, method: &str, params: Value) {
        self.write(method, params, true);
    }

    /// The reply to `id`; Laura's loop runs while the tools wait. The view's frames stay in its
    /// socket for the test's next wait: a frame the tool's action causes can come before the reply.
    pub fn wait(&mut self, r: &mut Rig, id: u64, method: &str) -> Value {
        let start = Instant::now();
        loop {
            match self.ws.read() {
                Ok(Message::Text(t)) => {
                    let v: Value = serde_json::from_str(&t).expect("CDP JSON");
                    if v["id"] == id {
                        assert!(v.get("error").is_none(), "{method}: {v}");
                        return v["result"].clone();
                    }
                }
                Ok(_) => {}
                Err(e) if laura::web::nothing_yet(&e) => {
                    r.pump();
                }
                Err(e) => panic!("{method}: {e}"),
            }
            assert!(start.elapsed() < common::DEADLINE, "timed out: {method}");
        }
    }

    /// `method` on the page's session: its result.
    pub fn call(&mut self, r: &mut Rig, method: &str, params: Value) -> Value {
        let id = self.write(method, params, true);
        self.wait(r, id, method)
    }

    /// `method` on the browser, with no session.
    pub fn browser_call(&mut self, r: &mut Rig, method: &str, params: Value) -> Value {
        let id = self.write(method, params, false);
        self.wait(r, id, method)
    }
}

/// A press and release at the point, as the tools click.
pub fn press_release(x: f64, y: f64) -> [Value; 2] {
    ["mousePressed", "mouseReleased"]
        .map(|typ| json!({"type": typ, "x": x, "y": y, "button": "left", "clickCount": 1}))
}

/// Pane `id` in `laura layout`.
pub fn pane_of(r: &mut Rig, id: u64) -> Value {
    layout(r)["panes"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["id"] == id).cloned())
        .expect("the pane")
}

/// The next mark of the agent's input for pane `id`: its numbers.
pub fn agent(r: &mut Rig, ws: &mut Ws, id: u64) -> Value {
    event(r, ws, "agent", id)
}

/// `v`'s numbers, as `f64`: JSON `10` is not `10.0`.
pub fn floats(v: &Value) -> Vec<f64> {
    v.as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}
