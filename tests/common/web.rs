//! The web app's test rig: a workspace hosted by the web server, run in-process, and a view's
//! websocket. Included with `#[path]` by the test crates that drive the web app.

use std::net::TcpStream;
use std::process::Output;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::Tab;
use laura::web::{DEFAULT_AREA, WebServer};
use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

pub type Ws = WebSocket<TcpStream>;

/// `ready` journals into `target/tmp/.laura`, not the real `~/.laura`.
pub fn isolate_home() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: once per binary; std's env lock serializes it with std's own readers (`Command` spawn, `env::var`).
    ONCE.call_once(|| unsafe {
        std::env::set_var("HOME", env!("CARGO_TARGET_TMPDIR"));
        std::env::set_var("USERPROFILE", env!("CARGO_TARGET_TMPDIR"));
    });
}

pub fn shell() -> portable_pty::CommandBuilder {
    let mut cmd =
        portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    // macOS's /bin/sh is bash, whose line editor shows the paste markers and scrolls a long line
    // sideways, so the screen wouldn't hold a submitted inline review.
    if cfg!(target_os = "macos") {
        cmd.arg("--noediting");
    }
    cmd
}

/// What a view receives.
#[derive(Debug)]
pub enum Frame {
    Json(Value),
    Bytes(u64, Vec<u8>),
    Closed,
}

impl Frame {
    pub fn is(&self, t: &str) -> bool {
        matches!(self, Frame::Json(v) if v["t"] == t)
    }

    pub fn json(self) -> Value {
        match self {
            Frame::Json(v) => v,
            other => panic!("not a JSON frame: {other:?}"),
        }
    }
}

/// The port and token from a printed URL.
pub fn port_token(url: &str) -> (u16, String) {
    let rest = url
        .strip_prefix("http://127.0.0.1:")
        .expect("a localhost URL");
    let (port, token) = rest.split_once("/?t=").expect("a token in the URL");
    (port.parse().expect("a port"), token.to_string())
}

/// A websocket to `/ws` with `token` and `origin`, as a browser tab opens it.
pub fn connect_to(port: u16, token: Option<&str>, origin: Option<&str>) -> tungstenite::Result<Ws> {
    let query = token.map(|t| format!("?t={t}")).unwrap_or_default();
    let mut req = format!("ws://127.0.0.1:{port}/ws{query}").into_client_request()?;
    if let Some(o) = origin {
        req.headers_mut()
            .insert("origin", o.parse().expect("header"));
    }
    let (ws, _) = tungstenite::client(req, TcpStream::connect(("127.0.0.1", port))?).map_err(
        |e| match e {
            tungstenite::HandshakeError::Failure(e) => e,
            tungstenite::HandshakeError::Interrupted(_) => tungstenite::Error::ConnectionClosed,
        },
    )?;
    ws.get_ref()
        .set_read_timeout(Some(Duration::from_millis(5)))?;
    Ok(ws)
}

/// One frame from `ws`, or `None` if none is waiting.
pub fn read(ws: &mut Ws) -> Option<Frame> {
    match ws.read() {
        Ok(Message::Text(t)) => Some(Frame::Json(serde_json::from_str(&t).expect("JSON"))),
        Ok(Message::Binary(b)) => {
            let (id, bytes) = b.split_first_chunk::<8>().expect("a pane id");
            Some(Frame::Bytes(u64::from_be_bytes(*id), bytes.to_vec()))
        }
        Ok(Message::Close(_)) => Some(Frame::Closed),
        Ok(_) => None,
        Err(e) if laura::web::nothing_yet(&e) => None,
        Err(_) => Some(Frame::Closed),
    }
}

pub fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::text(v.to_string())).expect("send");
}

/// A workspace hosted by the web server, run in-process as `laura --web`'s loop runs it.
pub struct Rig {
    pub tab: Tab,
    pub web: WebServer,
    pub port: u16,
    pub token: String,
}

impl Rig {
    pub fn new() -> Result<Rig> {
        Rig::with(shell())
    }

    pub fn with(cmd: portable_pty::CommandBuilder) -> Result<Rig> {
        isolate_home();
        // As `laura --web` starts it, with browser panes off.
        let tab = Tab::spawn_web(cmd, 24, 80, Err(laura::browser::FLAG_OFF.into()))?;
        let web = WebServer::bind()?;
        let (port, token) = port_token(web.url());
        Ok(Rig {
            tab,
            web,
            port,
            token,
        })
    }

    /// One turn of `laura --web`'s loop; false once the user quits.
    pub fn pump(&mut self) -> bool {
        let area = self.web.area().unwrap_or(DEFAULT_AREA);
        let changed = self.tab.drain(area);
        self.tab.tick(area);
        self.web.sync(&mut self.tab, changed)
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn connect(&self) -> Ws {
        connect_to(self.port, Some(&self.token), Some(&self.origin())).expect("view connects")
    }

    /// One frame from `ws`, as [`read`]. A browser pane's frame is acked, as the web app acks it:
    /// Laura's browser pauses the screencast while a frame waits for its ack.
    pub fn read(&self, ws: &mut Ws) -> Option<Frame> {
        let f = read(ws);
        if let Some(Frame::Bytes(id, _)) = &f
            && self.tab.browsers.contains_key(id)
        {
            send(ws, serde_json::json!({ "ack": id }));
        }
        f
    }

    /// Pump and read `ws` until a frame matches `want`; fail after [`crate::common::DEADLINE`], or
    /// when `ws` closes first.
    pub fn until(&mut self, ws: &mut Ws, what: &str, want: impl Fn(&Frame) -> bool) -> Frame {
        let start = Instant::now();
        loop {
            self.pump();
            while let Some(f) = self.read(ws) {
                if want(&f) {
                    return f;
                }
                // A closed socket reads `Closed` on every call: no wait can end.
                assert!(!matches!(f, Frame::Closed), "the view closed: {what}");
            }
            assert!(
                start.elapsed() < crate::common::DEADLINE,
                "timed out: {what}"
            );
        }
    }

    /// Pump `turns` times; every frame `ws` received meanwhile, up to its `Closed`.
    pub fn idle(&mut self, ws: &mut Ws, turns: usize) -> Vec<Frame> {
        let mut out = vec![];
        for _ in 0..turns {
            self.pump();
            while let Some(f) = self.read(ws) {
                let closed = matches!(f, Frame::Closed);
                out.push(f);
                // A closed socket reads `Closed` on every call; one is enough.
                if closed {
                    break;
                }
            }
        }
        out
    }

    /// Send one `act` frame; its `acted` reply.
    pub fn act(&mut self, ws: &mut Ws, act: Value) -> Value {
        send(ws, act);
        self.until(ws, "acted", |f| f.is("acted")).json()
    }

    /// Start `laura <args>` against the workspace without pumping; its output once it exits.
    pub fn start(&self, args: &[&str]) -> mpsc::Receiver<Output> {
        let name = self.tab.socket.clone();
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let out = Command::cargo_bin("laura")
                .unwrap()
                .args(&a)
                .env("LAURA_TAB", &name)
                .output()
                .unwrap();
            let _ = tx.send(out);
        });
        rx
    }

    /// Pump until the client behind `rx` exits; fail after [`crate::common::DEADLINE`].
    pub fn wait(&mut self, rx: &mpsc::Receiver<Output>) -> Output {
        let start = Instant::now();
        loop {
            self.pump();
            if let Ok(out) = rx.try_recv() {
                return out;
            }
            assert!(
                start.elapsed() < crate::common::DEADLINE,
                "client timed out"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Run `laura <args>` against the workspace, pumping until it exits.
    pub fn run(&mut self, args: &[&str]) -> Output {
        let rx = self.start(args);
        self.wait(&rx)
    }

    /// `run`, asserting success; trimmed stdout.
    pub fn drive(&mut self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "laura {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Pump until the shell's screen holds `text`.
    pub fn shell_shows(&mut self, text: &str) {
        let start = Instant::now();
        while !self.tab.pty.with_screen(|s| s.contents()).contains(text) {
            assert!(
                start.elapsed() < crate::common::DEADLINE,
                "shell never showed {text}"
            );
            self.pump();
            thread::sleep(Duration::from_millis(10));
        }
    }
}
