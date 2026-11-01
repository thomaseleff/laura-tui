//! `LAURA_BROWSER_URL` (#129): Laura serves the agent's browser tools in front of Laura's browser.
//! The proxy relays each message unchanged, so the tools work as with Chrome itself, and reads the
//! method of each command, so the web app shows when the agent captures a browser pane's page.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use crate::browser;
use crate::web::{header, nothing_yet, peek_head, respond};

/// The reply before Laura's browser starts, or after it exits.
pub const NOT_RUNNING: &str =
    "Laura's browser is not running. Open a browser pane, then try again.";

/// A capture by the agent's browser tool on Laura's browser's page `target`, from tool
/// connection `conn`.
pub struct Capture {
    pub conn: u64,
    pub target: String,
    pub kind: Kind,
}

pub enum Kind {
    Screenshot,
    /// A recording, such as a screencast or a performance trace, started (`true`) or stopped.
    Recording(bool),
}

/// The methods `Watch` parses a command for.
const WATCHED: &[&str] = &[
    "Page.captureScreenshot",
    "Page.startScreencast",
    "Page.stopScreencast",
    "Page.startScreenRecording",
    "Page.stopScreenRecording",
    "Tracing.start",
    "Tracing.end",
];

/// Serve `listener` in front of Laura's browser, which listens on `chrome` (`0` while it doesn't
/// run). The receiver gets each capture.
pub fn serve(listener: TcpListener, chrome: Arc<AtomicU16>) -> Receiver<Capture> {
    let port = listener.local_addr().map_or(0, |a| a.port());
    let (tx, rx) = channel();
    // shortcut: the thread blocks in `accept` and ends with the process, as the tab's socket
    // listener does; a nonblocking listener with a stop flag if tabs come and go in one process.
    std::thread::spawn(move || {
        for (n, stream) in (0..).zip(listener.incoming().flatten()) {
            let (chrome, tx) = (chrome.clone(), tx.clone());
            std::thread::spawn(move || {
                let _ = conn(stream, port, &chrome, Watch::new(n, tx));
            });
        }
    });
    rx
}

/// One connection: Chrome's `/json` list, or a CDP websocket relayed to Chrome.
fn conn(mut stream: TcpStream, port: u16, chrome: &AtomicU16, mut watch: Watch) -> Result<()> {
    let Some(mut head) = peek_head(&mut stream)? else {
        return Ok(());
    };
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut line = text.split(' ');
    let (method, target) = (line.next().unwrap_or(""), line.next().unwrap_or("/"));
    let path = target.split('?').next().unwrap_or("/");
    let chrome = chrome.load(Ordering::SeqCst);
    let ws = path.starts_with("/devtools/browser/") || path.starts_with("/devtools/page/");
    // Under DNS rebinding a page reaches this port with its own domain as the Host. Chrome checks
    // the same.
    let host = header(&text, "Host").is_some_and(local);
    // tungstenite reads a websocket's head. For any other reply, read the head first, else the
    // close can reset the socket and cut the reply.
    if !(host && method == "GET" && chrome != 0 && ws) {
        stream.read_exact(&mut head)?;
    }
    if !host {
        return respond(&mut stream, "403 Forbidden", None);
    }
    if method != "GET" {
        return respond(&mut stream, "405 Method Not Allowed", None);
    }
    if chrome == 0 {
        let body = ("text/plain", NOT_RUNNING.as_bytes());
        return respond(&mut stream, "503 Service Unavailable", Some(body));
    }
    if path.starts_with("/json") {
        let Some(v) = browser::get_json(chrome, target) else {
            return respond(&mut stream, "502 Bad Gateway", None);
        };
        // Chrome names its own port in `webSocketDebuggerUrl` and `devtoolsFrontendUrl`.
        let body = v
            .to_string()
            .replace(&format!("127.0.0.1:{chrome}"), &format!("127.0.0.1:{port}"));
        return respond(
            &mut stream,
            "200 OK",
            Some(("application/json", body.as_bytes())),
        );
    }
    if !ws {
        return respond(&mut stream, "404 Not Found", None);
    }
    let up = match upstream(chrome, target, header(&text, "Origin")) {
        Ok(up) => up,
        Err(_) => {
            stream.read_exact(&mut head)?;
            return respond(&mut stream, "502 Bad Gateway", None);
        }
    };
    let client = tungstenite::accept_with_config(stream, Some(config()))?;
    // A page's own socket takes commands with no session.
    watch.own = path.strip_prefix("/devtools/page/").map(str::to_string);
    relay(client, up, &mut watch);
    Ok(())
}

/// A frame at density 3 is several MB of base64, as for `Cdp::connect`.
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(256 << 20))
        .max_frame_size(Some(256 << 20))
}

/// `host` (`name:port`) names this machine: `localhost` or an IP address.
fn local(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(name, _)| name),
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<IpAddr>().is_ok()
}

/// Chrome's websocket at `target`, with the client's `Origin`, so Chrome checks the client's
/// origin as it would without the proxy.
fn upstream(chrome: u16, target: &str, origin: Option<&str>) -> Result<WebSocket<TcpStream>> {
    let mut req = format!("ws://127.0.0.1:{chrome}{target}").into_client_request()?;
    if let Some(o) = origin {
        req.headers_mut()
            .insert("Origin", HeaderValue::from_str(o)?);
    }
    let stream = TcpStream::connect(("127.0.0.1", chrome))?;
    let (ws, _) = tungstenite::client::client_with_config(req, stream, Some(config()))?;
    Ok(ws)
}

/// Relay both ways until one side closes, then close the other. One loop, as `browser::io`: a
/// short polling read on each side, which drains that side's burst before the other side's read.
fn relay(mut client: WebSocket<TcpStream>, mut chrome: WebSocket<TcpStream>, watch: &mut Watch) {
    for ws in [&client, &chrome] {
        let _ = ws
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(4)));
    }
    while pass(&mut client, &mut chrome, watch, true).unwrap_or(false)
        && pass(&mut chrome, &mut client, watch, false).unwrap_or(false)
    {}
    for ws in [&mut client, &mut chrome] {
        let _ = ws.close(None);
        let _ = ws.flush();
    }
}

/// Up to `BURST` messages from `from` to `to`, until `from` has nothing; `false` once `from`
/// closes. `up`: from the tool to Chrome.
fn pass(
    from: &mut WebSocket<TcpStream>,
    to: &mut WebSocket<TcpStream>,
    watch: &mut Watch,
    up: bool,
) -> tungstenite::Result<bool> {
    for _ in 0..BURST {
        match from.read() {
            Ok(m @ (Message::Text(_) | Message::Binary(_))) => {
                if let Message::Text(t) = &m {
                    watch.read(t, up);
                }
                to.send(m)?;
            }
            Ok(Message::Close(_)) => return Ok(false),
            Ok(_) => {}
            Err(e) if nothing_yet(&e) => return Ok(true),
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// The most messages one side relays before the other side's turn, so a page that logs without
/// end doesn't hold up the tool's commands.
const BURST: usize = 256;

/// One tool connection's view of its commands: which page each session names, and the captures.
struct Watch {
    conn: u64,
    tx: Sender<Capture>,
    /// The page of a `/devtools/page/<id>` socket.
    own: Option<String>,
    /// Session id to target id, from Chrome's `Target.attachedToTarget` events, which Chrome sends
    /// for both an attach and an auto-attach.
    sessions: HashMap<String, String>,
    /// The pages this connection records, which stop when it closes.
    /// shortcut: a screencast and a trace on one page at once share one mark, so the first to stop
    /// hides it; track by method if a tool does both.
    recording: HashSet<String>,
}

impl Watch {
    fn new(conn: u64, tx: Sender<Capture>) -> Watch {
        Watch {
            conn,
            tx,
            own: None,
            sessions: HashMap::new(),
            recording: HashSet::new(),
        }
    }

    /// A text message: from Chrome (`up` false), an attach; from the tool, a capture. A substring
    /// check first, as a screencast frame is MBs of JSON.
    fn read(&mut self, t: &str, up: bool) {
        if !up {
            if t.contains("\"Target.attachedToTarget\"")
                && let Ok(v) = serde_json::from_str::<Value>(t)
                && let (Some(s), Some(id)) = (
                    v["params"]["sessionId"].as_str(),
                    v["params"]["targetInfo"]["targetId"].as_str(),
                )
            {
                self.sessions.insert(s.into(), id.into());
            }
            return;
        }
        if !WATCHED.iter().any(|m| t.contains(m)) {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(t) else {
            return;
        };
        let kind = match v["method"].as_str() {
            Some("Page.captureScreenshot") => Kind::Screenshot,
            // chrome-devtools-mcp's `screencast_start` sends `startScreenRecording`, and
            // Playwright's video and tracing send `startScreencast`.
            Some("Page.startScreencast" | "Page.startScreenRecording" | "Tracing.start") => {
                Kind::Recording(true)
            }
            Some("Page.stopScreencast" | "Page.stopScreenRecording" | "Tracing.end") => {
                Kind::Recording(false)
            }
            _ => return,
        };
        // A command on the browser's own session names no page.
        let target = match v["sessionId"].as_str() {
            Some(s) => self.sessions.get(s).cloned(),
            None => self.own.clone(),
        };
        if let Some(target) = target {
            if let Kind::Recording(on) = kind {
                if on {
                    self.recording.insert(target.clone());
                } else {
                    self.recording.remove(&target);
                }
            }
            let _ = self.tx.send(Capture {
                conn: self.conn,
                target,
                kind,
            });
        }
    }
}

impl Drop for Watch {
    /// The tool disconnected: its recordings stop.
    fn drop(&mut self) {
        for target in self.recording.drain() {
            let _ = self.tx.send(Capture {
                conn: self.conn,
                target,
                kind: Kind::Recording(false),
            });
        }
    }
}
