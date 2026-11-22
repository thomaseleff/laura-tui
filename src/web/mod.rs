//! The web app (#88): `laura` serves one workspace to a browser tab over a websocket.
//!
//! Threads own the sockets; the loop that owns the `Tab` talks to the active view only through a
//! channel pair in `sync`.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, channel, sync_channel,
};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use tungstenite::Message;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::{HeaderValue, StatusCode};

use crate::http::{
    CSP, REFERRER_POLICY, foreign_subresource, listen, nonce, nothing_yet, our_origin, pct_decode,
    pct_encode, peek_head, respond, same,
};
use crate::panel::cursor::{cursor_block, on_card, on_stop, step, toggle_block_threads};
use crate::panel::is_page;
use crate::platform::{redirect_page, shown_root, write_private};
use crate::protocol::{PTY_PANE, PaneId};
use crate::serve::{live, record_path, workspaces_dir};
use crate::{Panel, Rect, Tab, UserAction, view};

mod frames;
mod html;
use frames::{layout_json, send_browsers, send_panes};
use html::tag_lines;

/// The workspace's size until a view reports one.
pub const DEFAULT_AREA: Rect = Rect::new(0, 0, 120, 40);

/// Frames queued for a view's socket; a full queue drops the view, and the browser reconnects.
const OUTBOUND_BOUND: usize = 1024;

/// Path, content type, body. The files hold no workspace data, so they need no token.
pub(crate) const ASSETS: &[(&str, &str, &[u8])] = &[
    (
        "/",
        "text/html",
        include_bytes!("../../assets/web/index.html"),
    ),
    (
        "/app.js",
        "text/javascript",
        include_bytes!("../../assets/web/app.js"),
    ),
    (
        "/style.css",
        "text/css",
        include_bytes!("../../assets/web/style.css"),
    ),
    (
        "/xterm.js",
        "text/javascript",
        include_bytes!("../../assets/web/xterm.js"),
    ),
    (
        "/xterm.css",
        "text/css",
        include_bytes!("../../assets/web/xterm.css"),
    ),
    (
        "/addon-webgl.js",
        "text/javascript",
        include_bytes!("../../assets/web/addon-webgl.js"),
    ),
    (
        "/addon-unicode11.js",
        "text/javascript",
        include_bytes!("../../assets/web/addon-unicode11.js"),
    ),
    (
        "/idiomorph.js",
        "text/javascript",
        include_bytes!("../../assets/web/idiomorph.js"),
    ),
    (
        "/fonts/Inter.woff2",
        "font/woff2",
        include_bytes!("../../assets/web/fonts/Inter.woff2"),
    ),
    (
        "/fonts/JetBrainsMono.woff2",
        "font/woff2",
        include_bytes!("../../assets/web/fonts/JetBrainsMono.woff2"),
    ),
    // The top bar's logo: Pebble, the Laura logo. The page's icon is the same mark, white in
    // dark mode.
    (
        "/logo.svg",
        "image/svg+xml",
        include_bytes!("../../assets/pebble.svg"),
    ),
    (
        "/favicon.svg",
        "image/svg+xml",
        include_bytes!("../../assets/favicon.svg"),
    ),
];

/// The image types the file route serves, by lowercased extension.
const IMAGES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("svg", "image/svg+xml"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("ico", "image/x-icon"),
];

/// What else an HTML page loads from its folder, by lowercased extension.
const PAGE_FILES: &[(&str, &str)] = &[
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("mjs", "text/javascript"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
];

/// What one nonce serves: a folder, and for an HTML pane, the page's file name and tagged HTML.
struct Served {
    root: PathBuf,
    page: Option<(String, String)>,
}

/// The file route's nonces, shared by `sync` and the connection threads.
type Files = Arc<Mutex<HashMap<String, Served>>>;

/// Each routed pane's path, its file route's nonce and its URL: the file's folder for markdown,
/// the page for HTML.
type Bases = HashMap<PaneId, (String, String, String)>;

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

/// A file-rendered pane's last frame: its rows, cursor row, file (path and content hash) and threads,
/// for `rows` diffs and reloads.
type Sent = (Vec<String>, Option<usize>, (String, u64), String);

/// The active view: its channels, its PTY streams, and what Laura last sent it.
struct View {
    rx: Receiver<Inbound>,
    tx: SyncSender<Outbound>,
    /// Set when `tx` is full or closed: `sync` drops the view.
    dead: bool,
    /// Every PTY the view streams: the shell and each editor pane's Neovim.
    ptys: HashMap<PaneId, Receiver<Vec<u8>>>,
    layout: String,
    panes: HashMap<PaneId, Sent>,
    /// Each HTML pane's last `page` frame.
    pages: HashMap<PaneId, String>,
    /// Each browser pane's last `browser` frame.
    browsers: HashMap<PaneId, String>,
    /// The browser panes the view holds a frame of.
    framed: HashSet<PaneId>,
    focus: PaneId,
    area: Rect,
    git_shown: bool,
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
    /// The page from `open_page`, until a view connects.
    page: Option<PathBuf>,
    conns: Receiver<Conn>,
    view: Option<View>,
    area: Option<Rect>,
    /// The folder Laura started in, canonical.
    root: PathBuf,
    files: Files,
    bases: Bases,
    /// When the workspace started, in Unix ms: of two records on one port, the newer one wins.
    started: u64,
    /// When the user last showed this workspace's browser tab, in Unix ms: the list's order.
    visited: u64,
    /// The `workspaces` frames from `watch`'s thread, the last one, which a new view gets, and
    /// the poke that has the thread read the records at once (a view connects or sends a visit).
    watch: Option<Receiver<String>>,
    poke: Option<Sender<()>>,
    workspaces: Option<String>,
    /// The drawer's height in % of the workspace's, from the settings at the start.
    drawer: u8,
}

impl WebServer {
    /// Bind `127.0.0.1` on `port` (`0`: a port the OS picks), mint a token, start accepting.
    pub fn bind(port: u16) -> Result<WebServer> {
        let listener = listen(port)?;
        let port = listener.local_addr()?.port();
        let token = nonce(32)?;
        let url = format!("http://127.0.0.1:{port}/?t={token}");
        let root = std::env::current_dir()?.canonicalize()?;
        let files = Files::default();
        let (join, conns) = channel();
        let shared = files.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (join, token, files) = (join.clone(), token.clone(), shared.clone());
                // A thread per connection: a socket that sends nothing can't block the rest.
                std::thread::spawn(move || {
                    let _ = serve(stream, &token, port, &join, &files);
                });
            }
        });
        let started = now_ms();
        Ok(WebServer {
            url,
            page: None,
            conns,
            view: None,
            area: None,
            root,
            files,
            bases: HashMap::new(),
            started,
            visited: started,
            watch: None,
            poke: None,
            workspaces: None,
            drawer: crate::settings::get("drawer")
                .as_ref()
                .and_then(Value::as_f64)
                .map_or(DRAWER, drawer_pct),
        })
    }

    /// When the user last showed this workspace's browser tab, for `register`'s caller.
    pub fn visited(&self) -> u64 {
        self.visited
    }

    /// Write this workspace's record, which the Laura server and the other workspaces list. The
    /// record holds the token, so only the user can read it.
    pub fn register(&self, title: Option<&str>, waiting: bool, program: &str) -> Result<()> {
        let dir = workspaces_dir();
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(&dir)?;
        let root = crate::platform::strip_verbatim(self.root.clone());
        let record = json!({"url": self.url, "root": root.to_string_lossy(), "pid": std::process::id(),
                            "title": title, "waiting": waiting, "program": program,
                            "started": self.started, "visited": self.visited});
        write_private(
            &record_path(std::process::id()),
            record.to_string().as_bytes(),
        )
    }

    /// Tell the view which workspaces run, and whether a Laura server runs: a thread reads the
    /// records every 2 s, and at once when a view connects or sends a visit, and `sync` sends the `workspaces` frame when the list changes. The view
    /// then holds the other workspaces' tokens: the same user, the same files on disk.
    pub fn watch(&mut self) {
        let (tx, rx) = channel();
        let (poke, poked) = channel();
        let url = self.url.clone();
        std::thread::spawn(move || {
            let mut last = String::new();
            loop {
                let mut list: Vec<Value> = live()
                    .iter()
                    .map(|w| {
                        let root = shown_root(Path::new(w["root"].as_str().unwrap_or("")));
                        json!({"url": w["url"], "title": w["title"], "root": root,
                               "program": w["program"], "waiting": w["waiting"],
                               "current": w["url"] == url.as_str()})
                    })
                    .collect();
                // This workspace first; the rest keep `live`'s order (stable sort).
                list.sort_by_key(|w| w["current"] != true);
                let frame = json!({"t": "workspaces", "list": list,
                                   "server": crate::serve::running()})
                .to_string();
                if frame != last {
                    if tx.send(frame.clone()).is_err() {
                        return; // the workspace ended
                    }
                    last = frame;
                }
                if let Err(RecvTimeoutError::Disconnected) =
                    poked.recv_timeout(Duration::from_secs(2))
                {
                    return; // the workspace ended
                }
            }
        });
        self.watch = Some(rx);
        self.poke = Some(poke);
    }

    /// Delete this workspace's record. A workspace that ends without it (`Ctrl+C`, a crash) leaves
    /// the record, which `live` deletes.
    pub fn unregister(&self) {
        let _ = std::fs::remove_file(record_path(std::process::id()));
    }

    /// The URL to print: the bound address plus the token.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The page that opens this workspace in the user's browser ([`redirect_page`]).
    pub fn open_page(&mut self, dir: &Path) -> Result<PathBuf> {
        let path = redirect_page(dir, &self.url)?;
        self.page = Some(path.clone());
        Ok(path)
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
        let fresh = self.watch.as_ref().and_then(|w| w.try_iter().last());
        if let Some(frame) = fresh {
            if let Some(v) = self.view.as_mut() {
                v.send(Outbound::Text(frame.clone()));
            }
            self.workspaces = Some(frame);
        }
        while let Ok((rx, tx)) = self.conns.try_recv() {
            // The token is in the browser now: the page has done its job.
            if let Some(page) = self.page.take() {
                let _ = std::fs::remove_file(page);
            }
            if let Some(old) = self.view.take() {
                // A takeover drops the old view's draft hold.
                tab.hold = false;
                let _ = old
                    .tx
                    .try_send(Outbound::Text(json!({"t": "moved"}).to_string()));
            }
            // A new view starts with comment mode off, so Laura's browser turns it off too.
            for page in tab.browsers.values_mut() {
                page.comment = false;
                page.new_view();
            }
            let mut v = View {
                rx,
                tx,
                dead: false,
                ptys: HashMap::new(),
                layout: String::new(),
                panes: HashMap::new(),
                pages: HashMap::new(),
                browsers: HashMap::new(),
                framed: HashSet::new(),
                focus: tab.focus,
                area: Rect::default(),
                git_shown: false,
            };
            let help = view::help(tab.web);
            v.text(json!({"t": "strings", "help": help, "help_note": view::HELP_NOTE, "moved": view::MOVED,
                           "ended": view::ENDED, "root": shown_root(&self.root),
                           "draft": {"failed": crate::tab::SUBMIT_FAILED_WEB},
                           "narrow": "This pane is too small to show {name}. Make the window larger or close a pane."}));
            if let Some(frame) = &self.workspaces {
                v.send(Outbound::Text(frame.clone()));
            }
            // The kept frame can miss a visit to another workspace in the last 2 s.
            if let Some(p) = &self.poke {
                let _ = p.send(());
            }
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
                        let res = match serde_json::from_value::<UserAction>(msg) {
                            // Input to a page changes only its frames: no reply, no layout.
                            Ok(
                                a @ (UserAction::BrowserMouse { .. }
                                | UserAction::BrowserKey { .. }
                                | UserAction::BrowserText { .. }
                                | UserAction::BrowserBlur { .. }
                                | UserAction::BrowserCommentMode { .. }
                                | UserAction::BrowserReveal { .. }),
                            ) => {
                                let _ = tab.act(a);
                                continue;
                            }
                            r => r.map_err(|e| e.to_string()),
                        };
                        let res = res.and_then(|a| {
                            let comment = match a {
                                UserAction::BeginComment { pane }
                                | UserAction::BrowserPick { pane, .. } => Some(pane),
                                _ => None,
                            };
                            let review = matches!(a, UserAction::BeginReview { .. });
                            // A new comment goes on the block's first line; on a stop, on that
                            // line; on a thread's card, it joins that thread.
                            if let Some(pane) = comment
                                && let Ok(p) = tab.panel_mut(pane)
                                && let Some(start) = cursor_block(p)
                                    .filter(|b| !on_card(p, b) && !on_stop(p, b))
                                    .map(|b| b.rows.start)
                            {
                                p.cursor = start;
                            }
                            let handled = match a {
                                UserAction::MoveCursor { pane, delta } => {
                                    step(tab.panel_mut(pane)?, delta)
                                }
                                UserAction::ToggleThread { pane, line: None } => {
                                    toggle_block_threads(tab.panel_mut(pane)?)
                                }
                                UserAction::DrawerHeight { percent } => {
                                    self.drawer = save_drawer(percent);
                                    // The web app showed the user's height before Laura took it:
                                    // send the layout even when the height ends where it was, after
                                    // a step and a reset in one loop.
                                    v.layout.clear();
                                    true
                                }
                                _ => false,
                            };
                            let seed = if handled { None } else { tab.act(a)? };
                            // Laura holds the agent from here, before any drain, until the
                            // browser closes the draft.
                            tab.hold |= comment.is_some() || review;
                            let panel = comment.and_then(|p| tab.panels.get(&p));
                            let line = panel.map(Panel::cursor_line);
                            // A browser pane's comment names its element.
                            let anchor = panel.and_then(|p| p.anchors.get(p.cursor));
                            let mode = match line {
                                Some(line) => Some(view::Hint::Comment {
                                    at: anchor.map_or_else(
                                        || format!("L{line}"),
                                        |a| view::cut_left(&a.selector, 40),
                                    ),
                                    edit: seed.as_ref().is_some_and(|s| !s.is_empty()),
                                }),
                                None => review.then_some(view::Hint::Review),
                            };
                            let anchor = anchor.map(|a| [a.selector.clone(), a.quote.clone()]);
                            Ok((seed, line, anchor, mode.map(|m| view::hint(tab, m))))
                        });
                        acted |= res.is_ok();
                        v.text(match res {
                            Ok((seed, line, anchor, hint)) => {
                                json!({"t": "acted", "ok": true, "text": seed, "line": line,
                                       "anchor": anchor, "hint": hint})
                            }
                            Err(e) => {
                                let e = if e == crate::tab::SUBMIT_FAILED {
                                    crate::tab::SUBMIT_FAILED_WEB.to_string()
                                } else {
                                    e
                                };
                                json!({"t": "acted", "ok": false, "text": e})
                            }
                        });
                    } else if msg.get("visit") == Some(&Value::Bool(true)) {
                        self.visited = now_ms();
                        // Back from another tab: that workspace's visit can be newer than the kept frame.
                        if let Some(p) = &self.poke {
                            let _ = p.send(());
                        }
                    } else if let Some((cols, rows)) = msg.get("view").and_then(cells) {
                        self.area = Some(Rect::new(0, 0, cols, rows));
                    } else if let Some((cols, rows)) = msg.get("shell").and_then(cells) {
                        tab.shell_size = Some((rows, cols));
                    } else if let Some(size) = msg.get("size") {
                        // A browser pane's size in CSS pixels, and the screen's pixel ratio.
                        let num = |k: &str| size.get(k).and_then(Value::as_f64);
                        if let (Some(id), Some(w), Some(h), Some(dpr)) =
                            (num("id"), num("w"), num("h"), num("dpr"))
                        {
                            let id = id as PaneId;
                            let size = (
                                w.clamp(50.0, 4000.0) as u32,
                                h.clamp(50.0, 4000.0) as u32,
                                dpr.clamp(0.5, 4.0),
                            );
                            if tab.panels.get(&id).is_some_and(|p| p.browser) {
                                tab.browser_sizes.insert(id, size);
                            }
                            if let Some(page) = tab.browsers.get_mut(&id) {
                                page.resize(size);
                            }
                        }
                    } else if let Some(id) = msg.get("ack").and_then(Value::as_u64) {
                        if let Some(page) = tab.browsers.get_mut(&id) {
                            page.ack();
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
        let rects = tab.rects(area);
        let dirty = changed || acted || v.focus != tab.focus || v.area != area;
        // Before `layout`, whose hint depends on a page's route.
        if dirty {
            reconcile(&mut self.bases, &self.files, &self.root, tab);
        }
        let layout = layout_json(tab, &rects, area, &self.bases, self.drawer);
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

        // Before the page's events: the web app resets a pane's typing on a new URL, then takes the
        // `editable` of the load that follows it.
        send_browsers(v, tab);
        // A browser pane's new frame, and each page's last frame to a new view.
        v.framed.retain(|id| tab.browsers.contains_key(id));
        for (&id, page) in &mut tab.browsers {
            let fresh = page.take_fresh();
            if (fresh || !v.framed.contains(&id))
                && let Some(f) = page.frame()
            {
                v.send(Outbound::Bytes(framed(id, f)));
                v.framed.insert(id);
            }
            for mut m in page.take_out() {
                m["id"] = id.into();
                v.text(m);
            }
        }
        for (pane, text) in tab.notices.drain(..) {
            let mut m = json!({"t": "notice", "text": text});
            if let Some(pane) = pane {
                m["pane"] = pane.into();
            }
            v.text(m);
        }

        if dirty {
            send_panes(v, tab, &rects, &self.bases);
            (v.focus, v.area) = (tab.focus, area);
        }

        if !v.git_shown && crate::gitdiff::git_missing() {
            v.git_shown = true;
            v.text(json!({"t": "notice", "text": view::NO_GIT}));
        }
        if v.dead {
            self.view = None;
            tab.hold = false;
        }
        !quit
    }

    /// Send `{"t":"exit"}` and wait (briefly) for the connection thread to write it.
    pub fn close(mut self) {
        if let Some(page) = self.page.take() {
            let _ = std::fs::remove_file(page);
        }
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

/// The drawer's height before the user drags its grip, in % of the workspace's.
const DRAWER: u8 = 54;

/// A drawer height from the user or the file, within 20–85%.
fn drawer_pct(p: f64) -> u8 {
    p.clamp(20.0, 85.0).round() as u8
}

/// Save the drawer's height, or remove it for `None`; the height to show.
fn save_drawer(percent: Option<f64>) -> u8 {
    let h = percent.map(drawer_pct);
    // Best-effort: an unwritable file keeps the height for this workspace only.
    let _ = crate::settings::set("drawer", h.map(Value::from));
    h.unwrap_or(DRAWER)
}

/// The time now, in Unix ms.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Give each markdown and HTML pane a file route, and drop the route of a pane that closed or
/// changed file. A markdown route serves the folder Laura started in when the file sits below it,
/// outside hidden folders, else the file's folder, so `../` reaches no further than one of those.
/// A page's route serves only the page's folder, which its scripts can read.
fn reconcile(bases: &mut Bases, files: &Files, root: &Path, tab: &Tab) {
    let mut files = files.lock().unwrap_or_else(PoisonError::into_inner);
    bases.retain(|id, (path, n, _)| {
        let keep = tab.panels.get(id).is_some_and(|p| p.path == *path);
        if !keep {
            files.remove(n);
        }
        keep
    });
    for (id, p) in &tab.panels {
        if bases.contains_key(id) || (!is_page(p) && p.blocks().is_empty()) {
            continue;
        }
        let Some(dir) = Path::new(&p.path)
            .canonicalize()
            .ok()
            .and_then(|f| f.parent().map(Path::to_path_buf))
        else {
            continue;
        };
        let Ok(n) = nonce(16) else {
            continue;
        };
        let segs: Option<Vec<&str>> = dir
            .strip_prefix(root)
            .ok()
            .filter(|_| !is_page(p))
            .and_then(|rel| {
                rel.iter()
                    .map(|c| c.to_str().filter(|c| segment(c)))
                    .collect()
            });
        let (served, segs) = match segs {
            Some(segs) => (root.to_path_buf(), segs),
            None => (dir.clone(), vec![]),
        };
        let mut base = format!("/f/{n}/");
        for s in segs {
            base.push_str(&pct_encode(s));
            base.push('/');
        }
        files.insert(
            n.clone(),
            Served {
                root: served,
                page: None,
            },
        );
        bases.insert(*id, (p.path.clone(), n, base));
    }
    // The URL changes with the content, so the frame reloads. A frozen pane's content is its
    // snapshot, so its page holds until `Ctrl+R`.
    for (id, p) in tab.panels.iter().filter(|(_, p)| is_page(p)) {
        let Some((_, n, url)) = bases.get_mut(id) else {
            continue;
        };
        let Some(served) = files.get_mut(n.as_str()) else {
            continue;
        };
        let Some(name) = Path::new(&p.path).file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        let page = format!("/f/{n}/{}?v={:016x}", pct_encode(name), content_hash(p));
        if *url != page {
            // The block keeps `lauraDraw` out of the page's globals.
            let script = concat!(
                "<script>{\n",
                include_str!("../../assets/web/draw.js"),
                "\n",
                include_str!("../../assets/web/page.js"),
                "\n}</script>"
            );
            let html = tag_lines(&p.content, script);
            served.page = Some((name.to_string(), html));
            *url = page;
        }
    }
}

/// The pane's content hash, which changes when Laura reloads the file.
fn content_hash(p: &Panel) -> u64 {
    let mut h = DefaultHasher::new();
    p.content.hash(&mut h);
    h.finish()
}

/// A path segment the file route serves: not empty, not hidden, no separator, drive or NUL.
fn segment(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('.') && !s.contains(['/', '\\', ':', '\0'])
}

/// A `view` or `shell` message's `cols` and `rows`, clamped: a minimized window or a hostile frame
/// can't size the shell to 0.
fn cells(size: &Value) -> Option<(u16, u16)> {
    let dim = |k: &str, lo: u64, hi: u64| {
        size.get(k)
            .and_then(Value::as_u64)
            .map(|n| n.clamp(lo, hi) as u16)
    };
    Some((dim("cols", 20, 1000)?, dim("rows", 5, 500)?))
}

/// A PTY chunk or a browser pane's frame for the browser: the pane id as 8 big-endian bytes, then
/// the bytes.
fn framed(id: PaneId, bytes: Vec<u8>) -> Vec<u8> {
    let mut out = id.to_be_bytes().to_vec();
    out.extend(bytes);
    out
}

/// One connection: a static file, a file beside a pane, or the websocket view.
fn serve(
    mut stream: TcpStream,
    token: &str,
    port: u16,
    join: &Sender<Conn>,
    files: &Files,
) -> Result<()> {
    let Some(mut buf) = peek_head(&mut stream)? else {
        return Ok(());
    };
    let text = String::from_utf8_lossy(&buf).into_owned();
    let target = text.split(' ').nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/").to_string();
    if path == "/ws" {
        return view_conn(stream, token, port, join);
    }
    stream.read_exact(&mut buf)?;
    // No token, so a page or image URL can't leak it.
    if let Some(rest) = path.strip_prefix("/f/") {
        return match file(rest, files) {
            Some((mime, body, page_root)) => respond_file(&mut stream, mime, &body, page_root),
            None => respond(&mut stream, "404 Not Found", None),
        };
    }
    // A page's `/style.css` is the page's, not the workspace's: a subresource from another site (a
    // page's opaque origin) gets none of these.
    if foreign_subresource(&text) {
        return respond(&mut stream, "404 Not Found", None);
    }
    match ASSETS.iter().find(|(p, ..)| *p == path) {
        Some((_, mime, body)) => respond(&mut stream, "200 OK", Some((mime, body))),
        None => respond(&mut stream, "404 Not Found", None),
    }
}

/// `<nonce>/<path>` from the file route: the nonce's page, or an image below its folder, or for a
/// page, a stylesheet, script or font too. Returns the file's type, its bytes, and whether a
/// page's folder served it.
fn file(rest: &str, files: &Files) -> Option<(&'static str, Vec<u8>, bool)> {
    let (n, rest) = rest.split_once('/')?;
    let segs = rest
        .split('/')
        .map(pct_decode)
        .collect::<Option<Vec<_>>>()?;
    let (root, page_root) = {
        let files = files.lock().unwrap_or_else(PoisonError::into_inner);
        let served = files.get(n)?;
        if let Some((name, html)) = &served.page
            && segs.len() == 1
            && segs[0] == *name
        {
            return Some(("text/html; charset=utf-8", html.clone().into_bytes(), true));
        }
        (served.root.clone(), served.page.is_some())
    };
    if !segs.iter().all(|s| segment(s)) {
        return None;
    }
    // A symlink or a Windows short name (`GIT~1`) gets past those checks: check the real path.
    let path = segs
        .iter()
        .fold(root.clone(), |p, s| p.join(s))
        .canonicalize()
        .ok()?;
    let real = path.strip_prefix(&root).ok()?;
    if !real.iter().all(|c| c.to_str().is_some_and(segment)) {
        return None;
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let page_files = if page_root { PAGE_FILES } else { &[] };
    let mime = IMAGES.iter().chain(page_files).find(|(e, _)| *e == ext)?.1;
    Some((mime, std::fs::read(path).ok()?, page_root))
}

/// A file from the file route. `sandbox`: an SVG opened on its own can't run script, and a page's
/// scripts and forms run in an opaque origin, away from the workspace. That origin needs CORS to
/// load a page's module scripts and fonts.
fn respond_file(stream: &mut TcpStream, mime: &str, body: &[u8], page_root: bool) -> Result<()> {
    let policy = if page_root {
        "Content-Security-Policy: sandbox allow-scripts allow-forms; frame-ancestors 'self'\r\n\
         Access-Control-Allow-Origin: *"
    } else {
        "Content-Security-Policy: sandbox"
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\
         X-Content-Type-Options: nosniff\r\n{policy}\r\n\
         Referrer-Policy: {REFERRER_POLICY}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
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
        let origin = req.headers().get("origin").and_then(|o| o.to_str().ok());
        let status = if !same(given.as_bytes(), token.as_bytes()) {
            Some(StatusCode::UNAUTHORIZED)
        } else if !our_origin(origin, port) {
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
            Err(e) if nothing_yet(&e) => None,
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
