//! The alt-screen and mouse-capture accessors that route scroll/mouse input: a real child that
//! flips the vt100 mode, observed through the public `PtyTab` surface. ponytail: real child, no
//! test-only parser seam; encoder correctness rides the `src/mouse.rs` unit tests.

use std::time::{Duration, Instant};

use laura::PtyTab;
use portable_pty::CommandBuilder;

/// Spawn a 24x80 PTY whose child emits `seq` (literal bytes) then exits.
fn spawn_emitting(seq: &str) -> PtyTab {
    let mut cmd = CommandBuilder::new(if cfg!(windows) { "cmd" } else { "sh" });
    if cfg!(windows) {
        cmd.args(["/c", &format!("echo {seq}")]);
    } else {
        cmd.args(["-c", &format!("printf '%s' '{seq}'")]);
    }
    PtyTab::spawn(cmd, 24, 80).expect("spawn pty")
}

/// Let the child exit and the reader drain its output into the parser.
fn settle(tab: &PtyTab) {
    let start = Instant::now();
    while !tab.has_exited() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "child never exited"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(150));
}

#[test]
fn alt_screen_switch_is_observable() {
    // `ESC[?1049h` enters the alternate screen (claude, vim, less); a plain child never does.
    let alt = spawn_emitting("\x1b[?1049h");
    settle(&alt);
    assert!(
        alt.on_alt_screen(),
        "child that entered the alt screen reads true"
    );

    let main = spawn_emitting("hello");
    settle(&main);
    assert!(!main.on_alt_screen(), "a main-screen child reads false");
}

// ceiling: ConPTY consumes the DECSET mouse-tracking sequences (it manages mouse itself) before our
// parser sees them, so `mouse_capture()` can't be observed through a Windows child. SGR passthrough is
// unix-effective; on Windows the wheel falls back to PageUp/PageDown key-forwarding (Phase B).
#[cfg(unix)]
#[test]
fn sgr_mouse_mode_is_observable() {
    // `ESC[?1000h ESC[?1006h` enables button reporting with SGR (1006) encoding — forwardable.
    let mouse = spawn_emitting("\x1b[?1000h\x1b[?1006h");
    settle(&mouse);
    assert!(
        mouse.mouse_capture().is_some(),
        "child in SGR mouse mode is forwardable"
    );

    let plain = spawn_emitting("hello");
    settle(&plain);
    assert!(
        plain.mouse_capture().is_none(),
        "a child with no mouse mode is not"
    );
}
