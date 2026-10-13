//! Pane titles, file pane rows, and the hint, help, and notice text a front-end draws.

use std::path::Path;

use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};

use crate::editor::Nvim;
use crate::protocol::{PTY_PANE, PaneId};
use crate::render::CODE_BG;
use crate::{ChangeKind, Panel, PanelLayout, Tab};

/// The diff gutter's added and removed colors, shared with the border's `+N -M`.
const ADDED: Color = Color::Rgb(56, 166, 96); // #38a660
const REMOVED: Color = Color::Rgb(179, 89, 107); // #b3596b

/// The cursor row's gray band (nord3) and the slightly-lighter gray code chips read on top of it.
const BAND: Color = Color::Rgb(67, 76, 94);
const CODE_ON_BAND: Color = Color::Rgb(82, 92, 110);

/// Warning yellow (nord13).
pub const WARN: Color = Color::Rgb(235, 203, 139);

/// A highlighted row's gutter number: soft white (nord4), brighter than the dimmed rest, so the
/// gutter itself marks which rows the spotlight is on — regardless of the row's content.
const HL_NUMBER: Color = Color::Rgb(216, 222, 233);

/// A file pane's border title: `#id path`, its lines added and removed vs HEAD, `● unsaved` while
/// an editor pane's Neovim has unsaved edits, then ` · editor`/` · file` on an editor pane and the
/// thread count.
pub fn pane_title(id: PaneId, panel: &Panel, editor: Option<&Nvim>) -> Line<'static> {
    let mut spans = vec![Span::raw(format!("#{id} {}", display_path(&panel.path)))];
    let (added, removed) = panel.diff_stat;
    if added > 0 {
        spans.push(Span::styled(
            format!(" +{added}"),
            Style::default().fg(ADDED),
        ));
    }
    if removed > 0 {
        spans.push(Span::styled(
            format!(" -{removed}"),
            Style::default().fg(REMOVED),
        ));
    }
    if editor.is_some_and(Nvim::modified) {
        spans.push(Span::styled(" ● unsaved", Style::default().fg(WARN)));
    }
    let review = panel.thread_count();
    let mut tail = String::new();
    if editor.is_some_and(|e| e.editor_view) {
        tail += " · editor";
        if review > 0 {
            tail += &format!("  [review: {review}]");
        }
    } else {
        if editor.is_some() {
            tail += " · file";
        }
        if review > 0 {
            let (above, below) = panel.thread_split();
            tail += &format!("  [review: {review} ↑{above} ↓{below}]");
        }
    }
    spans.push(Span::raw(tail));
    Line::from(spans)
}

/// A pane title's path: relative to Laura's cwd, so `[review: N]` fits a half-width pane.
fn display_path(path: &str) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            Path::new(path)
                .strip_prefix(cwd)
                .ok()
                .map(Path::to_path_buf)
        })
        .map_or_else(|| path.to_string(), |p| p.display().to_string())
}

/// Blend an RGB color toward white by `t` (0..1). Non-RGB colors pass through.
fn brighten(c: Color, t: f32) -> Color {
    match c {
        Color::Rgb(r, g, b) => {
            let up = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * t).round() as u8;
            Color::Rgb(up(r), up(g), up(b))
        }
        other => other,
    }
}

/// Brighten a highlighted row's span so it reads above the full-color rest without a band. A span
/// with no fg (default prose) starts from light gray.
fn bright_span(mut s: Span<'static>) -> Span<'static> {
    // Inline-code caps (fg = CODE_BG, no box) must keep matching the chip's dark bg, which brighten
    // leaves alone — lifting their fg would make the caps read lighter than the box they edge.
    if s.style.fg == Some(CODE_BG) {
        return s;
    }
    // HTML-block spans carry DIM (render.rs uses it to detect them) — leaving it on cancels the
    // brighten, so the row looks untouched. Drop it before lifting fg.
    s.style.add_modifier.remove(Modifier::DIM);
    let base = s.style.fg.unwrap_or(Color::Rgb(200, 200, 200));
    s.style.fg = Some(brighten(base, 0.3));
    s
}

/// Color-emoji glyphs ignore SGR DIM (the terminal paints them from the emoji font), so a dimmed
/// callout glyph would still pop. Replace each with two spaces, not nothing, so the text after it
/// keeps its columns. The `Note`/`Warning` word carries the callout.
///
/// ponytail: callout emoji render 2 cells wide (VS16 presentation); the fixed 2-space swap matches
/// that. A rare width-1 emoji would over-pad by one — widen this if a non-callout glyph shows up.
fn strip_emoji(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        if u == 0xFE0F {
            // variation selector — width 0, the base glyph's spaces already cover it
        } else if u == 0x2139 || (0x2600..=0x27BF).contains(&u) || (0x1F000..=0x1FAFF).contains(&u)
        {
            out.push_str("  ");
        } else {
            out.push(c);
        }
    }
    out
}

/// Spotlight dim for a non-highlighted row: strip BOLD (else it beats DIM), drop the code bg box
/// (it ignores DIM), drop color emoji (DIM can't touch them), then add DIM.
fn dim_span(mut s: Span<'static>) -> Span<'static> {
    // Inline-code caps (fg = CODE_BG, no box) would dim to a stray dark block once the chip's bg
    // is stripped, so drop the glyph — the run then reads as plain dimmed prose.
    if s.style.fg == Some(CODE_BG) {
        s.content = "".into();
    }
    s.style.add_modifier.remove(Modifier::BOLD);
    s.style.bg = None;
    s.style = s.style.add_modifier(Modifier::DIM);
    s.content = strip_emoji(&s.content).into();
    s
}

/// A file pane's styled rows over `layout` (pre-wrapped to `width`): gutter, agent-highlight
/// spotlight, and the cursor band when `focused`.
pub fn panel_lines(
    panel: &Panel,
    layout: &PanelLayout,
    focused: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let gw = layout.gutter_width;
    let spotlight = panel.highlight.is_some();
    layout
        .rows
        .iter()
        .map(|r| {
            let number = match r.gutter {
                Some(n) => format!("{n:>gw$}"),
                None => format!("{:>gw$}", ""),
            };
            // 5-cell decoration: number · sep · change · sep · caret · sep · body. The change cell is
            // a `▌` half-block in higher-contrast diff colors; the caret carries thread state.
            let change_cell = r
                .change
                .map(|k| {
                    Span::styled(
                        "▌",
                        Style::default().fg(match k {
                            ChangeKind::Added => ADDED,
                            ChangeKind::Modified => Color::Rgb(56, 150, 217), // #3896d9
                            ChangeKind::Removed(_) => REMOVED,
                        }),
                    )
                })
                .unwrap_or_else(|| Span::raw(" "));
            let caret_cell = match r.review {
                Some(collapsed) => Span::raw(if collapsed { "▸" } else { "▾" }),
                None => Span::raw(" "),
            };
            if r.comment {
                // Card/gap rows emit at column 0; blank number + 3 lands the card border under the caret.
                let mut spans = vec![Span::raw(number), Span::raw(" ".repeat(3))];
                spans.extend(r.spans.iter().cloned());
                Line::from(spans)
            } else {
                let highlighted = panel
                    .highlight
                    .is_some_and(|(lo, hi)| (lo..=hi).contains(&r.line));
                let on_cursor = focused && r.line == panel.cursor;

                let number_span = if highlighted {
                    Span::styled(number, Style::default().fg(HL_NUMBER))
                } else {
                    Span::raw(number).dim()
                };
                let mut spans = vec![
                    number_span,
                    Span::raw(" "),
                    change_cell,
                    Span::raw(" "),
                    caret_cell,
                    Span::raw(" "),
                ];
                let mut body: Vec<Span> = r.spans.to_vec();
                if !on_cursor {
                    if highlighted {
                        body = body.into_iter().map(bright_span).collect();
                    } else if spotlight {
                        body = body.into_iter().map(dim_span).collect();
                    }
                }

                // Pad to the pane edge so a row that shrinks between frames overwrites the old
                // cells, and so the cursor band's bg reaches the edge.
                let used = gw
                    + 5
                    + body
                        .iter()
                        .map(|s| s.content.chars().count())
                        .sum::<usize>();
                let pad = width.saturating_sub(used);
                if on_cursor {
                    // Gray band under the cursor. Repaint code chips + inline caps a shade lighter
                    // so they read as raised chips on the band, not holes.
                    spans.extend(body);
                    for s in spans.iter_mut() {
                        if s.style.bg.is_some() {
                            s.style.bg = Some(CODE_ON_BAND);
                        } else if s.style.fg == Some(CODE_BG) {
                            s.style.fg = Some(CODE_ON_BAND);
                        }
                    }
                    if pad > 0 {
                        spans.push(Span::styled(" ".repeat(pad), Style::default().bg(BAND)));
                    }
                    Line::from(spans).bg(BAND)
                } else {
                    spans.extend(body);
                    if pad > 0 {
                        spans.push(Span::raw(" ".repeat(pad)));
                    }
                    Line::from(spans)
                }
            }
        })
        .collect()
}

/// What the hint line describes.
pub enum Hint {
    Locked,
    Wizard,
    Quit,
    /// A comment draft on source `line`; `edit` when it edits the user's existing comment.
    Comment {
        line: u32,
        edit: bool,
    },
    Review,
    Panes,
    /// No popup or draft: the focused pane's keystrokes.
    Pane,
}

/// The hint line's text for `mode`.
pub fn hint(tab: &Tab, mode: Hint) -> String {
    let pty = tab.pane_pty(tab.focus).is_some();
    let editor_focused = pty && tab.focus != PTY_PANE;
    match mode {
        Hint::Locked if editor_focused => {
            "  🔒 locked — every keystroke goes to Neovim · F12 unlock".into()
        }
        Hint::Locked => "  🔒 locked — every keystroke goes to the shell · F12 unlock".into(),
        Hint::Wizard => "  y yes, use Laura's theme · n no, keep mine · Esc not now".into(),
        Hint::Quit => "  quit? press y to confirm · any other keystroke cancels".into(),
        Hint::Comment { line, edit } => format!(
            "  comment L{line} · \\+Enter newline · {} · Esc cancel",
            if edit {
                "Enter save (empty deletes)"
            } else {
                "Enter add"
            }
        ),
        Hint::Review => "  review body · \\+Enter newline · Enter submit · Esc cancel".into(),
        Hint::Panes => "  type a pane id · Enter pick · Esc dismiss".into(),
        Hint::Pane if !pty => {
            match (tab.agent, tab.editors.contains_key(&tab.focus)) {
                (true, false) => "  ↑/↓ move · n/N next/prev thread · c comment · r threads · Shift+S submit · Ctrl+R refresh · d diff · x close · h clear · Esc leave pane",
                (true, true) => "  ↑/↓ move · n/N next/prev thread · c comment · r threads · Shift+S submit · Ctrl+R refresh · x close · Ctrl+L edit · Esc leave pane",
                (false, false) => "  ↑/↓ move · n/N next/prev thread · r threads · d diff · x close · h clear · Esc leave pane · inline review: run `laura ready`",
                (false, true) => "  ↑/↓ move · n/N next/prev thread · r threads · x close · Ctrl+L edit · Esc leave pane · inline review: run `laura ready`",
            }
            .into()
        }
        Hint::Pane if editor_focused => {
            "  Ctrl+L file view · Ctrl+P panes · Ctrl+H help · Ctrl+Q quit".into()
        }
        Hint::Pane => "  Ctrl+P panes · Ctrl+H help · Ctrl+Q quit".into(),
    }
}

/// The help popup's lines: keystrokes grouped by context, mirroring the hint line.
pub fn help_lines() -> Vec<Line<'static>> {
    let key = |k: &'static str, desc: &'static str| {
        Line::from(vec![
            Span::raw("  "),
            k.bold().cyan(),
            Span::raw("  "),
            desc.dim(),
        ])
    };
    let group = |g: &'static str| Line::from(g.bold());
    vec![
        group("Global"),
        key("Ctrl+P", "panes popup"),
        key("Ctrl+H", "this help"),
        key("F12", "lock all input to the shell or editor pane"),
        key("drag", "select within pane (copies on release)"),
        key("Ctrl+Q", "quit (then y to confirm)"),
        Line::raw(""),
        group("Panes (Ctrl+P …)"),
        key("id", "type a pane id, Enter to focus"),
        key("Esc", "dismiss"),
        Line::raw(""),
        group("Focused pane"),
        key("↑/↓", "move the cursor"),
        key("n/N", "next/prev thread"),
        key("←/→", "scroll pre-formatted lines sideways"),
        key("d", "toggle inline diff vs HEAD"),
        key("c", "comment on line (needs `laura ready`)"),
        key("r", "collapse/expand thread"),
        key("Shift+S", "submit inline review (needs `laura ready`)"),
        key(
            "Ctrl+R",
            "refresh pane (discards unsubmitted inline review)",
        ),
        key("x", "close pane"),
        key("h", "clear highlight"),
        key("Esc", "leave pane"),
        key(
            "Ctrl+L",
            "editor pane: flip between Neovim and its file view",
        ),
        Line::raw(""),
        group("Draft"),
        key("←/→", "move"),
        key("Home/End", "line start/end"),
        key("Backspace/Delete", "delete before/after"),
        key("\\+Enter", "newline"),
        key("Enter", "confirm / submit"),
        key("Enter", "on an empty edit: delete your comment"),
        key("Esc", "cancel"),
    ]
}

/// The notice when a diff found no `git` binary; shown once.
pub const NO_GIT: &str = "diff views not available — install `git`";

/// The web app's overlay on a browser tab another tab took the workspace from.
pub const MOVED: &str = "This workspace moved to another tab.";

/// The web app's overlay once the workspace ended (the shell exited or the user quit).
pub const ENDED: &str = "This workspace ended.";

/// A path's file name (last `/`- or `\`-separated segment).
pub fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The frozen-pane notice for file `name`, shortened when the full text won't fit in `width`.
pub fn frozen_notice(name: &str, width: u16) -> String {
    // Trailing space: ⚠ (U+26A0) renders width-2 under VS16, so pad or the box clips ~1 cell.
    let full = format!(
        "⚠ {name} changed on disk — Shift+S submit · Ctrl+R refresh (discards unsubmitted inline review) "
    );
    // The TUI's notice box pads by 2 and keeps a 1-cell right margin.
    if full.chars().count() + 3 <= width as usize {
        full
    } else {
        // No filename: a long one overflows 80 cols, and the pane title already names the file.
        "⚠ Shift+S submit · Ctrl+R discards inline review ".to_string()
    }
}
