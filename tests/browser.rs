//! Browser panes (#107): the gate, the page Laura's browser shows in a pane, its frames, a page
//! that crashes or doesn't load, downloads, permissions and new windows, and what an exit leaves
//! behind. Each test that needs Chrome or Edge skips when neither is
//! installed; under `CI`, it fails. State and frames, not pixels.

#[path = "common/browser.rs"]
#[allow(dead_code)] // each browser test crate uses part of the browser rig
mod browser_rig;
mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use std::cell::Cell;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use laura::browser;
use serde_json::{Value, json};
use tungstenite::Message;

use browser_rig::*;
use rig::*;

#[test]
fn a_url_needs_the_web_app() -> Result<()> {
    let mut r = bare()?;
    r.tab.web = false;
    r.tab.browser_path = Ok(PathBuf::from("chrome"));
    let err = error(&mut r, &["open", "http://127.0.0.1:1/"]);
    assert!(err.contains(browser::NEED_WEB), "{err}");
    Ok(())
}

#[test]
fn a_url_takes_no_file_flags() -> Result<()> {
    let mut r = bare()?;
    r.tab.browser_path = Ok(PathBuf::from("chrome"));
    let url = "http://127.0.0.1:1/";
    for (flags, want) in [
        (&["--highlight", "3"][..], browser::NO_HIGHLIGHT),
        (&["--diff"][..], browser::NO_DIFF),
        (&["--edit"][..], browser::NO_EDIT),
    ] {
        let args: Vec<&str> = ["open", url].iter().chain(flags).copied().collect();
        let err = error(&mut r, &args);
        assert!(err.contains(want), "{flags:?}: {err}");
    }
    // `--follow` changes nothing: the pane shows the page live anyway.
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, _ws) = rig(path)?;
    let id: u64 = r
        .drive(&["open", &serve(), "--follow"])
        .parse()
        .expect("a pane id");
    assert!(r.tab.browsers.contains_key(&id));
    let err = error(&mut r, &["diff", "--pane", &id.to_string()]);
    assert!(err.contains(browser::NO_DIFF), "{err}");
    Ok(())
}

#[test]
fn a_browser_that_doesnt_start_keeps_the_file_pane() -> Result<()> {
    let mut r = bare()?;
    // A file that isn't a browser.
    r.tab.browser_path = Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
    let id: u64 = r.drive(&["open", "README.md"]).parse().expect("a pane id");
    let err = error(
        &mut r,
        &["open", "http://127.0.0.1:1/", "--panel", &id.to_string()],
    );
    assert!(err.contains("couldn't start Laura's browser"), "{err}");
    let report = layout(&mut r);
    let p = report["panes"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["id"] == id))
        .expect("the pane");
    assert_eq!(p["kind"], "panel");
    assert!(
        p["path"].as_str().is_some_and(|s| s.ends_with("README.md")),
        "{p}"
    );
    Ok(())
}

#[test]
fn the_same_page_twice_says_where_it_is() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let url = serve();
    let id = open(&mut r, &url);
    // Laura's browser reports the bare host with a trailing slash.
    let shown = format!("{url}/");
    r.until(&mut ws, "the page's URL", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "browser" && v["id"] == id && v["url"] == shown.as_str())
    });
    let out = r.run(&["open", &url]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!("already open in pane #{id}")),
        "{err}"
    );
    // Also when another browser pane moves to the page.
    let other = open(&mut r, &serve()).to_string();
    let out = r.run(&["open", &url, "--panel", &other]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!("already open in pane #{id}")),
        "{err}"
    );
    Ok(())
}

#[test]
fn a_browser_pane_shows_the_page() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    // An `.html` URL is a page in Laura's browser, not an HTML file.
    let url = format!("{}/page.html", serve());
    let id = open(&mut r, &url);
    let mut seen = vec![];
    let start = Instant::now();
    loop {
        r.pump();
        while let Some(f) = read(&mut ws) {
            seen.push(f);
        }
        let titled = seen.iter().any(|f| {
            matches!(f, Frame::Json(v) if v["t"] == "browser" && v["id"] == id
                && v["title"] == "Fast page" && v["url"] == url.as_str() && v["live"] == true)
        });
        // The page's one frame can come before or after its title: a still page sends no other.
        let framed = seen
            .iter()
            .any(|f| matches!(f, Frame::Bytes(n, _) if *n == id));
        if titled && framed {
            break;
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "no title or frame: {seen:?}"
        );
    }
    let shown = seen.iter().rev().find(|f| f.is("layout"));
    let Some(Frame::Json(l)) = shown else {
        panic!("no layout")
    };
    let p = l["panes"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["id"] == id))
        .expect("the pane");
    assert_eq!(p["kind"], "browser");
    assert_eq!(p["icon"], "globe");
    assert!(
        !seen
            .iter()
            .any(|f| matches!(f, Frame::Json(v) if ["pane", "rows", "page"].contains(&v["t"].as_str().unwrap_or("")) && v["id"] == id)),
        "file frames for a browser pane"
    );
    let report = layout(&mut r);
    let p = report["panes"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["id"] == id))
        .expect("the pane");
    assert_eq!(p["kind"], "browser");
    assert_eq!(p["path"], url.as_str());
    assert_eq!(p["title"], "Fast page");
    Ok(())
}

#[test]
fn a_slow_server_doesnt_stall_the_workspace() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, _ws) = rig(path)?;
    // Laura's browser starts first, so the timing is the slow page's alone.
    open(&mut r, &serve());
    let id = open(&mut r, &format!("{}/slow", serve()));
    let start = Instant::now();
    layout(&mut r);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "layout waited {:?} on the slow page",
        start.elapsed()
    );
    assert!(r.tab.browsers.contains_key(&id));
    Ok(())
}

#[test]
fn a_loading_page_says_so() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    // Laura's browser starts first, so the slow page is still loading.
    open(&mut r, &serve());
    let id = open(&mut r, &format!("{}/slow", serve()));
    let pane = id.to_string();
    let err = error(&mut r, &["highlight", "h1", "--pane", &pane]);
    assert!(
        err.contains(&format!("pane #{id}'s page is still loading")),
        "{err}"
    );
    state_until(&mut r, &mut ws, "the slow page", id, |v| {
        v["title"] == "Fast page" && v["busy"] == false
    });
    r.drive(&["highlight", "h1", "--pane", &pane]);
    Ok(())
}

#[test]
fn a_url_that_doesnt_load_shows_a_notice() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let url = format!("http://127.0.0.1:{port}/");
    let id = open(&mut r, &url);
    let want = format!("⚠ The page {url} did not load (net::ERR_CONNECTION_REFUSED).");
    r.until(
        &mut ws,
        "the notice",
        |f| matches!(f, Frame::Json(v) if v["t"] == "notice" && v["text"] == want.as_str() && v["pane"] == id),
    );
    Ok(())
}

#[test]
fn a_stopped_navigation_shows_no_notice() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let url = serve();
    let id = open(&mut r, &format!("{url}/hang")).to_string();
    r.drive(&["open", &format!("{url}/one"), "--panel", &id]);
    let notice = |f: &Frame| matches!(f, Frame::Json(v) if v["t"] == "notice");
    r.until(&mut ws, "the second page", |f| {
        assert!(!notice(f), "{f:?}");
        matches!(f, Frame::Json(v) if v["t"] == "browser" && v["title"] == "One")
    });
    for f in r.idle(&mut ws, 50) {
        assert!(!notice(&f), "{f:?}");
    }
    Ok(())
}

#[test]
fn a_dead_browser_starts_again() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let id = open(&mut r, &serve());
    next_frame(&mut r, &mut ws, id);
    // The notice goes to the focused browser pane, not to the first one.
    let two = open(&mut r, &serve());
    next_frame(&mut r, &mut ws, two);
    assert_eq!(
        r.act(&mut ws, json!({"act": "focus", "pane": two}))["ok"],
        true
    );
    // A second CDP client closes Laura's browser, as a crash would end it.
    let url = r
        .tab
        .browser
        .as_ref()
        .expect("Laura's browser")
        .ws_url
        .clone();
    let (mut cdp, _) = tungstenite::connect(url)?;
    cdp.send(Message::text(
        json!({"id": 1, "method": "Browser.close"}).to_string(),
    ))?;
    // The pane's `browser` frame goes before the notice.
    r.until(&mut ws, "not live", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "browser" && v["id"] == id && v["live"] == false)
    });
    r.until(
        &mut ws,
        "the exit notice",
        |f| matches!(f, Frame::Json(v) if v["t"] == "notice" && v["text"] == browser::EXITED && v["pane"] == two),
    );
    assert!(r.tab.panels.contains_key(&id), "the pane stays");
    // A start that fails shows the fixed notice in the pane, and the next `Ctrl+R` tries again.
    let chrome = std::mem::replace(
        &mut r.tab.browser_path,
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")),
    );
    let acted = r.act(&mut ws, json!({"act": "refresh", "pane": id}));
    assert_eq!(acted["ok"], true, "{acted}");
    r.until(
        &mut ws,
        "the start-failed notice",
        |f| matches!(f, Frame::Json(v) if v["t"] == "notice" && v["text"] == browser::START_FAILED && v["pane"] == id),
    );
    r.tab.browser_path = chrome;
    let acted = r.act(&mut ws, json!({"act": "refresh", "pane": id}));
    assert_eq!(acted["ok"], true, "{acted}");
    next_frame(&mut r, &mut ws, id);
    Ok(())
}

#[test]
fn a_frame_matches_the_pane() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let id = open(&mut r, &serve());
    next_frame(&mut r, &mut ws, id);
    send(
        &mut ws,
        json!({"size": {"id": id, "w": 400, "h": 300, "dpr": 2}}),
    );
    let start = Instant::now();
    let (painted, png) = loop {
        let (painted, png) = next_frame(&mut r, &mut ws, id);
        if png_size(&png) == (800, 600) {
            break (painted, png);
        }
        assert!(start.elapsed() < common::DEADLINE, "no 800×600 frame");
    };
    assert!(!png.is_empty());
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as f64;
    assert!(
        (now - painted).abs() < 5000.0,
        "painted {painted}, now {now}"
    );
    Ok(())
}

/// A canvas sized in device pixels draws at the page's scale (#175).
#[test]
fn a_canvas_draws_at_the_page_scale() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let id = open(&mut r, &format!("{}/scale", serve()));
    // The user's scale in the issue.
    send(
        &mut ws,
        json!({"size": {"id": id, "w": 400, "h": 300, "dpr": 0.8}}),
    );
    state_until(&mut r, &mut ws, "a canvas at the page's scale", id, |v| {
        v["title"]
            .as_str()
            .and_then(|t| t.split_once(' '))
            .is_some_and(|(canvas, page)| canvas == page)
    });
    Ok(())
}

#[test]
fn a_new_view_gets_the_last_frame() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let id = open(&mut r, &serve());
    next_frame(&mut r, &mut ws, id);
    let mut ws2 = r.connect();
    next_frame(&mut r, &mut ws2, id);
    Ok(())
}

#[test]
fn a_still_page_sends_no_frames() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    // A frame can come before the load (a blank page), so the test waits for the load.
    let (id, _) = load(&mut r, &mut ws, "/");
    let loaded = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as f64;
    // The paint times of the frames for `id` in `d`.
    let frames = |r: &mut Rig, ws: &mut Ws, d: Duration| {
        let (start, mut out) = (Instant::now(), vec![]);
        while start.elapsed() < d {
            r.pump();
            while let Some(f) = r.read(ws) {
                if let Frame::Bytes(i, b) = f
                    && i == id
                {
                    let t = b.split_first_chunk::<8>().expect("a paint time").0;
                    out.push(f64::from_be_bytes(*t) - loaded);
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        out
    };
    // On a busy host, Chrome can send a frame more than 500 ms after it paints the frame, so the
    // test judges each frame by its paint time. The load's last paints came 660 ms after the load
    // on macOS CI; a frame painted after 1.5 s is a still page painting again.
    let painted = frames(&mut r, &mut ws, Duration::from_secs(3));
    let late: Vec<_> = painted.iter().filter(|&&t| t > 1500.0).collect();
    assert!(
        late.is_empty(),
        "frames painted at {painted:?} ms after the load"
    );
    Ok(())
}

#[test]
fn closing_the_workspace_ends_the_browser() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let id = open(&mut r, &serve());
    next_frame(&mut r, &mut ws, id);
    let b = r.tab.browser.as_ref().expect("Laura's browser");
    let (profile, port) = (b.profile().to_path_buf(), b.port);
    assert!(profile.exists());
    drop(ws);
    drop(r);
    assert!(!profile.exists(), "{} stays", profile.display());
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "Laura's browser still listens"
    );
    Ok(())
}

#[test]
fn a_page_that_denies_framing_shows() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let id = open(&mut r, &format!("{}/deny", serve()));
    // A still page sends one frame, which can come before or after the load's state.
    let (framed, loaded) = (Cell::new(false), Cell::new(false));
    r.until(&mut ws, "a frame and the loaded page", |f| {
        match f {
            Frame::Bytes(n, _) if *n == id => framed.set(true),
            Frame::Json(v) if v["t"] == "browser" && v["id"] == id => {
                loaded.set(v["title"] == "Deny" && v["busy"] == false)
            }
            _ => {}
        }
        framed.get() && loaded.get()
    });
    Ok(())
}

#[test]
fn a_download_is_denied() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    let file = PathBuf::from(home.expect("a home folder"))
        .join("Downloads")
        .join(DOWNLOAD);
    let _ = std::fs::remove_file(&file);
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/download");
    click(&mut ws, id, 50.0, 25.0);
    let want = "⚠ Laura's browser does not support downloads.";
    r.until(
        &mut ws,
        "the notice",
        |f| matches!(f, Frame::Json(v) if v["t"] == "notice" && v["text"] == want && v["pane"] == id),
    );
    let start = Instant::now();
    while !DOWNLOADED.load(Ordering::SeqCst) {
        assert!(
            start.elapsed() < common::DEADLINE,
            "timed out: the download"
        );
        r.idle(&mut ws, 1);
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
        r.idle(&mut ws, 1);
    }
    let saved = file.exists();
    let _ = std::fs::remove_file(&file);
    assert!(!saved, "{} exists", file.display());
    Ok(())
}

#[test]
fn a_permission_prompt_is_denied() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, _) = load(&mut r, &mut ws, "/ask");
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "the answer", id, |v| {
        v["title"] == "denied"
    });
    Ok(())
}

#[test]
fn a_new_window_opens_in_the_pane() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let (id, url) = load(&mut r, &mut ws, "/named");
    let (named, two) = (format!("{url}/named"), format!("{url}/two"));
    // A link to a named target.
    click(&mut ws, id, 50.0, 25.0);
    state_until(&mut r, &mut ws, "the named target's page", id, |v| {
        v["url"] == two.as_str() && v["title"] == "Two" && v["back"] == true
    });
    one_page(&mut r, &mut ws, &two);
    r.act(
        &mut ws,
        json!({"act": "browser_history", "pane": id, "delta": -1}),
    );
    state_until(&mut r, &mut ws, "back to the links", id, |v| {
        v["url"] == named.as_str() && v["title"] == "Named"
    });
    // A middle-click on a plain link.
    for (kind, buttons) in [("down", 4), ("up", 0)] {
        send(
            &mut ws,
            json!({"act": "browser_mouse", "pane": id, "kind": kind, "x": 50.0, "y": 125.0,
                   "button": 1, "buttons": buttons, "clicks": 1}),
        );
    }
    state_until(&mut r, &mut ws, "the middle-click's page", id, |v| {
        v["url"] == two.as_str() && v["title"] == "Two"
    });
    one_page(&mut r, &mut ws, &two);
    Ok(())
}

#[test]
fn opening_a_url_into_a_browser_pane_navigates() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    r.tab.agent = true;
    let (id, url) = load(&mut r, &mut ws, "/one");
    comment_at(&mut r, &mut ws, id, (50.0, 25.0), "Link");
    let target = one_page(&mut r, &mut ws, &format!("{url}/one"));
    let two = format!("{url}/two");
    r.drive(&["open", &two, "--panel", &id.to_string()]);
    let shown = state_until(&mut r, &mut ws, "the second page", id, |v| {
        v["url"] == two.as_str() && v["title"] == "Two"
    });
    assert_eq!(shown["threads"][0]["away"], "/one", "{shown}");
    assert_eq!(one_page(&mut r, &mut ws, &two), target);
    Ok(())
}

#[test]
fn only_http_urls_open() -> Result<()> {
    let mut r = bare()?;
    r.tab.browser_path = Ok(PathBuf::from("no-such-browser"));
    for url in [
        "file:///etc/hosts",
        "chrome://version",
        "javascript:alert(1)",
    ] {
        let out = r.run(&["open", url]);
        let err = String::from_utf8_lossy(&out.stderr);
        if out.status.success() {
            let id: u64 = String::from_utf8_lossy(&out.stdout).trim().parse()?;
            let p = &r.tab.panels[&id];
            assert!(!p.browser, "{url} opened a browser pane");
            eprintln!("{url}: a file pane, {:?}", p.read_error);
        } else {
            assert!(!err.contains("Laura's browser"), "{url}: {err}");
            eprintln!("{url}: {err}");
        }
    }
    assert!(r.tab.browser.is_none(), "Laura's browser started");
    Ok(())
}

#[test]
fn a_crashed_page_says_so() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let url = format!("{}/two", serve());
    let id = open(&mut r, &url);
    state_until(&mut r, &mut ws, "the page", id, |v| v["title"] == "Two");
    let mut tools = Tools::attach(&mut r, &url);
    // A kill, not `Page.crash`: on Ubuntu CI the crashed renderer hung, probably in its core dump.
    tools.send(
        "Page.navigate",
        json!({"url": "chrome://kill", "transitionType": "typed"}),
    );
    let want = "⚠ To load the page again after it stopped, press Ctrl+R.";
    r.until(
        &mut ws,
        "the crash notice",
        |f| matches!(f, Frame::Json(v) if v["t"] == "notice" && v["text"] == want && v["pane"] == id),
    );
    assert!(!r.tab.browsers.contains_key(&id));
    let acted = r.act(&mut ws, json!({"act": "refresh", "pane": id}));
    assert_eq!(acted["ok"], true, "{acted}");
    assert!(r.tab.browsers.contains_key(&id));
    Ok(())
}

/// #158: a real Chrome's helpers (zygote, GPU, renderers) stay where the watcher's kill reaches
/// them, so a killed Laura leaves no Chrome process and no profile behind.
#[cfg(unix)]
#[test]
fn a_killed_laura_closes_real_chrome_and_its_helpers() -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let shell = format!(
        "{} open {}/two & sleep 60",
        env!("CARGO_BIN_EXE_laura"),
        serve()
    );
    let mut laura = Kill(
        Command::new(env!("CARGO_BIN_EXE_laura"))
            .args(["--web", "--", "/bin/sh", "-c", &shell])
            .env("LAURA_BROWSER", &path)
            // Journals into the target dir; only this Laura's home moves, not the other tests'.
            .env("HOME", env!("CARGO_TARGET_TMPDIR"))
            .env_remove("LAURA_TAB")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Laura leads its own process group, so the test can kill the group as a supervisor does.
            .process_group(0)
            .spawn()?,
    );
    // (pid, ppid, pgid, command) of each running process; zombies are gone already.
    let procs = || -> Vec<(u32, u32, u32, String)> {
        let out = Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,pgid=,stat=,command="])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        out.lines()
            .filter_map(|l| {
                let mut f = l.split_whitespace();
                let mut num = || f.next()?.parse().ok();
                let (pid, ppid, pgid) = (num()?, num()?, num()?);
                let stat = f.next()?;
                let cmd = f.collect::<Vec<_>>().join(" ");
                (!stat.starts_with('Z')).then_some((pid, ppid, pgid, cmd))
            })
            .collect()
    };
    // Chrome is Laura's child with a profile; its tree holds a renderer once the page loads.
    let t0 = Instant::now();
    let (chrome_pid, pgid, profile, tree) = loop {
        let all = procs();
        let found = all
            .iter()
            .find(|p| p.1 == laura.0.id() && p.3.contains("--user-data-dir="))
            .map(|c| {
                let mut tree = vec![c.0];
                let mut i = 0;
                while i < tree.len() {
                    let parent = tree[i];
                    tree.extend(all.iter().filter(|p| p.1 == parent).map(|p| p.0));
                    i += 1;
                }
                (c.clone(), tree)
            });
        if let Some((c, tree)) = found
            && all
                .iter()
                .any(|p| tree.contains(&p.0) && p.3.contains("--type=renderer"))
        {
            let profile =
                c.3.split_whitespace()
                    .find_map(|a| a.strip_prefix("--user-data-dir="))
                    .map(PathBuf::from)
                    .expect("a --user-data-dir argument");
            break (c.0, c.2, profile, tree);
        }
        assert!(
            t0.elapsed() < common::DEADLINE,
            "Laura's browser never started a renderer"
        );
        thread::sleep(Duration::from_millis(100));
    };
    assert!(profile.exists(), "{} is missing", profile.display());
    assert!(tree.len() > 1, "Chrome {chrome_pid} started no helper");

    // A SIGKILL to Laura's group kills Laura too, so this covers both kinds of kill.
    Command::new("kill")
        .args(["-s", "KILL", "--", &format!("-{}", laura.0.id())])
        .status()?;
    laura.0.wait()?;
    let t0 = Instant::now();
    loop {
        let left: Vec<_> = procs()
            .into_iter()
            .filter(|p| tree.contains(&p.0) || p.2 == pgid)
            .map(|p| format!("{} {}", p.0, p.3.chars().take(80).collect::<String>()))
            .collect();
        if left.is_empty() && !profile.exists() {
            break;
        }
        assert!(
            t0.elapsed() < common::DEADLINE,
            "after Laura's kill, still running: {left:?}; the profile holds: {:?}",
            std::fs::read_dir(&profile)
                .map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
        );
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_millis(500));
    assert!(!profile.exists(), "a helper wrote the profile again");
    Ok(())
}

/// Workspace B, under `laura --web` with `script` as its shell, for a browser pane of A to show.
// Unix only: B's shell hands over its `LAURA_TAB` with `sh`. `app.js` is the same on each OS.
#[cfg(unix)]
struct WorkspaceB {
    _proc: Kill,
    url: String,
    dir: tempfile::TempDir,
    home: tempfile::TempDir,
    socket: String,
}

#[cfg(unix)]
impl WorkspaceB {
    /// `script` must first write `$LAURA_TAB` to `tab`.
    fn start(script: &str) -> Result<WorkspaceB> {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        use std::sync::mpsc;

        // Its own HOME, so its workspace switcher lists only B and no record goes to the real
        // home. Not `laura_start`: its `isolate_home` moves HOME for this whole test process.
        let dir = tempfile::tempdir()?;
        let home = tempfile::tempdir()?;
        let mut b = Kill(
            Command::new(env!("CARGO_BIN_EXE_laura"))
                .args(["--web", "--", "/bin/sh", "-c", script])
                .current_dir(dir.path())
                .env("HOME", home.path())
                .env("LAURA_BROWSER", dir.path().join("no-browser"))
                .env_remove("LAURA_TAB")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?,
        );
        let stdout = b.0.stdout.take().expect("stdout is piped");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let url = rx.recv_timeout(common::DEADLINE)?.trim().to_string();
        let tab = dir.path().join("tab");
        let t0 = Instant::now();
        let socket = loop {
            if let Ok(s) = std::fs::read_to_string(&tab)
                && !s.is_empty()
            {
                break s;
            }
            assert!(
                t0.elapsed() < common::DEADLINE,
                "B's shell wrote no LAURA_TAB"
            );
            thread::sleep(Duration::from_millis(50));
        };
        Ok(WorkspaceB {
            _proc: b,
            url,
            dir,
            home,
            socket,
        })
    }

    /// `laura <args>` on B, asserting success.
    fn drive(&self, args: &[&str]) -> Result<()> {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_laura"))
            .args(args)
            .env("LAURA_TAB", &self.socket)
            .output()?;
        assert!(
            out.status.success(),
            "laura {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(())
    }
}

/// `app.js` paints the browser tab: the folder name, then the session label after `ready --title`.
#[cfg(unix)]
#[test]
fn the_browser_tab_shows_the_session_label() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let b = WorkspaceB::start(r#"printf %s "$LAURA_TAB" > tab; exec sleep 60"#)?;
    let id = open(&mut r, &b.url);
    // macOS: the temp folder is a link into /private/var.
    let folder = b.dir.path().canonicalize()?;
    let folder = folder.file_name().expect("a name").to_string_lossy();
    let want = format!("Laura · {folder}");
    state_until(&mut r, &mut ws, "the folder name", id, |v| {
        v["title"] == want.as_str()
    });
    b.drive(&["ready", "--title", "flat elevation profile"])?;
    state_until(&mut r, &mut ws, "the session label", id, |v| {
        v["title"] == "Laura · flat elevation profile"
    });
    Ok(())
}

/// #193: a file path in the shell is a link from the start of its word, also through route folders
/// such as `[slug]`; a path in a URL or a path that wraps is not, and a click on
/// `src/foo.test.ts:12` opens `foo.test.ts` on line 12.
#[cfg(unix)]
#[test]
fn a_file_path_in_the_shell_is_a_link() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let b = WorkspaceB::start(r#"printf %s "$LAURA_TAB" > tab; exec sleep 60"#)?;
    std::fs::create_dir(b.dir.path().join("src"))?;
    let lines: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    std::fs::write(b.dir.path().join("src/foo.test.ts"), lines)?;
    let id = open(&mut r, &b.url);
    state_until(&mut r, &mut ws, "B loads", id, |v| {
        v["title"].as_str().is_some_and(|t| t.starts_with("Laura"))
    });
    let url = pane_of(&mut r, id)["path"]
        .as_str()
        .expect("a URL")
        .to_string();
    let mut tools = Tools::attach(&mut r, &url);
    let mut eval = |r: &mut Rig, js: &str| {
        tools.call(
            r,
            "Runtime.evaluate",
            json!({"expression": js, "returnByValue": true, "awaitPromise": true}),
        )["result"]["value"]
            .clone()
    };
    let t0 = Instant::now();
    while eval(&mut r, "!!terms.get(0)") != true {
        assert!(t0.elapsed() < common::DEADLINE, "B has no shell");
        thread::sleep(Duration::from_millis(50));
    }
    // Rows 9–10 wrap a path after `src/foo.test`, rows 11–12 wrap a URL after `https://x.com/`.
    let got = eval(
        &mut r,
        r#"(async () => {
            const t = terms.get(0);
            // `fitShell` can resize the shell after the page loads, which moves the wrap: write again until the width holds.
            for (let cols; cols !== t.cols;) {
                cols = t.cols;
                t.reset();
                await new Promise(w => t.write([
                    "see src/foo.test.ts:12 src\\tab.rs:41 C:\\proj\\a.rs ./README.md:3",
                    "https://example.com/@scope/pkg/index.js README.md ~/a/b.rs",
                    "Update(src/a.rs) (src/b.rs) app/[slug]/page.tsx:12",
                    "node_modules/@types/x/index.d.ts src/café/a.rs:3 1/2.5",
                    "**src/c.rs** “src/d.rs” node_modules/lodash.merge/index",
                    "a/foo.test.js/index logs/app.log.1 src/**/a.rs src/*x/a.rs",
                    "**/b.rs src/a.rs.bak src/e.rs. (src/f.rs:3), src/g.rs's",
                    "src\\b.c.d\\e C:\\p\\v1.2\\a.rs:4",
                    " ".repeat(t.cols - 12) + "src/foo.test.ts:12",
                    " ".repeat(t.cols - 14) + "https://x.com/@a/b.js",
                ].join("\r\n"), w));
                await new Promise(r => setTimeout(r, 200));
            }
            const rows = [];
            for (let y = 1; y <= 12; y++) {
                const texts = [];
                for (const p of t._core._linkProviderService.linkProviders)
                    for (const l of await new Promise(cb => p.provideLinks(y, cb)) ?? []) texts.push(l.text);
                rows.push(texts);
            }
            const b = t.buffer.active;
            return { rows, wrapped: [b.getLine(9).isWrapped, b.getLine(11).isWrapped] };
        })()"#,
    );
    assert_eq!(
        got,
        json!({
            "rows": [
                ["src/foo.test.ts:12", "src\\tab.rs:41", "C:\\proj\\a.rs", "./README.md:3"],
                [],
                ["src/a.rs", "src/b.rs", "app/[slug]/page.tsx:12"],
                ["node_modules/@types/x/index.d.ts", "src/café/a.rs:3"],
                ["src/c.rs", "src/d.rs"],
                [],
                ["src/a.rs.bak", "src/e.rs", "src/f.rs:3", "src/g.rs"],
                ["C:\\p\\v1.2\\a.rs:4"],
                [], [], [], [],
            ],
            "wrapped": [true, true],
        }),
    );
    let clicked = eval(
        &mut r,
        "(async () => { for (const p of terms.get(0)._core._linkProviderService.linkProviders) \
         for (const l of await new Promise(cb => p.provideLinks(1, cb)) ?? []) \
         if (l.text === 'src/foo.test.ts:12') return l.activate(new MouseEvent('click'), l.text), true; \
         return false; })()",
    );
    assert_eq!(clicked, true, "no src/foo.test.ts:12 link to click");
    let line = "(() => { const p = layout.panes.find(p => p.path?.endsWith('src/foo.test.ts')); \
                const rows = p && document.querySelector(`#p${p.id} .rows`); \
                return rows ? cursorEl(rows)?.dataset.line ?? null : null; })()";
    let t0 = Instant::now();
    while eval(&mut r, line) != "12" {
        assert!(
            t0.elapsed() < common::DEADLINE,
            "the link opened no foo.test.ts on line 12: {}",
            eval(&mut r, line)
        );
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(unix)]
/// Poll `js` in B's page until it is true.
fn until(r: &mut Rig, eval: &mut impl FnMut(&mut Rig, &str) -> Value, js: &str, what: &str) {
    let t0 = Instant::now();
    while eval(r, js) != true {
        assert!(t0.elapsed() < common::DEADLINE, "timed out: {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
/// The point `at` in B's page, where `document.elementFromPoint` must give an element `e` with `hit`.
fn point(
    r: &mut Rig,
    eval: &mut impl FnMut(&mut Rig, &str) -> Value,
    at: &str,
    hit: &str,
) -> (f64, f64) {
    let js = format!(
        "(() => {{ const [x, y] = {at}; const e = document.elementFromPoint(x, y); \
         return e && ({hit}) ? [x, y] : null; }})()"
    );
    let p = eval(r, &js);
    let (Some(x), Some(y)) = (p[0].as_f64(), p[1].as_f64()) else {
        panic!("{at} in B's page doesn't hit {hit}");
    };
    (x, y)
}

/// #190: in the web app, a keystroke or a click that leaves the shell with the keystrokes ends the
/// wait, a keystroke or a click that sends Laura an action does not, and before `laura ready` the
/// hint line dims `c` and `Shift+S`, with the reason on hover.
#[cfg(unix)]
#[test]
fn the_web_app_ends_the_wait_and_dims_comment_before_ready() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    // B's shell rings the bell each time the test writes `bell`.
    let b = WorkspaceB::start(
        r#"printf %s "$LAURA_TAB" > tab; while :; do [ -e bell ] && rm bell && printf '\a'; sleep 0.05; done"#,
    )?;
    let ring = || std::fs::write(b.dir.path().join("bell"), "");
    let id = open(&mut r, &b.url);
    state_until(&mut r, &mut ws, "B loads", id, |v| {
        v["title"].as_str().is_some_and(|t| t.starts_with("Laura"))
    });
    let file = b.dir.path().join("a.rs");
    std::fs::write(&file, "fn main() {}\n")?;
    // The highlight shows a chip in pane 1's header, which acts in the page alone.
    b.drive(&["open", &file.to_string_lossy(), "--highlight", "1"])?;

    let url = pane_of(&mut r, id)["path"]
        .as_str()
        .expect("a URL")
        .to_string();
    let mut tools = Tools::attach(&mut r, &url);
    let mut eval = |r: &mut Rig, js: &str| {
        tools.call(
            r,
            "Runtime.evaluate",
            json!({"expression": js, "returnByValue": true}),
        )["result"]["value"]
            .clone()
    };
    let t0 = Instant::now();
    // B's pane is narrow: the hint line keeps `c`, and drops `Shift+S` with the other groups.
    let dimmed = "document.querySelector('#hint .off')?.textContent ?? ''";
    while eval(&mut r, dimmed) != "ccomment" {
        assert!(
            t0.elapsed() < common::DEADLINE,
            "{}",
            eval(&mut r, "hint.textContent")
        );
        thread::sleep(Duration::from_millis(50));
    }
    let shown = eval(
        &mut r,
        "document.querySelector('#hint .off').dispatchEvent(new MouseEvent('mouseover', {bubbles: true})), \
         [document.querySelectorAll('#sbar .si').length, htip.hidden, htip.innerHTML]",
    );
    assert_eq!(
        shown,
        json!([
            0,
            false,
            "Available after the agent runs <code>laura ready</code>"
        ]),
    );

    /// The wait stays for 500 ms.
    fn holds(r: &mut Rig, eval: &mut impl FnMut(&mut Rig, &str) -> Value, what: &str) {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(500) {
            assert_eq!(eval(r, "layout.waiting"), true, "{what}");
            thread::sleep(Duration::from_millis(50));
        }
    }
    /// #200: the wait shows as the badged icon in B's browser tab, and the title has no `●`.
    fn waiting(r: &mut Rig, eval: &mut impl FnMut(&mut Rig, &str) -> Value, what: &str, on: bool) {
        let js = format!(
            "document.querySelector('link[rel=icon]').href.startsWith('data:image/svg+xml') === {on} \
             && document.title.startsWith('Laura · ')"
        );
        until(r, eval, &js, what);
    }
    ring()?;
    waiting(&mut r, &mut eval, "the wait", true);
    // The page's CSP lets the browser load the badged icon.
    eval(
        &mut r,
        "(() => { const i = new Image(); window.iconLoad = null; \
         i.onload = () => iconLoad = true; i.onerror = () => iconLoad = false; \
         i.src = document.querySelector('link[rel=icon]').href; })()",
    );
    until(
        &mut r,
        &mut eval,
        "iconLoad !== null",
        "the badged icon never loaded",
    );
    assert_eq!(
        eval(&mut r, "iconLoad"),
        true,
        "the page's CSP blocks the badged icon"
    );
    assert_eq!(
        eval(
            &mut r,
            "decodeURIComponent(document.querySelector('link[rel=icon]').href).includes('mask=\"url(#b)\"')"
        ),
        true,
        "the badged icon has no ring cut around the dot"
    );
    // F12 while the pane selector keeps the keystrokes: the user has not reached the shell.
    press_key(&mut ws, id, ("p", "KeyP", 80, 2, ""));
    press(&mut ws, id, "F12", 123);
    until(
        &mut r,
        &mut eval,
        "locked && popup !== null",
        "no lock over the pane selector",
    );
    holds(
        &mut r,
        &mut eval,
        "F12 over the pane selector ended the wait",
    );
    // The close gives the lock's keystrokes to the shell.
    press(&mut ws, id, "Escape", 27);
    waiting(
        &mut r,
        &mut eval,
        "Esc out of the pane selector ends the wait",
        false,
    );
    press(&mut ws, id, "F12", 123);
    ring()?;
    waiting(&mut r, &mut eval, "the wait after the unlock", true);
    press(&mut ws, id, "F12", 123);
    waiting(&mut r, &mut eval, "F12 ends the wait", false);
    press(&mut ws, id, "F12", 123);
    press(&mut ws, id, "Escape", 27);
    // The move to the shell ends a wait: ring the bell after B's server moved the focus (the
    // layout comes from B's server) and the shell has the keystrokes.
    until(
        &mut r,
        &mut eval,
        "layout.focus === 0 && !!document.activeElement?.closest('#p0')",
        "the shell has no focus",
    );
    ring()?;
    waiting(&mut r, &mut eval, "the wait again", true);
    press(&mut ws, id, "PageUp", 33);
    waiting(&mut r, &mut eval, "PageUp ends the wait", false);
    // With no lock too: the open of the pane selector keeps the wait, the close ends the wait.
    ring()?;
    waiting(&mut r, &mut eval, "the wait in the shell", true);
    press_key(&mut ws, id, ("p", "KeyP", 80, 2, ""));
    until(&mut r, &mut eval, "popup !== null", "no pane selector");
    holds(
        &mut r,
        &mut eval,
        "the open of the pane selector ended the wait",
    );
    press(&mut ws, id, "Escape", 27);
    waiting(
        &mut r,
        &mut eval,
        "Esc out of the pane selector into the shell ends the wait",
        false,
    );
    ring()?;
    waiting(&mut r, &mut eval, "the wait before the help", true);
    press_key(&mut ws, id, ("h", "KeyH", 72, 2, ""));
    until(&mut r, &mut eval, "help", "no help");
    // The help leaves the focus in the shell, but keeps the keystrokes.
    holds(&mut r, &mut eval, "the open of the help ended the wait");
    press(&mut ws, id, "Escape", 27);
    waiting(
        &mut r,
        &mut eval,
        "Esc out of the help into the shell ends the wait",
        false,
    );

    let centre = |sel: &str| {
        format!(
            "(b => [b.left + b.width / 2, b.top + b.height / 2])\
             (document.querySelector('{sel}').getBoundingClientRect())"
        )
    };
    // A modifier alone: the user may be leaving the browser, as with `Alt+Tab`.
    ring()?;
    waiting(&mut r, &mut eval, "the wait before Control", true);
    press_key(&mut ws, id, ("Control", "ControlLeft", 17, 2, ""));
    holds(&mut r, &mut eval, "a modifier alone ended the wait");
    let (x, y) = point(
        &mut r,
        &mut eval,
        &centre("#p0 .term"),
        "e.closest('#p0 .term')",
    );
    click(&mut ws, id, x, y);
    waiting(
        &mut r,
        &mut eval,
        "a click in the shell ends the wait",
        false,
    );
    // The press comes before the click that opens the help.
    ring()?;
    waiting(&mut r, &mut eval, "the wait before the help button", true);
    let (x, y) = point(
        &mut r,
        &mut eval,
        &centre("#helpbtn"),
        "e.closest('#helpbtn')",
    );
    mouse(&mut ws, id, "down", x, y);
    holds(
        &mut r,
        &mut eval,
        "a press on the help button ended the wait",
    );
    mouse(&mut ws, id, "up", x, y);
    until(&mut r, &mut eval, "help", "the help button opens the help");
    holds(
        &mut r,
        &mut eval,
        "the click on the help button ended the wait",
    );
    let (x, y) = point(&mut r, &mut eval, "[8, 8]", "e === veil");
    click(&mut ws, id, x, y);
    waiting(
        &mut r,
        &mut eval,
        "a click outside the help into the shell ends the wait",
        false,
    );
    // A click outside the shell that sends no action keeps the wait, though the shell keeps the keystrokes.
    ring()?;
    waiting(
        &mut r,
        &mut eval,
        "the wait before the highlight chip",
        true,
    );
    let (x, y) = point(
        &mut r,
        &mut eval,
        &centre("#p1 [data-act=highlight]"),
        "e.closest('#p1 [data-act=highlight]')",
    );
    click(&mut ws, id, x, y);
    holds(
        &mut r,
        &mut eval,
        "a click on a file pane's highlight chip ended the wait",
    );
    assert_eq!(
        eval(&mut r, "!!document.activeElement?.closest('#p0')"),
        true,
        "the click took the focus from the shell"
    );
    press(&mut ws, id, "PageDown", 34);
    waiting(
        &mut r,
        &mut eval,
        "PageDown in the shell ends the wait",
        false,
    );
    // `Ctrl+`` sends Laura an action, and the focus moves after the reply.
    ring()?;
    waiting(&mut r, &mut eval, "the wait before Ctrl+`", true);
    press_key(&mut ws, id, ("`", "Backquote", 192, 2, ""));
    until(
        &mut r,
        &mut eval,
        "layout.focus === 1",
        "Ctrl+` moves to the file pane",
    );
    holds(&mut r, &mut eval, "Ctrl+` out of the shell ended the wait");
    press(&mut ws, id, "Escape", 27);
    waiting(
        &mut r,
        &mut eval,
        "Esc back into the shell ends the wait",
        false,
    );

    b.drive(&["ready"])?;
    let t0 = Instant::now();
    while eval(&mut r, dimmed) != "" {
        assert!(t0.elapsed() < common::DEADLINE, "dimmed after laura ready");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

/// #212: a drag of the drawer's grip sets the drawer's height, and Laura saves the height in B's
/// `~/.laura/settings.json`. The shell resizes once, on release. The drawer keeps 5 rows of the
/// shell, a drag ends on any loss of the pointer capture, and a double-click resets the height.
#[cfg(unix)]
#[test]
fn the_drawers_grip_sets_the_height_and_keeps_5_rows() -> Result<()> {
    let Some((path, _one)) = chrome() else {
        return Ok(());
    };
    let (mut r, mut ws) = rig(path)?;
    let b = WorkspaceB::start(r#"printf %s "$LAURA_TAB" > tab; while :; do sleep 1; done"#)?;
    let id = open(&mut r, &b.url);
    state_until(&mut r, &mut ws, "B loads", id, |v| {
        v["title"].as_str().is_some_and(|t| t.starts_with("Laura"))
    });
    let file = b.dir.path().join("a.rs");
    std::fs::write(&file, "fn main() {}\n")?;
    b.drive(&["open", &file.to_string_lossy()])?;
    b.drive(&["focus", "0"])?;
    let url = pane_of(&mut r, id)["path"]
        .as_str()
        .expect("a URL")
        .to_string();
    let mut tools = Tools::attach(&mut r, &url);
    let mut eval = |r: &mut Rig, js: &str| {
        tools.call(
            r,
            "Runtime.evaluate",
            json!({"expression": js, "returnByValue": true}),
        )["result"]["value"]
            .clone()
    };
    let settings = b.home.path().join(".laura/settings.json");
    let drawer = || {
        std::fs::read_to_string(&settings)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map_or(Value::Null, |v| v["drawer"].clone())
    };
    // Poll B's saved `drawer` until it is `want`. Each poll pumps A, which forwards the events.
    fn saved(
        r: &mut Rig,
        eval: &mut impl FnMut(&mut Rig, &str) -> Value,
        drawer: impl Fn() -> Value,
        want: Value,
        what: &str,
    ) {
        let t0 = Instant::now();
        while drawer() != want {
            assert!(
                t0.elapsed() < common::DEADLINE,
                "{what}: drawer {}",
                drawer()
            );
            eval(r, "0");
            thread::sleep(Duration::from_millis(50));
        }
    }
    let held_move = |ws: &mut Ws, x: f64, y: f64| {
        send(
            ws,
            json!({"act": "browser_mouse", "pane": id, "kind": "move", "x": x, "y": y, "buttons": 1}),
        );
    };
    let grip = "e.closest('#p0 .grip')";
    let centre = "(b => [b.left + b.width / 2, b.top + b.height / 2])\
                  (document.querySelector('#p0 .grip').getBoundingClientRect())";
    until(
        &mut r,
        &mut eval,
        "document.getElementById('p0').classList.contains('open') && !!terms.get(0)?.rows",
        "the drawer never opened",
    );

    // The drag shows the height, and the shell resizes only on release.
    let rows = eval(&mut r, "terms.get(0).rows");
    let (x, y) = point(&mut r, &mut eval, centre, grip);
    mouse(&mut ws, id, "down", x, y);
    held_move(&mut ws, x, y - 120.0);
    until(
        &mut r,
        &mut eval,
        "document.querySelector('#p0 .grip').dataset.tip.startsWith('Drawer · ')",
        "the drag shows no height",
    );
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(300) {
        assert_eq!(
            eval(&mut r, "terms.get(0).rows"),
            rows,
            "the shell resized during the drag"
        );
        thread::sleep(Duration::from_millis(50));
    }
    mouse(&mut ws, id, "up", x, y - 120.0);
    let shown = eval(&mut r, "parseInt(root.style.getPropertyValue('--drawer'))");
    saved(&mut r, &mut eval, drawer, shown, "the drag saved no height");
    // The shell keeps the keystrokes after a drag.
    until(
        &mut r,
        &mut eval,
        "!!document.activeElement?.closest('#p0 .term')",
        "the drag took the focus from the shell",
    );
    until(
        &mut r,
        &mut eval,
        &format!("terms.get(0).rows !== {rows}"),
        "the shell never fit the new height",
    );

    // At the bottom of the page, the drawer keeps 5 rows, and no row is cut off.
    let (x, y) = point(&mut r, &mut eval, centre, grip);
    mouse(&mut ws, id, "down", x, y);
    let bottom = eval(&mut r, "innerHeight - 1").as_f64().expect("a height");
    held_move(&mut ws, x, bottom);
    until(&mut r, &mut eval, "resizing", "no drag to the bottom");
    mouse(&mut ws, id, "up", x, bottom);
    let least = eval(&mut r, "drawerRange()[0]");
    // B's page is short: the 5 rows need more than 20%.
    assert!(least.as_u64().is_some_and(|l| l > 20), "{least}");
    saved(&mut r, &mut eval, drawer, least, "the drag to the bottom");
    until(
        &mut r,
        &mut eval,
        "(() => { const s = document.querySelector('#p0 .xterm-screen').getBoundingClientRect(), \
         t = document.querySelector('#p0 .term').getBoundingClientRect(); \
         return !resizing && terms.get(0).rows >= 5 && s.bottom <= t.bottom; })()",
        "the drawer cuts off a row of the shell",
    );

    // Any loss of the capture ends the drag.
    let (x, y) = point(&mut r, &mut eval, centre, grip);
    mouse(&mut ws, id, "down", x, y);
    held_move(&mut ws, x, y - 40.0);
    until(&mut r, &mut eval, "resizing", "no drag");
    eval(
        &mut r,
        "document.querySelector('#p0 .grip').dispatchEvent(new PointerEvent('lostpointercapture'))",
    );
    until(&mut r, &mut eval, "!resizing", "the drag never ended");
    mouse(&mut ws, id, "up", x, y - 40.0);

    // A double-click goes back to the default height.
    let (x, y) = point(&mut r, &mut eval, centre, grip);
    for clicks in [1, 2] {
        for kind in ["down", "up"] {
            send(
                &mut ws,
                json!({"act": "browser_mouse", "pane": id, "kind": kind, "x": x, "y": y,
                       "button": 0, "buttons": u8::from(kind == "down"), "clicks": clicks}),
            );
        }
    }
    saved(
        &mut r,
        &mut eval,
        drawer,
        Value::Null,
        "the double-click kept the height",
    );
    until(
        &mut r,
        &mut eval,
        "root.style.getPropertyValue('--drawer') === '54%'",
        "the double-click shows no default height",
    );

    // In the shell, Ctrl+Shift+↑ ↓ change the height by 5%.
    until(
        &mut r,
        &mut eval,
        "!!document.activeElement?.closest('#p0 .term')",
        "the shell has no focus",
    );
    press_key(&mut ws, id, ("ArrowUp", "ArrowUp", 38, 10, ""));
    saved(&mut r, &mut eval, drawer, json!(59), "Ctrl+Shift+↑");
    press_key(&mut ws, id, ("ArrowDown", "ArrowDown", 40, 10, ""));
    saved(&mut r, &mut eval, drawer, json!(54), "Ctrl+Shift+↓");
    // Under the lock, the shell gets them.
    press(&mut ws, id, "F12", 123);
    until(&mut r, &mut eval, "locked", "no lock");
    press_key(&mut ws, id, ("ArrowUp", "ArrowUp", 38, 10, ""));
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(300) {
        eval(&mut r, "0");
        assert_eq!(drawer(), json!(54), "Ctrl+Shift+↑ under the lock");
        thread::sleep(Duration::from_millis(50));
    }
    press(&mut ws, id, "F12", 123);
    until(&mut r, &mut eval, "!locked", "the lock stays");

    // The focused grip: ↑ ↓ step the height, and Home resets it.
    eval(&mut r, "document.querySelector('#p0 .grip').focus()");
    press(&mut ws, id, "ArrowUp", 38);
    saved(&mut r, &mut eval, drawer, json!(59), "↑ on the grip");
    until(
        &mut r,
        &mut eval,
        "document.querySelector('#p0 .grip').getAttribute('aria-valuenow') === '59'",
        "the grip tells no height",
    );
    press(&mut ws, id, "Home", 36);
    saved(&mut r, &mut eval, drawer, Value::Null, "Home on the grip");
    until(
        &mut r,
        &mut eval,
        "root.style.getPropertyValue('--drawer') === '54%'",
        "Home shows no default height",
    );

    // The quit confirmation and the help take the grip's keystrokes first.
    let unchanged = |r: &mut Rig, eval: &mut dyn FnMut(&mut Rig, &str) -> Value, what: &str| {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(300) {
            assert_eq!(
                eval(r, "root.style.getPropertyValue('--drawer')"),
                json!("54%"),
                "{what}"
            );
            assert_eq!(drawer(), Value::Null, "{what}");
            thread::sleep(Duration::from_millis(50));
        }
    };
    press_key(&mut ws, id, ("q", "KeyQ", 81, 2, ""));
    until(&mut r, &mut eval, "confirmQuit", "Ctrl+Q asks nothing");
    press(&mut ws, id, "ArrowUp", 38);
    until(
        &mut r,
        &mut eval,
        "!confirmQuit",
        "↑ kept the quit confirmation",
    );
    unchanged(&mut r, &mut eval, "↑ after Ctrl+Q");
    eval(&mut r, "document.querySelector('#p0 .grip').focus()");
    press_key(&mut ws, id, ("h", "KeyH", 72, 2, ""));
    until(&mut r, &mut eval, "help", "Ctrl+H shows no help");
    press(&mut ws, id, "ArrowUp", 38);
    until(&mut r, &mut eval, "!help", "↑ kept the help");
    unchanged(&mut r, &mut eval, "↑ after Ctrl+H");
    // The grip still steps the height after them.
    eval(&mut r, "document.querySelector('#p0 .grip').focus()");
    press(&mut ws, id, "ArrowUp", 38);
    saved(&mut r, &mut eval, drawer, json!(59), "↑ on the grip again");
    Ok(())
}
