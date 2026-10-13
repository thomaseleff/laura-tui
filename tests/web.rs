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
        let tab = Tab::spawn(cmd, 24, 80)?;
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

    /// Run `laura <args>` against the workspace, pumping until it exits.
    fn run(&mut self, args: &[&str]) -> Output {
        let name = self.tab.socket.clone();
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (tx, rx) = mpsc::channel();
        let h = thread::spawn(move || {
            let out = Command::cargo_bin("laura")
                .unwrap()
                .args(&a)
                .env("LAURA_TAB", &name)
                .output()
                .unwrap();
            tx.send(out).unwrap();
        });
        let start = Instant::now();
        let out = loop {
            self.pump();
            if let Ok(out) = rx.try_recv() {
                break out;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
            thread::sleep(Duration::from_millis(5));
        };
        h.join().unwrap();
        out
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
fn ready_lists_web() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.web = true;
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
