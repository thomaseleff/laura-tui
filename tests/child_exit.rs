//! Child exit is detected without reader EOF (ConPTY never sends one); `PtyTab` catches it via `child.wait()`.

use std::time::{Duration, Instant};

use laura::PtyTab;
use portable_pty::CommandBuilder;

#[test]
fn detects_child_exit() {
    let mut cmd = CommandBuilder::new(if cfg!(windows) { "cmd" } else { "sh" });
    if cfg!(windows) {
        cmd.args(["/c", "exit"]);
    } else {
        cmd.args(["-c", "exit 0"]);
    }

    let tab = PtyTab::spawn(cmd, 24, 80).expect("spawn pty");

    let start = Instant::now();
    while !tab.has_exited() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "child exit not detected within 10s"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
