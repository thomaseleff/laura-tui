//! Process/terminal: one PTY-hosted shell/agent and its parsed vt100 screen.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// One PTY-hosted shell/agent plus its parsed screen. Killed on drop, winding down the reader/wait threads.
pub struct PtyTab {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: SharedWriter,
    exited: Arc<AtomicBool>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
}

impl PtyTab {
    /// Spawn `cmd` in a fresh `rows`x`cols` PTY, streaming output into a vt100 parser.
    pub fn spawn(cmd: CommandBuilder, rows: u16, cols: u16) -> Result<Self> {
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

        spawn_reader(reader, parser.clone(), writer.clone());

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
        })
    }

    /// True once the hosted child has exited.
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// Forward raw input bytes (keystrokes) to the child.
    pub fn write(&self, bytes: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Resize both the PTY and the parser grid.
    pub fn resize(&self, rows: u16, cols: u16) {
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

// Feed every PTY byte into the vt100 parser and answer ConPTY's DSR query. No clean shutdown — ConPTY never EOFs, the loop ends with the process.
fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
    writer: SharedWriter,
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
                    }
                }
                Err(_) => break,
            }
        }
    });
}
