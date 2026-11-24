//! The TUI's mouse: SGR passthrough, pane-clamped selection, OSC 52 copy, and the border between
//! two panes that the user drags (`handle_at`, `ratio_at`), with the pointer's state across events
//! (`Pointer`). The pure parts' unit tests live here.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Margin, Rect};
use ratatui::style::Modifier;

use laura::PaneId;
use laura::layout::Handle;
use laura::protocol::Dir;
use vt100::MouseProtocolMode;

/// The TUI's pointer state across mouse events. A drag can't span a tab switch, so this is a
/// `run()` local — `Tab` stays untouched.
#[derive(Default)]
pub struct Pointer {
    /// An in-progress left-drag selection.
    pub selection: Option<Selection>,
    /// The split whose line the user drags, from the press to the release.
    pub split: Option<Handle>,
    /// The last press on a line, with the line's two panes: a second press within
    /// `DOUBLE_CLICK` is a double-click.
    last_down: Option<(Instant, (PaneId, PaneId))>,
}

impl Pointer {
    /// A press on `line` at `now`: true when it is the second press of a double-click.
    pub fn press_line(&mut self, line: (PaneId, PaneId), now: Instant) -> bool {
        let again = self
            .last_down
            .is_some_and(|(t, l)| l == line && now - t < DOUBLE_CLICK);
        self.last_down = (!again).then_some((now, line));
        again
    }

    /// The user dragged the line: the next press starts a new double-click.
    pub fn moved_line(&mut self) {
        self.last_down = None;
    }
}

/// The longest time between the two presses of a double-click.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// An in-progress (or just-finished) drag selection, in absolute buffer coords.
pub struct Selection {
    /// The pane the drag started in; the head is clamped to this pane for the whole drag.
    pub pane: PaneId,
    pub anchor: (u16, u16),
    pub head: (u16, u16),
}

/// How much of the button/motion protocol a mode reports (cumulative): Press=1 (down+wheel),
/// PressRelease=2 (+release), ButtonMotion=3 (+drag), AnyMotion=4 (+bare motion).
fn mode_level(mode: MouseProtocolMode) -> u8 {
    match mode {
        MouseProtocolMode::None => 0,
        MouseProtocolMode::Press => 1,
        MouseProtocolMode::PressRelease => 2,
        MouseProtocolMode::ButtonMotion => 3,
        MouseProtocolMode::AnyMotion => 4,
    }
}

fn button_code(b: MouseButton) -> u16 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// Encode a mouse event as SGR (1006) mouse-report bytes relative to `pty_inner` (1-based), or
/// `None` when `mode` doesn't report that event kind. Sgr-only is a documented ceiling — every
/// modern app negotiates 1006, so Default/Utf8 encoders would be dead code.
pub fn sgr_mouse_bytes(
    m: &MouseEvent,
    pty_inner: Rect,
    mode: MouseProtocolMode,
) -> Option<Vec<u8>> {
    use MouseEventKind::*;
    let lvl = mode_level(mode);
    // `Cb` base + press/release final byte. Drag/motion add +32; bare motion bases at 3; wheel = 64/65.
    let (mut cb, release) = match m.kind {
        Down(b) => (button_code(b), false),
        ScrollUp => (64, false),
        ScrollDown => (65, false),
        Up(b) if lvl >= 2 => (button_code(b), true),
        Drag(b) if lvl >= 3 => (button_code(b) + 32, false),
        Moved if lvl >= 4 => (3 + 32, false),
        _ => return None,
    };
    if m.modifiers.contains(KeyModifiers::SHIFT) {
        cb += 4;
    }
    if m.modifiers.contains(KeyModifiers::ALT) {
        cb += 8;
    }
    if m.modifiers.contains(KeyModifiers::CONTROL) {
        cb += 16;
    }
    let cx = m.column.saturating_sub(pty_inner.x) + 1;
    let cy = m.row.saturating_sub(pty_inner.y) + 1;
    let final_ = if release { 'm' } else { 'M' };
    Some(format!("\x1b[<{cb};{cx};{cy}{final_}").into_bytes())
}

/// Clamp `(col, row)` into `rect`'s inclusive interior (never past the last cell, never before the first).
pub fn clamp_point(rect: Rect, col: u16, row: u16) -> (u16, u16) {
    let max_x = rect.right().saturating_sub(1).max(rect.x);
    let max_y = rect.bottom().saturating_sub(1).max(rect.y);
    (col.clamp(rect.x, max_x), row.clamp(rect.y, max_y))
}

/// Order two points in reading order (row-major): the earlier one first.
pub fn normalize(anchor: (u16, u16), head: (u16, u16)) -> ((u16, u16), (u16, u16)) {
    if (anchor.1, anchor.0) <= (head.1, head.0) {
        (anchor, head)
    } else {
        (head, anchor)
    }
}

/// The inclusive `[x0, x1]` cells selected on row `y`: linear/reading-order — anchor col→right edge on
/// the first row, full width on middle rows, left edge→head col on the last (start/end pre-clamped to `inner`).
fn row_span(inner: Rect, start: (u16, u16), end: (u16, u16), y: u16) -> (u16, u16) {
    let x0 = if y == start.1 { start.0 } else { inner.x };
    let x1 = if y == end.1 {
        end.0
    } else {
        inner.right().saturating_sub(1)
    };
    (x0, x1)
}

/// Update `sel` from a left button event: `Down` starts a selection in the pane under the pointer;
/// `Drag` extends the head, clamped to the *original* pane. Release is finalized by the caller (copy
/// needs the terminal buffer).
pub fn update_selection(
    sel: &mut Option<Selection>,
    m: &MouseEvent,
    over: Option<PaneId>,
    rect_map: &HashMap<PaneId, Rect>,
) {
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(pane) = over
                && let Some(rect) = rect_map.get(&pane)
            {
                let p = clamp_point(rect.inner(Margin::new(1, 1)), m.column, m.row);
                *sel = Some(Selection {
                    pane,
                    anchor: p,
                    head: p,
                });
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if let Some(s) = sel.as_mut()
                && let Some(rect) = rect_map.get(&s.pane)
            {
                s.head = clamp_point(rect.inner(Margin::new(1, 1)), m.column, m.row);
            }
        }
        _ => {}
    }
}

/// Reverse-video every cell in the selected span (uniform for PTY + panels): a post-draw buffer pass.
pub fn highlight(buf: &mut Buffer, inner: Rect, anchor: (u16, u16), head: (u16, u16)) {
    let (start, end) = normalize(anchor, head);
    for y in start.1..=end.1 {
        let (x0, x1) = row_span(inner, start, end, y);
        for x in x0..=x1 {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.modifier ^= Modifier::REVERSED;
            }
        }
    }
}

/// Read the selected glyphs from the drawn buffer (respects scrollback + wrapping automatically),
/// right-trimming each row and joining with `\n`.
pub fn extract(buf: &Buffer, inner: Rect, anchor: (u16, u16), head: (u16, u16)) -> String {
    let (start, end) = normalize(anchor, head);
    let mut rows = Vec::new();
    for y in start.1..=end.1 {
        let (x0, x1) = row_span(inner, start, end, y);
        let mut row = String::new();
        for x in x0..=x1 {
            if let Some(cell) = buf.cell((x, y)) {
                row.push_str(cell.symbol());
            }
        }
        rows.push(row.trim_end().to_string());
    }
    rows.join("\n")
}

/// Panel twin of `extract`: reads the same drawn glyphs but drops the leading `gutter` columns and
/// rejoins a source line's soft-wrapped rows with a space — only *distinct* source lines break with
/// `\n`, so a drag copies clean source, not gutter digits or display wrapping. `off` is the panel's
/// scroll offset; `line_at[layout_row]` maps a laid-out row to its source line, `None` for a comment
/// row (a removed-lines row or a thread card row), which always takes a line of its own. Rows past
/// `line_at` (below the file) are not copied.
///
/// ponytail: a word hard-split mid-token across rows (word wider than the pane) rejoins with a stray
/// space — rare; live with it until it bites.
pub fn extract_panel(
    buf: &Buffer,
    inner: Rect,
    gutter: u16,
    off: usize,
    line_at: &[Option<usize>],
    anchor: (u16, u16),
    head: (u16, u16),
) -> String {
    let (start, end) = normalize(anchor, head);
    let text_x0 = inner.x + gutter;
    let mut out = String::new();
    let mut prev: Option<Option<usize>> = None; // None before the first row
    for y in start.1..=end.1 {
        // `off + y` only grows, so every later row is below the file too.
        let Some(&line) = line_at.get(off + (y - inner.y) as usize) else {
            break;
        };
        let (x0, x1) = row_span(inner, start, end, y);
        let mut row = String::new();
        for x in x0.max(text_x0)..=x1 {
            if let Some(cell) = buf.cell((x, y)) {
                row.push_str(cell.symbol());
            }
        }
        let row = row.trim_end();
        if let Some(p) = prev {
            out.push(if line.is_some() && line == p {
                ' '
            } else {
                '\n'
            });
        }
        out.push_str(row);
        prev = Some(line);
    }
    out
}

/// Wrap `text` in an OSC 52 clipboard-set sequence for Laura's own stdout.
pub fn osc52(text: &str) -> Vec<u8> {
    format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes())).into_bytes()
}

/// Terminal.app ignores OSC 52, so on macOS Laura also hands the selection to `pbcopy`.
#[cfg(target_os = "macos")]
pub fn pbcopy(text: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    // Without a UTF-8 locale, pbcopy reads the bytes as Mac Roman. `LC_ALL` overrides a user's
    // `LC_CTYPE` and `LANG`.
    let Ok(mut c) = Command::new("pbcopy")
        .env("LC_ALL", "en_US.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(mut stdin) = c.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = c.wait();
}

/// Copies `text` with `pbcopy` off the render loop, as a process start is slow. One worker copies
/// the selections in order, so a slow `pbcopy` can't put an older selection back on the clipboard.
#[cfg(target_os = "macos")]
pub fn pbcopy_in_order(text: String) {
    use std::sync::{OnceLock, mpsc};
    static TX: OnceLock<mpsc::Sender<String>> = OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || rx.into_iter().for_each(|t| pbcopy(&t)));
        tx
    });
    let _ = tx.send(text);
}

/// Standard base64 (RFC 4648). Inline (~15 lines) since base64 is only a transitive dep and the repo
/// pins every direct dep — a few lines beat a new pin.
pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// The line under `col`, `row`: the two border cells where `first`'s last column (or row) meets
/// `second`'s first. `handles` lists outer splits first, so where two lines meet, the outer wins.
pub fn handle_at(handles: &[Handle], col: u16, row: u16) -> Option<&Handle> {
    let on = |p: u16, start: u16, at: u16| p + 1 >= start + at && p <= start + at;
    let within = |p: u16, start: u16, len: u16| p >= start && p < start + len;
    handles.iter().find(|h| {
        let a = h.area;
        match h.dir {
            Dir::Horizontal => on(col, a.x, h.at) && within(row, a.y, a.height),
            Dir::Vertical => on(row, a.y, h.at) && within(col, a.x, a.width),
        }
    })
}

/// The ratio for `first` that puts `h`'s line under `col`, `row`, within the split's range.
pub fn ratio_at(h: &Handle, col: u16, row: u16) -> u16 {
    let (p, start, len) = match h.dir {
        Dir::Horizontal => (col, h.area.x, h.area.width),
        Dir::Vertical => (row, h.area.y, h.area.height),
    };
    let len = u32::from(len.max(1));
    let share = (u32::from(p.saturating_sub(start)) * 100 + len / 2) / len;
    (share.min(100) as u16).clamp(h.min, h.max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_double_click_on_a_line_needs_two_presses_without_a_drag() {
        let (t, line) = (Instant::now(), (0, 1));
        let ms = Duration::from_millis;
        let mut p = Pointer::default();
        assert!(!p.press_line(line, t));
        assert!(p.press_line(line, t + ms(300)));
        // A third press starts again.
        assert!(!p.press_line(line, t + ms(400)));

        let mut p = Pointer::default();
        p.press_line(line, t);
        p.moved_line();
        assert!(!p.press_line(line, t + ms(300)));

        let mut p = Pointer::default();
        p.press_line(line, t);
        assert!(!p.press_line(line, t + ms(600)));
        assert!(!p.press_line((1, 2), t + ms(700)));
    }

    fn ev(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn sgr_encodes_press_release_and_drag() {
        let inner = Rect::new(0, 0, 80, 24);
        // Left press at the origin, then release.
        assert_eq!(
            sgr_mouse_bytes(
                &ev(MouseEventKind::Down(MouseButton::Left), 0, 0),
                inner,
                MouseProtocolMode::PressRelease,
            )
            .unwrap(),
            b"\x1b[<0;1;1M"
        );
        assert_eq!(
            sgr_mouse_bytes(
                &ev(MouseEventKind::Up(MouseButton::Left), 0, 0),
                inner,
                MouseProtocolMode::PressRelease,
            )
            .unwrap(),
            b"\x1b[<0;1;1m"
        );
        // Drag carries the +32 motion bit.
        assert_eq!(
            sgr_mouse_bytes(
                &ev(MouseEventKind::Drag(MouseButton::Left), 0, 0),
                inner,
                MouseProtocolMode::ButtonMotion,
            )
            .unwrap(),
            b"\x1b[<32;1;1M"
        );
        // Wheel up is button 64.
        assert_eq!(
            sgr_mouse_bytes(
                &ev(MouseEventKind::ScrollUp, 0, 0),
                inner,
                MouseProtocolMode::Press
            )
            .unwrap(),
            b"\x1b[<64;1;1M"
        );
    }

    #[test]
    fn sgr_gates_events_the_mode_does_not_report() {
        let inner = Rect::new(0, 0, 80, 24);
        // Drag is silent under PressRelease; bare motion is silent under ButtonMotion.
        assert!(
            sgr_mouse_bytes(
                &ev(MouseEventKind::Drag(MouseButton::Left), 0, 0),
                inner,
                MouseProtocolMode::PressRelease,
            )
            .is_none()
        );
        assert!(
            sgr_mouse_bytes(
                &ev(MouseEventKind::Moved, 0, 0),
                inner,
                MouseProtocolMode::ButtonMotion
            )
            .is_none()
        );
    }

    #[test]
    fn clamp_holds_points_inside_the_rect() {
        let r = Rect::new(2, 3, 10, 5); // cols 2..=11, rows 3..=7
        assert_eq!(clamp_point(r, 0, 0), (2, 3));
        assert_eq!(clamp_point(r, 100, 100), (11, 7));
        assert_eq!(clamp_point(r, 5, 4), (5, 4));
    }

    #[test]
    fn normalize_orders_anchor_and_head() {
        assert_eq!(normalize((5, 2), (1, 1)), ((1, 1), (5, 2)));
        assert_eq!(normalize((1, 1), (5, 1)), ((1, 1), (5, 1)));
    }

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(osc52("hi"), b"\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn extract_reads_the_multi_row_span_from_the_drawn_buffer() {
        // Copy-on-release reads the rendered buffer; this locks the reading-order geometry:
        // anchor→edge on the first row, full width on the middle, edge→head on the last, right-trimmed.
        let inner = Rect::new(0, 0, 6, 3);
        let mut buf = Buffer::empty(inner);
        for (y, s) in ["abcdef", "ghijkl", "mnopqr"].iter().enumerate() {
            buf.set_string(0, y as u16, s, ratatui::style::Style::default());
        }
        // Drag from (3,0) to (2,2): "def" / "ghijkl" / "mno".
        assert_eq!(extract(&buf, inner, (3, 0), (2, 2)), "def\nghijkl\nmno");
        // Reversed endpoints select the same span (normalize orders them).
        assert_eq!(extract(&buf, inner, (2, 2), (3, 0)), "def\nghijkl\nmno");
    }

    #[test]
    fn extract_panel_drops_gutter_and_rejoins_wrapped_lines() {
        // A 2-col gutter, source line 0 wrapped across rows 0-1 ("hello" / "world"), line 1 on row 2.
        let inner = Rect::new(0, 0, 8, 3);
        let mut buf = Buffer::empty(inner);
        for (y, s) in [" 1 hello", "   world", " 2 bye"].iter().enumerate() {
            buf.set_string(0, y as u16, s, ratatui::style::Style::default());
        }
        let line_at = [Some(0), Some(0), Some(1)];
        // Whole selection: gutter dropped, wrap rejoined with a space, distinct line breaks.
        assert_eq!(
            extract_panel(&buf, inner, 3, 0, &line_at, (3, 0), (5, 2)),
            "hello world\nbye"
        );
        // Partial: start column past "he" honored (still floored past the gutter).
        assert_eq!(
            extract_panel(&buf, inner, 3, 0, &line_at, (5, 0), (5, 2)),
            "llo world\nbye"
        );
        // A removed-lines row tagged with line 1 still takes a line of its own; row 2 is below the file.
        let inner = Rect::new(0, 0, 22, 3);
        let mut buf = Buffer::empty(inner);
        for (y, s) in [" 2 bye", "   ── 1 line removed"].iter().enumerate() {
            buf.set_string(0, y as u16, s, ratatui::style::Style::default());
        }
        assert_eq!(
            extract_panel(&buf, inner, 3, 0, &[Some(1), None], (3, 0), (21, 1)),
            "bye\n── 1 line removed"
        );
        // Rows below the file copy nothing.
        assert_eq!(
            extract_panel(&buf, inner, 3, 0, &[Some(1)], (3, 0), (21, 2)),
            "bye"
        );
        assert_eq!(
            extract_panel(&buf, inner, 3, 0, &[Some(1)], (3, 1), (21, 2)),
            ""
        );
    }

    /// The clipboard tests run in parallel and share one clipboard.
    #[cfg(target_os = "macos")]
    static CLIPBOARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(target_os = "macos")]
    fn pbpaste() -> String {
        let out = std::process::Command::new("pbpaste")
            .env("LC_ALL", "en_US.UTF-8")
            .output()
            .expect("pbpaste runs on macOS");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pbcopy_puts_the_text_on_the_clipboard() {
        let _clipboard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        // shortcut: puts back text only, an image on the clipboard is lost; save the pasteboard
        // types if a developer hits it.
        let saved = pbpaste();
        let text = "laura ✓ copy";
        pbcopy(text);
        let got = pbpaste();
        pbcopy(&saved);
        assert_eq!(got, text);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn quick_selections_reach_the_clipboard_in_order() {
        use std::time::{Duration, Instant};
        let _clipboard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        // shortcut: puts back text only, as in `pbcopy_puts_the_text_on_the_clipboard`.
        let saved = pbpaste();
        for i in 1..=20 {
            pbcopy_in_order(i.to_string());
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while pbpaste() != "20" && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        // A late older copy would replace "20" in this time.
        std::thread::sleep(Duration::from_millis(300));
        let got = pbpaste();
        pbcopy(&saved);
        assert_eq!(got, "20");
    }

    /// The root's vertical line at column 40 of 80, and a stacked split in its right side.
    fn two_lines() -> Vec<Handle> {
        let line = |dir, area, at| Handle {
            dir,
            area,
            at,
            ratio: 50,
            pinned: false,
            first: vec![1],
            second: vec![2],
            min: 10,
            max: 90,
        };
        vec![
            line(Dir::Horizontal, Rect::new(0, 0, 80, 20), 40),
            line(Dir::Vertical, Rect::new(40, 0, 40, 20), 10),
        ]
    }

    #[test]
    fn a_press_on_either_border_cell_finds_the_line() {
        let hs = two_lines();
        assert_eq!(handle_at(&hs, 39, 5), Some(&hs[0]));
        assert_eq!(handle_at(&hs, 40, 5), Some(&hs[0]), "the outer line wins");
        assert_eq!(handle_at(&hs, 41, 5), None);
        assert_eq!(handle_at(&hs, 60, 9), Some(&hs[1]));
        assert_eq!(handle_at(&hs, 60, 10), Some(&hs[1]));
        assert_eq!(handle_at(&hs, 60, 11), None);
        assert_eq!(
            handle_at(&hs, 20, 10),
            None,
            "the stacked line is right of the root's"
        );
    }

    #[test]
    fn a_drag_sets_the_share_within_the_range() {
        let hs = two_lines();
        assert_eq!(ratio_at(&hs[0], 60, 5), 75);
        assert_eq!(ratio_at(&hs[0], 1, 5), 10, "clamped to min");
        assert_eq!(ratio_at(&hs[0], 300, 5), 90, "clamped to max");
        assert_eq!(ratio_at(&hs[1], 50, 15), 75);
    }
}
