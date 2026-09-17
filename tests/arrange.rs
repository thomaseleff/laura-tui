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
    pane_rect(json, id).2
}

/// `(x, y, width, height)` of the pane with `id` from a layout report.
fn pane_rect(json: &str, id: u64) -> (u16, u16, u16, u16) {
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    let r = &v["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"].as_u64() == Some(id))
        .unwrap()["rect"];
    let f = |k| r[k].as_u64().unwrap() as u16;
    (f("x"), f("y"), f("width"), f("height"))
}

/// `path` of the pane with `id` from a layout report.
fn pane_path(json: &str, id: u64) -> String {
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    v["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"].as_u64() == Some(id))
        .unwrap()["path"]
        .as_str()
        .unwrap()
        .to_string()
}

/// `true` if pane `id` reports `clipped`.
fn pane_clipped(json: &str, id: u64) -> bool {
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    v["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"].as_u64() == Some(id))
        .unwrap()["clipped"]
        .as_bool()
        .unwrap()
}

/// Like `drive`, but returns whether the client exited zero (for asserting errors).
fn try_drive(tab: &mut Tab, args: &[&str]) -> bool {
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
    out.status.success()
}

/// #36: `open <path> --panel <id>` swaps a pane's file in place — same id, rect, focus, no split.
#[test]
fn open_panel_replaces_in_place() -> Result<()> {
    let mut a = tempfile::Builder::new().suffix(".txt").tempfile()?;
    writeln!(a, "alpha")?;
    let mut b = tempfile::Builder::new().suffix(".txt").tempfile()?;
    writeln!(b, "beta")?;
    let pa = a.path().to_str().unwrap().to_string();
    let pb = b.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &pa]).parse()?;
    let before = drive(&mut tab, &["layout"]);
    let rect_before = pane_rect(&before, id);

    // Replace in place: same id, no new split.
    let replaced: u64 = drive(&mut tab, &["open", &pb, "--panel", &id.to_string()]).parse()?;
    assert_eq!(replaced, id, "replace keeps the same pane id");

    let after = drive(&mut tab, &["layout"]);
    assert_eq!(pane_count(&after), pane_count(&before), "no new pane");
    assert_eq!(pane_rect(&after, id), rect_before, "rect unchanged");
    let expected = std::path::absolute(&pb)?.to_string_lossy().into_owned();
    assert_eq!(pane_path(&after, id), expected, "content swapped to b");

    // The PTY and absent ids can't be replaced.
    assert!(
        !try_drive(&mut tab, &["open", &pb, "--panel", "0"]),
        "PTY errors"
    );
    assert!(
        !try_drive(&mut tab, &["open", &pb, "--panel", "999"]),
        "absent id errors"
    );
    Ok(())
}

/// #37: a bare `open` (no flags) dwindles — splits the *newest* pane, alternating orientation
/// by depth, so pane 0 is split once then never re-targeted and no pane collapses.
#[test]
fn bare_open_dwindles() -> Result<()> {
    let mut f = tempfile::Builder::new().suffix(".txt").tempfile()?;
    writeln!(f, "hello")?;
    let p = f.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    let mut ids = vec![0u64]; // the PTY
    ids.push(drive(&mut tab, &["open", &p]).parse()?);

    // pane 0's rect after the first split — dwindle must never touch it again.
    let pane0_after_first = pane_rect(&drive(&mut tab, &["layout"]), 0);

    for _ in 0..2 {
        ids.push(drive(&mut tab, &["open", &p]).parse()?);
        assert_eq!(
            pane_rect(&drive(&mut tab, &["layout"]), 0),
            pane0_after_first,
            "dwindle must split off the newest pane, never re-split pane 0"
        );
    }

    let layout = drive(&mut tab, &["layout"]);
    assert_eq!(pane_count(&layout), 4);
    let rects: Vec<_> = ids.iter().map(|&id| pane_rect(&layout, id)).collect();

    for &id in &ids {
        assert!(!pane_clipped(&layout, id), "pane #{id} collapsed/clipped");
    }
    // Both orientations were used: rects vary in x AND in y.
    let xs: std::collections::HashSet<u16> = rects.iter().map(|r| r.0).collect();
    let ys: std::collections::HashSet<u16> = rects.iter().map(|r| r.1).collect();
    assert!(
        xs.len() > 1,
        "expected varied x offsets (horizontal splits)"
    );
    assert!(ys.len() > 1, "expected varied y offsets (vertical splits)");

    // Explicit flags bypass inference: a horizontal split preserves the target's height.
    let target = ids[1];
    let (_, _, _, h_before) = pane_rect(&layout, target);
    let id: u64 = drive(
        &mut tab,
        &["open", &p, "--dir", "h", "--split", &target.to_string()],
    )
    .parse()?;
    let (_, _, _, h_after) = pane_rect(&drive(&mut tab, &["layout"]), id);
    assert_eq!(h_after, h_before, "horizontal split should preserve height");
    Ok(())
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
