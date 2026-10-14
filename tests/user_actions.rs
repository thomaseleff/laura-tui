//! What the user does on the workspace goes through `Tab::act`: the gates, the source-line
//! mapping, untrusted fields, and `tick`'s upkeep. State, not pixels.

use std::process::Output;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use laura::tab::{NEEDS_READY, NO_THREADS};
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
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
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
        assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
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
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out: {what}"
        );
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
