//! The shell in the web app: its name and size, its bytes and pastes, and the wait that a bell or
//! a notification starts.

use super::*;

/// The web app sizes the shell from its own type, and Laura clamps the size as it clamps `view`.
#[test]
fn a_shell_size_resizes_the_shell() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    for (sent, want) in [((50, 12), (12, 50)), ((0, 0), (5, 20))] {
        send(&mut ws, json!({"shell": {"cols": sent.0, "rows": sent.1}}));
        let start = Instant::now();
        while rig.tab.pty.with_screen(|s| s.size()) != want {
            assert!(
                start.elapsed() < common::DEADLINE,
                "never resized to {want:?}"
            );
            rig.pump();
            thread::sleep(Duration::from_millis(5));
        }
        // `laura layout` reports the shell's own rows, not its rect's.
        let report: Value = serde_json::from_str(&rig.drive(&["layout"]))?;
        assert_eq!(report["panes"][0]["visible_rows"], want.0, "{report}");
        // The child sees the size too.
        #[cfg(unix)]
        {
            rig.tab.pty.write(b"clear; stty size\r")?;
            rig.shell_shows(&format!("{} {}", want.0, want.1));
        }
    }
    Ok(())
}

/// Runs `script` in a 30-row shell, makes the shell 10 rows once the screen shows `ready`, and
/// returns the screen's trimmed lines once the screen shows `done`.
#[cfg(unix)]
fn shrink_during(script: &[u8], ready: &str, done: &str) -> Result<Vec<String>> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    let mut resize = |rig: &mut Rig, rows: u16| {
        send(&mut ws, json!({"shell": {"cols": 50, "rows": rows}}));
        let start = Instant::now();
        while rig.tab.pty.with_screen(|s| s.size()) != (rows, 50) {
            assert!(
                start.elapsed() < common::DEADLINE,
                "never resized to {rows}"
            );
            rig.pump();
            thread::sleep(Duration::from_millis(5));
        }
    };
    resize(&mut rig, 30);
    rig.tab.pty.write(script)?;
    rig.shell_shows(ready);
    resize(&mut rig, 10);
    rig.shell_shows(done);
    Ok(rig
        .tab
        .pty
        .with_screen(|s| s.contents())
        .lines()
        .map(|l| l.trim().to_owned())
        .collect())
}

/// A shorter shell scrolls its top rows into the scrollback, as xterm.js does, else a new view's
/// replay loses the last lines (#187). The shrink lands inside the child's unfinished escape code,
/// which must still end as a color, not as text.
#[cfg(unix)]
#[test]
fn a_shorter_shell_keeps_its_last_lines() -> Result<()> {
    // `'A''Z'` keeps the echoed command from matching `AZ`.
    let lines = shrink_during(
        b"clear; seq 40; printf 'A''Z\\033['; sleep 2; printf '31mB\\n'\r",
        "AZ",
        "AZB",
    )?;
    assert!(
        lines.iter().any(|l| l == "40") && lines.iter().any(|l| l == "33"),
        "lost the last lines: {lines:?}"
    );
    assert!(!lines.iter().any(|l| l == "12"), "kept the top: {lines:?}");
    assert!(
        lines.iter().any(|l| l == "AZB"),
        "broke the color: {lines:?}"
    );
    Ok(())
}

/// A shrink under a full-screen app keeps the main screen's last lines for when the app quits (#187).
#[cfg(unix)]
#[test]
fn a_shorter_shell_keeps_its_last_lines_under_a_full_screen_app() -> Result<()> {
    let lines = shrink_during(
        b"clear; seq 40; printf '\\033[?1049hAL''T'; sleep 2; printf '\\033[?1049lDO''NE\\n'\r",
        "ALT",
        "DONE",
    )?;
    assert!(
        lines.iter().any(|l| l == "40") && lines.iter().any(|l| l == "33"),
        "lost the last lines: {lines:?}"
    );
    assert!(!lines.iter().any(|l| l == "12"), "kept the top: {lines:?}");
    Ok(())
}

/// A reload's xterm takes the saved screen's size from the layout before the screen comes, else
/// rows past xterm's default 24 write over others.
#[test]
fn a_new_view_gets_the_shells_size_before_its_screen() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    send(&mut ws, json!({"shell": {"cols": 50, "rows": 12}}));
    let start = Instant::now();
    while rig.tab.pty.with_screen(|s| s.size()) != (12, 50) {
        assert!(start.elapsed() < common::DEADLINE, "never resized");
        rig.pump();
        thread::sleep(Duration::from_millis(5));
    }
    let mut reload = rig.connect();
    let first = rig.until(&mut reload, "layout or screen", |f| {
        f.is("layout") || matches!(f, Frame::Bytes(0, _))
    });
    assert_eq!(first.json()["shell"], json!({"cols": 50, "rows": 12}));
    Ok(())
}

/// A markdown pane's header names the highlight's source lines, not its rendered rows.
#[test]
fn a_markdown_highlight_names_its_source_lines() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "a.md",
        "# Title\n\nIntro.\n\n```rust\nfn a() {}\nfn b() {}\n```\n\nTarget.\n",
    )?;
    rig.drive(&["open", &path]);
    rig.drive(&["highlight", "--pane", "1", "10", "10"]);
    let layout = rig
        .until(&mut ws, "highlight", |f| {
            matches!(f, Frame::Json(v) if v["t"] == "layout" && !v["panes"][1]["highlight"].is_null())
        })
        .json();
    assert_eq!(layout["panes"][1]["highlight"], json!([10, 10]), "{layout}");
    Ok(())
}

/// `bytes` typed into pane `id`, as the web app's terminal sends them.
fn type_in(ws: &mut Ws, id: u64, bytes: &[u8]) -> Result<()> {
    let mut frame = id.to_be_bytes().to_vec();
    frame.extend(bytes);
    ws.send(Message::binary(frame))?;
    Ok(())
}

/// #190: the terminal bell makes the workspace wait; a hover over the shell doesn't end the wait,
/// and a wheel step does.
#[cfg(unix)]
#[test]
fn the_bell_makes_the_workspace_wait_until_the_user_scrolls_the_shell() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let layout = layout_when(&mut rig, &mut ws, "layout", |_| true);
    assert_eq!(layout["waiting"], false, "{layout}");
    type_in(&mut ws, 0, b"printf '\\007'\r")?;
    layout_when(&mut rig, &mut ws, "waiting", |l| l["waiting"] == true);
    type_in(&mut ws, 0, b"\x1b[<35;10;5M")?;
    // Frames apply in order: once the act replies, the hover has reached the shell.
    rig.act(&mut ws, json!({"act": "drawer"}));
    rig.idle(&mut ws, 10);
    assert!(rig.tab.waiting, "a hover ended the wait");
    type_in(&mut ws, 0, b"\x1b[<64;10;5M")?;
    layout_when(&mut rig, &mut ws, "no wait", |l| l["waiting"] == false);
    Ok(())
}

/// #190: the user's move into the shell ends the wait; the agent's `laura focus 0` doesn't.
#[cfg(unix)]
#[test]
fn moving_focus_into_the_shell_ends_the_wait() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    assert_eq!(rig.drive(&["open", &path]), "1");
    layout_when(&mut rig, &mut ws, "file pane", |l| l["focus"] == 1);
    rig.tab.pty.write(b"printf '\\007'\r")?;
    layout_when(&mut rig, &mut ws, "waiting", |l| l["waiting"] == true);
    rig.drive(&["focus", "0"]);
    rig.idle(&mut ws, 10);
    assert!(rig.tab.waiting, "the agent's focus ended the wait");
    rig.act(&mut ws, json!({"act": "focus", "pane": 1}));
    rig.tab.pty.write(b"printf '\\007'\r")?;
    layout_when(&mut rig, &mut ws, "waiting again", |l| l["waiting"] == true);
    rig.act(&mut ws, json!({"act": "focus", "pane": 0}));
    layout_when(&mut rig, &mut ws, "no wait", |l| l["waiting"] == false);
    Ok(())
}

/// #190: the user's close that moves the focus to the shell ends the wait; the agent's
/// `laura close` doesn't.
#[cfg(unix)]
#[test]
fn closing_the_last_pane_ends_the_wait() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    assert_eq!(rig.drive(&["open", &path]), "1");
    layout_when(&mut rig, &mut ws, "file pane", |l| l["focus"] == 1);
    rig.tab.pty.write(b"printf '\\007'\r")?;
    layout_when(&mut rig, &mut ws, "waiting", |l| l["waiting"] == true);
    rig.act(&mut ws, json!({"act": "close", "pane": 1}));
    layout_when(&mut rig, &mut ws, "no wait", |l| {
        l["waiting"] == false && l["focus"] == 0
    });
    let id = rig.drive(&["open", &path]);
    layout_when(&mut rig, &mut ws, "file pane again", |l| l["focus"] != 0);
    rig.tab.pty.write(b"printf '\\007'\r")?;
    layout_when(&mut rig, &mut ws, "waiting again", |l| l["waiting"] == true);
    rig.drive(&["close", &id]);
    rig.idle(&mut ws, 10);
    assert!(rig.tab.waiting, "the agent's close ended the wait");
    Ok(())
}

/// #190: before `laura ready`, the web app's hint line keeps `c` and `Shift+S`, which the client
/// dims, and has no `laura ready` note.
#[cfg(unix)]
#[test]
fn before_laura_ready_the_hint_line_keeps_comment_and_submit() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    assert_eq!(rig.drive(&["open", &path]), "1");
    let l = layout_when(&mut rig, &mut ws, "file pane", |l| l["focus"] == 1);
    assert_eq!(l["agent"], false, "{l}");
    let hint = pairs(&l["hints"]["pane"]);
    assert!(hint.contains("c comment"), "{hint}");
    assert!(hint.contains("Shift+S submit"), "{hint}");
    assert!(!hint.contains("laura ready"), "{hint}");
    Ok(())
}

/// #216: the focused shell's hint line lists keystrokes only, with no line about the other
/// keystrokes.
#[test]
fn a_focused_shells_hint_lists_keystrokes_only() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let l = layout_when(&mut rig, &mut ws, "the shell", |l| l["focus"] == 0);
    let hint = l["hints"]["pane"].as_array().cloned().unwrap_or_default();
    assert!(!hint.is_empty(), "{l}");
    assert!(hint.iter().all(|p| p[0] != ""), "{hint:?}");
    assert!(!pairs(&l["hints"]["pane"]).contains("Other keystrokes"));
    Ok(())
}

/// #190: `seen`, which the web app sends for a keystroke or a click that leaves the shell with the
/// keystrokes and for a wheel step over the shell, ends the wait, and focus stays on the file pane.
#[cfg(unix)]
#[test]
fn a_click_or_a_wheel_step_at_a_prompt_ends_the_wait() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    assert_eq!(rig.drive(&["open", &path]), "1");
    layout_when(&mut rig, &mut ws, "file pane", |l| l["focus"] == 1);
    rig.tab.pty.write(b"printf '\\007'\r")?;
    let l = layout_when(&mut rig, &mut ws, "waiting", |l| l["waiting"] == true);
    rig.act(&mut ws, json!({"act": "seen", "wait": l["wait"]}));
    let l = layout_when(&mut rig, &mut ws, "no wait", |l| l["waiting"] == false);
    assert_eq!(l["focus"], 1, "seen moved the focus");
    Ok(())
}

/// #190: a `seen` ends only the wait the web app showed at the user's press: not a wait that began
/// after it, and not a wait it does not name. A bell during a wait is the same wait.
#[cfg(unix)]
#[test]
fn seen_ends_only_the_wait_the_web_app_showed() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    layout_when(&mut rig, &mut ws, "layout", |_| true);
    rig.tab.pty.write(b"printf '\\007'\r")?;
    let w1 = layout_when(&mut rig, &mut ws, "waiting", |l| l["waiting"] == true)["wait"].clone();
    // `Ctrl+U`: the user's input, which ends the wait and leaves the line empty for the next bell.
    type_in(&mut ws, 0, b"\x15")?;
    layout_when(&mut rig, &mut ws, "no wait", |l| l["waiting"] == false);
    rig.tab.pty.write(b"printf '\\007'\r")?;
    let w2 = layout_when(&mut rig, &mut ws, "new wait", |l| l["waiting"] == true)["wait"].clone();
    assert_ne!(w1, w2);

    rig.act(&mut ws, json!({"act": "seen", "wait": w1}));
    rig.idle(&mut ws, 5);
    assert!(rig.tab.waiting, "a seen from before the new wait ended it");
    rig.act(&mut ws, json!({"act": "seen", "wait": 0xFFFF_FFFFu64}));
    rig.idle(&mut ws, 5);
    assert!(rig.tab.waiting, "a seen that names no wait ended the wait");

    let bells = rig.tab.pty.bells();
    rig.tab.pty.write(b"printf '\\007'\r")?;
    let start = Instant::now();
    while rig.tab.pty.bells() == bells {
        assert!(start.elapsed() < common::DEADLINE, "timed out: bell");
        thread::sleep(Duration::from_millis(10));
    }
    rig.idle(&mut ws, 5);
    rig.act(&mut ws, json!({"act": "seen", "wait": w2}));
    layout_when(
        &mut rig,
        &mut ws,
        "a bell during the wait is the same wait",
        |l| l["waiting"] == false,
    );
    rig.act(&mut ws, json!({"act": "seen", "wait": w2}));
    rig.idle(&mut ws, 5);
    assert!(!rig.tab.waiting, "a late seen brought the wait back");
    Ok(())
}

/// #190: a bell that rings between two frames, just before the user types, doesn't bring the wait
/// back once the user typed.
#[cfg(unix)]
#[test]
fn a_bell_just_before_the_user_types_does_not_bring_the_wait_back() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    layout_when(&mut rig, &mut ws, "layout", |_| true);
    // `sync` alone applies the socket's input; no `drain` sees the bell until the user typed.
    let sync_until = |rig: &mut Rig, what: &str, done: &dyn Fn(&Rig) -> bool| {
        let start = Instant::now();
        while !done(rig) {
            assert!(start.elapsed() < common::DEADLINE, "timed out: {what}");
            rig.web.sync(&mut rig.tab, false);
            thread::sleep(Duration::from_millis(10));
        }
    };
    type_in(&mut ws, 0, b"printf '\\007'\r")?;
    sync_until(&mut rig, "bell", &|r| r.tab.pty.bells() == 1);
    type_in(&mut ws, 0, b"zq")?;
    sync_until(&mut rig, "zq", &|r| {
        r.tab.pty.with_screen(|s| s.contents()).contains("zq")
    });
    rig.idle(&mut ws, 5);
    assert!(
        !rig.tab.waiting,
        "a bell before the keystroke made the workspace wait after it"
    );
    Ok(())
}

/// #190: OSC 9 and OSC 777 `notify` make the workspace wait; OSC 9's numbered ConEmu commands (progress, folder) don't.
#[cfg(unix)]
#[test]
fn a_notification_makes_the_workspace_wait_and_a_conemu_command_does_not() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    layout_when(&mut rig, &mut ws, "layout", |_| true);
    for seq in ["\\033]9;done\\007", "\\033]777;notify;Laura;done\\007"] {
        type_in(&mut ws, 0, format!("printf '{seq}'\r").as_bytes())?;
        layout_when(&mut rig, &mut ws, seq, |l| l["waiting"] == true);
        // `Ctrl+U` empties the line for the next command.
        type_in(&mut ws, 0, b"\x15")?;
        layout_when(&mut rig, &mut ws, "no wait", |l| l["waiting"] == false);
    }
    // `'pro''gress'` keeps the echoed command from matching.
    type_in(
        &mut ws,
        0,
        b"printf '\\033]9;4;1;50\\007\\033]9;9;/tmp\\033\\\\'; echo pro''gress\r",
    )?;
    rig.shell_shows("progress");
    rig.idle(&mut ws, 5);
    assert!(!rig.tab.waiting, "a ConEmu command made the workspace wait");
    Ok(())
}

/// #190: Neovim's bell in an editor pane isn't the agent's: the workspace doesn't wait.
#[cfg(unix)]
#[test]
fn a_bell_in_an_editor_pane_does_not_make_the_workspace_wait() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.editor = Ok(fake_nvim());
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    assert_eq!(rig.drive(&["open", "--edit", &path]), "1");
    rig.until(&mut ws, "editor bytes", |f| matches!(f, Frame::Bytes(1, _)));
    let start = Instant::now();
    while rig.tab.editors[&1].pty.bells() == 0 {
        assert!(start.elapsed() < Duration::from_secs(10), "no editor bell");
        rig.idle(&mut ws, 1);
    }
    rig.idle(&mut ws, 5);
    assert!(
        !rig.tab.waiting,
        "the editor's bell made the workspace wait"
    );
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
