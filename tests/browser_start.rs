//! Laura's browser fails to start (#139): the agent gets the reason, and the journal gets Chrome's
//! own output. Its own test binary, so no other test's spawn holds a fake Chrome open for writing
//! (ETXTBSY on Linux).

mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;
use serde_json::Value;

use rig::*;

/// The fake Chrome, written once before any test spawns it. Under the target dir: a `.cmd` whose
/// path has a space can't be spawned. It prints a D-Bus error and its arguments to stderr, as
/// Chrome does on a host with no session bus, then exits 1.
fn fake() -> &'static Path {
    static FAKE: OnceLock<PathBuf> = OnceLock::new();
    FAKE.get_or_init(|| {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fake_chrome");
        #[cfg(windows)]
        let (name, body) = (
            "chrome.cmd",
            "@echo [1:1:ERROR:dbus/bus.cc(408)] Failed to connect to the bus 1>&2\r\n@echo %* 1>&2\r\n@exit /b 1\r\n",
        );
        #[cfg(not(windows))]
        let (name, body) = (
            "chrome",
            "#!/bin/sh\necho '[1:1:ERROR:dbus/bus.cc(408)] Failed to connect to the bus' >&2\necho \"$@\" >&2\nexit 1\n",
        );
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(name);
        std::fs::write(&f, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        f
    })
}

#[test]
fn a_failed_start_records_chromes_output() -> Result<()> {
    let mut r = Rig::new()?;
    r.tab.browser_path = Ok(fake().to_path_buf());
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
    Ok(())
}
