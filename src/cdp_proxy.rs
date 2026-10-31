//! `LAURA_BROWSER_URL` (#129): Laura serves the agent's browser tools in front of Laura's browser.
//! The proxy relays each message unchanged, so the tools work as with Chrome itself.

use std::io::Read;
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use anyhow::Result;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use crate::browser;
use crate::web::{header, nothing_yet, peek_head, respond};

/// The reply before Laura's browser starts, or after it exits.
pub const NOT_RUNNING: &str =
    "Laura's browser is not running. Open a browser pane, then try again.";

/// Serve `listener` in front of Laura's browser, which listens on `chrome` (`0` while it doesn't
/// run).
pub fn serve(listener: TcpListener, chrome: Arc<AtomicU16>) {
    let port = listener.local_addr().map_or(0, |a| a.port());
    // shortcut: the thread blocks in `accept` and ends with the process, as the tab's socket
    // listener does; a nonblocking listener with a stop flag if tabs come and go in one process.
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let chrome = chrome.clone();
            std::thread::spawn(move || {
                let _ = conn(stream, port, &chrome);
            });
        }
    });
}

/// One connection: Chrome's `/json` list, or a CDP websocket relayed to Chrome.
fn conn(mut stream: TcpStream, port: u16, chrome: &AtomicU16) -> Result<()> {
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
    relay(client, up);
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
/// short polling read on each side.
fn relay(mut client: WebSocket<TcpStream>, mut chrome: WebSocket<TcpStream>) {
    for ws in [&client, &chrome] {
        let _ = ws
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(4)));
    }
    while pass(&mut client, &mut chrome).unwrap_or(false)
        && pass(&mut chrome, &mut client).unwrap_or(false)
    {}
    for ws in [&mut client, &mut chrome] {
        let _ = ws.close(None);
        let _ = ws.flush();
    }
}

/// One message from `from` to `to`, if one arrived; `false` once `from` closes.
fn pass(
    from: &mut WebSocket<TcpStream>,
    to: &mut WebSocket<TcpStream>,
) -> tungstenite::Result<bool> {
    match from.read() {
        Ok(m @ (Message::Text(_) | Message::Binary(_))) => to.send(m)?,
        Ok(Message::Close(_)) => return Ok(false),
        Ok(_) => {}
        Err(e) if nothing_yet(&e) => {}
        Err(e) => return Err(e),
    }
    Ok(true)
}
