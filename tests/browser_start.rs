//! Laura's browser fails to start (#139): the agent gets the reason, the journal gets Chrome's
//! own output, and the workspace responds while the start runs. `LAURA_BROWSER_URL` answers
//! before Laura's browser starts (#129). Its own test binary, so no other test's spawn holds a fake Chrome open for writing
//! (ETXTBSY on Linux).

mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value, json};

use rig::*;

/// The fake Chromes, written once before any test spawns one (rewriting a script another test is
/// executing fails with ETXTBSY on Linux). Under the target dir: a `.cmd` whose path has a space
/// can't be spawned. `fail` prints a D-Bus error and its arguments to stderr, as Chrome does on a
/// host with no session bus, then exits 1. `slow` writes `started` next to itself, waits about
/// 3 s, then exits 1. `hang` (Unix) writes its pid and arguments to `argv` next to itself, starts
/// a helper that writes the profile every 50 ms for about 60 s, as Chrome's helpers do, then
/// waits 60 s. `crash` (Unix) does the same as `hang` but exits 1 at once, and leaves the helper
/// running.
fn fakes() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fake_chrome");
        #[cfg(windows)]
        let (name, fail, slow) = (
            "chrome.cmd",
            "@echo [1:1:ERROR:dbus/bus.cc(408)] Failed to connect to the bus 1>&2\r\n@echo %* 1>&2\r\n@exit /b 1\r\n",
            "@echo.>\"%~dp0started\"\r\n@ping -n 4 127.0.0.1 >nul\r\n@exit /b 1\r\n",
        );
        #[cfg(not(windows))]
        let (name, fail, slow) = (
            "chrome",
            "#!/bin/sh\necho '[1:1:ERROR:dbus/bus.cc(408)] Failed to connect to the bus' >&2\necho \"$@\" >&2\nexit 1\n",
            "#!/bin/sh\ntouch \"$(dirname \"$0\")/started\"\nsleep 3\nexit 1\n",
        );
        #[allow(unused_mut)] // only Unix adds `hang` and `crash`
        let mut kinds = vec![("fail", fail), ("slow", slow)];
        #[cfg(unix)]
        let helper = "#!/bin/sh\necho \"$$ $*\" > \"$(dirname \"$0\")/argv\"\n\
             for a; do case $a in --user-data-dir=*) p=${a#--user-data-dir=};; esac; done\n\
             (i=0; while [ $i -lt 1200 ]; do mkdir -p \"$p\"; touch \"$p/helper\"; sleep 0.05; i=$((i+1)); done) &\n";
        #[cfg(unix)]
        let (hang, crash) = (format!("{helper}exec sleep 60\n"), format!("{helper}exit 1\n"));
        #[cfg(unix)]
        kinds.extend([("hang", hang.as_str()), ("crash", crash.as_str())]);
        for (kind, body) in kinds {
            let d = dir.join(kind);
            std::fs::create_dir_all(&d).unwrap();
            let f = d.join(name);
            std::fs::write(&f, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        dir
    })
}

fn fake(kind: &str) -> PathBuf {
    let name = if cfg!(windows) {
        "chrome.cmd"
    } else {
        "chrome"
    };
    fakes().join(kind).join(name)
}

#[test]
fn a_failed_start_records_chromes_output() -> Result<()> {
    let mut r = Rig::new()?;
    r.tab.browser_path = Ok(fake("fail"));
    let ready: Value = serde_json::from_str(&r.drive(&["ready"]))?;
    let out = r.run(&["open", "http://127.0.0.1:1/"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    for want in ["couldn't start Laura's browser", "exited as it started"] {
        assert!(err.contains(want), "{want:?} not in {err}");
    }
    assert!(!err.contains("--disable-background-networking"), "{err}");

    let journal = std::fs::read_to_string(ready["journal"].as_str().unwrap_or(""))?;
    let event: Value = journal
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|e| e["type"] == "browser")
        .expect("a browser event");
    assert!(
        event["error"]
            .as_str()
            .is_some_and(|e| e.contains("exited as it started")),
        "{event}"
    );
    let output = event["output"].as_str().unwrap_or_default();
    for want in [
        "--disable-background-networking",
        "--disable-component-update",
    ] {
        assert!(output.contains(want), "{want:?} not in {output}");
    }
    assert!(!output.contains(":ERROR:dbus/"), "{output}");
    #[cfg(target_os = "macos")]
    assert!(output.contains("--use-mock-keychain"), "{output}");
    Ok(())
}

/// Laura reaps a Chrome that exits as it starts before closing it, so the watcher kills a
/// group whose leader is already gone.
#[cfg(unix)]
#[test]
fn a_crashed_browser_s_helpers_close_with_it() -> Result<()> {
    let crash = fake("crash");
    let argv = crash.with_file_name("argv");
    let _ = std::fs::remove_file(&argv);
    let mut r = Rig::new()?;
    r.tab.browser_path = Ok(crash);
    let out = r.run(&["open", "http://127.0.0.1:1/"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("exited as it started"), "{err}");

    let line = std::fs::read_to_string(&argv)?;
    let profile = PathBuf::from(
        line.split_whitespace()
            .find_map(|a| a.strip_prefix("--user-data-dir="))
            .expect("a --user-data-dir argument"),
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(!profile.exists(), "a helper wrote the profile again");
    Ok(())
}

#[test]
fn a_slow_start_keeps_the_workspace_responding() -> Result<()> {
    let slow = fake("slow");
    let started = slow.with_file_name("started");
    let _ = std::fs::remove_file(&started);
    let mut r = Rig::new()?;
    r.tab.browser_path = Ok(slow);
    let mut ws = r.connect();
    let rx = r.start(&["open", "http://127.0.0.1:1/"]);
    // Each turn of the loop stays short while the fake waits: the start runs off the loop.
    let t0 = Instant::now();
    while !started.exists() {
        let t = Instant::now();
        r.pump();
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "a turn took {:?}",
            t.elapsed()
        );
        assert!(t0.elapsed() < common::DEADLINE, "the fake never started");
    }
    let t = Instant::now();
    assert_eq!(
        r.act(&mut ws, json!({"act": "focus", "pane": 0}))["ok"],
        true
    );
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "the view waited {:?}",
        t.elapsed()
    );
    assert!(
        rx.try_recv().is_err(),
        "open answered before the start ended"
    );

    let out = r.wait(&rx);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("couldn't start Laura's browser"), "{err}");
    Ok(())
}

/// A raw `GET /json/version` to `LAURA_BROWSER_URL` with `host`: the status line and the body.
fn version(port: u16, host: &str) -> Result<(String, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(s, "GET /json/version HTTP/1.1\r\nHost: {host}\r\n\r\n")?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    Ok((head.lines().next().unwrap_or("").into(), body.into()))
}

#[test]
fn the_browser_url_answers_before_laura_s_browser_starts() -> Result<()> {
    isolate_home();
    // No browser pane, so Laura doesn't start the fake.
    let tab = laura::Tab::spawn_web(shell(), 24, 80, Ok(fake("fail")))?;
    let port = tab.browser_port.expect("a browser port");
    let (status, _) = version(port, &format!("evil.example:{port}"))?;
    assert_eq!(status, "HTTP/1.1 403 Forbidden");
    let (status, body) = version(port, &format!("localhost:{port}"))?;
    assert_eq!(status, "HTTP/1.1 503 Service Unavailable");
    assert_eq!(
        body,
        "Laura's browser is not running. Open a browser pane, then try again."
    );
    Ok(())
}

/// Kills the child on drop, so a failed assertion doesn't leave a workspace running.
#[cfg(unix)]
struct Kill(std::process::Child);

#[cfg(unix)]
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(unix)]
#[test]
fn a_killed_laura_closes_its_browser() -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let hang = fake("hang");
    let argv = hang.with_file_name("argv");
    let _ = std::fs::remove_file(&argv);
    isolate_home();
    let shell = format!(
        "{} open http://127.0.0.1:1/ & sleep 60",
        env!("CARGO_BIN_EXE_laura")
    );
    let mut laura = Kill(
        Command::new(env!("CARGO_BIN_EXE_laura"))
            .args(["--web", "--", "/bin/sh", "-c", &shell])
            .env("LAURA_EXPERIMENTAL_WEB", "1")
            .env("LAURA_EXPERIMENTAL_BROWSER", "1")
            .env("LAURA_BROWSER", &hang)
            .env_remove("LAURA_TAB")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Laura leads its own process group, so the test can kill the group as a supervisor does.
            .process_group(0)
            .spawn()?,
    );
    let t0 = Instant::now();
    let line = loop {
        if let Ok(l) = std::fs::read_to_string(&argv)
            && l.ends_with('\n')
        {
            break l;
        }
        assert!(
            t0.elapsed() < common::DEADLINE,
            "Laura never started the fake"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let pid = line
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    let profile = PathBuf::from(
        line.split_whitespace()
            .find_map(|a| a.strip_prefix("--user-data-dir="))
            .expect("a --user-data-dir argument"),
    );
    // Running, and not a zombie.
    let alive = || {
        let out = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid])
            .output();
        out.is_ok_and(|o| {
            let stat = String::from_utf8_lossy(&o.stdout);
            !stat.trim().is_empty() && !stat.trim().starts_with('Z')
        })
    };
    assert!(alive(), "the fake isn't running");
    assert!(profile.exists(), "{} is missing", profile.display());

    // A SIGKILL to Laura's group kills Laura too, so this covers both kinds of kill.
    Command::new("kill")
        .args(["-s", "KILL", "--", &format!("-{}", laura.0.id())])
        .status()?;
    laura.0.wait()?;
    let t0 = Instant::now();
    while alive() || profile.exists() {
        assert!(
            t0.elapsed() < common::DEADLINE,
            "after Laura's kill, the fake is running: {}, the profile exists: {}",
            alive(),
            profile.exists()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(500));
    assert!(!profile.exists(), "a helper wrote the profile again");
    Ok(())
}
