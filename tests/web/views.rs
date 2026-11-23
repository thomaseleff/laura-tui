//! A view's connection: the auth gate, the first frames, a stalled or silent view, a second tab
//! that takes over, closing the tab, and quit.

use super::*;

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
    let groups = strings["help"].as_array().expect("help groups");
    for key in ["Ctrl+P", "Ctrl+Shift+↑ ↓"] {
        assert!(groups.iter().any(|g| {
            g[1].as_array()
                .is_some_and(|keys| keys.iter().any(|k| k[0] == key))
        }));
    }
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
fn a_view_says_neovim_is_missing_when_editor_panes_are_on() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.editor = Err(laura::editor::Error::NoNvim);
    let mut ws = rig.connect();
    let notice = rig.until(&mut ws, "notice", |f| f.is("notice")).json();
    assert_eq!(notice["text"], format!("⚠ {}", laura::editor::NO_NVIM));
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
fn a_stalled_view_never_blocks_the_shell() -> Result<()> {
    let rig = Rig::new()?;
    let (_, stalled) = rig.tab.pty.subscribe();
    let (_, counter) = rig.tab.pty.subscribe();
    // Each keystroke echoes as its own chunk; type until the reader has sent well past the bound.
    let (mut chunks, start) = (0, Instant::now());
    while chunks < 400 {
        assert!(start.elapsed() < common::DEADLINE, "only {chunks} chunks");
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
        assert!(start.elapsed() < common::DEADLINE, "the shell stalled");
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
            start.elapsed() < common::DEADLINE,
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
            _ => assert!(start.elapsed() < common::DEADLINE, "no exit frame"),
        }
    }
    Ok(())
}

#[test]
fn a_file_panes_threads_show_in_the_layout() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    let threads = |n: u64| move |f: &Frame| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["threads"] == n);
    rig.until(&mut ws, "no threads", threads(0));
    rig.drive(&["ready"]);
    rig.drive(&["comment", "--pane", "1", "2", "agent note"]);
    rig.until(&mut ws, "one thread", threads(1));
    Ok(())
}

/// #229: the page loads its client as ES modules, so each module that the page and the modules
/// import must come back as JavaScript. A module missing from Laura's files fails here.
#[test]
fn each_client_module_is_served_as_javascript() -> Result<()> {
    let rig = Rig::new()?;
    let page = get(rig.port, "/")?;
    let main = page
        .split(r#"<script type="module" src=""#)
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("a module script in the page");
    let mut todo = vec![format!("/{main}")];
    let mut seen = std::collections::HashSet::new();
    while let Some(path) = todo.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let resp = get(rig.port, &path)?;
        assert!(resp.starts_with("HTTP/1.1 200 OK"), "{path}: {resp:.80}");
        assert!(
            resp.contains("Content-Type: text/javascript"),
            "{path}: {resp:.200}"
        );
        for part in resp.split("\"./").skip(1) {
            let name = part.split('"').next().unwrap_or_default();
            if name.ends_with(".js") && !name.contains(['/', ' ']) {
                todo.push(format!("/{name}"));
            }
        }
    }
    assert!(seen.len() > 1, "the page imports no module: {seen:?}");
    Ok(())
}
