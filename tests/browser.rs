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

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use laura::browser;
use serde_json::json;
use tungstenite::Message;

use browser_rig::*;
use rig::*;

#[test]
fn a_url_needs_the_browser_flag() -> Result<()> {
    let mut r = bare()?;
    let err = error(&mut r, &["open", "http://127.0.0.1:1/"]);
    assert!(err.contains(browser::FLAG_OFF), "{err}");
    Ok(())
}

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
    let (id, _) = load(&mut r, &mut ws, "/deny");
    next_frame(&mut r, &mut ws, id);
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
            .env("LAURA_EXPERIMENTAL_WEB", "1")
            .env("LAURA_EXPERIMENTAL_BROWSER", "1")
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
            "after Laura's kill, still running: {left:?}; the profile exists: {}",
            profile.exists()
        );
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_millis(500));
    assert!(!profile.exists(), "a helper wrote the profile again");
    Ok(())
}
