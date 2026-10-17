//! The web app (#88) through its websocket, as a browser tab drives it: the auth gate, the frames a
//! view receives, the user's actions as `act` frames, and `laura --web` end to end. State and
//! frames, not pixels.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command as StdCommand, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use assert_cmd::Command;
use laura::tab::NO_DIFF_IN_EDITOR;
use laura::web::{DEFAULT_AREA, FLAG_OFF, WebServer};
use laura::{Tab, UserAction};
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

type Ws = WebSocket<TcpStream>;

/// `ready` journals into `target/tmp/.laura`, not the real `~/.laura`.
fn isolate_home() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: once per binary; std's env lock serializes it with std's own readers (`Command` spawn, `env::var`).
    ONCE.call_once(|| unsafe {
        std::env::set_var("HOME", env!("CARGO_TARGET_TMPDIR"));
        std::env::set_var("USERPROFILE", env!("CARGO_TARGET_TMPDIR"));
    });
}

fn shell() -> portable_pty::CommandBuilder {
    portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" })
}

/// What a view receives.
#[derive(Debug)]
enum Frame {
    Json(Value),
    Bytes(u64, Vec<u8>),
    Closed,
}

impl Frame {
    fn is(&self, t: &str) -> bool {
        matches!(self, Frame::Json(v) if v["t"] == t)
    }

    fn json(self) -> Value {
        match self {
            Frame::Json(v) => v,
            other => panic!("not a JSON frame: {other:?}"),
        }
    }
}

/// The port and token from a printed URL.
fn port_token(url: &str) -> (u16, String) {
    let rest = url
        .strip_prefix("http://127.0.0.1:")
        .expect("a localhost URL");
    let (port, token) = rest.split_once("/?t=").expect("a token in the URL");
    (port.parse().expect("a port"), token.to_string())
}

/// A websocket to `/ws` with `token` and `origin`, as a browser tab opens it.
fn connect_to(port: u16, token: Option<&str>, origin: Option<&str>) -> tungstenite::Result<Ws> {
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
fn read(ws: &mut Ws) -> Option<Frame> {
    match ws.read() {
        Ok(Message::Text(t)) => Some(Frame::Json(serde_json::from_str(&t).expect("JSON"))),
        Ok(Message::Binary(b)) => {
            let (id, bytes) = b.split_first_chunk::<8>().expect("a pane id");
            Some(Frame::Bytes(u64::from_be_bytes(*id), bytes.to_vec()))
        }
        Ok(Message::Close(_)) => Some(Frame::Closed),
        Ok(_) => None,
        Err(tungstenite::Error::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            None
        }
        Err(_) => Some(Frame::Closed),
    }
}

fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::text(v.to_string())).expect("send");
}

/// A workspace hosted by the web server, run in-process as `laura --web`'s loop runs it.
struct Rig {
    tab: Tab,
    web: WebServer,
    port: u16,
    token: String,
}

impl Rig {
    fn new() -> Result<Rig> {
        Rig::with(shell())
    }

    fn with(cmd: portable_pty::CommandBuilder) -> Result<Rig> {
        isolate_home();
        let mut tab = Tab::spawn(cmd, 24, 80)?;
        tab.web = true; // as `laura --web` sets it
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
    fn pump(&mut self) -> bool {
        let area = self.web.area().unwrap_or(DEFAULT_AREA);
        let changed = self.tab.drain(area);
        self.tab.tick(area);
        self.web.sync(&mut self.tab, changed)
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn connect(&self) -> Ws {
        connect_to(self.port, Some(&self.token), Some(&self.origin())).expect("view connects")
    }

    /// Pump and read `ws` until a frame matches `want`; fail after 5 s.
    fn until(&mut self, ws: &mut Ws, what: &str, want: impl Fn(&Frame) -> bool) -> Frame {
        let start = Instant::now();
        loop {
            self.pump();
            while let Some(f) = read(ws) {
                if want(&f) {
                    return f;
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out: {what}"
            );
        }
    }

    /// Pump `turns` times; every frame `ws` received meanwhile.
    fn idle(&mut self, ws: &mut Ws, turns: usize) -> Vec<Frame> {
        let mut out = vec![];
        for _ in 0..turns {
            self.pump();
            while let Some(f) = read(ws) {
                out.push(f);
            }
        }
        out
    }

    /// Send one `act` frame; its `acted` reply.
    fn act(&mut self, ws: &mut Ws, act: Value) -> Value {
        send(ws, act);
        self.until(ws, "acted", |f| f.is("acted")).json()
    }

    /// Start `laura <args>` against the workspace without pumping; its output once it exits.
    fn start(&self, args: &[&str]) -> mpsc::Receiver<Output> {
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

    /// Pump until the client behind `rx` exits; fail after 5 s.
    fn wait(&mut self, rx: &mpsc::Receiver<Output>) -> Output {
        let start = Instant::now();
        loop {
            self.pump();
            if let Ok(out) = rx.try_recv() {
                return out;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Run `laura <args>` against the workspace, pumping until it exits.
    fn run(&mut self, args: &[&str]) -> Output {
        let rx = self.start(args);
        self.wait(&rx)
    }

    /// `run`, asserting success; trimmed stdout.
    fn drive(&mut self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "laura {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Pump until the shell's screen holds `text`.
    fn shell_shows(&mut self, text: &str) {
        let start = Instant::now();
        while !self.tab.pty.with_screen(|s| s.contents()).contains(text) {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "shell never showed {text}"
            );
            self.pump();
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// `body` as `name` in a fresh dir.
fn file(name: &str, body: &str) -> Result<(tempfile::TempDir, String)> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(name);
    std::fs::write(&path, body)?;
    Ok((dir, path.to_string_lossy().into_owned()))
}

fn five_lines() -> String {
    (1..=5).map(|n| format!("line {n}\n")).collect()
}

/// Run `git <args>` in `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=t@t", "-c", "user.name=t"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git spawns (test host needs git on PATH)");
    assert!(out.status.success(), "git {args:?} failed");
}

/// `name` committed with `committed`, then changed on disk to `now`.
fn changed_file(name: &str, committed: &str, now: &str) -> Result<(tempfile::TempDir, String)> {
    let (dir, path) = file(name, committed)?;
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", name]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    std::fs::write(&path, now)?;
    Ok((dir, path))
}

/// Each row in a `pane` frame's HTML: its `data-line` and its text, tags dropped.
fn rows(html: &str) -> Vec<(u32, String)> {
    html.split(r#"<div class="row"#)
        .skip(1)
        .map(|row| {
            let line = row
                .split(r#"data-line=""#)
                .nth(1)
                .and_then(|s| s.split('"').next())
                .and_then(|s| s.parse().ok())
                .expect("data-line");
            let mut text = String::new();
            let mut tag = false;
            for c in row.chars() {
                match c {
                    '<' => tag = true,
                    '>' => tag = false,
                    c if !tag => text.push(c),
                    _ => {}
                }
            }
            (line, text)
        })
        .collect()
}

/// Each rendered markdown block in a `pane` frame's HTML: its `data-line` and its HTML, up to the
/// next row.
fn blocks(html: &str) -> Vec<(u32, String)> {
    html.split(r#"<div class="blk"#)
        .skip(1)
        .map(|blk| {
            let blk = blk.split(r#"<div class="row"#).next().unwrap_or("");
            let line = blk
                .split(r#"data-line=""#)
                .nth(1)
                .and_then(|s| s.split('"').next())
                .and_then(|s| s.parse().ok())
                .expect("data-line");
            (line, blk.to_string())
        })
        .collect()
}

/// The first `pane` frame for pane `id`.
fn pane_frame(rig: &mut Rig, ws: &mut Ws, id: u64) -> Value {
    rig.until(
        ws,
        "pane frame",
        |f| matches!(f, Frame::Json(v) if v["t"] == "pane" && v["id"] == id),
    )
    .json()
}

#[test]
fn refuses_a_view_without_the_token_or_from_another_origin() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    let ours = rig.origin();
    for (token, origin, status) in [
        (None, Some(ours.as_str()), 401),
        (Some("0000"), Some(ours.as_str()), 401),
        (Some(rig.token.as_str()), Some("http://evil.example"), 403),
        (Some(rig.token.as_str()), None, 403),
    ] {
        match connect_to(rig.port, token, origin) {
            Err(tungstenite::Error::Http(resp)) => assert_eq!(resp.status().as_u16(), status),
            other => panic!("{token:?} from {origin:?} wasn't refused: {other:?}"),
        }
    }
    // The open view keeps the workspace: no `moved`, and it still gets frames.
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    let f = rig.until(&mut ws, "layout after open", |f| {
        f.is("moved") || matches!(f, Frame::Json(v) if v["t"] == "layout" && v["focus"] == 1)
    });
    assert!(f.is("layout"), "{f:?}");
    Ok(())
}

#[test]
fn first_frames_carry_the_layout_and_the_replay() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.pty.write(b"echo webmark\r")?;
    rig.shell_shows("webmark");
    let mut ws = rig.connect();
    let strings = rig.until(&mut ws, "strings", |f| f.is("strings")).json();
    assert!(
        strings["help"]
            .as_str()
            .is_some_and(|h| h.contains("Ctrl+P"))
    );
    let layout = rig.until(&mut ws, "layout", |f| f.is("layout")).json();
    assert_eq!(layout["focus"], 0);
    assert_eq!(layout["panes"][0]["kind"], "shell");
    assert_eq!(layout["panes"][0]["pty"], true);
    let replay = rig.until(&mut ws, "replay", |f| matches!(f, Frame::Bytes(0, _)));
    let Frame::Bytes(_, bytes) = replay else {
        unreachable!()
    };
    assert!(String::from_utf8_lossy(&bytes).contains("webmark"));
    Ok(())
}

#[test]
fn open_focuses_the_new_pane_and_sends_its_lines_once() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.until(
        &mut ws,
        "focus on #1",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["focus"] == 1),
    );
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        rows(&html).iter().any(|(_, t)| t.contains("line 3")),
        "{html}"
    );
    let idle = rig.idle(&mut ws, 10);
    assert!(
        !idle.iter().any(|f| f.is("pane") || f.is("rows")),
        "an idle frame resent rows: {idle:?}"
    );
    Ok(())
}

#[test]
fn pane_html_escapes_the_file() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.html", "<script>alert(1)</script>\n")?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(rows(&html)[0].1.contains("&lt;script&gt;"), "{html}");
    assert!(!html.contains("<script"), "{html}");
    Ok(())
}

#[test]
fn set_cursor_moves_the_cursor_and_sends_two_rows() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let acted = rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    assert_eq!(acted["ok"], true, "{acted}");
    assert_eq!(rig.tab.panels[&1].cursor, 3);
    let rows = rig.until(&mut ws, "rows", |f| f.is("rows")).json();
    assert_eq!(rows["rows"].as_array().map(Vec::len), Some(2), "{rows}");
    assert_eq!(rows["cursor"], 3);
    Ok(())
}

#[test]
fn file_pane_keys_reach_the_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file(
        "a.rs",
        &five_lines(),
        "line 1\nline 2 CHANGED\nline 3\nline 4\nline 5\n",
    )?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "4", "agent note"]);
    rig.drive(&["highlight", "--pane", "1", "2", "3"]);
    let mut ok = |rig: &mut Rig, act: Value| {
        let acted = rig.act(&mut ws, act.clone());
        assert_eq!(acted["ok"], true, "{act} → {acted}");
    };
    ok(
        &mut rig,
        json!({"act": "move_cursor", "pane": 1, "delta": -10}),
    );
    assert_eq!(rig.tab.panels[&1].cursor, 0);
    ok(&mut rig, json!({"act": "scroll_h", "pane": 1, "delta": 1}));
    ok(
        &mut rig,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    assert_eq!(rig.tab.panels[&1].cursor_line(), 4);
    ok(&mut rig, json!({"act": "toggle_thread", "pane": 1}));
    assert!(rig.tab.panels[&1].threads[0].collapsed);
    ok(
        &mut rig,
        json!({"act": "move_cursor", "pane": 1, "delta": -10}),
    );
    ok(
        &mut rig,
        json!({"act": "jump_thread", "pane": 1, "next": false}),
    );
    assert_eq!(rig.tab.panels[&1].cursor_line(), 4);
    ok(&mut rig, json!({"act": "toggle_diff", "pane": 1}));
    assert!(rig.tab.panels[&1].diff_view);
    ok(&mut rig, json!({"act": "clear_highlight", "pane": 1}));
    assert_eq!(rig.tab.panels[&1].highlight, None);
    ok(&mut rig, json!({"act": "refresh", "pane": 1}));
    assert_eq!(rig.tab.panels[&1].thread_count(), 0);
    ok(&mut rig, json!({"act": "focus", "pane": 0}));
    assert_eq!(rig.tab.focus, 0);
    ok(&mut rig, json!({"act": "close", "pane": 1}));
    assert!(!rig.tab.panels.contains_key(&1));
    Ok(())
}

#[test]
fn a_markdown_diff_view_tags_each_row_with_its_source_line() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file(
        "doc.md",
        "# Title\n\nalpha\nbeta\n",
        "# Title\n\nalpha\nbeta CHANGED\n",
    )?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let acted = rig.act(&mut ws, json!({"act": "toggle_diff", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let rows = rows(&html);
    let line_of = |text: &str| {
        rows.iter()
            .find(|(_, t)| t.contains(text))
            .map(|(l, _)| *l)
            .unwrap_or_else(|| panic!("no row with {text}: {rows:?}"))
    };
    assert_eq!(line_of("# Title"), 1);
    assert_eq!(line_of("alpha"), 3);
    assert_eq!(line_of("beta CHANGED"), 4);
    Ok(())
}

#[test]
fn a_markdown_pane_sends_each_block_as_html() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "# Title\n\nalpha\nbeta\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let blocks = blocks(&html);
    let lines: Vec<u32> = blocks.iter().map(|(l, _)| *l).collect();
    assert_eq!(lines, [1, 3, 6, 7, 10], "{html}");
    assert!(blocks[0].1.contains("<h1>Title</h1>"), "{html}");
    assert!(blocks[1].1.contains("alpha\nbeta"), "{html}");
    // A fenced block in a known language is syntax-highlighted.
    assert!(blocks[4].1.contains("<pre style="), "{html}");
    Ok(())
}

#[test]
fn a_comment_on_a_block_lands_on_its_first_line() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# Title\n\nalpha\nbeta\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    // `set_cursor` on a folded paragraph's second line lands on its first.
    let acted = rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    assert_eq!(acted["ok"], true, "{acted}");
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 3, "{acted}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 3, "text": "block note"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let acted = rig.act(&mut ws, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows("L3-4");
    Ok(())
}

#[test]
fn arrow_keys_reach_every_row_of_a_markdown_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "# T\n\n```rust\nfn a() {}\nfn b() {}\n```\n\nafter\n\n",
    )?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let acted = rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    assert_eq!(acted["ok"], true, "{acted}");
    let mut lines = vec![];
    for delta in [1, 1, 1, 1, 1, -1, -1, -1, -1] {
        let acted = rig.act(
            &mut ws,
            json!({"act": "move_cursor", "pane": 1, "delta": delta}),
        );
        assert_eq!(acted["ok"], true, "{acted}");
        let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
        lines.push(acted["line"].clone());
    }
    assert_eq!(lines, [5, 7, 8, 9, 9, 8, 7, 5, 4], "{lines:?}");
    Ok(())
}

#[test]
fn a_highlight_marks_markdown_blocks() -> Result<()> {
    let mut rig = Rig::new()?;
    let (_dir, path) = file("doc.md", "# T\n\nalpha\nbeta\n\nafter\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["highlight", "--pane", "1", "4"]);
    // A view that connects now gets the highlighted pane whole, not as a `rows` diff.
    let mut ws = rig.connect();
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let blocks = blocks(&html);
    let class = |line| {
        let (_, b) = blocks.iter().find(|(l, _)| *l == line).expect("block");
        b.split('"').next().unwrap_or("").to_string()
    };
    assert!(class(3).ends_with(" hl"), "{html}");
    assert_eq!(class(1), " dim", "{html}");
    assert_eq!(class(6), " dim", "{html}");
    Ok(())
}

#[test]
fn a_highlight_inside_a_block_lights_its_number() -> Result<()> {
    let mut rig = Rig::new()?;
    let (_dir, path) = file("doc.md", "# T\n\n```rust\nfn a() {}\nfn b() {}\n```\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["highlight", "--pane", "1", "5"]);
    let mut ws = rig.connect();
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let blocks = blocks(&html);
    let gutter = |line| {
        let (_, b) = blocks.iter().find(|(l, _)| *l == line).expect("block");
        let g = b.split(r#"<span class="g">"#).nth(1).unwrap_or("");
        g.split(r#"<div class="md">"#)
            .next()
            .unwrap_or("")
            .to_string()
    };
    assert!(gutter(4).contains("color:#d8dee9"), "{html}");
    assert!(!gutter(1).contains("color:#d8dee9"), "{html}");
    Ok(())
}

#[test]
fn markdown_html_is_sanitized() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "# T {#overlay .row data-line=1}\n\n<script>alert(1)</script>\n\n\
         <b>x</b> [x](javascript:alert(1)) [y](https://example.com) note[^1]\n\n[^1]: foot\n\n```x\trow\ncode data-line=\"1\">\n```\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(html.contains("&lt;b&gt;"), "{html}");
    assert!(!html.contains("<script") && !html.contains("<b>"), "{html}");
    // Only the `https` link keeps an `href`; the footnote reference is text.
    assert_eq!(html.matches("href=").count(), 1, "{html}");
    assert!(html.contains(r#"href="https://example.com""#), "{html}");
    assert!(html.contains("note[^1]"), "{html}");
    assert!(html.contains("[1]: foot"), "{html}");
    let heading = &blocks(&html)[0].1;
    assert!(!heading.contains("id="), "{heading}");
    assert!(!heading.contains(r#"class="row""#), "{heading}");
    assert_eq!(heading.matches("data-line").count(), 1, "{heading}");
    // An unknown fence language can't add a class, and code can't fake a line's attribute.
    assert!(!html.contains("language-"), "{html}");
    assert!(html.contains("data-line=&quot;1&quot;&gt;"), "{html}");
    Ok(())
}

#[test]
fn footnotes_and_math_read_as_in_the_tui() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "> x[^q]\n>\n> [^q]: foot\n\n$a+b$\n")?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(html.contains("x[q]"), "{html}");
    assert!(html.contains("$a+b$"), "{html}");
    assert!(!html.contains(r#"class="math"#), "{html}");
    Ok(())
}

#[test]
fn a_reference_link_resolves_in_a_markdown_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "- see [x][r] and [y][s]\n\n```\n[r]\n```\n\n\
         [r]: <https://example.com/a b>\n[s]: https://example.com/?b=1&amp;amp;c=2\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let hrefs = |h: &str| -> Vec<String> {
        h.split(r#"href=""#)
            .skip(1)
            .filter_map(|s| s.split('"').next())
            .map(str::to_string)
            .collect()
    };
    // The entity decodes once, as in a render of the whole file.
    assert_eq!(
        hrefs(&html),
        [
            "https://example.com/a%20b",
            "https://example.com/?b=1&amp;amp;c=2"
        ],
        "{html}"
    );
    // The code block shows its own text, not the file's reference definitions.
    assert_eq!(html.matches("https://example.com").count(), 2, "{html}");
    Ok(())
}

#[test]
fn a_thread_on_an_inner_line_shows_under_its_block() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# Title\n\n> one\n>\n> two\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "5", "agent note"]);
    let frame = rig
        .until(&mut ws, "the card", |f| {
            matches!(f, Frame::Json(v) if v["t"] == "pane" && v["id"] == 1
                && v["html"].as_str().is_some_and(|h| h.contains("agent note")))
        })
        .json();
    assert_eq!(frame["cursor"], 0, "{frame}");
    let html = frame["html"].as_str().unwrap_or("");
    let quote = html.find(r#"data-line="3""#).expect("the quote's block");
    let card = html.find("agent note").expect("the card");
    assert!(quote < card, "{html}");
    assert!(
        rows(html)
            .iter()
            .any(|(l, t)| *l == 5 && t.contains("agent note")),
        "{html}"
    );
    // The quote's block shows the thread's caret.
    assert!(blocks(html)[1].1.contains('▾'), "{html}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    // The cursor shows on the thread's card, below the quote's block.
    rig.until(&mut ws, "the cursor on the card", |f| {
        matches!(f, Frame::Json(v) if (v["t"] == "rows" || v["t"] == "pane") && v["id"] == 1 && v["cursor"] == 3)
    });
    let mut ws = rig.connect();
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        html.contains(r#"<div class="row cur" data-line="5""#),
        "{html}"
    );
    let (_, quote) = blocks(&html)
        .into_iter()
        .find(|(l, _)| *l == 3)
        .expect("the quote's block");
    assert!(
        !quote.split('>').next().unwrap_or("").contains(" cur"),
        "{quote}"
    );
    let acted = rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": 1}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    // `↓` in the last block keeps the thread's row.
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 5, "{acted}");
    // `↑` from the card goes to its block.
    let acted = rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": -1}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 3, "{acted}");
    Ok(())
}

#[test]
fn a_comment_beside_a_collapsed_thread_goes_on_the_blocks_first_line() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# T\n\n> a\n>\n> b\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["comment", "--pane", "1", "5", "note"]);
    rig.drive(&["ready"]);
    for act in [
        json!({"act": "jump_thread", "pane": 1, "next": true}),
        json!({"act": "toggle_thread", "pane": 1}),
    ] {
        let acted = rig.act(&mut ws, act);
        assert_eq!(acted["ok"], true, "{acted}");
    }
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 3, "{acted}");
    Ok(())
}

#[test]
fn a_highlight_deep_in_a_block_marks_its_place() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    // L1 opens the fence, L2-41 are code, L44 is a one-line paragraph.
    let code: String = (1..=40).map(|n| format!("c{n}\n")).collect();
    let (_dir, path) = file("doc.md", &format!("```\n{code}```\n\ntail\n"))?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let mut at = |rig: &mut Rig, line: &str| -> f64 {
        rig.drive(&["highlight", line, "--pane", "1"]);
        let frame = rig
            .until(&mut ws, "an hl block", |f| {
                matches!(f, Frame::Json(v) if v["id"] == 1 && v.to_string().contains("data-at"))
            })
            .json()
            .to_string();
        frame
            .split(r#"data-at=\""#)
            .nth(1)
            .and_then(|s| s.split('\\').next())
            .and_then(|s| s.parse().ok())
            .expect("data-at")
    };
    let deep = at(&mut rig, "30");
    assert!((0.6..0.8).contains(&deep), "{deep}");
    assert_eq!(at(&mut rig, "44"), 0.5);
    Ok(())
}

/// The index of the first row with `data-line` `line` among the units (blocks and rows) of a
/// `pane` frame's HTML.
fn unit_of(html: &str, line: u32) -> usize {
    let at = html
        .find(&format!(r#"data-line="{line}""#))
        .expect("a row on the line");
    let head = &html[..at];
    head.matches(r#"<div class="blk"#).count() + head.matches(r#"<div class="row"#).count() - 1
}

#[test]
fn jump_thread_between_threads_in_one_block_moves_to_its_card() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "x\n\n> a\n>\n> b\n>\n> c\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "5", "first"]);
    rig.drive(&["comment", "--pane", "1", "7", "second"]);
    let frame = rig
        .until(&mut ws, "both cards", |f| {
            matches!(f, Frame::Json(v) if v["t"] == "pane" && v["id"] == 1
                && v["html"].as_str().is_some_and(|h| h.contains("second")))
        })
        .json();
    let html = frame["html"].as_str().unwrap_or("");
    for (needle, line) in [("first", 5), ("second", 7)] {
        let want = unit_of(html, line);
        let acted = rig.act(
            &mut ws,
            json!({"act": "jump_thread", "pane": 1, "next": true}),
        );
        assert_eq!(acted["ok"], true, "{acted}");
        rig.until(&mut ws, needle, |f| {
            matches!(f, Frame::Json(v) if (v["t"] == "rows" || v["t"] == "pane") && v["id"] == 1 && v["cursor"] == want)
        });
    }
    Ok(())
}

#[test]
fn r_on_a_block_toggles_only_its_threads() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\nafter\n",
    )?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "3", "t1"]);
    rig.drive(&["comment", "--pane", "1", "4", "t2"]);
    rig.drive(&["comment", "--pane", "1", "6", "out"]);
    pane_frame(&mut rig, &mut ws, 1);
    let collapsed = |rig: &Rig| -> Vec<(String, bool)> {
        rig.tab.panels[&1]
            .threads
            .iter()
            .map(|t| (t.root.body.clone(), t.collapsed))
            .collect()
    };
    let table = |rig: &mut Rig, ws: &mut Ws| {
        blocks(pane_frame(rig, ws, 1)["html"].as_str().unwrap_or(""))[0]
            .1
            .clone()
    };
    let toggle = json!({"act": "toggle_thread", "pane": 1});

    // The cursor starts on the table's block.
    rig.act(&mut ws, toggle.clone());
    assert_eq!(
        collapsed(&rig),
        [
            ("t1".into(), true),
            ("t2".into(), true),
            ("out".into(), false)
        ]
    );
    let t = table(&mut rig, &mut ws);
    assert!(t.contains(r#"<tr data-line="3" class="th shut">"#), "{t}");
    assert!(t.contains(r#"<tr data-line="4" class="th shut">"#), "{t}");
    // A stop's caret shows on its row, not in the block's gutter.
    assert!(!t.contains('▸') && !t.contains('▾'), "{t}");

    rig.act(&mut ws, toggle.clone());
    assert!(collapsed(&rig).iter().all(|(_, c)| !c));
    let t = table(&mut rig, &mut ws);
    assert!(t.contains(r#"<tr data-line="3" class="th">"#), "{t}");
    assert!(t.contains(r#"<tr data-line="4" class="th">"#), "{t}");

    // On the second table thread's row, `r` toggles only that thread.
    for _ in 0..2 {
        rig.act(
            &mut ws,
            json!({"act": "jump_thread", "pane": 1, "next": true}),
        );
    }
    rig.act(&mut ws, toggle.clone());
    assert_eq!(
        collapsed(&rig),
        [
            ("t1".into(), false),
            ("t2".into(), true),
            ("out".into(), false)
        ]
    );
    let t = table(&mut rig, &mut ws);
    assert!(t.contains(r#"<tr data-line="3" class="th">"#), "{t}");
    assert!(
        t.contains(r#"<tr data-line="4" class="cur th shut">"#),
        "{t}"
    );

    // On a blank line, `r` collapses every thread, as in the TUI.
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 5}));
    rig.act(&mut ws, toggle);
    assert!(collapsed(&rig).iter().all(|(_, c)| *c));
    Ok(())
}

#[test]
fn a_comment_in_a_followed_markdown_pane_lands_on_the_blocks_first_line() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# T\n\n> a\n>\n> b\n")?;
    rig.drive(&["open", "--follow", &path]);
    rig.drive(&["ready"]);
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 3, "{acted}");
    Ok(())
}

#[test]
fn a_thread_on_a_blank_line_shows_in_a_markdown_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# Title\n\nalpha\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "2", "gap note"]);
    rig.until(
        &mut ws,
        "the card",
        |f| matches!(f, Frame::Json(v) if v["id"] == 1 && v.to_string().contains("gap note")),
    );
    let acted = rig.act(
        &mut ws,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.until(&mut ws, "the cursor on the blank line", |f| {
        matches!(f, Frame::Json(v) if (v["t"] == "rows" || v["t"] == "pane") && v["id"] == 1 && v["cursor"] == 1)
    });
    Ok(())
}

#[test]
fn a_markdown_pane_keeps_the_files_text() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "---\ntitle: x\n---\n\nRun --web \"q\" it's...\n\n> [!NOTE]\n> hi\n\n    indented\n\n[^2]: - item\n\nend\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(html.contains(r#"<span data-line="2">title: x"#), "{html}");
    assert!(html.contains(r#"Run --web "q" it's..."#), "{html}");
    // The alert keeps the TUI's title; indented code stays code.
    assert!(html.contains("Note</strong>"), "{html}");
    assert!(
        html.contains(r#"<pre><code><span data-line="10">indented"#),
        "{html}"
    );
    // A footnote that opens with a list keeps its label out of the next paragraph.
    assert!(html.contains("[2]: "), "{html}");
    assert!(!html.contains("[2]: end"), "{html}");
    Ok(())
}

#[test]
fn a_markdown_block_shows_its_removed_lines_above_it() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file(
        "doc.md",
        "# T\n\na\nb\nc\nd\n\n```rust\nx\ny\nz\n```\n",
        "# T\n\nA\nb\nd\n\n```rust\nx\nz\n```\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    // One row above the modified paragraph, one above the code block it was inside.
    let gaps: Vec<usize> = html
        .match_indices("1 line removed")
        .map(|(i, _)| i)
        .collect();
    let at = |s: &str| html.find(s).unwrap_or_else(|| panic!("no {s}: {html}"));
    assert_eq!(gaps.len(), 2, "{html}");
    assert!(gaps[0] < at("<p>A") && at("<p>A") < gaps[1], "{html}");
    assert!(gaps[1] < at("<pre"), "{html}");
    // A block's `data-row` can't equal a row's, so copy keeps them on separate lines.
    assert!(
        html.split(r#"<div class="blk"#).skip(1).all(|b| b
            .split('>')
            .next()
            .unwrap_or("")
            .contains(r#"data-row="b"#)),
        "{html}"
    );
    Ok(())
}

#[test]
fn arrow_up_from_an_inner_row_leaves_the_block() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# T\n\n> a\n>\n> b\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    // The highlight puts the cursor on the quote's last row, which isn't a stop.
    rig.drive(&["highlight", "--pane", "1", "5"]);
    let acted = rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": -1}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 2, "{acted}");
    Ok(())
}

#[test]
fn a_code_line_is_its_own_unit() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "# T\n\n```rust\na\nb\nc\n```\n\nafter\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    for line in 4..=6 {
        let span = format!(r#"<span data-line="{line}">"#);
        assert!(blocks(&html)[1].1.contains(&span), "{html}");
    }
    let acted = rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    assert_eq!(acted["ok"], true, "{acted}");
    let mut lines = vec![];
    for delta in [1, 1, 1, -1] {
        rig.act(
            &mut ws,
            json!({"act": "move_cursor", "pane": 1, "delta": delta}),
        );
        let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
        lines.push(acted["line"].clone());
    }
    assert_eq!(lines, [5, 6, 8, 6], "{lines:?}");
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 5}));
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 5, "{acted}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 5, "text": "line note"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 1}));
    rig.act(
        &mut ws,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    // The cursor and the thread show on the code line, not on the block.
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let code = &blocks(&html)[1].1;
    assert!(
        code.contains(r#"<span data-line="5" class="cur th">"#),
        "{code}"
    );
    assert!(
        !code.split('>').next().unwrap_or("").contains(" cur"),
        "{code}"
    );
    // The line keeps its cursor while another pane has focus, so the browser doesn't scroll.
    // A new view takes over from the old one and gets every pane fresh.
    rig.act(&mut view, json!({"act": "focus", "pane": 0}));
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        blocks(&html)[1]
            .1
            .contains(r#"<span data-line="5" class="cur th">"#),
        "{html}"
    );
    let acted = rig.act(&mut view, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows("L5  b");
    Ok(())
}

#[test]
fn a_table_row_is_its_own_unit() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "| a | b |\n|---|:-:|\n| 1 | 2 |\n| 3 | 4 |\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let table = &blocks(&html)[0].1;
    let head = table.split("<tbody>").next().unwrap_or("");
    assert!(!head.contains("<tr data-line"), "{table}");
    assert!(table.contains(r#"<tr data-line="3">"#), "{table}");
    // The second row keeps the column's alignment.
    assert!(
        table.contains(r#"<tr data-line="4"><td>3</td><td style="text-align: center">4</td>"#),
        "{table}"
    );
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["line"], 4, "{acted}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 4, "text": "row note"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let acted = rig.act(&mut ws, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows("L4  | 3 | 4 |");
    Ok(())
}

#[test]
fn a_nested_item_is_its_own_unit() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("doc.md", "- a\n  - b\n\n- c\n  - d\n    e\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "2", "note"]);
    let frame = rig
        .until(&mut ws, "the thread", |f| {
            matches!(f, Frame::Json(v) if v["t"] == "pane" && v["id"] == 1
                && v["html"].as_str().is_some_and(|h| h.contains("note")))
        })
        .json();
    let html = frame["html"].as_str().unwrap_or("");
    assert!(html.contains(r#"<li data-line="2" class="th">"#), "{html}");
    // The item's caret shows on the item, not in the block's gutter.
    assert!(!html.contains('▾'), "{html}");
    // A hand-wrapped item renders on one row in the TUI, so it has no line of its own.
    assert!(!html.contains(r#"<li data-line="5""#), "{html}");
    rig.act(
        &mut ws,
        json!({"act": "jump_thread", "pane": 1, "next": true}),
    );
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        html.contains(r#"<li data-line="2" class="cur th">"#),
        "{html}"
    );
    rig.act(&mut view, json!({"act": "toggle_thread", "pane": 1}));
    let mut view = rig.connect();
    let html = pane_frame(&mut rig, &mut view, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        html.contains(r#"<li data-line="2" class="cur th shut">"#),
        "{html}"
    );
    Ok(())
}

/// A stand-in editor that stays up and prints nothing; a `--headless` push exits at once.
fn fake_nvim() -> std::path::PathBuf {
    static ONCE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        // Under the target dir: a `.cmd` whose path has a space can't be spawned.
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("web_fake_nvim");
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(windows)]
        let (name, body) = (
            "nvim.cmd",
            "@echo off\r\nif \"%~1\"==\"--headless\" exit /b 0\r\necho fake\r\nping -n 30 127.0.0.1 >nul\r\n",
        );
        #[cfg(not(windows))]
        let (name, body) = (
            "nvim",
            "#!/bin/sh\n[ \"$1\" = --headless ] && exit 0\necho fake\nsleep 30\n",
        );
        let f = dir.join(name);
        std::fs::write(&f, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        f
    })
    .clone()
}

#[test]
fn an_editor_pane_streams_its_terminal_and_refuses_diff() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.editor = Ok(fake_nvim());
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    assert_eq!(rig.drive(&["open", "--edit", &path]), "1");
    let layout = rig
        .until(&mut ws, "editor pane", |f| {
            matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"].as_array().is_some_and(|p| p.len() == 2))
        })
        .json();
    assert_eq!(layout["panes"][1]["kind"], "editor");
    assert_eq!(layout["panes"][1]["pty"], true);
    rig.until(&mut ws, "editor bytes", |f| matches!(f, Frame::Bytes(1, _)));
    let acted = rig.act(&mut ws, json!({"act": "toggle_editor_view", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    // In its file view the pane is drawn as a file pane, and keeps its terminal.
    let layout = rig
        .until(&mut ws, "file view", |f| {
            matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["kind"] == "file")
        })
        .json();
    assert_eq!(layout["panes"][1]["pty"], true);
    let acted = rig.act(&mut ws, json!({"act": "toggle_diff", "pane": 1}));
    assert_eq!(acted["ok"], false);
    assert_eq!(acted["text"], NO_DIFF_IN_EDITOR);
    Ok(())
}

#[test]
fn a_frozen_pane_shows_in_the_layout_and_a_notice() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.tab
        .act(UserAction::Comment {
            pane: 1,
            line: 2,
            text: "x".into(),
        })
        .map_err(|e| anyhow!(e))?;
    pane_frame(&mut rig, &mut ws, 1);
    std::fs::write(&path, "changed on disk\n")?;
    rig.until(
        &mut ws,
        "frozen layout",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["frozen"] == true),
    );
    let notice = rig.until(
        &mut ws,
        "frozen notice",
        |f| matches!(f, Frame::Json(v) if v["t"] == "notice" && v["frozen"] == true),
    );
    let text = notice.json()["text"].as_str().unwrap_or("").to_string();
    assert!(text.contains("a.rs"), "{text}");
    Ok(())
}

#[test]
fn a_binary_frame_reaches_the_shell() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    let mut frame = 0u64.to_be_bytes().to_vec();
    frame.extend(b"echo binmark\r");
    ws.send(Message::binary(frame))?;
    rig.shell_shows("binmark");
    Ok(())
}

/// The bytes a paste writes, read raw by `cat -v`; ConPTY reads the markers itself, so unix only.
#[cfg(unix)]
#[test]
fn a_paste_reaches_the_shell_bracketed() -> Result<()> {
    let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
    cmd.args(["-c", "stty raw -echo; echo ready; cat -v"]);
    let mut rig = Rig::with(cmd)?;
    let mut ws = rig.connect();
    rig.shell_shows("ready");
    let acted = rig.act(&mut ws, json!({"act": "paste", "pane": 0, "text": "a\nb"}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows("^[[201~");
    assert!(
        rig.tab
            .pty
            .with_screen(|s| s.contents())
            .contains("^[[200~")
    );
    Ok(())
}

#[test]
fn long_headers_still_get_the_page() -> Result<()> {
    let rig = Rig::new()?;
    let mut s = TcpStream::connect(("127.0.0.1", rig.port))?;
    let cookie = "c".repeat(20 * 1024);
    write!(
        s,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n"
    )?;
    let mut resp = String::new();
    s.read_to_string(&mut resp)?;
    assert!(
        resp.starts_with("HTTP/1.1 200"),
        "{}",
        &resp[..resp.len().min(200)]
    );
    assert!(resp.contains("Content-Security-Policy: default-src 'self'"));
    assert!(resp.contains(r#"<div id="ws""#));
    Ok(())
}

#[test]
fn a_silent_socket_does_not_block_a_second_connection() -> Result<()> {
    let mut rig = Rig::new()?;
    let _silent = TcpStream::connect(("127.0.0.1", rig.port))?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    Ok(())
}

#[test]
fn a_stalled_view_never_blocks_the_shell() -> Result<()> {
    let rig = Rig::new()?;
    let (_, stalled) = rig.tab.pty.subscribe();
    let (_, counter) = rig.tab.pty.subscribe();
    // Each keystroke echoes as its own chunk; type until the reader has sent well past the bound.
    let (mut chunks, start) = (0, Instant::now());
    while chunks < 400 {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "only {chunks} chunks"
        );
        rig.tab.pty.write(b"x")?;
        thread::sleep(Duration::from_millis(2));
        chunks += counter.try_iter().count();
    }
    rig.tab.pty.write(b"\recho stallmark\r")?;
    let start = Instant::now();
    while !rig
        .tab
        .pty
        .with_screen(|s| s.contents())
        .contains("stallmark")
    {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the shell stalled"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let mut left = 0;
    loop {
        match stalled.try_recv() {
            Ok(_) => left += 1,
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => panic!("the stalled view is still subscribed"),
        }
    }
    assert!(left <= 256, "{left}");
    Ok(())
}

#[test]
fn a_second_tab_takes_over() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut first = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut first, 1);
    let mut second = rig.connect();
    rig.until(&mut first, "moved", |f| f.is("moved"));
    rig.until(&mut first, "closed", |f| matches!(f, Frame::Closed));
    rig.until(&mut second, "layout", |f| f.is("layout"));
    rig.until(&mut second, "replay", |f| matches!(f, Frame::Bytes(0, _)));
    let acted = rig.act(
        &mut second,
        json!({"act": "move_cursor", "pane": 1, "delta": 2}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    assert_eq!(rig.tab.panels[&1].cursor, 2);
    Ok(())
}

#[test]
fn closing_the_tab_keeps_the_workspace() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    send(&mut ws, json!({"view": {"cols": 100, "rows": 30}}));
    rig.until(
        &mut ws,
        "resized layout",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["cols"] == 100),
    );
    ws.close(None)?;
    drop(ws);
    for _ in 0..20 {
        rig.pump();
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(rig.web.area().map(|a| (a.width, a.height)), Some((100, 30)));
    let mut again = rig.connect();
    let layout = rig.until(&mut again, "layout", |f| f.is("layout")).json();
    assert_eq!(
        (layout["cols"].clone(), layout["rows"].clone()),
        (json!(100), json!(30))
    );
    rig.until(&mut again, "replay", |f| matches!(f, Frame::Bytes(0, _)));
    Ok(())
}

#[test]
fn quit_ends_the_loop_and_tells_the_view() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    send(&mut ws, json!({"quit": true}));
    let start = Instant::now();
    while rig.pump() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "quit didn't end the loop"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let Rig { web, .. } = rig;
    web.close();
    let start = Instant::now();
    loop {
        match read(&mut ws) {
            Some(f) if f.is("exit") => break,
            Some(Frame::Closed) => panic!("closed before exit"),
            _ => assert!(start.elapsed() < Duration::from_secs(5), "no exit frame"),
        }
    }
    Ok(())
}

#[test]
fn a_file_panes_threads_show_in_the_layout() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    let threads = |n: u64| move |f: &Frame| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["threads"] == n);
    rig.until(&mut ws, "no threads", threads(0));
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "2", "agent note"]);
    rig.until(&mut ws, "one thread", threads(1));
    Ok(())
}

#[test]
fn comment_and_submit_from_the_browser() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.act(
        &mut ws,
        json!({"act": "move_cursor", "pane": 1, "delta": 1}),
    );
    let begin = json!({"act": "begin_comment", "pane": 1});
    let acted = rig.act(&mut ws, begin.clone());
    assert_eq!(acted["ok"], true, "{acted}");
    assert_eq!(acted["text"], "", "{acted}");
    assert_eq!(acted["line"], 2, "{acted}");
    let hint = acted["hint"].as_str().unwrap_or("");
    assert!(
        hint.contains("L2") && hint.contains("Shift+Enter"),
        "{hint}"
    );
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 2, "text": "user note\nsecond line"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    assert_eq!(rig.tab.panels[&1].thread_count(), 1);
    // A newline starts a new row inside the thread's card.
    let html = pane_frame(&mut rig, &mut ws, 1)["html"].clone();
    let card = rows(html.as_str().unwrap_or(""));
    assert!(
        card.iter().all(|(_, t)| !t.contains('\n'))
            && card
                .iter()
                .any(|(_, t)| t.contains("second line") && !t.contains("user note")),
        "{card:?}"
    );
    // Again on the same line: the draft edits the user's comment.
    let acted = rig.act(&mut ws, begin);
    assert_eq!(acted["text"], "user note\nsecond line", "{acted}");
    let hint = acted["hint"].as_str().unwrap_or("");
    assert!(hint.contains("empty deletes"), "{hint}");
    let acted = rig.act(&mut ws, json!({"act": "begin_review", "pane": 1}));
    let hint = acted["hint"].as_str().unwrap_or("");
    assert!(hint.contains("review body"), "{hint}");
    let acted = rig.act(
        &mut ws,
        json!({"act": "submit", "pane": 1, "body": "overall"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows("[laura review · ");
    Ok(())
}

#[test]
fn a_draft_holds_the_agent() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, a) = file("a.rs", &five_lines())?;
    let (_dir_b, b) = file("b.rs", &five_lines())?;
    rig.drive(&["open", &a]);
    rig.drive(&["ready"]);
    let acted = rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    // A failed begin, here a review with no comments yet, keeps the open draft's hold.
    let acted = rig.act(&mut ws, json!({"act": "begin_review", "pane": 1}));
    assert_eq!(acted["ok"], false, "{acted}");
    assert!(rig.tab.hold);
    let open = rig.start(&["open", &b]);
    for _ in 0..20 {
        rig.pump();
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(rig.tab.focus, 1);
    assert!(!rig.tab.panels.contains_key(&2));
    assert!(open.try_recv().is_err(), "the agent's open didn't wait");
    send(&mut ws, json!({"hold": false}));
    assert!(rig.wait(&open).status.success());
    assert!(rig.tab.panels.contains_key(&2));
    Ok(())
}

#[test]
fn a_takeover_or_a_closed_view_releases_the_hold() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut first = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.until(&mut first, "layout", |f| f.is("layout"));
    send(&mut first, json!({"hold": true}));
    rig.idle(&mut first, 5);
    assert!(rig.tab.hold);
    let highlight = rig.start(&["highlight", "--pane", "1", "2"]);
    let mut second = rig.connect();
    rig.until(&mut second, "layout", |f| f.is("layout"));
    assert!(rig.wait(&highlight).status.success());
    send(&mut second, json!({"hold": true}));
    rig.idle(&mut second, 5);
    assert!(rig.tab.hold);
    second.close(None)?;
    drop(second);
    let highlight = rig.start(&["highlight", "--pane", "1", "3"]);
    assert!(rig.wait(&highlight).status.success());
    Ok(())
}

#[test]
fn ready_lists_web() -> Result<()> {
    let mut rig = Rig::new()?;
    let v: Value = serde_json::from_str(&rig.drive(&["ready"]))?;
    assert_eq!(v["experimental"], json!(["web"]), "{v}");
    Ok(())
}

#[test]
fn web_needs_the_flag() {
    Command::cargo_bin("laura")
        .unwrap()
        .arg("--web")
        .env_remove("LAURA_EXPERIMENTAL_WEB")
        .assert()
        .code(1)
        .stderr(predicates::str::contains(FLAG_OFF));
}

/// Kills the child on drop, so a failed assertion doesn't leave a workspace running.
struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn laura_web_serves_a_workspace_until_the_user_quits() -> Result<()> {
    isolate_home();
    let program = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
    let mut child = Kill(
        StdCommand::new(env!("CARGO_BIN_EXE_laura"))
            .args(["--web", "--", program])
            .env("LAURA_EXPERIMENTAL_WEB", "1")
            .env_remove("LAURA_TAB")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let stdout = child.0.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let url = rx.recv_timeout(Duration::from_secs(10))?;
    let (port, token) = port_token(url.trim());
    let mut ws = connect_to(
        port,
        Some(&token),
        Some(&format!("http://localhost:{port}")),
    )?;
    let start = Instant::now();
    while !read(&mut ws).is_some_and(|f| f.is("layout")) {
        assert!(start.elapsed() < Duration::from_secs(5), "no layout");
    }
    send(&mut ws, json!({"quit": true}));
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.0.try_wait()? {
            break s;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "laura --web kept running"
        );
        let _ = read(&mut ws);
    };
    assert!(status.success(), "{status}");
    Ok(())
}
