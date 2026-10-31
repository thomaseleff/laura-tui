//! Laura's browser fails to start (#139): the error ends with Chrome's own output. Its own test
//! binary, so no other test's spawn holds the fake Chrome open for writing (ETXTBSY on Linux).

mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use std::path::Path;

use anyhow::Result;

use rig::*;

#[test]
fn a_failed_start_shows_chromes_output() -> Result<()> {
    // Under the target dir: a `.cmd` whose path has a space can't be spawned.
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fake_chrome");
    std::fs::create_dir_all(&dir)?;
    #[cfg(windows)]
    let (name, body) = ("chrome.cmd", "@echo %* 1>&2\r\n@exit /b 1\r\n");
    #[cfg(not(windows))]
    let (name, body) = ("chrome", "#!/bin/sh\necho \"$@\" >&2\nexit 1\n");
    let fake = dir.join(name);
    std::fs::write(&fake, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))?;
    }

    let mut r = Rig::new()?;
    r.tab.browser_path = Ok(fake);
    let out = r.run(&["open", "http://127.0.0.1:1/"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    for want in [
        "couldn't start Laura's browser",
        "exited as it started",
        "--disable-background-networking",
        "--disable-component-update",
    ] {
        assert!(err.contains(want), "{want:?} not in {err}");
    }
    Ok(())
}
