//! Browser panes with no browser to run (#107): `LAURA_BROWSER` names a missing file. Its own test
//! binary, so the environment it sets reaches no other test.

mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use anyhow::Result;
use laura::browser;

use rig::*;

#[test]
fn a_missing_browser_shows_how_to_get_one() -> Result<()> {
    // SAFETY: the binary's one test, before anything reads the environment.
    unsafe {
        std::env::set_var("LAURA_EXPERIMENTAL_BROWSER", "1");
        std::env::set_var("LAURA_BROWSER", "/no/such/chrome");
    }
    assert_eq!(browser::resolve(), Err(browser::NO_BROWSER.to_string()));
    let mut r = Rig::new()?;
    r.tab.browser_path = browser::resolve();
    let out = r.run(&["open", "http://127.0.0.1:1/"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(browser::NO_BROWSER), "{err}");
    Ok(())
}
