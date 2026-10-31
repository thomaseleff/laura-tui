//! `laura tail` spools into a per-user temp dir, and the spool goes with its pane: on `close` and on quit.

mod common;

use std::io::Write;
use std::path::Path;
use std::process::Output;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::tab::UserAction;
use laura::{PaneId, Rect, Tab};

/// Journal into `target/tmp/.laura`, not the real `~/.laura`, and drop an enclosing Laura's spool dir.
fn isolate_home() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: once per binary; std's env lock serializes it with std's own readers (`Command` spawn, `env::var`).
    ONCE.call_once(|| unsafe {
        std::env::set_var("HOME", env!("CARGO_TARGET_TMPDIR"));
        std::env::set_var("USERPROFILE", env!("CARGO_TARGET_TMPDIR"));
        std::env::remove_var("LAURA_RUNTIME");
        // The host takes the per-OS dir, so Linux CI covers the `~/.laura` fallback too.
        std::env::remove_var("XDG_RUNTIME_DIR");
    });
}

fn spawn_tab() -> Result<Tab> {
    isolate_home();
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

/// Run `laura <args>` with `stdin` piped in against a real tab, draining until the client exits.
fn drive_tab(tab: &mut Tab, args: &[&str], stdin: &str) -> Output {
    drive_tab_env(tab, args, stdin, &[])
}

/// `drive_tab`, with `env` set on the client.
fn drive_tab_env(tab: &mut Tab, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
    let name = tab.socket.clone();
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let env: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let input = stdin.to_string();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let out = Command::cargo_bin("laura")
            .unwrap()
            .args(&a)
            .env("LAURA_TAB", &name)
            .envs(env)
            .write_stdin(input)
            .output()
            .unwrap();
        tx.send(out).unwrap();
    });
    let start = Instant::now();
    let out = loop {
        tab.drain(Rect::new(0, 0, 120, 40));
        if let Ok(out) = rx.try_recv() {
            break out;
        }
        assert!(start.elapsed() < common::DEADLINE, "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    assert!(out.status.success(), "laura {args:?} exited non-zero");
    out
}

/// Tail `hello\n` into a new pane; return its id and spool path.
fn tail(tab: &mut Tab) -> Result<(PaneId, String)> {
    let out = drive_tab(tab, &["tail", "--title", "t"], "hello\n");
    let id: PaneId = String::from_utf8(out.stdout)?.trim().parse()?;
    Ok((id, tab.panels[&id].path.clone()))
}

#[test]
fn tail_spools_into_the_runtime_dir() -> Result<()> {
    let mut tab = spawn_tab()?;
    let (id, path) = tail(&mut tab)?;
    assert!(
        Path::new(&path).starts_with(laura::journal::runtime_dir()),
        "{path}"
    );
    assert_eq!(std::fs::read_to_string(&path)?, "hello\n");

    drive_tab(&mut tab, &["close", &id.to_string()], "");
    assert!(!Path::new(&path).exists(), "close should delete the spool");
    Ok(())
}

#[test]
fn quitting_removes_tail_spools() -> Result<()> {
    let mut tab = spawn_tab()?;
    let (_, path) = tail(&mut tab)?;
    assert!(Path::new(&path).exists());

    drop(tab);
    assert!(!Path::new(&path).exists(), "quit should delete the spool");
    Ok(())
}

#[test]
fn close_keeps_a_file_opened_from_the_temp_dir() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    writeln!(file, "content")?;
    let path = file.path().to_str().unwrap();
    let mut tab = spawn_tab()?;

    drive_tab(&mut tab, &["open", path], "");
    let id = *tab.panels.keys().next().expect("open added a panel");
    drive_tab(&mut tab, &["close", &id.to_string()], "");
    assert!(
        file.path().exists(),
        "close must not delete a file it didn't spool"
    );
    Ok(())
}

/// Unix `std::path::absolute` keeps `..`, so the path still starts with the spool dir.
#[test]
fn close_keeps_a_file_reached_through_dotdot() -> Result<()> {
    isolate_home();
    let rt = laura::journal::runtime_dir();
    std::fs::create_dir_all(&rt)?;
    let victim = tempfile::NamedTempFile::new_in(rt.parent().expect("spool dir has a parent"))?;
    let path = rt.join("..").join(victim.path().file_name().unwrap());
    let mut tab = spawn_tab()?;

    drive_tab(&mut tab, &["open", path.to_str().unwrap()], "");
    let id = *tab.panels.keys().next().expect("open added a panel");
    drive_tab(&mut tab, &["close", &id.to_string()], "");
    assert!(
        victim.path().exists(),
        "close must not delete {}",
        path.display()
    );
    Ok(())
}

#[test]
fn the_shell_gets_the_hosts_spool_dir() -> Result<()> {
    isolate_home();
    let cmd = if cfg!(windows) {
        let mut c = portable_pty::CommandBuilder::new("cmd.exe");
        c.args(["/k", "echo %LAURA_RUNTIME%"]);
        c
    } else {
        let mut c = portable_pty::CommandBuilder::new("/bin/sh");
        c.args(["-c", "echo \"$LAURA_RUNTIME\"; exec cat"]);
        c
    };
    // 200 cols, so the path doesn't wrap.
    let mut tab = Tab::spawn(cmd, 24, 200)?;
    let want = laura::journal::runtime_dir().to_string_lossy().into_owned();
    let start = Instant::now();
    while !tab.pty.with_screen(|s| s.contents()).contains(&want) {
        assert!(
            start.elapsed() < common::DEADLINE,
            "shell never printed {want}: {}",
            tab.pty.with_screen(|s| s.contents())
        );
        tab.drain(Rect::new(0, 0, 200, 24));
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// A client whose temp env differs from the host's (sandbox, `sudo`, ssh) still spools where `close` looks.
#[test]
fn close_deletes_a_spool_from_a_client_with_other_temp_env() -> Result<()> {
    let mut tab = spawn_tab()?;
    let other = tempfile::tempdir()?;
    let other = other.path().to_str().unwrap();
    let runtime = laura::journal::runtime_dir();
    let env = [
        ("TMP", other),
        ("TEMP", other),
        ("TMPDIR", other),
        ("XDG_RUNTIME_DIR", other),
        ("LAURA_RUNTIME", runtime.to_str().unwrap()),
    ];
    let out = drive_tab_env(
        &mut tab,
        &["tail", "--title", "t"],
        "hello
",
        &env,
    );
    let id: PaneId = String::from_utf8(out.stdout)?.trim().parse()?;
    let path = tab.panels[&id].path.clone();
    assert!(Path::new(&path).exists());

    drive_tab(&mut tab, &["close", &id.to_string()], "");
    assert!(!Path::new(&path).exists(), "close should delete {path}");
    Ok(())
}

#[test]
fn tail_falls_back_when_xdg_runtime_dir_is_unusable() -> Result<()> {
    let mut tab = spawn_tab()?;
    // A regular file: Laura can't create a dir in it. (Not System32: CI runners run as admin.)
    let bogus = tempfile::NamedTempFile::new()?;
    let env = [("XDG_RUNTIME_DIR", bogus.path().to_str().unwrap())];
    let out = drive_tab_env(&mut tab, &["tail", "--title", "t"], "hello\n", &env);
    let id: PaneId = String::from_utf8(out.stdout)?.trim().parse()?;
    let path = tab.panels[&id].path.clone();
    assert!(!Path::new(&path).starts_with(bogus.path()), "{path}");
    assert_eq!(std::fs::read_to_string(&path)?, "hello\n");
    // The test never closes the pane, so it removes the spool itself.
    std::fs::remove_file(&path)?;
    Ok(())
}

#[test]
fn tail_falls_back_when_laura_runtime_is_unusable() -> Result<()> {
    let mut tab = spawn_tab()?;
    // A regular file as the parent: the client can't create the host's spool dir (sandbox).
    let bogus = tempfile::NamedTempFile::new()?;
    let runtime = bogus.path().join("laura");
    let env = [("LAURA_RUNTIME", runtime.to_str().unwrap())];
    let out = drive_tab_env(&mut tab, &["tail", "--title", "t"], "hello\n", &env);
    let id: PaneId = String::from_utf8(out.stdout)?.trim().parse()?;
    let path = tab.panels[&id].path.clone();
    assert!(!Path::new(&path).starts_with(bogus.path()), "{path}");
    assert_eq!(std::fs::read_to_string(&path)?, "hello\n");
    // The test never closes the pane, so it removes the spool itself.
    std::fs::remove_file(&path)?;
    Ok(())
}

/// Append `text` to the spool at `path`, then let the tab reload.
fn append(tab: &mut Tab, path: &str, text: &str) -> Result<()> {
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())?;
    tab.drain(Rect::new(0, 0, 120, 40));
    Ok(())
}

/// The `paused` field of pane `id` in `laura layout`, or `None` when the layout omits it.
fn layout_paused(tab: &mut Tab, id: PaneId) -> Result<Option<serde_json::Value>> {
    let out = drive_tab(tab, &["layout"], "");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let pane = v["panes"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["id"] == id))
        .expect("layout lists the pane");
    Ok(pane.get("paused").cloned())
}

#[test]
fn a_paused_tail_pane_holds_its_content() -> Result<()> {
    let mut tab = spawn_tab()?;
    let out = drive_tab(
        &mut tab,
        &["tail", "--follow", "--title", "t"],
        "one\ntwo\n",
    );
    let id: PaneId = String::from_utf8(out.stdout)?.trim().parse()?;
    let path = tab.panels[&id].path.clone();
    let lines = tab.panels[&id].line_count();

    tab.act(UserAction::TogglePause { pane: id })
        .map_err(anyhow::Error::msg)?;
    append(&mut tab, &path, "three\nfour\n")?;
    assert_eq!(tab.panels[&id].line_count(), lines, "a paused pane holds");
    assert!(
        std::fs::read_to_string(&path)?.contains("four"),
        "the spool grows"
    );
    assert_eq!(layout_paused(&mut tab, id)?, Some(serde_json::json!(true)));
    assert_eq!(layout_paused(&mut tab, 0)?, None, "the shell never pauses");

    tab.act(UserAction::TogglePause { pane: id })
        .map_err(anyhow::Error::msg)?;
    let p = &tab.panels[&id];
    assert!(
        p.content.contains("four"),
        "a resume reloads: {}",
        p.content
    );
    assert_eq!(
        p.cursor,
        p.line_count() - 1,
        "--follow catches the cursor up"
    );
    assert_eq!(layout_paused(&mut tab, id)?, None);
    Ok(())
}

#[test]
fn a_paused_pane_freezes_on_resume() -> Result<()> {
    let mut tab = spawn_tab()?;
    let (id, path) = tail(&mut tab)?;
    tab.act(UserAction::TogglePause { pane: id })
        .map_err(anyhow::Error::msg)?;
    drive_tab(
        &mut tab,
        &["comment", "1", "look", "--pane", &id.to_string()],
        "",
    );
    append(&mut tab, &path, "more\n")?;
    assert!(
        !tab.panels[&id].source_changed,
        "a paused pane doesn't freeze"
    );

    tab.act(UserAction::TogglePause { pane: id })
        .map_err(anyhow::Error::msg)?;
    let p = &tab.panels[&id];
    assert!(p.source_changed, "a resume with threads freezes the pane");
    assert!(!p.content.contains("more"), "the pane keeps its snapshot");
    Ok(())
}

#[test]
fn refresh_resumes_a_paused_pane() -> Result<()> {
    let mut tab = spawn_tab()?;
    let (id, path) = tail(&mut tab)?;
    tab.act(UserAction::TogglePause { pane: id })
        .map_err(anyhow::Error::msg)?;
    append(&mut tab, &path, "more\n")?;

    tab.act(UserAction::Refresh { pane: id })
        .map_err(anyhow::Error::msg)?;
    let p = &tab.panels[&id];
    assert!(!p.paused, "Ctrl+R resumes the pane");
    assert!(p.content.contains("more"), "and reloads it: {}", p.content);
    Ok(())
}

#[test]
fn a_file_pane_does_not_pause() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    writeln!(file, "content")?;
    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", file.path().to_str().unwrap()], "");
    let id = *tab.panels.keys().next().expect("open added a panel");

    tab.act(UserAction::TogglePause { pane: id })
        .map_err(anyhow::Error::msg)?;
    assert!(!tab.panels[&id].paused);
    Ok(())
}
