//! A file-backed panel re-reads its source on disk change — no `update` call. Through the real run-loop seam: `laura open` → tab socket → `Tab::drain` re-renders on the next tick. State, not pixels.

use std::io::{Seek, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Rect, Tab};

/// Spawn a tab hosting a trivial, quick-exiting command (we only need its socket).
fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a real tab, draining (and replying) until the client exits.
fn drive_tab(tab: &mut Tab, args: &[&str]) {
    let name = tab.name.clone();
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let ok = Command::cargo_bin("laura")
            .unwrap()
            .args(&a)
            .env("LAURA_TAB", &name)
            .output()
            .unwrap()
            .status
            .success();
        tx.send(ok).unwrap();
    });
    let start = Instant::now();
    let ok = loop {
        tab.drain(area());
        if let Ok(ok) = rx.try_recv() {
            break ok;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    assert!(ok, "laura {args:?} exited non-zero");
}

#[test]
fn panel_reloads_when_source_changes() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "v1")?;
    file.flush()?;
    let path = file.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    drive_tab(&mut tab, &["open", &path]);
    let id = *tab.panels.keys().next().expect("open created a panel");
    assert_eq!(tab.panels[&id].content, "v1");

    // Overwrite with a different-length body; the (mtime, len) signature changes.
    file.as_file().set_len(0)?;
    file.rewind()?;
    write!(file, "v2!")?;
    file.flush()?;

    // The next drain tick live-reloads changed panels — the same seam the run loop calls.
    tab.drain(area());
    assert_eq!(tab.panels[&id].content, "v2!");
    Ok(())
}
