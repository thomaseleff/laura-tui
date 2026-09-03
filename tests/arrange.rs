//! The agent composes a frame through the CLI: split panes, read their ids, introspect with `layout`, prove `--dry-run` commits nothing, and close panes individually or `--all`. State, not pixels.

use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{Rect, Tab};

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a real tab, draining (and replying) until it exits; return trimmed stdout.
fn drive(tab: &mut Tab, args: &[&str]) -> String {
    let name = tab.socket.clone();
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let out = Command::cargo_bin("laura")
            .unwrap()
            .args(&a)
            .env("LAURA_TAB", &name)
            .output()
            .unwrap();
        tx.send(out).unwrap();
    });
    let start = Instant::now();
    let out = loop {
        tab.drain(area());
        if let Ok(out) = rx.try_recv() {
            break out;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    assert!(
        out.status.success(),
        "laura {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn pane_count(json: &str) -> usize {
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    v["panes"].as_array().unwrap().len()
}

/// Width of the pane with `id` from a layout report.
fn pane_width(json: &str, id: u64) -> u16 {
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    let pane = v["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"].as_u64() == Some(id))
        .unwrap();
    pane["rect"]["width"].as_u64().unwrap() as u16
}

/// #13: `--ratio N` sizes the NEW panel to N%, independent of `--side`.
#[test]
fn ratio_targets_the_new_panel() -> Result<()> {
    let mut f = tempfile::Builder::new().suffix(".txt").tempfile()?;
    writeln!(f, "hello")?;
    let p = f.path().to_str().unwrap().to_string();

    // Tab is 120 wide (see `area`); a horizontal --ratio 75 → new panel ~90 cols.
    for side in ["second", "first"] {
        let mut tab = spawn_tab()?;
        let id: u64 = drive(
            &mut tab,
            &["open", &p, "--dir", "h", "--ratio", "75", "--side", side],
        )
        .parse()?;
        let w = pane_width(&drive(&mut tab, &["layout"]), id);
        assert!(
            (85..=95).contains(&w),
            "--side {side}: new panel should be ~75% of 120 (~90), got {w}"
        );
    }
    Ok(())
}

#[test]
fn compose_introspect_and_close() -> Result<()> {
    let mut f1 = tempfile::Builder::new().suffix(".txt").tempfile()?;
    writeln!(f1, "hello")?;
    let mut f2 = tempfile::Builder::new().suffix(".txt").tempfile()?;
    writeln!(f2, "logs")?;
    let p1 = f1.path().to_str().unwrap().to_string();
    let p2 = f2.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;

    // Split the shell horizontally; `open` prints the new pane id.
    let id1: u64 = drive(&mut tab, &["open", &p1, "--dir", "h"]).parse()?;
    // Split that pane vertically.
    let id2: u64 = drive(
        &mut tab,
        &["open", &p2, "--split", &id1.to_string(), "--dir", "v"],
    )
    .parse()?;
    assert_ne!(id1, id2);

    // `layout` shows all three panes (shell + two panels).
    let layout = drive(&mut tab, &["layout"]);
    assert_eq!(pane_count(&layout), 3);

    // `--dry-run` reports but commits nothing: layout before == after.
    let before = drive(&mut tab, &["layout"]);
    let dry = drive(&mut tab, &["open", &p1, "--dry-run"]);
    assert!(pane_count(&dry) >= 3, "dry-run reports the would-be layout");
    let after = drive(&mut tab, &["layout"]);
    assert_eq!(before, after, "dry-run must not mutate state");

    // Close one pane, then all.
    drive(&mut tab, &["close", &id2.to_string()]);
    assert_eq!(pane_count(&drive(&mut tab, &["layout"])), 2);
    drive(&mut tab, &["close", "--all"]);
    assert_eq!(
        pane_count(&drive(&mut tab, &["layout"])),
        1,
        "back to PTY-only"
    );
    assert!(tab.panels.is_empty());

    // Back-compat: a bare `open` (no flags) still works.
    let id3: u64 = drive(&mut tab, &["open", &p1]).parse()?;
    assert!(id3 > 0);
    Ok(())
}
