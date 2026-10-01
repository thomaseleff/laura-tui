//! Editor panes through the CLI, with a fake `nvim`: the gate, the pane Neovim runs in, `--panel
//! N --edit` keeping threads, and what an exit leaves behind. State, not pixels.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{OnceLock, mpsc};
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

/// The fake editors, written once before any test spawns one (rewriting a script another test is
/// executing fails with ETXTBSY on Linux). Under the target dir: a `.cmd` whose path has a space
/// can't be spawned. Both append their argv to `argv_log()`; a `--headless` push exits at once.
fn fakes() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fake_nvim");
        let log = argv_log().display().to_string();
        #[cfg(windows)]
        let (name, head, stay, exit) = (
            "nvim.cmd",
            // Retry the append: another fake holding the log open fails the redirect.
            format!(
                "@echo off\r\n:log\r\necho %* >>\"{log}\" 2>nul || (ping -n 1 127.0.0.1 >nul & goto log)\r\nif \"%~1\"==\"--headless\" exit /b 0\r\n"
            ),
            "echo fake %LAURA_TAB%\r\nping -n 30 127.0.0.1 >nul\r\n",
            "echo bye\r\n",
        );
        #[cfg(not(windows))]
        let (name, head, stay, exit) = (
            "nvim",
            format!("#!/bin/sh\necho \"$@\" >>'{log}'\n[ \"$1\" = --headless ] && exit 0\n"),
            "echo \"fake $LAURA_TAB\"\nsleep 30\n",
            "echo bye\n",
        );
        for (kind, body) in [("stay", stay), ("exit", exit)] {
            let d = dir.join(kind);
            std::fs::create_dir_all(&d).unwrap();
            let f = d.join(name);
            std::fs::write(&f, format!("{head}{body}")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        dir
    })
}

/// Every fake launch and push, one argv per line. Shared by the tests: filter by the tab's socket,
/// which names the listen address.
fn argv_log() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("fake_nvim_argv.log")
}

/// Logged argv lines for `tab`'s editor panes, quotes stripped (a batch file sees `"` as `""`).
fn logged(tab: &Tab) -> Vec<String> {
    std::fs::read_to_string(argv_log())
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains(&tab.socket))
        .map(|l| l.replace('"', ""))
        .collect()
}

/// The fake that prints `fake $LAURA_TAB` and stays up, or the one that exits at once.
fn fake(kind: &str) -> PathBuf {
    fakes()
        .join(kind)
        .join(if cfg!(windows) { "nvim.cmd" } else { "nvim" })
}

/// A ten-line file to edit, under the target dir like the fakes.
fn file() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let path = dir.path().join("a.rs");
    let body: String = (1..=10).map(|n| format!("line {n}\n")).collect();
    std::fs::write(&path, body).unwrap();
    (dir, path.to_string_lossy().into_owned())
}

/// Pane `id`'s entry in `laura layout`.
fn pane(tab: &mut Tab, id: u64) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(&drive(tab, &["layout"])).unwrap();
    v["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"].as_u64() == Some(id))
        .cloned()
        .unwrap_or_default()
}

#[test]
fn edit_is_refused_without_editor_support() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    let (_dir, path) = file();
    let before = drive(&mut tab, &["layout"]);
    let out = run(&mut tab, &["open", "--edit", &path]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(laura::editor::FLAG_OFF), "{stderr}");
    assert_eq!(drive(&mut tab, &["layout"]), before);
    Ok(())
}

#[test]
fn ready_lists_editor_panes_only_when_on() -> Result<()> {
    let mut tab = spawn_tab()?;
    let out = run(&mut tab, &["ready"]);
    assert!(out.status.success());
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    tab.editor = Ok(fake("stay"));
    let out = run(&mut tab, &["ready"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("experimental: editor panes are on"),
        "{stderr}"
    );
    // stdout stays the bare journal path: skills capture it.
    assert!(!String::from_utf8_lossy(&out.stdout).contains("experimental"));
    Ok(())
}

#[test]
fn a_failed_launch_leaves_no_bootstrap() -> Result<()> {
    let mut tab = spawn_tab()?;
    // Neovim removed after Laura resolved it.
    let exe = if cfg!(windows) { "nvim.exe" } else { "nvim" };
    tab.editor = Ok(Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("no_nvim")
        .join(exe));
    let (_dir, path) = file();
    let before = drive(&mut tab, &["layout"]);
    let out = run(&mut tab, &["open", "--edit", &path]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("couldn't edit"), "{stderr}");
    assert_eq!(drive(&mut tab, &["layout"]), before);
    let lua = std::env::temp_dir().join(format!("{}-1.lua", tab.socket));
    assert!(!lua.exists(), "{}", lua.display());
    Ok(())
}

#[test]
fn open_edit_runs_neovim_in_a_pane_without_focus() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (_dir, path) = file();
    assert_eq!(drive(&mut tab, &["open", "--edit", &path]), "1");
    let p = pane(&mut tab, 1);
    assert_eq!(p["kind"], "editor");
    assert_eq!(p["path"], path.as_str());
    assert_eq!(tab.pending_focus, None);
    assert_eq!(tab.focus, 0);
    // The script's output reaches the pane, and it saw the tab's LAURA_TAB.
    let want = format!("fake {}", tab.socket);
    drain_until(&mut tab, "fake editor output", |t| {
        t.editors[&1]
            .pty
            .with_screen(|s| s.contents())
            .contains(&want)
    });
    // An agent comment lands on the editor pane's file panel.
    drive(&mut tab, &["comment", "--pane", "1", "3", "check this"]);
    assert_eq!(tab.panels[&1].thread_count(), 1);
    Ok(())
}

#[test]
fn panel_edit_attaches_in_place_keeping_threads() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (_dir, path) = file();
    assert_eq!(drive(&mut tab, &["open", "--no-focus", &path]), "1");
    drive(&mut tab, &["comment", "--pane", "1", "2", "before"]);
    let before = pane(&mut tab, 1);
    assert_eq!(before["kind"], "panel");
    assert_eq!(
        drive(&mut tab, &["open", "--panel", "1", "--edit", &path]),
        "1"
    );
    let after = pane(&mut tab, 1);
    assert_eq!(after["kind"], "editor");
    assert_eq!(after["rect"], before["rect"]);
    assert_eq!(tab.panels[&1].thread_count(), 1);
    assert!(!tab.editors[&1].pty.has_exited());
    Ok(())
}

#[test]
fn panel_edit_on_another_file_keeps_its_bootstrap() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (dir, a) = file();
    let b = dir.path().join("b.rs");
    std::fs::copy(&a, &b)?;
    let b = b.to_string_lossy().into_owned();
    drive(&mut tab, &["open", "--edit", &a]);
    drive(&mut tab, &["open", "--panel", "1", "--edit", &b]);
    // The old Neovim's cleanup ran before the new one wrote its bootstrap, not after.
    let lua = std::env::temp_dir().join(format!("{}-1.lua", tab.socket));
    assert!(lua.exists());
    let p = pane(&mut tab, 1);
    assert_eq!(p["path"], b.as_str());
    assert_eq!(p["kind"], "editor");
    Ok(())
}

#[test]
fn editor_panes_refuse_highlight_and_diff() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (dir, a) = file();
    let b = dir.path().join("b.rs");
    std::fs::copy(&a, &b)?;
    let b = b.to_string_lossy().into_owned();
    drive(&mut tab, &["open", "--edit", &a]);
    let before = drive(&mut tab, &["layout"]);
    for args in [
        &["open", &b, "--edit", "--highlight", "2"][..],
        &["open", "--edit", "--diff", &b],
        &["open", &b, "--panel", "1", "--edit", "--highlight", "2"],
        &["open", "--panel", "1", "--edit", "--diff", &a],
        &["highlight", "--pane", "1", "2"],
        &["highlight", "--pane", "1", "--off"],
        &["diff", "--pane", "1"],
    ] {
        let out = run(&mut tab, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(laura::editor::SHOWS_NEITHER),
            "{args:?}: {stderr}"
        );
        assert_eq!(drive(&mut tab, &["layout"]), before, "{args:?}");
    }
    let out = run(&mut tab, &["open", "--no-focus", &a]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("already open in editor pane #1"),
        "{stderr}"
    );
    assert!(!stderr.contains("laura highlight"), "{stderr}");
    Ok(())
}

#[test]
fn panel_edit_on_the_focused_pane_opens_the_file_view() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (_dir, path) = file();
    drive(&mut tab, &["open", &path]);
    tab.focus = 1;
    drive(&mut tab, &["open", "--panel", "1", "--edit", &path]);
    assert_eq!(tab.focus, 1);
    assert!(!tab.editors[&1].editor_view);
    Ok(())
}

#[test]
fn panel_edit_over_an_editor_pane_keeps_its_view() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (dir, path) = file();
    let b = dir.path().join("b.rs");
    std::fs::write(&b, "b\n")?;
    let b = b.to_string_lossy().into_owned();
    drive(&mut tab, &["open", "--edit", &path]);
    tab.focus = 1;
    assert!(tab.editors[&1].editor_view);
    drive(&mut tab, &["open", "--panel", "1", "--edit", &b]);
    assert!(tab.editors[&1].editor_view);
    Ok(())
}

#[test]
fn becoming_an_editor_pane_drops_highlight_and_diff() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (dir, path) = file();
    for args in [&["init"][..], &["add", "."], &["commit", "-m", "init"]] {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["-c", "user.email=t@t", "-c", "user.name=t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .expect("git spawns (test host needs git on PATH)");
        assert!(out.status.success(), "git {args:?}");
    }
    std::fs::write(&path, "changed\n")?;
    drive(&mut tab, &["open", "--highlight", "1", "--diff", &path]);
    assert!(tab.panels[&1].diff_view);
    assert!(tab.panels[&1].highlight.is_some());
    drive(&mut tab, &["open", "--panel", "1", "--edit", &path]);
    assert_eq!(tab.panels[&1].highlight, None);
    assert!(!tab.panels[&1].diff_view);
    Ok(())
}

#[test]
fn unsaved_edits_refuse_close_and_replace() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (dir, path) = file();
    let other = dir.path().join("b.rs");
    std::fs::copy(&path, &other)?;
    let other = other.to_string_lossy().into_owned();
    drive(&mut tab, &["open", "--edit", &path]);
    // Stand in for Neovim: the fake can't run the bootstrap that keeps this marker.
    let dirty = std::env::temp_dir().join(format!("{}-1.dirty", tab.socket));
    std::fs::write(&dirty, "")?;
    for args in [
        &["close", "1"][..],
        &["close", "--all"],
        &["open", "--panel", "1", &other],
    ] {
        let out = run(&mut tab, args);
        assert!(!out.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("unsaved edits"), "{args:?}: {stderr}");
        assert_eq!(pane(&mut tab, 1)["path"], path.as_str());
        assert!(!tab.editors[&1].pty.has_exited());
    }
    // The user's `x` closes through the same path.
    tab.focus = 1;
    assert!(matches!(
        tab.close_pane(None),
        laura::Response::Error { message } if message.contains("unsaved edits")
    ));
    assert!(tab.editors.contains_key(&1));
    std::fs::remove_file(&dirty)?;
    drive(&mut tab, &["close", "1"]);
    assert!(tab.editors.is_empty());
    Ok(())
}

#[test]
fn unsaved_edits_refuse_closing_the_tab() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (dir, path) = file();
    let other = dir.path().join("b.rs");
    std::fs::copy(&path, &other)?;
    drive(&mut tab, &["open", "--edit", &path]);
    drive(&mut tab, &["open", "--edit", &other.to_string_lossy()]);
    assert_eq!(tab.unsaved_edits(), None);
    // Stand in for Neovim: the fake can't run the bootstrap that keeps these markers.
    let dirty = |id| std::env::temp_dir().join(format!("{}-{id}.dirty", tab.socket));
    std::fs::write(dirty(2), "")?;
    let m = tab.unsaved_edits().expect("pane #2 has unsaved edits");
    assert!(m.starts_with("pane #2 has unsaved edits"), "{m}");
    std::fs::write(dirty(1), "")?;
    let m = tab.unsaved_edits().expect("both panes have unsaved edits");
    assert!(
        m.starts_with("pane #1 ") && m.ends_with(" (+1 more)"),
        "{m}"
    );
    std::fs::remove_file(dirty(1))?;
    std::fs::remove_file(dirty(2))?;
    assert_eq!(tab.unsaved_edits(), None);
    Ok(())
}

#[test]
fn neovim_exiting_closes_its_pane() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("exit"));
    let (_dir, path) = file();
    assert_eq!(drive(&mut tab, &["open", "--edit", &path]), "1");
    drain_until(&mut tab, "pane closed", |t| !t.panels.contains_key(&1));
    assert!(tab.editors.is_empty());
    Ok(())
}

#[test]
fn neovim_exiting_with_threads_leaves_a_file_pane() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("exit"));
    let (_dir, path) = file();
    assert_eq!(drive(&mut tab, &["open", "--no-focus", &path]), "1");
    drive(&mut tab, &["comment", "--pane", "1", "2", "keep me"]);
    drive(&mut tab, &["open", "--panel", "1", "--edit", &path]);
    drain_until(&mut tab, "editor reaped", |t| t.editors.is_empty());
    assert_eq!(pane(&mut tab, 1)["kind"], "panel");
    assert_eq!(tab.panels[&1].thread_count(), 1);
    Ok(())
}

#[test]
fn flipping_views_keeps_neovim_and_the_threads() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (_dir, path) = file();
    drive(&mut tab, &["open", "--edit", &path]);
    assert!(tab.editors[&1].editor_view);
    // An agent comment in the editor view lands on the pane's file view.
    drive(&mut tab, &["comment", "--pane", "1", "4", "look here"]);
    for view in [false, true] {
        tab.editors.get_mut(&1).unwrap().editor_view = view;
        tab.drain(area());
        assert!(!tab.editors[&1].pty.has_exited());
        assert_eq!(tab.panels[&1].thread_count(), 1);
        assert_eq!(pane(&mut tab, 1)["kind"], "editor");
    }
    Ok(())
}

#[test]
fn submit_in_the_file_view_returns_to_neovim() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (_dir, path) = file();
    drive(&mut tab, &["open", "--edit", &path]);
    tab.focus = 1;
    tab.editors.get_mut(&1).unwrap().editor_view = false;
    let p = tab.panels.get_mut(&1).unwrap();
    p.move_cursor(2);
    p.author_note("tighten this".into());
    assert!(matches!(tab.submit_focused(""), Some(Ok(()))));
    assert_eq!(tab.focus, 0);
    assert!(tab.editors[&1].editor_view);
    assert_eq!(tab.panels[&1].thread_count(), 0);
    drain_until(&mut tab, "inline review in the shell", |t| {
        t.pty.with_screen(|s| s.contents()).contains("laura review")
    });
    Ok(())
}

#[test]
fn neovim_launches_listening_with_the_bootstrap() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (_dir, path) = file();
    drive(&mut tab, &["open", "--edit", &path]);
    drain_until(&mut tab, "launch logged", |t| {
        logged(t).iter().any(|l| l.contains("--listen"))
    });
    let launch = logged(&tab)
        .into_iter()
        .find(|l| l.contains("--listen"))
        .unwrap();
    assert!(launch.contains("--cmd"), "{launch}");
    let lua = std::env::temp_dir().join(format!("{}-1.lua", tab.socket));
    assert!(launch.contains(&lua.display().to_string()), "{launch}");
    let bootstrap = std::fs::read_to_string(&lua)?;
    assert!(
        bootstrap.contains("laura_threads")
            && bootstrap.contains("LAURA_DIRTY")
            && bootstrap.contains("OptionSet")
    );
    // Laura's theme is off by default: the user's Neovim stays as it is.
    assert_eq!(launch.matches("lua dofile").count(), 1, "{launch}");
    Ok(())
}

#[test]
fn laura_s_theme_adds_a_second_bootstrap() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    tab.editor_theme = true;
    let (_dir, path) = file();
    drive(&mut tab, &["open", "--edit", &path]);
    drain_until(&mut tab, "launch logged", |t| {
        logged(t).iter().any(|l| l.contains("--listen"))
    });
    let launch = logged(&tab)
        .into_iter()
        .find(|l| l.contains("--listen"))
        .unwrap();
    assert_eq!(launch.matches("lua dofile").count(), 2, "{launch}");
    let theme = std::env::temp_dir().join(format!("{}-1-theme.lua", tab.socket));
    assert!(launch.contains(&theme.display().to_string()), "{launch}");
    let body = std::fs::read_to_string(&theme)?;
    assert!(
        body.contains("colors_name") && body.contains("signcolumn") && body.contains("cursorline")
    );
    // Colors resolve through the file view's theme, not to its default foreground everywhere.
    assert!(body.contains("String = '#A3BE8C'"), "{body}");
    Ok(())
}

#[test]
fn threads_are_pushed_to_neovim() -> Result<()> {
    fakes();
    let mut tab = spawn_tab()?;
    tab.editor = Ok(fake("stay"));
    let (_dir, path) = file();
    drive(&mut tab, &["open", "--edit", &path]);
    drive(&mut tab, &["comment", "--pane", "1", "3", "check this"]);
    let pushed = |t: &Tab, want: &[&str]| {
        logged(t).iter().position(|l| {
            l.contains("--server")
                && l.contains("--remote-expr")
                && want.iter().all(|w| l.contains(w))
        })
    };
    drain_until(&mut tab, "comment pushed", |t| {
        pushed(t, &["start:3", "end:3", "check this"]).is_some()
    });
    // Submitting empties the threads, which clears Neovim's diagnostics.
    tab.focus = 1;
    tab.editors.get_mut(&1).unwrap().editor_view = false;
    assert!(matches!(tab.submit_focused(""), Some(Ok(()))));
    let comment = pushed(&tab, &["check this"]).unwrap();
    drain_until(&mut tab, "empty push after the comment", |t| {
        logged(t)
            .iter()
            .skip(comment + 1)
            .any(|l| l.contains("--remote-expr") && l.contains("threads:[]"))
    });
    Ok(())
}
