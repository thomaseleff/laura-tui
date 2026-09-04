//! Feat #15: `laura highlight <start> <end>` sets a panel's 0-based highlight range and centers the
//! span in the viewport. State through the real seam (binary → socket → `Tab::drain`), no pixels.

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

#[test]
fn highlight_sets_range_and_anchor() -> Result<()> {
    // 10-line file so clamps have room.
    let mut f = tempfile::Builder::new().suffix(".txt").tempfile()?;
    for i in 1..=10 {
        writeln!(f, "line {i}")?;
    }
    let p = f.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &p, "--no-focus"]).parse()?;
    let id_s = id.to_string();

    // 1-based 3..=5 → 0-based (2, 4); cursor anchors on hi.
    drive(&mut tab, &["highlight", "3", "5", "--pane", &id_s]);
    let panel = &tab.panels[&id];
    assert_eq!(panel.highlight, Some((2, 4)));
    assert_eq!(panel.cursor, 4);

    // Fully out-of-range clamps to the last line.
    drive(&mut tab, &["highlight", "999", "--pane", &id_s]);
    assert_eq!(tab.panels[&id].highlight, Some((9, 9)));

    // Single-line form: end defaults to start.
    drive(&mut tab, &["highlight", "2", "--pane", &id_s]);
    assert_eq!(tab.panels[&id].highlight, Some((1, 1)));
    Ok(())
}

/// A fresh highlight centers its span in the viewport (short passage), and top-anchors it when the
/// passage is taller than the pane — asserted through the public `scroll_offset` seam.
#[test]
fn highlight_centers_then_top_anchors() -> Result<()> {
    let mut f = tempfile::Builder::new().suffix(".txt").tempfile()?;
    for i in 1..=100 {
        writeln!(f, "line {i}")?;
    }
    let p = f.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &p, "--no-focus"]).parse()?;

    // Short span (1-based 40..=52 → rows 39..=51, height 13) in a 20-row viewport: centered, so
    // ~3 rows of context sit above the span's first row (margin = (20-13)/2 = 3).
    drive(
        &mut tab,
        &["highlight", "40", "52", "--pane", &id.to_string()],
    );
    let panel = &tab.panels[&id];
    let layout = panel.layout(80);
    assert_eq!(panel.scroll_offset(&layout, 20), 36);

    // Span taller than the 8-row viewport: margin saturates to 0, top-anchored at the first row.
    drive(
        &mut tab,
        &["highlight", "40", "60", "--pane", &id.to_string()],
    );
    let panel = &tab.panels[&id];
    assert_eq!(panel.scroll_offset(&panel.layout(80), 8), 39);
    Ok(())
}

/// `open --highlight <start> <end>` paints the new panel already pointed-at: same panel state as the
/// standalone verb, proving the shared `set_highlight` fires on open (open-and-point in one call).
#[test]
fn open_highlight_points_on_open() -> Result<()> {
    let mut f = tempfile::Builder::new().suffix(".txt").tempfile()?;
    for i in 1..=10 {
        writeln!(f, "line {i}")?;
    }
    let p = f.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    let id: u64 = drive(
        &mut tab,
        &["open", &p, "--no-focus", "--highlight", "3", "5"],
    )
    .parse()?;
    let panel = &tab.panels[&id];
    assert_eq!(panel.highlight, Some((2, 4)));
    assert_eq!(panel.cursor, 4);

    // Single-line form: end defaults to start.
    let id2: u64 = drive(&mut tab, &["open", &p, "--no-focus", "--highlight", "2"]).parse()?;
    assert_eq!(tab.panels[&id2].highlight, Some((1, 1)));
    Ok(())
}
