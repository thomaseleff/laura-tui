//! The web app (#88) through its websocket, as a browser tab drives it: the auth gate, the frames a
//! view receives, the user's actions as `act` frames, and `laura --web` end to end. State and
//! frames, not pixels.

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
use laura::tab::{NO_DIFF_IN_EDITOR, NO_ROOM_TO_DOCK};
use serde_json::{Value, json};
use tungstenite::Message;

use rig::*;

mod file_panes;
mod layout;
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

#[test]
fn a_heading_link_moves_the_cursor() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) =
        image_doc("# A\n\n[s](#setup)\n\n## Setup\n\nrun it\n\n## Two {#Install}\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#setup"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&1].cursor_line(), 5);
    rig.tab
        .panels
        .get_mut(&1)
        .expect("pane 1 is open")
        .set_cursor_line(1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#Setup"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&1].cursor_line(), 5);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#install"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&1].cursor_line(), 9);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#nope"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    rig.until(&mut ws, "heading notice", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "notice" && v["pane"] == 1
            && v["text"] == "⚠ The file a.md has no heading #nope.")
    });
    Ok(())
}

#[test]
fn a_link_to_a_missing_file_shows_a_notice() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = image_doc("[n](nope.md)\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "nope.md"}),
    );
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(
        r["text"], "⚠ The link target nope.md does not exist.",
        "{r}"
    );
    assert!(!rig.tab.panels.contains_key(&2));
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "../img/"}),
    );
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(r["text"], "⚠ The link target ../img/ is a folder.", "{r}");
    assert!(!rig.tab.panels.contains_key(&2));
    // A rooted path to a real file: the full path on Unix, drive-less on Windows.
    let rooted = &path[path.find(['/', '\\']).unwrap_or(0)..];
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": rooted}),
    );
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(
        r["text"],
        format!("⚠ The link target {rooted} does not exist."),
        "{r}"
    );
    assert!(!rig.tab.panels.contains_key(&2));
    Ok(())
}

#[test]
fn a_link_in_a_full_workspace_shows_a_notice() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[x](x.md)\n")?;
    std::fs::write(dir.path().join("docs/x.md"), "x\n")?;
    let y = dir.path().join("y.txt");
    std::fs::write(&y, "y\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let mut stderr = String::new();
    for n in 0..40 {
        let f = dir.path().join(format!("f{n}.txt"));
        std::fs::write(&f, "f\n")?;
        let out = rig.run(&["open", &f.to_string_lossy()]);
        if !out.status.success() {
            stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            break;
        }
    }
    // The agent keeps its own text.
    assert!(stderr.contains("lower --ratio"), "{stderr}");
    let panes = rig.tab.panels.len();
    let no_room = "⚠ No room for another pane. Close a pane, then click the link again.";
    let r = rig.act(
        &mut ws,
        json!({"act": "open_path", "pane": 0, "path": y.to_string_lossy()}),
    );
    assert_eq!(
        (r["ok"].clone(), r["text"].clone()),
        (json!(false), json!(no_room))
    );
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "x.md"}),
    );
    assert_eq!(
        (r["ok"].clone(), r["text"].clone()),
        (json!(false), json!(no_room))
    );
    assert_eq!(rig.tab.panels.len(), panes);
    Ok(())
}

#[test]
fn a_link_to_a_file_in_an_editor_pane_focuses_it() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.editor = Ok(fake_nvim());
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[b](b.md#two)\n")?;
    let b = dir.path().join("docs/b.md");
    std::fs::write(&b, "# B\n\n## Two\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["open", "--edit", &b.to_string_lossy()]);
    pane_frame(&mut rig, &mut ws, 1);
    rig.tab.focus = 1;
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "b.md#two"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.focus, 2);
    assert_eq!(rig.tab.panels[&2].cursor_line(), 3);
    Ok(())
}

#[test]
fn a_missing_heading_in_another_pane_shows_its_notice_there() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[b](b.md#nope)\n")?;
    let b = dir.path().join("docs/b.md");
    std::fs::write(&b, "# B\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["open", &b.to_string_lossy()]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "b.md#nope"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.focus, 2);
    rig.until(&mut ws, "heading notice in pane 2", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "notice" && v["pane"] == 2
            && v["text"] == "⚠ The file b.md has no heading #nope.")
    });
    Ok(())
}
