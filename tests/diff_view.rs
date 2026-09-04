//! Feat #18: inline diff view. A real git repo in a tempdir, driven through the real
//! seam (binary → socket → `Tab::drain`); assert on `panel.diff_view` + laid-out rows,
//! not pixels, per CLAUDE.md.

use std::path::Path;
use std::process::{Command as StdCommand, Output};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Rect, Tab};

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a real tab, draining until it exits; return the raw output.
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

/// Run `laura <args>`, asserting success; return trimmed stdout.
fn drive(tab: &mut Tab, args: &[&str]) -> String {
    let out = run(tab, args);
    assert!(
        out.status.success(),
        "laura {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Run `git <args>` in `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git spawns (test host needs git on PATH)");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Init a repo in a fresh tempdir and commit `name` with `body`. Returns the dir + file path.
fn repo_with(name: &str, body: &str) -> Result<(tempfile::TempDir, String)> {
    let dir = tempfile::tempdir()?;
    git(dir.path(), &["init", "-q"]);
    let file = dir.path().join(name);
    std::fs::write(&file, body)?;
    git(dir.path(), &["add", name]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    let path = file.to_str().unwrap().to_string();
    Ok((dir, path))
}

/// The plain text of every laid-out row, joined — what the diff view would render.
fn rows_text(tab: &Tab, id: u64) -> String {
    tab.panels[&id]
        .layout(78)
        .rows
        .iter()
        .map(|r| r.text())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn open_diff_shows_removed_text_then_toggles_off() -> Result<()> {
    let committed: String = (1..=6).map(|i| format!("line {i}\n")).collect();
    let (_dir, path) = repo_with("f.txt", &committed)?;

    // Modify line 2, delete line 4.
    std::fs::write(&path, "line 1\nline 2 CHANGED\nline 3\nline 5\nline 6\n")?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus", "--diff"]).parse()?;
    assert!(tab.panels[&id].diff_view, "opened straight into diff view");

    let text = rows_text(&tab, id);
    assert!(
        text.contains("-line 2"),
        "old line rendered as a `-` row:\n{text}"
    );
    assert!(
        text.contains("+line 2 CHANGED"),
        "new line as a `+` row:\n{text}"
    );
    assert!(
        text.contains("-line 4"),
        "deleted line 4 shown as a `-` row:\n{text}"
    );

    // Toggle off restores the normal view.
    let id_s = id.to_string();
    drive(&mut tab, &["diff", "--pane", &id_s, "--off"]);
    assert!(!tab.panels[&id].diff_view, "--off turns the diff view off");

    // Toggle back on (no --off = toggle).
    drive(&mut tab, &["diff", "--pane", &id_s]);
    assert!(tab.panels[&id].diff_view, "toggle turns it back on");
    Ok(())
}

#[test]
fn diff_on_clean_file_is_refused() -> Result<()> {
    // Committed and unmodified → nothing to diff.
    let (_dir, path) = repo_with("clean.txt", "line 1\nline 2\n")?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;

    let out = run(&mut tab, &["diff", "--pane", &id.to_string()]);
    assert!(
        !out.status.success(),
        "toggling on a clean file exits non-zero"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("nothing to diff"),
        "clean file refusal warns"
    );
    assert!(!tab.panels[&id].diff_view, "refused toggle is a no-op");
    Ok(())
}
