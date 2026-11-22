//! HTTP for Laura's two local servers (the web app, the Laura server) and the CDP proxy:
//! request heads, responses, tokens, origin checks and URL encoding.

use std::io::{ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};

pub(crate) const REFERRER_POLICY: &str = "no-referrer";

/// `'unsafe-inline'` styles: pane rows carry inline `style`. Script only from Laura's own files,
/// so an escaping bug in pane HTML can't run code that holds the shell's websocket. `data:`
/// images: the tab icon with the wait's dot, which the page makes from `favicon.svg`.
pub(crate) const CSP: &str = "default-src 'self'; img-src 'self' data:; \
     style-src 'self' 'unsafe-inline'; frame-ancestors 'none'";

/// Browsers send every `localhost` cookie to every port, so headers easily pass 8 KB.
const MAX_HEAD: usize = 64 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);

/// A read on a socket with a read timeout that timed out: no frame yet. The timeout is `TimedOut`
/// on Windows and `WouldBlock` on Unix; Windows sometimes reports it as `ERROR_IO_PENDING` (997)
/// instead, which ended Laura's CDP connection as if Laura's browser had exited. A signal ends a
/// timed read with `Interrupted` (EINTR) on Unix, even under `SA_RESTART`: the read is retried.
pub fn nothing_yet(e: &tungstenite::Error) -> bool {
    let tungstenite::Error::Io(e) = e else {
        return false;
    };
    matches!(
        e.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    ) || (cfg!(windows) && e.raw_os_error() == Some(997))
}

/// Listen on `127.0.0.1:port`; `0` picks a free port.
pub(crate) fn listen(port: u16) -> Result<TcpListener> {
    TcpListener::bind(("127.0.0.1", port)).map_err(|e| match e.kind() {
        ErrorKind::AddrInUse => anyhow!("port {port} is in use: pass --port"),
        _ => e.into(),
    })
}

/// Whether a process listens on `127.0.0.1:port`.
pub(crate) fn answers(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_ok()
}

/// `n` random bytes as hex.
pub(crate) fn nonce(n: usize) -> Result<String> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|e| anyhow!("no randomness: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `s` with every byte but `A-Za-z0-9-._~` percent-encoded.
pub(crate) fn pct_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// `s` percent-decoded; a `%` without two hex digits stays as is, as in a browser. `None` on bytes
/// that aren't UTF-8.
pub(crate) fn pct_decode(s: &str) -> Option<String> {
    let (b, mut out, mut i) = (s.as_bytes(), vec![], 0);
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .filter(|h| b[i] == b'%' && h.iter().all(u8::is_ascii_hexdigit));
        if let Some(hex) = hex {
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Constant-time, so the token can't be guessed a byte at a time.
pub(crate) fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Peek a request's head, so tungstenite can still read it for a websocket: the head, or `None`
/// when the client closes or stalls, or sends a head over `MAX_HEAD` (answered 431).
pub(crate) fn peek_head(stream: &mut TcpStream) -> Result<Option<Vec<u8>>> {
    stream.set_read_timeout(Some(HEAD_TIMEOUT))?;
    let deadline = Instant::now() + HEAD_TIMEOUT;
    let mut buf = vec![0u8; MAX_HEAD];
    loop {
        let n = stream.peek(&mut buf)?;
        if n == 0 {
            return Ok(None);
        }
        if let Some(i) = buf[..n].windows(4).position(|w| w == b"\r\n\r\n") {
            buf.truncate(i + 4);
            return Ok(Some(buf));
        }
        if n == buf.len() {
            respond(stream, "431 Request Header Fields Too Large", None)?;
            return Ok(None);
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Header `name`'s value in a request head.
pub(crate) fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// A request from another site's page that isn't a navigation: a subresource or a `fetch`.
pub(crate) fn foreign_subresource(head: &str) -> bool {
    let site = header(head, "Sec-Fetch-Site").unwrap_or("");
    (site.eq_ignore_ascii_case("cross-site") || site.eq_ignore_ascii_case("same-site"))
        && !header(head, "Sec-Fetch-Mode").is_some_and(|m| m.eq_ignore_ascii_case("navigate"))
}

/// Whether `origin` is the page that the server on `port` serves. Checked against our own address:
/// under DNS rebinding, Host carries the attacker's domain.
pub(crate) fn our_origin(origin: Option<&str>, port: u16) -> bool {
    origin.is_some_and(|o| {
        o == format!("http://127.0.0.1:{port}") || o == format!("http://localhost:{port}")
    })
}

pub(crate) fn respond(
    stream: &mut TcpStream,
    status: &str,
    body: Option<(&str, &[u8])>,
) -> Result<()> {
    let (mime, body) = body.unwrap_or(("text/plain", b""));
    let charset = if mime.starts_with("text/") {
        "; charset=utf-8"
    } else {
        ""
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}{charset}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: {REFERRER_POLICY}\r\n\
         Content-Security-Policy: {CSP}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}
