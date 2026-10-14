//! Process/terminal: one PTY-hosted shell/agent and its parsed vt100 screen.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

/// Answer ConPTY's `ESC[6n` cursor query; it withholds child output until replied. Fixed `1;1` — nothing tracks the real cursor.
pub fn dsr_reply(chunk: &[u8]) -> Option<&'static [u8]> {
    chunk
        .windows(4)
        .any(|w| w == b"\x1b[6n")
        .then_some(b"\x1b[1;1R")
}

/// Lines of PTY history kept for wheel/PageUp scrollback.
/// ponytail: fixed 10k-line cap; configurable only if asked.
const SCROLLBACK: usize = 10_000;

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;
/// Live copies of the PTY's raw bytes (the web app's terminals). Locked inside the parser lock.
type Subscribers = Arc<Mutex<Vec<SyncSender<Vec<u8>>>>>;

/// Chunks a subscriber may fall behind before the reader drops it, so a stalled view never blocks the shell.
const SUBSCRIBER_BOUND: usize = 256;

/// Floor a PTY/vt100 size to what vt100 can hold: a wide char needs ≥2 cols, and it
/// panics (screen.rs wide-char path unwraps `None`) on a 0/1-col grid — reachable when
/// many splits shrink a pane to nothing. Rows likewise never zero.
fn floor_size(rows: u16, cols: u16) -> (u16, u16) {
    (rows.max(1), cols.max(2))
}

/// One PTY-hosted shell/agent plus its parsed screen. Killed on drop, winding down the reader/wait threads.
pub struct PtyTab {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: SharedWriter,
    exited: Arc<AtomicBool>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    subscribers: Subscribers,
}

impl PtyTab {
    /// Spawn `cmd` in a fresh `rows`x`cols` PTY, streaming output into a vt100 parser.
    pub fn spawn(cmd: CommandBuilder, rows: u16, cols: u16) -> Result<Self> {
        let (rows, cols) = floor_size(rows, cols);
        let pair = native_pty_system().openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let reader = pair.master.try_clone_reader()?;
        let writer: SharedWriter = Arc::new(Mutex::new(pair.master.take_writer()?));
        let mut child = pair.slave.spawn_command(cmd)?;
        // Drop the slave or the master read side never sees the child's output.
        drop(pair.slave);

        let killer = child.clone_killer();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)));

        let subscribers = Subscribers::default();
        spawn_reader(reader, parser.clone(), writer.clone(), subscribers.clone());

        // ConPTY never returns EOF on child exit; detect it via child.wait().
        let exited = Arc::new(AtomicBool::new(false));
        {
            let exited = exited.clone();
            std::thread::spawn(move || {
                let _ = child.wait();
                exited.store(true, Ordering::SeqCst);
            });
        }

        Ok(Self {
            parser,
            writer,
            exited,
            killer,
            master: pair.master,
            subscribers,
        })
    }

    /// The screen so far as escape codes, then every later byte. Taken under the parser lock so a
    /// late joiner neither misses nor repeats bytes. The receiver disconnects once it falls
    /// `SUBSCRIBER_BOUND` chunks behind; subscribe again for a fresh replay.
    pub fn subscribe(&self) -> (Vec<u8>, Receiver<Vec<u8>>) {
        let (tx, rx) = sync_channel(SUBSCRIBER_BOUND);
        let mut p = self.parser.lock().expect("parser mutex poisoned");
        let mut replay = scrollback_formatted(p.screen_mut());
        let s = p.screen();
        // vt100's state_formatted() doesn't emit the alt-screen switch.
        if s.alternate_screen() {
            replay.extend(b"\x1b[?1049h");
        }
        replay.extend(s.state_formatted());
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.push(tx);
        }
        (replay, rx)
    }

    /// True once the hosted child has exited.
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// Forward raw input bytes (keystrokes, a review) to the child.
    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("can't write to the shell"))?;
        w.write_all(bytes)?;
        w.flush()
    }

    /// Resize both the PTY and the parser grid.
    pub fn resize(&self, rows: u16, cols: u16) {
        let (rows, cols) = floor_size(rows, cols);
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut p) = self.parser.lock() {
            p.screen_mut().set_size(rows, cols);
            p.screen_mut().set_scrollback(0); // a resize never leaves a stale mid-history view
        }
    }

    /// Scroll the view `delta` rows into history (`+`) or toward live (`-`). Clamps at live (0); vt100 clamps the far end.
    pub fn scroll(&self, delta: isize) {
        if let Ok(mut p) = self.parser.lock() {
            let cur = p.screen().scrollback() as isize;
            p.screen_mut().set_scrollback((cur + delta).max(0) as usize);
        }
    }

    /// Snap the view back to live (offset 0).
    pub fn to_live(&self) {
        if let Ok(mut p) = self.parser.lock() {
            p.screen_mut().set_scrollback(0);
        }
    }

    /// Current scrollback offset from live (0 = live).
    pub fn scrollback_offset(&self) -> usize {
        self.parser
            .lock()
            .map(|p| p.screen().scrollback())
            .unwrap_or(0)
    }

    /// Rows of history above the live view (the largest valid offset). Net-zero probe: bump to the internal clamp, read it, restore.
    pub fn scrollback_max(&self) -> usize {
        self.parser
            .lock()
            .map(|mut p| {
                let cur = p.screen().scrollback();
                p.screen_mut().set_scrollback(usize::MAX); // clamps to the real history height
                let max = p.screen().scrollback();
                p.screen_mut().set_scrollback(cur); // restore — net no-op
                max
            })
            .unwrap_or(0)
    }

    /// True while the child is on the vt100 alternate screen (claude, vim, less) — no scrollback there.
    pub fn on_alt_screen(&self) -> bool {
        self.with_screen(|s| s.alternate_screen())
    }

    /// The child's mouse mode *iff* it's forwardable: some mode enabled **and** SGR (1006) encoding.
    /// `None` otherwise — folds the SGR gate in so callers get one clean decision.
    pub fn mouse_capture(&self) -> Option<vt100::MouseProtocolMode> {
        self.with_screen(|s| {
            let mode = s.mouse_protocol_mode();
            (mode != vt100::MouseProtocolMode::None
                && s.mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr)
                .then_some(mode)
        })
    }

    /// Run `f` against the current parsed screen (for rendering).
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        let p = self.parser.lock().expect("parser mutex poisoned");
        f(p.screen())
    }
}

impl Drop for PtyTab {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}

/// The main screen's history as escape codes, oldest row first, then enough newlines to scroll it
/// all off-screen, so the late joiner's own scrollback holds it. The alt screen keeps no history.
fn scrollback_formatted(s: &mut vt100::Screen) -> Vec<u8> {
    let (rows, cols) = s.size();
    let cur = s.scrollback();
    s.set_scrollback(usize::MAX);
    let max = s.scrollback();
    let mut out = vec![];
    // ponytail: one set_scrollback per history row, fine at SCROLLBACK's 10k.
    for off in (1..=max).rev() {
        s.set_scrollback(off);
        out.extend(s.rows_formatted(0, cols).next().unwrap_or_default());
        out.extend(b"\x1b[m\r\n");
    }
    s.set_scrollback(cur);
    if max > 0 {
        out.extend(b"\r\n".repeat(rows.saturating_sub(1).into()));
    }
    out
}

// Feed every PTY byte into the vt100 parser and answer ConPTY's DSR query. No clean shutdown — ConPTY never EOFs, the loop ends with the process.
fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
    writer: SharedWriter,
    subscribers: Subscribers,
) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Some(reply) = dsr_reply(&buf[..n])
                        && let Ok(mut w) = writer.lock()
                    {
                        let _ = w.write_all(reply);
                        let _ = w.flush();
                    }
                    if let Ok(mut p) = parser.lock() {
                        p.process(&buf[..n]);
                        // Drop a full or dead subscriber rather than wait on it.
                        if let Ok(mut subs) = subscribers.lock() {
                            subs.retain(|tx| match tx.try_send(buf[..n].to_vec()) {
                                Ok(()) => true,
                                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
                            });
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::floor_size;

    #[test]
    fn floors_below_vt100_wide_char_minimum() {
        // vt100 panics on a 0/1-col grid; the floor keeps every size viable.
        assert_eq!(floor_size(0, 0), (1, 2));
        assert_eq!(floor_size(24, 1), (24, 2));
        assert_eq!(floor_size(24, 80), (24, 80)); // roomy sizes pass through
    }
}
