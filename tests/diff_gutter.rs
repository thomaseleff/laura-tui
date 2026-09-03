//! Feat #26: gutter diff markers. A real git repo in a tempdir, driven through the
//! real seam (binary → socket → `Tab::drain`); assert on `panel.changes` state, not
//! pixels, per CLAUDE.md.

use std::path::Path;
use std::process::Command as StdCommand;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{ChangeKind, Rect, Tab};

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a real tab, draining until it exits; return trimmed stdout.
fn drive(tab: &mut Tab, args: &[&str]) -> String {
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
        // Identity + no signing so `commit` works on a bare CI account.
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

#[test]
fn markers_track_modify_add_delete() -> Result<()> {
    // 10-line committed file.
    let committed: String = (1..=10).map(|i| format!("line {i}\n")).collect();
    let (_dir, path) = repo_with("f.txt", &committed)?;

    // Modify line 2, delete line 5, insert a line after line 8.
    let edited = "\
line 1
line 2 CHANGED
line 3
line 4
line 6
line 7
line 8
inserted
line 9
line 10
";
    std::fs::write(&path, edited)?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let changes = &tab.panels[&id].changes;

    assert_eq!(changes[1], Some(ChangeKind::Modified), "line 2 modified");
    // The deleted line 5 marks the surviving line below the gap (new line 5, "line 6").
    assert_eq!(
        changes[4],
        Some(ChangeKind::Removed(1)),
        "deletion below gap"
    );
    assert_eq!(changes[7], Some(ChangeKind::Added), "inserted line");
    for (i, c) in changes.iter().enumerate() {
        if ![1, 4, 7].contains(&i) {
            assert_eq!(*c, None, "line {i} unchanged");
        }
    }

    // Reload picks up a further edit: also modify line 1.
    std::fs::write(&path, edited.replacen("line 1\n", "line 1 CHANGED\n", 1))?;
    tab.drain(area());
    assert_eq!(
        tab.panels[&id].changes[0],
        Some(ChangeKind::Modified),
        "reload refreshed markers"
    );
    Ok(())
}

#[test]
fn markdown_is_out_of_scope() -> Result<()> {
    let (_dir, path) = repo_with("doc.md", "# title\n\nbody\n")?;
    // Modify it so a diff exists — markers must still be empty (rendered projection).
    std::fs::write(&path, "# title\n\nbody changed\n")?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    assert!(
        tab.panels[&id].changes.is_empty(),
        "markdown panels carry no gutter markers"
    );
    Ok(())
}

#[test]
fn untracked_file_has_no_markers() -> Result<()> {
    let dir = tempfile::tempdir()?;
    git(dir.path(), &["init", "-q"]);
    let file = dir.path().join("new.txt");
    std::fs::write(&file, "brand new\n")?; // never added/committed
    let path = file.to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    assert!(
        tab.panels[&id].changes.is_empty(),
        "untracked file → git diff empty, no markers (v1 scope)"
    );
    Ok(())
}
