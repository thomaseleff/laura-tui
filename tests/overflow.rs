//! A panel with known long content reports overflow in a small rect and none in a large one, through `Tab::report` (which drives `rects` + `Panel::layout`).

use std::io::Write;

use anyhow::Result;
use laura::protocol::{Dir, Side};
use laura::{PTY_PANE, Panel, Rect, Tab};

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
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
