//! The web app (#88) through its websocket, as a browser tab drives it: the auth gate, the frames a
//! view receives, the user's actions as `act` frames, and `laura --web` end to end. State and
//! frames, not pixels.
//!
//! Each module holds one feature's tests. This file holds the helpers that two or more modules call.

#[path = "../common/mod.rs"]
mod common;
#[path = "../common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::Command as StdCommand;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use assert_cmd::Command;
use laura::UserAction;
use laura::tab::{NO_DIFF_IN_EDITOR, NO_ROOM_TO_DOCK, NO_ROOM_TO_RESIZE};
use serde_json::{Value, json};
use tungstenite::Message;

use rig::*;

mod file_panes;
mod glance;
mod layout;
mod links;
mod markdown;
mod pages;
mod review;
mod shell;
mod start;
mod views;

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

/// The first `layout` frame that `want` accepts.
fn layout_when(rig: &mut Rig, ws: &mut Ws, what: &str, want: impl Fn(&Value) -> bool) -> Value {
    rig.until(
        ws,
        what,
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && want(v)),
    )
    .json()
}

/// The Nord theme's code colors, which the light web app never shows.
const NORD: [&str; 7] = [
    "#81a1c1", "#a3be8c", "#8fbcbb", "#88c0d0", "#b48ead", "#616e88", "#d8dee9",
];

const RUST: &str = "fn main() {\n    let s = \"hi\";\n}\n";

/// A stand-in editor that stays up and prints `fake` (after a bell on unix); a `--headless` push exits at once.
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
            "#!/bin/sh\n[ \"$1\" = --headless ] && exit 0\nprintf '\\007'\necho fake\nsleep 30\n",
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

/// `GET path` without the token; the raw response.
fn get(port: u16, path: &str) -> Result<String> {
    get_as(port, path, "")
}

/// [`get`] with `headers`, each line ending in `\r\n`.
fn get_as(port: u16, path: &str, headers: &str) -> Result<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{headers}\r\n")?;
    let mut resp = vec![];
    s.read_to_end(&mut resp)?;
    Ok(String::from_utf8_lossy(&resp).into_owned())
}

/// A fresh dir under the crate's `tmp/`, below the folder the tests run in, which Laura serves from.
fn served_dir() -> Result<tempfile::TempDir> {
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp");
    std::fs::create_dir_all(&tmp)?;
    Ok(tempfile::Builder::new().prefix("web").tempdir_in(tmp)?)
}

/// `docs/a.md` holding `md`, beside `img/s.png`, in a [`served_dir`].
fn image_doc(md: &str) -> Result<(tempfile::TempDir, String)> {
    let dir = served_dir()?;
    std::fs::create_dir_all(dir.path().join("docs"))?;
    std::fs::create_dir_all(dir.path().join("img"))?;
    std::fs::write(dir.path().join("img/s.png"), "PNGBYTES")?;
    let path = dir.path().join("docs/a.md");
    std::fs::write(&path, md)?;
    Ok((dir, path.to_string_lossy().into_owned()))
}

/// Pane `id`'s first frame's HTML, and its first `<img>`'s `src`.
fn img_src(rig: &mut Rig, ws: &mut Ws, id: u64) -> (String, String) {
    let html = pane_frame(rig, ws, id)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let src = html
        .split(r#"<img src=""#)
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap_or("")
        .to_string();
    (html, src)
}

/// The first `page` frame for pane `id` that `want` accepts.
fn page_frame(rig: &mut Rig, ws: &mut Ws, id: u64, want: impl Fn(&Value) -> bool) -> Value {
    rig.until(
        ws,
        "page frame",
        |f| matches!(f, Frame::Json(v) if v["t"] == "page" && v["id"] == id && want(v)),
    )
    .json()
}

/// A hint's `[keystroke, label]` pairs as one line: `key label · label`.
fn pairs(v: &Value) -> String {
    let items: Vec<String> = v
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let (k, l) = (p[0].as_str().unwrap_or(""), p[1].as_str().unwrap_or(""));
            if k.is_empty() {
                l.to_string()
            } else {
                format!("{k} {l}")
            }
        })
        .collect();
    items.join(" · ")
}

/// `laura --web` with its own `home`, and a view connected to it.
fn laura_web_in(home: &Path) -> Result<(Kill, Ws)> {
    laura_web_at(home, home)
}

/// `laura --web` started in `dir`, with its own `home`, and a view connected to it.
fn laura_web_at(dir: &Path, home: &Path) -> Result<(Kill, Ws)> {
    let (child, url) = laura_start(dir, &["--web", "--", PROGRAM], Some(home))?;
    let (port, token) = port_token(&url);
    let origin = format!("http://localhost:{port}");
    Ok((child, connect_to(port, Some(&token), Some(&origin))?))
}

/// The first `layout` frame on a raw view that `want` accepts.
fn raw_layout_when(ws: &mut Ws, what: &str, want: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        if let Some(Frame::Json(v)) = read(ws)
            && v["t"] == "layout"
            && want(&v)
        {
            return v;
        }
        assert!(start.elapsed() < common::DEADLINE, "no layout with {what}");
    }
}

/// A `review` event in `home`'s journal; `error` makes it a failed submit.
fn journal_review(home: &Path, error: bool) -> Result<()> {
    let dir = home.join(".laura/sessions");
    std::fs::create_dir_all(&dir)?;
    let mut event = json!({"type": "review", "ts": 1, "pane": 1, "path": "a.rs"});
    if error {
        event["error"] = json!("the shell exited");
    }
    std::fs::write(dir.join("x.ndjson"), format!("{event}\n"))?;
    Ok(())
}
