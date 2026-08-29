//! PTY scrollback: a real child emitting more than one screen of lines, then the view moves
//! into history and clamps back to live. ponytail: real child; no test-only parser seam.

use std::time::{Duration, Instant};

use laura::PtyTab;
use portable_pty::CommandBuilder;

/// Spawn a 24x80 PTY whose child prints `line 1`..`line 60` (well over one screen) and exits.
fn spawn_sixty_lines() -> PtyTab {
    let mut cmd = CommandBuilder::new(if cfg!(windows) { "cmd" } else { "sh" });
    if cfg!(windows) {
        cmd.args(["/c", "for /L %i in (1,1,60) do @echo line %i"]);
    } else {
        cmd.args(["-c", "for i in $(seq 60); do echo line $i; done"]);
    }
    PtyTab::spawn(cmd, 24, 80).expect("spawn pty")
}

#[test]
fn scroll_moves_into_history_then_clamps() {
    let tab = spawn_sixty_lines();

    // Let the child finish emitting, then let the reader drain the tail.
    let start = Instant::now();
    while !tab.has_exited() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "child never exited"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(150));

    // Live view: offset 0, the tail is on screen.
    assert_eq!(tab.scrollback_offset(), 0, "starts at live");
    let live = tab.with_screen(|s| s.contents());
    assert!(
        live.contains("line 60"),
        "live view shows the last line: {live:?}"
    );

    // Scroll up past the top: offset grows and clamps; an early line comes into view.
    tab.scroll(50);
    assert!(
        tab.scrollback_offset() > 0,
        "scroll up increases the offset"
    );
    let hist = tab.with_screen(|s| s.contents());
    assert!(
        hist.contains("line 1\n"),
        "history view shows an early line: {hist:?}"
    );

    // Scroll far toward live clamps back to 0; to_live also snaps.
    tab.scroll(-1000);
    assert_eq!(
        tab.scrollback_offset(),
        0,
        "scroll down past live clamps to 0"
    );
    tab.scroll(50);
    tab.to_live();
    assert_eq!(tab.scrollback_offset(), 0, "to_live snaps to live");
}
