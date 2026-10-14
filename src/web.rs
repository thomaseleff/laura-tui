//! The web app (#88): `laura --web` serves one workspace to a browser tab over a websocket.
//!
//! Threads own the sockets; the loop that owns the `Tab` talks to the active view only through a
//! channel pair in `sync`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, channel, sync_channel,
};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::{Value, json};
use tungstenite::Message;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::{HeaderValue, StatusCode};

use crate::protocol::{PTY_PANE, PaneId};
use crate::{Panel, PanelRow, Rect, Tab, UserAction, view};

/// The error when `LAURA_EXPERIMENTAL_WEB` isn't set.
pub const FLAG_OFF: &str = "the web app is experimental: start Laura with LAURA_EXPERIMENTAL_WEB=1";

/// The workspace's size until a view reports one.
pub const DEFAULT_AREA: Rect = Rect::new(0, 0, 120, 40);

/// The startup gate, as `editor::resolve` reads `LAURA_EXPERIMENTAL_EDITOR`.
pub fn enabled() -> bool {
    std::env::var("LAURA_EXPERIMENTAL_WEB").is_ok_and(|v| !v.is_empty() && v != "0")
}

const REFERRER_POLICY: &str = "no-referrer";
/// `'unsafe-inline'` styles: pane rows carry inline `style`. Script only from Laura's own files,
/// so an escaping bug in pane HTML can't run code that holds the shell's websocket.
const CSP: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'";

/// Browsers send every `localhost` cookie to every port, so headers easily pass 8 KB.
const MAX_HEAD: usize = 64 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);
/// Frames queued for a view's socket; a full queue drops the view, and the browser reconnects.
const OUTBOUND_BOUND: usize = 1024;

/// Path, content type, body. The files hold no workspace data, so they need no token.
const ASSETS: &[(&str, &str, &str)] = &[
    ("/", "text/html", include_str!("../assets/web/index.html")),
    (
        "/app.js",
        "text/javascript",
        include_str!("../assets/web/app.js"),
    ),
    (
        "/style.css",
        "text/css",
        include_str!("../assets/web/style.css"),
    ),
    (
        "/xterm.js",
        "text/javascript",
        include_str!("../assets/web/xterm.js"),
    ),
    (
        "/xterm.css",
        "text/css",
        include_str!("../assets/web/xterm.css"),
    ),
    (
        "/addon-webgl.js",
        "text/javascript",
        include_str!("../assets/web/addon-webgl.js"),
    ),
    (
        "/addon-unicode11.js",
        "text/javascript",
        include_str!("../assets/web/addon-unicode11.js"),
    ),
    (
        "/idiomorph.js",
        "text/javascript",
        include_str!("../assets/web/idiomorph.js"),
    ),
];

enum Inbound {
    Text(String),
    Bytes(Vec<u8>),
}

enum Outbound {
    Text(String),
    Bytes(Vec<u8>),
}

/// One authenticated view: its frames in, Laura's frames out.
type Conn = (Receiver<Inbound>, SyncSender<Outbound>);

/// The active view: its channels, its PTY streams, and what Laura last sent it.
struct View {
    rx: Receiver<Inbound>,
    tx: SyncSender<Outbound>,
    /// Set when `tx` is full or closed: `sync` drops the view.
    dead: bool,
    /// Every PTY the view streams: the shell and each editor pane's Neovim.
    ptys: HashMap<PaneId, Receiver<Vec<u8>>>,
    layout: String,
    /// Each file-rendered pane's rows and cursor row, for `rows` diffs.
    panes: HashMap<PaneId, (Vec<String>, Option<usize>)>,
    focus: PaneId,
    area: Rect,
    git_shown: bool,
    frozen: String,
}

impl View {
    fn send(&mut self, o: Outbound) {
        if !self.dead && self.tx.try_send(o).is_err() {
            self.dead = true;
        }
    }

    fn text(&mut self, v: Value) {
        self.send(Outbound::Text(v.to_string()));
    }
}

/// The web app's server for one workspace. The loop that owns the `Tab` calls `sync` every frame.
pub struct WebServer {
    url: String,
    conns: Receiver<Conn>,
    view: Option<View>,
    area: Option<Rect>,
}

impl WebServer {
    /// Bind `127.0.0.1` on a port the OS picks, mint a token, start accepting.
    pub fn bind() -> Result<WebServer> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| anyhow!("no randomness for the token: {e}"))?;
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let url = format!("http://127.0.0.1:{port}/?t={token}");
        let (join, conns) = channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (join, token) = (join.clone(), token.clone());
                // A thread per connection: a socket that sends nothing can't block the rest.
                std::thread::spawn(move || {
                    let _ = serve(stream, &token, port, &join);
                });
            }
        });
        Ok(WebServer {
            url,
            conns,
            view: None,
            area: None,
        })
    }

    /// The URL to print: the bound address plus the token.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The last view's workspace size in cells, kept after the view disconnects so the shell
    /// doesn't resize when a tab closes; `None` until a view first reports one.
    pub fn area(&self) -> Option<Rect> {
        self.area
    }

    /// Take over from the old view if a new one connected, apply the view's queued user actions,
    /// forward PTY bytes, and send the panes whose HTML changed. `changed` is `drain`'s return
    /// this frame. Returns false once the user quits.
    pub fn sync(&mut self, tab: &mut Tab, changed: bool) -> bool {
        while let Ok((rx, tx)) = self.conns.try_recv() {
            if let Some(old) = self.view.take() {
                // A takeover drops the old view's draft hold.
                tab.hold = false;
                let _ = old
                    .tx
                    .try_send(Outbound::Text(json!({"t": "moved"}).to_string()));
            }
            let mut v = View {
                rx,
                tx,
                dead: false,
                ptys: HashMap::new(),
                layout: String::new(),
                panes: HashMap::new(),
                focus: tab.focus,
                area: Rect::default(),
                git_shown: false,
                frozen: String::new(),
            };
            let help: String = view::help_lines(tab.web).iter().map(line_html).collect();
            v.text(json!({"t": "strings", "help": help, "moved": view::MOVED,
                           "ended": view::ENDED,
                           "draft": {"comment": view::DRAFT_COMMENT, "review": view::DRAFT_REVIEW,
                                     "failed": crate::tab::SUBMIT_FAILED}}));
            if let Err(e) = &tab.editor
                && e == crate::editor::NO_NVIM
            {
                v.text(json!({"t": "notice", "text": format!("⚠ {e}")}));
            }
            self.view = Some(v);
        }
        let Some(v) = self.view.as_mut() else {
            return true;
        };

        let (mut quit, mut acted) = (false, false);
        loop {
            match v.rx.try_recv() {
                Ok(Inbound::Text(t)) => {
                    let Ok(msg) = serde_json::from_str::<Value>(&t) else {
                        continue;
                    };
                    if msg.get("act").is_some() {
                        let res = serde_json::from_value::<UserAction>(msg)
                            .map_err(|e| e.to_string())
                            .and_then(|a| {
                                let comment = match a {
                                    UserAction::BeginComment { pane } => Some(pane),
                                    _ => None,
                                };
                                let review = matches!(a, UserAction::BeginReview { .. });
                                let seed = tab.act(a)?;
                                // Laura holds the agent from here, before any drain, until the
                                // browser closes the draft.
                                tab.hold |= comment.is_some() || review;
                                let line = comment
                                    .and_then(|p| tab.panels.get(&p).map(Panel::cursor_line));
                                let mode = match line {
                                    Some(line) => Some(view::Hint::Comment {
                                        line,
                                        edit: seed.as_ref().is_some_and(|s| !s.is_empty()),
                                    }),
                                    None => review.then_some(view::Hint::Review),
                                };
                                Ok((seed, line, mode.map(|m| view::hint(tab, m))))
                            });
                        acted |= res.is_ok();
                        v.text(match res {
                            Ok((seed, line, hint)) => {
                                json!({"t": "acted", "ok": true, "text": seed, "line": line, "hint": hint})
                            }
                            Err(e) => json!({"t": "acted", "ok": false, "text": e}),
                        });
                    } else if let Some(size) = msg.get("view") {
                        // A minimized window or a hostile frame can't size the shell to 0.
                        let dim = |k: &str, lo: u64, hi: u64| {
                            size.get(k)
                                .and_then(Value::as_u64)
                                .map(|n| n.clamp(lo, hi) as u16)
                        };
                        if let (Some(cols), Some(rows)) =
                            (dim("cols", 20, 1000), dim("rows", 5, 500))
                        {
                            self.area = Some(Rect::new(0, 0, cols, rows));
                        }
                    } else if let Some(hold) = msg.get("hold").and_then(Value::as_bool) {
                        tab.hold = hold;
                    } else if msg.get("quit") == Some(&Value::Bool(true)) {
                        // Again, after the user's `y`: edits made since still block it.
                        match tab.act(UserAction::Quit) {
                            Ok(_) => quit = true,
                            Err(e) => v.text(json!({"t": "notice", "text": e})),
                        }
                    }
                }
                Ok(Inbound::Bytes(b)) => {
                    if let Some((id, bytes)) = b.split_first_chunk::<8>() {
                        let pane = u64::from_be_bytes(*id);
                        let _ = tab.act(UserAction::Input {
                            pane,
                            bytes: bytes.to_vec(),
                        });
                    }
                }
                Err(TryRecvError::Empty) => break,
                // The socket died (a closed tab): keep the workspace and its size.
                Err(TryRecvError::Disconnected) => {
                    self.view = None;
                    tab.hold = false;
                    return !quit;
                }
            }
        }

        let area = self.area.unwrap_or(DEFAULT_AREA);
        let rects = crate::rects(&tab.layout, area);
        let layout = layout_json(tab, &rects, area);
        if v.layout != layout {
            v.send(Outbound::Text(layout.clone()));
            v.layout = layout;
        }

        // After `layout`, so the browser has a pane's xterm before its first byte.
        v.ptys
            .retain(|id, _| *id == PTY_PANE || tab.editors.contains_key(id));
        let ptys = std::iter::once((PTY_PANE, &tab.pty))
            .chain(tab.editors.iter().map(|(id, e)| (*id, &e.pty)));
        for (id, pty) in ptys {
            if !v.ptys.contains_key(&id) {
                let (replay, rx) = pty.subscribe();
                v.send(Outbound::Bytes(framed(id, replay)));
                v.ptys.insert(id, rx);
            }
        }
        let mut chunks = vec![];
        for (&id, rx) in &v.ptys {
            loop {
                match rx.try_recv() {
                    Ok(b) => chunks.push(framed(id, b)),
                    Err(TryRecvError::Empty) => break,
                    // The reader dropped a full subscription: a fresh replay is the only way back.
                    Err(TryRecvError::Disconnected) => {
                        v.dead = true;
                        break;
                    }
                }
            }
        }
        for c in chunks {
            v.send(Outbound::Bytes(c));
        }

        if changed || acted || v.focus != tab.focus || v.area != area {
            send_panes(v, tab, &rects);
            (v.focus, v.area) = (tab.focus, area);
        }

        if !v.git_shown && crate::gitdiff::git_missing() {
            v.git_shown = true;
            v.text(json!({"t": "notice", "text": view::NO_GIT}));
        }
        // The browser shows the frozen notice whenever no other notice is up; empty clears it.
        let frozen = tab
            .focused_panel()
            .filter(|p| p.source_changed)
            .map(|p| view::frozen_notice(view::base_name(&p.path), area.width))
            .unwrap_or_default();
        if v.frozen != frozen {
            v.text(json!({"t": "notice", "text": frozen, "frozen": true}));
            v.frozen = frozen;
        }

        if v.dead {
            self.view = None;
            tab.hold = false;
        }
        !quit
    }

    /// Send `{"t":"exit"}` and wait (briefly) for the connection thread to write it.
    pub fn close(mut self) {
        let Some(View { rx, tx, .. }) = self.view.take() else {
            return;
        };
        let _ = tx.try_send(Outbound::Text(json!({"t": "exit"}).to_string()));
        drop(tx);
        let deadline = Instant::now() + Duration::from_secs(1);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

/// A PTY chunk for the browser: the pane id as 8 big-endian bytes, then the bytes.
fn framed(id: PaneId, bytes: Vec<u8>) -> Vec<u8> {
    let mut out = id.to_be_bytes().to_vec();
    out.extend(bytes);
    out
}

fn layout_json(tab: &Tab, rects: &HashMap<PaneId, Rect>, area: Rect) -> String {
    let mut ids: Vec<_> = rects.keys().copied().collect();
    ids.sort();
    let panes: Vec<Value> = ids
        .iter()
        .map(|id| {
            let r = rects[id];
            let editor = tab.editors.get(id);
            let (kind, title, frozen, threads) = match tab.panels.get(id) {
                Some(p) => {
                    let title: String = view::pane_title(*id, p, editor)
                        .spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect();
                    let kind = if editor.is_some_and(|e| e.editor_view) {
                        "editor"
                    } else {
                        "file"
                    };
                    (kind, title, p.source_changed, p.thread_count())
                }
                None => ("shell", format!("#{id} shell"), false, 0),
            };
            // `pty`: the pane streams a terminal, in either view, so the browser keeps its xterm.
            let pty = *id == PTY_PANE || editor.is_some();
            json!({"id": id, "x": r.x, "y": r.y, "w": r.width, "h": r.height,
                   "kind": kind, "pty": pty, "title": title, "frozen": frozen,
                   "threads": threads})
        })
        .collect();
    let hints = json!({
        "pane": view::hint(tab, view::Hint::Pane),
        "locked": view::hint(tab, view::Hint::Locked),
        "quit": view::hint(tab, view::Hint::Quit),
        "panes": view::hint(tab, view::Hint::Panes),
    });
    json!({"t": "layout", "cols": area.width, "rows": area.height, "focus": tab.focus,
           "hints": hints, "panes": panes})
    .to_string()
}

/// Send each file-rendered pane's rows that changed since the view last saw them: `rows` when the
/// row count holds, else the whole `pane`.
fn send_panes(v: &mut View, tab: &Tab, rects: &HashMap<PaneId, Rect>) {
    let shown: Vec<(PaneId, &Panel, Rect)> = rects
        .iter()
        .filter(|(id, _)| !tab.editors.get(id).is_some_and(|e| e.editor_view))
        .filter_map(|(id, r)| tab.panels.get(id).map(|p| (*id, p, *r)))
        .collect();
    v.panes.retain(|id, _| shown.iter().any(|(s, ..)| s == id));
    for (id, panel, r) in shown {
        let width = r.width.saturating_sub(2) as usize;
        let layout = panel.layout(width);
        let lines = view::panel_lines(panel, &layout, tab.focus == id, width);
        let rows: Vec<String> = layout
            .rows
            .iter()
            .zip(&lines)
            .map(|(row, line)| row_html(panel, row, line))
            .collect();
        // The row `panel_lines` draws the band on.
        let cursor = layout
            .rows
            .iter()
            .position(|r| !r.comment && r.line == panel.cursor);
        let recenter = panel.take_recenter();
        let msg = match v.panes.get(&id) {
            Some((old, old_cursor)) if old.len() == rows.len() => {
                let diff: Vec<Value> = (0..rows.len())
                    .filter(|&i| old[i] != rows[i])
                    .map(|i| json!([i, rows[i]]))
                    .collect();
                if diff.is_empty() && *old_cursor == cursor && !recenter {
                    continue;
                }
                json!({"t": "rows", "id": id, "rows": diff, "cursor": cursor, "recenter": recenter})
            }
            _ => json!({"t": "pane", "id": id, "html": rows.concat(), "cursor": cursor,
                        "recenter": recenter}),
        };
        v.text(msg);
        v.panes.insert(id, (rows, cursor));
    }
}

/// One layout row. `data-line` is the 1-based source line the browser sends back; `data-row` the
/// rendered line, which copy joins on. The gutter can't be selected. `hl` marks a highlighted row,
/// which the browser scrolls to on `recenter`.
fn row_html(panel: &Panel, row: &PanelRow, line: &Line) -> String {
    let highlighted = !row.comment
        && panel
            .highlight
            .is_some_and(|(lo, hi)| (lo..=hi).contains(&row.line));
    let mut h = String::with_capacity(160);
    let _ = write!(
        h,
        r#"<div class="row{}" data-line="{}" data-row="{}"{}><span class="g">"#,
        if highlighted { " hl" } else { "" },
        panel.row_source_line(row),
        row.line,
        style_attr(line.style)
    );
    // `panel_lines`' gutter: 6 spans, 2 on a comment row.
    let gutter = (if row.comment { 2 } else { 6 }).min(line.spans.len());
    spans_html(&mut h, &line.spans[..gutter]);
    h.push_str("</span>");
    spans_html(&mut h, &line.spans[gutter..]);
    h.push_str("</div>");
    h
}

fn line_html(line: &Line) -> String {
    let mut h = format!("<div{}>", style_attr(line.style));
    spans_html(&mut h, &line.spans);
    h.push_str("</div>");
    h
}

fn spans_html(h: &mut String, spans: &[Span]) {
    for s in spans {
        let _ = write!(h, "<span{}>", style_attr(s.style));
        escape(h, &s.content);
        h.push_str("</span>");
    }
}

fn escape(h: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => h.push_str("&amp;"),
            '<' => h.push_str("&lt;"),
            '>' => h.push_str("&gt;"),
            '"' => h.push_str("&quot;"),
            c => h.push(c),
        }
    }
}

fn style_attr(st: Style) -> String {
    let m = st.add_modifier;
    let (mut fg, mut bg) = (st.fg.and_then(color), st.bg.and_then(color));
    if m.contains(Modifier::REVERSED) {
        (fg, bg) = (
            Some(bg.unwrap_or("var(--bg)".into())),
            Some(fg.unwrap_or("var(--fg)".into())),
        );
    }
    let mut css = String::new();
    if let Some(c) = fg {
        let _ = write!(css, "color:{c};");
    }
    if let Some(c) = bg {
        let _ = write!(css, "background:{c};");
    }
    for (flag, rule) in [
        (Modifier::BOLD, "font-weight:bold;"),
        (Modifier::DIM, "opacity:.5;"),
        (Modifier::ITALIC, "font-style:italic;"),
        (Modifier::UNDERLINED, "text-decoration:underline;"),
    ] {
        if m.contains(flag) {
            css.push_str(rule);
        }
    }
    if css.is_empty() {
        css
    } else {
        format!(r#" style="{css}""#)
    }
}

/// RGB as hex; the 16 ANSI colors as `style.css`'s palette.
fn color(c: Color) -> Option<String> {
    let ansi = |i: u8| Some(format!("var(--ansi-{i})"));
    match c {
        Color::Reset => None,
        Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
        Color::Indexed(i) if i < 16 => ansi(i),
        // ponytail: Laura's own styles use RGB or the 16 colors; map the 256-color cube if one appears.
        Color::Indexed(_) => None,
        Color::Black => ansi(0),
        Color::Red => ansi(1),
        Color::Green => ansi(2),
        Color::Yellow => ansi(3),
        Color::Blue => ansi(4),
        Color::Magenta => ansi(5),
        Color::Cyan => ansi(6),
        Color::Gray => ansi(7),
        Color::DarkGray => ansi(8),
        Color::LightRed => ansi(9),
        Color::LightGreen => ansi(10),
        Color::LightYellow => ansi(11),
        Color::LightBlue => ansi(12),
        Color::LightMagenta => ansi(13),
        Color::LightCyan => ansi(14),
        Color::White => ansi(15),
    }
}

/// Constant-time, so the token can't be guessed a byte at a time.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// One connection: a static file, or the websocket view.
fn serve(mut stream: TcpStream, token: &str, port: u16, join: &Sender<Conn>) -> Result<()> {
    stream.set_read_timeout(Some(HEAD_TIMEOUT))?;
    // Peek, so tungstenite can still read the head for `/ws`.
    let deadline = Instant::now() + HEAD_TIMEOUT;
    let mut buf = vec![0u8; MAX_HEAD];
    let head = loop {
        let n = stream.peek(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        if let Some(i) = buf[..n].windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if n == buf.len() {
            return respond(&mut stream, "431 Request Header Fields Too Large", None);
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let text = String::from_utf8_lossy(&buf[..head]);
    let target = text.split(' ').nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/").to_string();
    if path == "/ws" {
        return view_conn(stream, token, port, join);
    }
    stream.read_exact(&mut buf[..head])?;
    match ASSETS.iter().find(|(p, ..)| *p == path) {
        Some((_, mime, body)) => respond(&mut stream, "200 OK", Some((mime, body))),
        None => respond(&mut stream, "404 Not Found", None),
    }
}

fn respond(stream: &mut TcpStream, status: &str, body: Option<(&str, &str)>) -> Result<()> {
    let (mime, body) = body.unwrap_or(("text/plain", ""));
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: {REFERRER_POLICY}\r\n\
         Content-Security-Policy: {CSP}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(())
}

/// The websocket: check the token and `Origin`, hand the view to `sync`, then shuttle frames.
#[allow(clippy::result_large_err)] // tungstenite's handshake `Callback` fixes `check`'s signature.
fn view_conn(stream: TcpStream, token: &str, port: u16, join: &Sender<Conn>) -> Result<()> {
    let check = |req: &Request, mut resp: Response| -> Result<Response, ErrorResponse> {
        let given = req
            .uri()
            .query()
            .unwrap_or("")
            .split('&')
            .find_map(|kv| kv.strip_prefix("t="))
            .unwrap_or("");
        // Check Origin against our own address: under DNS rebinding, Host carries the attacker's domain.
        let origin = req.headers().get("origin").and_then(|o| o.to_str().ok());
        let ours = |o: &str| {
            o == format!("http://127.0.0.1:{port}") || o == format!("http://localhost:{port}")
        };
        let status = if !same(given.as_bytes(), token.as_bytes()) {
            Some(StatusCode::UNAUTHORIZED)
        } else if !origin.is_some_and(ours) {
            Some(StatusCode::FORBIDDEN)
        } else {
            None
        };
        let headers = [
            ("referrer-policy", REFERRER_POLICY),
            ("content-security-policy", CSP),
        ];
        match status {
            Some(code) => {
                let mut err = ErrorResponse::new(None);
                *err.status_mut() = code;
                for (k, v) in headers {
                    err.headers_mut().insert(k, HeaderValue::from_static(v));
                }
                Err(err)
            }
            None => {
                for (k, v) in headers {
                    resp.headers_mut().insert(k, HeaderValue::from_static(v));
                }
                Ok(resp)
            }
        }
    };
    let mut ws = tungstenite::accept_hdr(stream, check).map_err(|e| anyhow!("handshake: {e}"))?;
    // ponytail: a polling read per view (one view at a time); split reader and writer if it lags.
    ws.get_ref()
        .set_read_timeout(Some(Duration::from_millis(16)))?;
    let (in_tx, in_rx) = channel();
    let (out_tx, out_rx) = sync_channel(OUTBOUND_BOUND);
    if join.send((in_rx, out_tx)).is_err() {
        return Ok(());
    }
    loop {
        let frame = match ws.read() {
            Ok(Message::Text(t)) => Some(Inbound::Text(t.to_string())),
            Ok(Message::Binary(b)) => Some(Inbound::Bytes(b.to_vec())),
            Ok(_) => None,
            // A timed-out read is `TimedOut` on Windows and `WouldBlock` on Unix: no frame yet.
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                None
            }
            Err(_) => return Ok(()),
        };
        if let Some(f) = frame
            && in_tx.send(f).is_err()
        {
            return Ok(());
        }
        loop {
            match out_rx.try_recv() {
                Ok(Outbound::Text(s)) => ws.write(Message::text(s))?,
                Ok(Outbound::Bytes(b)) => ws.write(Message::binary(b))?,
                Err(TryRecvError::Empty) => break,
                // `sync` dropped the view: a takeover, a full queue, or the workspace ended.
                Err(TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return Ok(());
                }
            }
        }
        ws.flush()?;
    }
}
