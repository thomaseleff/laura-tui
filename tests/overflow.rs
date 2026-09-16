//! A panel with known long content reports overflow in a small rect and none in a large one, through `Tab::report` (which drives `rects` + `Panel::layout`).

use std::io::Write;
use std::process::Output;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::protocol::{Dir, Side};
use laura::{PTY_PANE, Panel, Rect, Tab};

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a real tab, draining until it exits; return the raw `Output` (success or not).
fn try_drive(tab: &mut Tab, args: &[&str]) -> Output {
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
    out
}

/// Like `try_drive` but asserts success; returns trimmed stdout.
fn drive(tab: &mut Tab, args: &[&str]) -> String {
    let out = try_drive(tab, args);
    assert!(
        out.status.success(),
        "laura {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn small_rect_clips_a_long_panel_large_rect_fits() -> Result<()> {
    // 100 short lines: ~100 content rows, no wrapping at a normal width.
    let mut file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    for i in 1..=100 {
        writeln!(file, "line {i}")?;
    }
    file.flush()?;
    let path = file.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    tab.layout
        .split(PTY_PANE, Dir::Horizontal, 50, Side::Second, 1)
        .unwrap();
    tab.panels.insert(1, Panel::open(path));

    // Small area: the panel gets a short right half — content overflows.
    let small = tab.report(Rect::new(0, 0, 80, 12));
    let p = small.panes.iter().find(|p| p.id == 1).unwrap();
    assert!(p.overflow_rows > 0, "long content overflows a short rect");
    assert!(p.clipped);

    // Tall area: everything fits.
    let big = tab.report(Rect::new(0, 0, 80, 300));
    let p = big.panes.iter().find(|p| p.id == 1).unwrap();
    assert_eq!(p.overflow_rows, 0, "tall rect fits the content");
    assert!(!p.clipped);

    Ok(())
}

fn min_pane_width(json: &str) -> u16 {
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    v["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["rect"]["width"].as_u64().unwrap() as u16)
        .min()
        .unwrap()
}

/// #57: repeatedly halving a pane refuses the split that would drop it below the render floor.
#[test]
fn split_below_floor_is_refused() -> Result<()> {
    let mut f = tempfile::Builder::new().suffix(".txt").tempfile()?;
    writeln!(f, "hello")?;
    let p = f.path().to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;

    // area() is 120 wide; each --ratio 50 halves the targeted pane: 120→60→30→15→7→3(→refuse).
    let mut target = PTY_PANE;
    let refused = loop {
        let out = try_drive(
            &mut tab,
            &[
                "open",
                &p,
                "--split",
                &target.to_string(),
                "--dir",
                "h",
                "--ratio",
                "50",
            ],
        );
        if !out.status.success() {
            break out;
        }
        target = String::from_utf8_lossy(&out.stdout).trim().parse()?;
    };

    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("too small to render"),
        "expected floor-refusal message, got: {stderr}"
    );

    // The collapse never committed: no pane sits below the 3-cell floor.
    let layout = drive(&mut tab, &["layout"]);
    assert!(
        min_pane_width(&layout) >= 3,
        "a pane committed below the render floor: {layout}"
    );

    Ok(())
}
