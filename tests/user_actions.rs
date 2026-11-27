//! What the user does on the workspace goes through `Tab::act`: the gates, the source-line
//! mapping, the input modes, untrusted fields, and `tick`'s upkeep. State, not pixels.

mod common;

use std::path::Path;
use std::process::{Command as StdCommand, Output};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use laura::tab::{
    Draft, Mode, NEEDS_READY, NO_COMMENT_IN_DIFF, NO_ROOM_TO_RESIZE, NO_ROOM_TO_UNPIN, NO_THREADS,
    Overlay,
};
use laura::{Rect, Tab, UserAction};

/// `act` errors are `String`s; a boxed error takes those, `io` and `anyhow` alike.
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// `ready` journals into `target/tmp/.laura`, not the real `~/.laura`.
fn isolate_home() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: once per binary; std's env lock serializes it with std's own readers (`Command` spawn, `env::var`).
    ONCE.call_once(|| unsafe {
        std::env::set_var("HOME", env!("CARGO_TARGET_TMPDIR"));
        std::env::set_var("USERPROFILE", env!("CARGO_TARGET_TMPDIR"));
    });
}

fn spawn_tab() -> Result<Tab> {
    isolate_home();
    let mut cmd = portable_pty::CommandBuilder::new(if cfg!(windows) {
        "cmd.exe"
    } else if cfg!(target_os = "macos") {
        "/bin/bash"
    } else {
        "/bin/sh"
    });
    // bash's line editor shows the paste markers and scrolls a long line sideways, so the screen
    // wouldn't hold a submitted inline review. /bin/sh can be dash, which rejects --noediting.
    if cfg!(target_os = "macos") {
        cmd.arg("--noediting");
    }
    Ok(Tab::spawn(cmd, 24, 80)?)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a real tab, draining (and replying) until it exits.
fn run(tab: &mut Tab, args: &[&str]) -> Output {
    let name = tab.socket.clone();
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
        tab.drain(area());
        if let Ok(out) = rx.try_recv() {
            break out;
        }
        assert!(start.elapsed() < common::DEADLINE, "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    out
}

/// `run`, asserting success; return trimmed stdout.
fn drive(tab: &mut Tab, args: &[&str]) -> String {
    let out = run(tab, args);
    assert!(
        out.status.success(),
        "laura {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Drain until `done` holds, or fail after 5 s.
fn drain_until(tab: &mut Tab, what: &str, done: impl Fn(&Tab) -> bool) {
    let start = Instant::now();
    while !done(tab) {
        assert!(start.elapsed() < common::DEADLINE, "timed out: {what}");
        tab.drain(area());
        thread::sleep(Duration::from_millis(20));
    }
}

/// A file named `name` holding `body`, opened as pane #1 of a fresh tab.
fn tab_with(name: &str, body: &str) -> Result<(Tab, tempfile::TempDir, String)> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(name);
    std::fs::write(&path, body)?;
    let path = path.to_string_lossy().into_owned();
    let mut tab = spawn_tab()?;
    assert_eq!(drive(&mut tab, &["open", "--no-focus", &path]), "1");
    Ok((tab, dir, path))
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
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn five_lines() -> String {
    (1..=5).map(|n| format!("line {n}\n")).collect()
}

fn comment(line: u32, text: &str) -> UserAction {
    UserAction::Comment {
        pane: 1,
        line,
        text: text.into(),
    }
}

fn shell_text(tab: &Tab) -> String {
    tab.pty.with_screen(|s| s.contents())
}

/// #245: in the TUI as in the web app, `F12` from a file pane moves the focus to the shell, and the
/// unlock moves it back. Under the help, the user hasn't reached the shell: the focus stays.
#[test]
fn f12_moves_the_focus_to_the_shell_and_back() -> Result<()> {
    let (mut tab, _dir, _) = tab_with("a.rs", &five_lines())?;
    tab.act(UserAction::Focus { pane: 1 })?;
    tab.act(UserAction::Lock)?;
    assert_eq!((tab.focus, tab.modes()), (0, vec![Mode::Locked]));
    tab.act(UserAction::Lock)?;
    assert_eq!((tab.focus, tab.mode()), (1, None));
    let help = Some(Overlay::Help);
    tab.act(UserAction::Overlay { overlay: help })?;
    tab.act(UserAction::Lock)?;
    assert_eq!(
        (tab.focus, tab.modes()),
        (1, vec![Mode::Help, Mode::Locked])
    );
    Ok(())
}

/// #245: a comment that Laura can't add keeps its draft, so the user can try again.
#[test]
fn a_failed_comment_keeps_its_draft() -> Result<()> {
    let (mut tab, _dir, _) = tab_with("a.rs", &five_lines())?;
    drive(&mut tab, &["ready"]);
    tab.act(UserAction::BeginComment { pane: 1 })?;
    let draft = Draft::Comment {
        pane: 1,
        line: 1,
        edit: false,
    };
    assert_eq!(tab.draft(), Some(&draft));
    tab.agent = false;
    assert_eq!(tab.act(comment(1, "x")), Err(NEEDS_READY.into()));
    assert_eq!(tab.draft(), Some(&draft));
    tab.agent = true;
    tab.act(comment(1, "x"))?;
    assert_eq!(tab.draft(), None);
    Ok(())
}

#[test]
fn inline_review_needs_ready() -> Result<()> {
    let (mut tab, _dir, _) = tab_with("a.rs", &five_lines())?;
    assert_eq!(tab.act(comment(3, "x")), Err(NEEDS_READY.into()));
    assert_eq!(
        tab.act(UserAction::BeginComment { pane: 1 }),
        Err(NEEDS_READY.into())
    );
    assert_eq!(
        tab.act(UserAction::BeginReview { pane: 1 }),
        Err(NEEDS_READY.into())
    );
    assert_eq!(tab.panels[&1].thread_count(), 0);
    drive(&mut tab, &["ready"]);
    assert_eq!(tab.act(comment(3, "x")), Ok(None));
    assert_eq!(tab.panels[&1].threads[0].line, 2);
    Ok(())
}

/// Diff rows aren't source lines: a removed row would shift the cursor's line.
#[test]
fn comments_need_the_diff_view_off() -> Result<()> {
    let dir = tempfile::tempdir()?;
    git(dir.path(), &["init", "-q"]);
    let file = dir.path().join("a.rs");
    std::fs::write(&file, five_lines())?;
    git(dir.path(), &["add", "a.rs"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    std::fs::write(
        &file,
        "line 1
line 3
line 4
line 5
",
    )?;
    let path = file.to_string_lossy().into_owned();
    let mut tab = spawn_tab()?;
    drive(&mut tab, &["open", "--no-focus", "--diff", &path]);
    drive(&mut tab, &["ready"]);
    assert_eq!(
        tab.act(UserAction::BeginComment { pane: 1 }),
        Err(NO_COMMENT_IN_DIFF.into())
    );
    assert_eq!(tab.act(comment(3, "x")), Err(NO_COMMENT_IN_DIFF.into()));
    assert_eq!(tab.panels[&1].thread_count(), 0);
    tab.act(UserAction::ToggleDiff { pane: 1 })?;
    tab.act(comment(3, "x"))?;
    assert_eq!(tab.panels[&1].threads[0].line, 2);
    Ok(())
}

#[test]
fn comment_adds_edits_deletes_and_replies() -> Result<()> {
    let (mut tab, _dir, _) = tab_with("a.rs", &five_lines())?;
    drive(&mut tab, &["ready"]);
    tab.act(comment(2, "first"))?;
    let begin = UserAction::BeginComment { pane: 1 };
    assert_eq!(tab.act(begin), Ok(Some("first".into())));
    tab.act(comment(2, "second"))?;
    let p = &tab.panels[&1];
    assert_eq!(
        (p.thread_count(), p.threads[0].root.body.as_str()),
        (1, "second")
    );
    tab.act(comment(2, ""))?;
    assert_eq!(tab.panels[&1].thread_count(), 0);
    // On a thread the agent annotated last, the user's comment is a reply.
    drive(&mut tab, &["comment", "--pane", "1", "2", "x"]);
    let begin = UserAction::BeginComment { pane: 1 };
    assert_eq!(tab.act(begin), Ok(Some(String::new())));
    tab.act(comment(2, "reply"))?;
    let t = &tab.panels[&1].threads[0];
    assert_eq!((t.replies.len(), t.replies[0].body.as_str()), (1, "reply"));
    Ok(())
}

#[test]
fn submit_writes_the_inline_review_into_the_shell() -> Result<()> {
    let (mut tab, _dir, _) = tab_with("a.rs", &five_lines())?;
    drive(&mut tab, &["ready"]);
    let submit = || UserAction::Submit {
        pane: 1,
        body: "overall".into(),
    };
    let begin = || UserAction::BeginReview { pane: 1 };
    assert_eq!(tab.act(begin()), Err(NO_THREADS.into()));
    assert_eq!(tab.act(submit()), Err(NO_THREADS.into()));
    tab.act(comment(2, "tighten this"))?;
    assert_eq!(tab.act(begin()), Ok(None));
    tab.act(UserAction::Focus { pane: 1 })?;
    assert_eq!(tab.act(submit()), Ok(None));
    assert_eq!(tab.panels[&1].thread_count(), 0);
    assert_eq!(tab.focus, 0);
    drain_until(&mut tab, "inline review in the shell", |t| {
        shell_text(t).contains("[laura review")
    });
    Ok(())
}

/// Two panes on one file: the header and the journal name the pane the user submitted from (#85).
#[test]
fn each_inline_review_names_its_pane() -> Result<()> {
    let (mut tab, _dir, path) = tab_with("a.rs", &five_lines())?;
    assert_eq!(drive(&mut tab, &["open", "--no-focus", &path]), "2");
    let ready: serde_json::Value = serde_json::from_str(&drive(&mut tab, &["ready"]))?;
    let journal = ready["journal"].as_str().ok_or("ready names no journal")?;
    tab.act(UserAction::Comment {
        pane: 2,
        line: 2,
        text: "x".into(),
    })?;
    tab.act(UserAction::Submit {
        pane: 2,
        body: String::new(),
    })?;
    drain_until(&mut tab, "pane #2 in the header", |t| {
        shell_text(t).contains("[laura review · pane #2 · ")
    });
    let body = std::fs::read_to_string(journal)?;
    let review: serde_json::Value = body
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["type"] == "review")
        .ok_or("no review event")?;
    assert_eq!(review["pane"], 2, "{review}");
    Ok(())
}

#[test]
fn refresh_drops_the_threads_and_reloads() -> Result<()> {
    let (mut tab, _dir, path) = tab_with("a.rs", &five_lines())?;
    drive(&mut tab, &["ready"]);
    tab.act(comment(2, "x"))?;
    std::fs::write(&path, "changed on disk, and longer than before\n")?;
    drain_until(&mut tab, "pane frozen", |t| t.panels[&1].source_changed);
    tab.act(UserAction::Refresh { pane: 1 })?;
    let p = &tab.panels[&1];
    assert_eq!(p.thread_count(), 0);
    assert!(p.content.contains("changed on disk"), "{}", p.content);
    Ok(())
}

#[test]
fn a_comment_inside_a_markdown_paragraph_lands_on_its_row() -> Result<()> {
    let (mut tab, _dir, _) = tab_with("a.md", "first half\nsecond half\n")?;
    drive(&mut tab, &["ready"]);
    tab.act(comment(2, "x"))?;
    let p = &tab.panels[&1];
    assert_eq!(p.threads[0].line, 0);
    assert_eq!((p.cursor, p.cursor_line()), (0, 1));
    Ok(())
}

#[test]
fn tick_follows_open_and_close() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("a.rs");
    std::fs::write(&path, five_lines())?;
    let mut tab = spawn_tab()?;
    drive(&mut tab, &["open", &path.to_string_lossy()]);
    let rects = tab.tick(area());
    assert_eq!(tab.focus, 1);
    assert!(rects.contains_key(&0) && rects.contains_key(&1));
    drive(&mut tab, &["close", "1"]);
    tab.tick(area());
    assert_eq!(tab.focus, 0);
    Ok(())
}

#[test]
fn untrusted_fields_return_errors_or_clamp() -> Result<()> {
    let (mut tab, _dir, _) = tab_with("a.rs", &five_lines())?;
    drive(&mut tab, &["ready"]);
    for action in [
        UserAction::Comment {
            pane: 9,
            line: 1,
            text: "x".into(),
        },
        UserAction::Comment {
            pane: 0,
            line: 1,
            text: "x".into(),
        },
        UserAction::Focus { pane: 9 },
        UserAction::Input {
            pane: 1,
            bytes: b"x".to_vec(),
        },
        UserAction::Close { pane: 0 },
    ] {
        assert!(tab.act(action).is_err());
    }
    tab.act(comment(0, "x"))?;
    assert_eq!(tab.panels[&1].threads[0].line, 0);
    tab.act(UserAction::MoveCursor {
        pane: 1,
        delta: isize::MAX,
    })?;
    assert_eq!(tab.panels[&1].cursor, tab.panels[&1].line_count() - 1);
    tab.act(UserAction::MoveCursor {
        pane: 1,
        delta: isize::MIN,
    })?;
    assert_eq!(tab.panels[&1].cursor, 0);
    tab.act(UserAction::ScrollH {
        pane: 1,
        delta: isize::MAX,
    })?;
    Ok(())
}

/// The bytes a paste writes, read raw by `cat -v`; ConPTY reads the markers itself, so unix only.
#[cfg(unix)]
#[test]
fn paste_reaches_the_shell_bracketed() -> Result<()> {
    isolate_home();
    let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
    cmd.args(["-c", "stty raw -echo; echo ready; cat -v"]);
    let mut tab = Tab::spawn(cmd, 24, 80)?;
    drain_until(&mut tab, "raw shell", |t| shell_text(t).contains("ready"));
    tab.act(UserAction::Paste {
        pane: 0,
        text: "a\nb".into(),
    })?;
    drain_until(&mut tab, "bracketed paste", |t| {
        let s = shell_text(t);
        s.contains("^[[200~") && s.contains("^[[201~")
    });
    Ok(())
}

/// The protocol has no submit message: the agent can't submit the user's inline review.
#[test]
fn the_agent_cannot_act_as_the_user() -> Result<()> {
    use interprocess::local_socket::{GenericNamespaced, Stream, prelude::*};
    use std::io::{BufRead, BufReader, Write};

    let (mut tab, _dir, _) = tab_with("a.rs", &five_lines())?;
    drive(&mut tab, &["ready"]);
    tab.act(comment(1, "mine"))?;
    let name = tab.socket.as_str().to_ns_name::<GenericNamespaced>()?;
    let mut conn = BufReader::new(Stream::connect(name)?);
    conn.get_mut()
        .write_all(b"{\"type\":\"submit\",\"pane\":1,\"body\":\"x\"}\n")?;
    // The host closes without a reply once the frame doesn't parse; a parsed one waits for `drain`.
    let reply = thread::spawn(move || conn.read_line(&mut String::new()));
    drain_until(&mut tab, "host reply", |_| reply.is_finished());
    reply.join().unwrap()?;
    assert_eq!(tab.panels[&1].thread_count(), 1);
    assert!(!shell_text(&tab).contains("laura review"));
    Ok(())
}

/// #187: the TUI always shows the shell as a pane. `laura layout` has no `drawer`, and the web
/// app's **Show the shell as a pane** and
/// **Show the shell in a drawer** change nothing.
#[test]
fn the_tui_has_no_drawer() -> Result<()> {
    let (mut tab, _dir, _path) = tab_with("a.rs", &five_lines())?;
    let before = drive(&mut tab, &["layout"]);
    assert!(!before.contains("drawer"), "{before}");
    let layout = tab.layout.clone();
    tab.act(UserAction::Dock)?;
    tab.act(UserAction::Drawer)?;
    assert_eq!(tab.layout, layout);
    assert_eq!(drive(&mut tab, &["layout"]), before);
    Ok(())
}

#[test]
fn a_file_path_in_the_shell_opens_in_a_pane() -> Result<()> {
    let mut tab = spawn_tab()?;
    tab.drain(area());
    let open = |path: String| UserAction::OpenPath {
        path,
        line: Some(3),
        diff: false,
    };
    tab.act(open("src/lib.rs".into()))?;
    tab.tick(area());
    let lib = std::env::current_dir()?.join("src/lib.rs");
    let ids: Vec<_> = tab.panels.keys().copied().collect();
    assert_eq!(ids.len(), 1);
    let p = &tab.panels[&ids[0]];
    assert_eq!(Path::new(&p.path), lib);
    assert_eq!((tab.focus, p.cursor_line()), (ids[0], 3));
    // The absolute path names the same file: Laura focuses its pane.
    tab.focus = 0;
    tab.act(open(lib.to_string_lossy().into_owned()))?;
    assert_eq!((tab.panels.len(), tab.focus), (1, ids[0]));
    // On Windows a `\` path names the same file as the `/` path.
    #[cfg(windows)]
    {
        tab.focus = 0;
        tab.act(open("src\\lib.rs".into()))?;
        assert_eq!((tab.panels.len(), tab.focus), (1, ids[0]));
    }
    Ok(())
}

#[test]
fn a_file_path_in_the_shell_centers_its_line_once() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("long.txt");
    let body: String = (1..=400).map(|n| format!("line {n}\n")).collect();
    std::fs::write(&file, body)?;
    let mut tab = spawn_tab()?;
    tab.drain(area());
    let open = |line| UserAction::OpenPath {
        path: file.to_string_lossy().into_owned(),
        line: Some(line),
        diff: false,
    };
    tab.act(open(300))?;
    tab.tick(area());
    let id = tab.focus;
    let off = |tab: &Tab| {
        let p = &tab.panels[&id];
        let layout = p.layout(80);
        (p.scroll_offset(&layout, 20), layout.starts[p.cursor])
    };
    // Line 300 shows in the middle of 20 rows, with rows below it.
    let (top, start) = off(&tab);
    assert_eq!((top, start), (299 - 9, 299));
    // A click on a line on screen, and a jump to one, keep the view.
    tab.act(UserAction::SetCursor {
        pane: id,
        line: 305,
    })?;
    assert_eq!(off(&tab).0, top);
    tab.act(open(302))?;
    assert_eq!(off(&tab), (top, 301));
    Ok(())
}

#[test]
fn a_file_path_that_is_not_a_file_returns_the_link_notice() -> Result<()> {
    let mut tab = spawn_tab()?;
    tab.tick(area());
    let open = |path: &str| UserAction::OpenPath {
        path: path.into(),
        line: None,
        diff: false,
    };
    assert_eq!(
        tab.act(open("src/nope.rs")),
        Err("⚠ The link target src/nope.rs does not exist.".into())
    );
    assert_eq!(
        tab.act(open("src")),
        Err("⚠ The link target src is a folder.".into())
    );
    assert!(tab.panels.is_empty());
    Ok(())
}

/// #211: in the TUI, a drag of the border between two panes pins their split, as in the web app.
/// `Tab::handles` gives the TUI each border and the range a drag may reach; the mouse can't be
/// driven here, so this sends the action the drag sends.
#[test]
fn a_resize_in_the_tui_pins_the_split() -> Result<()> {
    let (mut tab, dir, _) = tab_with("a.md", &five_lines())?;
    let b = dir.path().join("b.md");
    std::fs::write(&b, five_lines())?;
    drive(&mut tab, &["open", "--no-focus", &b.to_string_lossy()]);
    tab.drain(area());
    let before = tab.rects(area());
    let handles = tab.handles(area());
    assert_eq!(handles.len(), 2, "{handles:?}");
    let root = &handles[0];
    assert_eq!((&root.first[..], &root.second[..]), (&[0][..], &[1, 2][..]));
    assert!(!root.pinned && root.min < 30 && 30 < root.max, "{root:?}");

    tab.act(UserAction::Resize {
        a: 0,
        b: 1,
        ratio: 30,
    })?;
    let report: serde_json::Value = serde_json::from_str(&drive(&mut tab, &["layout"]))?;
    assert_eq!(
        report["panes"][0]["rect"]["width"],
        120 * 30 / 100,
        "{report}"
    );
    for id in 0..=2 {
        assert_eq!(report["panes"][id]["pinned"], true, "{report}");
    }
    assert!(tab.handles(area())[0].pinned);

    tab.act(UserAction::Unpin { a: 0, b: 1 })?;
    assert_eq!(tab.rects(area()), before);
    assert!(!tab.handles(area())[0].pinned);
    Ok(())
}

/// #211: a window that got narrower leaves a pane below the smallest size. The user can still
/// make that pane larger, but not smaller, and Laura's own size, which doesn't fit, stays off.
#[test]
fn a_too_small_pane_can_still_grow() -> Result<()> {
    let (mut tab, dir, _) = tab_with("a.md", &five_lines())?;
    let b = dir.path().join("b.md");
    std::fs::write(&b, five_lines())?;
    let b = b.to_string_lossy();
    drive(
        &mut tab,
        &[
            "open",
            "--no-focus",
            &b,
            "--split",
            "1",
            "--dir",
            "h",
            "--ratio",
            "10",
        ],
    );
    let narrow = Rect::new(0, 0, 40, 40);
    tab.drain(narrow);
    assert_eq!(tab.rects(narrow)[&2].width, 2);
    let line = |tab: &Tab| {
        tab.handles(narrow)
            .into_iter()
            .find(|h| h.first == [1] && h.second == [2])
            .expect("the line between panes 1 and 2")
    };
    let h = line(&tab);
    assert!(h.min < 90 && h.max < 95, "{h:?}");
    // The other lines still move.
    let root = &tab.handles(narrow)[0];
    assert!(root.min < root.ratio && root.ratio < root.max, "{root:?}");
    tab.act(UserAction::Resize {
        a: 0,
        b: 1,
        ratio: root.ratio + 2,
    })?;

    let resize = |ratio| UserAction::Resize { a: 1, b: 2, ratio };
    assert_eq!(tab.act(resize(95)), Err(NO_ROOM_TO_RESIZE.into()));
    assert_eq!(tab.rects(narrow)[&2].width, 2);
    tab.act(resize(50))?;
    assert!(tab.rects(narrow)[&2].width >= 3);
    // Pane 2 fits now, so the line's range shrinks: a drag reads the range again at each step.
    let after = line(&tab);
    assert!(after.max < h.max, "{h:?} → {after:?}");

    let grown = tab.rects(narrow);
    assert_eq!(
        tab.act(UserAction::Unpin { a: 1, b: 2 }),
        Err(NO_ROOM_TO_UNPIN.into())
    );
    assert_eq!(tab.rects(narrow), grown);
    Ok(())
}

/// #211: while the user drags the line beside the shell, the shell keeps its terminal size, so the
/// program in it doesn't redraw at each step. Laura fits the shell once the user stops.
#[test]
fn the_shell_keeps_its_size_while_the_user_drags() -> Result<()> {
    let (mut tab, _dir, _) = tab_with("a.md", &five_lines())?;
    tab.tick(area());
    let before = tab.pty_size();
    tab.act(UserAction::Resize {
        a: 0,
        b: 1,
        ratio: 30,
    })?;
    tab.tick(area());
    assert_eq!(tab.pty_size(), before);
    let want = tab.rects(area())[&0].width - 2;
    assert_ne!(before.1, want);
    let start = Instant::now();
    while tab.pty_size().1 != want {
        assert!(start.elapsed() < common::DEADLINE, "the shell never fit");
        thread::sleep(Duration::from_millis(20));
        tab.tick(area());
    }
    Ok(())
}
