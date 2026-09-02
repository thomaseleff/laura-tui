//! A file's review state: an opened file rendered beside the PTY, its cursor, comments, and live reload.

use std::time::SystemTime;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::render::{RULE_SENTINEL, Rendered, render};

/// An opened file rendered beside the PTY. State logic lives here, off the render loop, so tests can build one directly.
pub struct Panel {
    pub path: String,
    /// Plain-text projection reviews and line counting work off, so `L<n>` quotes clean lines even when the display is styled.
    pub content: String,
    /// Styled display spans, 1:1 with `content.lines()` so a gutter number equals the review's `L<n>`.
    styled: Vec<Line<'static>>,
    /// Display-only left indent per line (heading depth); never touches `content`, so reviews stay flush-left.
    indent: Vec<usize>,
    /// Selected line, 0-based; where a new comment pins.
    pub cursor: usize,
    /// `(line, text)` comments; multiple allowed, even several per line.
    pub comments: Vec<(usize, String)>,
    /// Autoscroll: pin the cursor to the last line on every reload (tail/`--follow`).
    pub follow: bool,
    /// Last-seen source signature (mtime, byte len); drives `reload_if_changed`.
    sig: Option<(SystemTime, u64)>,
    /// Terse read error from the last render, or `None` if the file read cleanly. Drives the open-time stderr warning.
    pub read_error: Option<String>,
}

impl Panel {
    /// Read `path` into a panel; a read error becomes visible text, never a panic (the PTY input path must not crash).
    pub fn open(path: String) -> Panel {
        let Rendered {
            content,
            styled,
            indent,
            error,
        } = render(&path);
        let sig = stat_sig(&path);
        Panel {
            path,
            content,
            styled,
            indent,
            cursor: 0,
            comments: vec![],
            follow: false,
            sig,
            read_error: error,
        }
    }

    /// Enable/disable autoscroll; enabling snaps the cursor to the last line now.
    pub fn set_follow(&mut self, on: bool) {
        self.follow = on;
        if on {
            self.cursor = self.line_count() - 1;
        }
    }

    /// Line count, floored at 1 so the cursor always has a valid slot.
    pub fn line_count(&self) -> usize {
        self.content.lines().count().max(1)
    }

    /// Move the line cursor by `delta`, clamped to `[0, line_count-1]`.
    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.line_count() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
    }

    /// Pin a comment to the current cursor line.
    pub fn add_comment(&mut self, text: String) {
        self.comments.push((self.cursor, text));
    }

    /// Assemble a PR-style review for PTY injection: comments grouped under one 1-based `L<n>` header per line, `overall` omitted when empty.
    pub fn assemble_review(&self, overall: &str) -> String {
        let mut out = format!("[laura review · {}]\n", self.path);
        if !overall.is_empty() {
            out.push_str(&format!("\n{overall}\n"));
        }
        // Group by line, sorted; several comments share one L<n> header.
        let mut by_line: Vec<(usize, Vec<&str>)> = vec![];
        let mut sorted: Vec<&(usize, String)> = self.comments.iter().collect();
        sorted.sort_by_key(|(l, _)| *l);
        for (line, text) in sorted {
            match by_line.last_mut() {
                Some((l, cs)) if *l == *line => cs.push(text),
                _ => by_line.push((*line, vec![text])),
            }
        }
        for (line, comments) in by_line {
            out.push('\n');
            match self.content.lines().nth(line) {
                Some(text) => out.push_str(&format!("L{}  {text}\n", line + 1)),
                None => out.push_str(&format!("L{}\n", line + 1)),
            }
            for c in comments {
                out.push_str(&format!("      > {c}\n"));
            }
        }
        out
    }

    /// Lay the panel into visual rows for an `inner_w`-wide area. Wrapping here (not the widget) keeps rows 1:1 so scroll, scrollbar, and `L<n>` stay exact.
    pub fn layout(&self, inner_w: usize) -> PanelLayout {
        let total = self.styled.len().max(1);
        let gutter_width = total.to_string().len();
        let tw = inner_w.saturating_sub(gutter_width + 1).max(1);
        let mut rows = vec![];
        let mut starts = vec![];
        for (i, line) in self.styled.iter().enumerate() {
            starts.push(rows.len());
            // Indent is display-only; it eats text width but never `content`.
            let ind = self.indent.get(i).copied().unwrap_or(0).min(tw - 1);
            let pad = |spans: &mut Vec<Span<'static>>| {
                if ind > 0 {
                    spans.insert(0, Span::raw(" ".repeat(ind)));
                }
            };
            let avail = tw - ind;
            // A thematic-break sentinel stretches full-width; everything else word-wraps.
            let chunks = if line.spans.len() == 1 && line.spans[0].content.as_ref() == RULE_SENTINEL
            {
                vec![vec![Span::styled("─".repeat(avail), line.spans[0].style)]]
            } else {
                wrap_spans(&line.spans, avail)
            };
            for (k, mut chunk) in chunks.into_iter().enumerate() {
                pad(&mut chunk);
                rows.push(PanelRow {
                    line: i,
                    gutter: (k == 0).then_some(i + 1),
                    spans: chunk,
                    comment: false,
                });
            }
            for (_, c) in self.comments.iter().filter(|(l, _)| *l == i) {
                for chunk in wrap_line(&format!("<- {c}"), avail) {
                    let mut spans = vec![Span::raw(chunk)];
                    pad(&mut spans);
                    rows.push(PanelRow {
                        line: i,
                        gutter: None,
                        spans,
                        comment: true,
                    });
                }
            }
        }
        PanelLayout {
            rows,
            starts,
            gutter_width,
        }
    }

    /// Rows to scroll off the top so the cursor line's *last* wrapped row stays on-screen (its
    /// continuations don't clip). Shared by render and copy so both read the same viewport.
    pub fn scroll_offset(&self, layout: &PanelLayout, view_h: usize) -> usize {
        let cursor_end = layout
            .starts
            .get(self.cursor + 1)
            .map(|n| n - 1)
            .unwrap_or(layout.rows.len().saturating_sub(1));
        cursor_end.saturating_sub(view_h.saturating_sub(1))
    }

    /// Re-read `content` when the source's `(mtime, len)` changed; returns whether it reloaded. Errors surface as text but still update `sig`, so a missing file doesn't respin.
    ///
    /// ponytail: same-length edit within one mtime tick is missed — content hash or `notify` if it bites.
    /// ponytail: a comment past a shrunk file's EOF won't render; re-anchor on reload if it confuses.
    pub fn reload_if_changed(&mut self) -> bool {
        let sig = stat_sig(&self.path);
        if sig == self.sig {
            return false;
        }
        let Rendered {
            content,
            styled,
            indent,
            error,
        } = render(&self.path);
        self.content = content;
        self.styled = styled;
        self.indent = indent;
        self.sig = sig;
        self.read_error = error;
        self.cursor = if self.follow {
            self.line_count() - 1 // autoscroll: newest line stays selected, so it renders at the bottom
        } else {
            self.cursor.min(self.line_count() - 1)
        };
        true
    }
}

/// One visual row of a laid-out panel. `gutter` is `Some(n)` on a line's first row, `None` on continuations/comments; `comment` marks a dimmed `<- …` row.
pub struct PanelRow {
    pub line: usize,
    pub gutter: Option<usize>,
    /// Styled, width-fit spans for this visual row (leading indent included).
    pub spans: Vec<Span<'static>>,
    pub comment: bool,
}

impl PanelRow {
    /// The row's plain text, styling and indent flattened.
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.content.as_ref()).collect()
    }
}

/// A panel's rows, each source line's start row (scroll), and the gutter digit width.
pub struct PanelLayout {
    pub rows: Vec<PanelRow>,
    pub starts: Vec<usize>,
    pub gutter_width: usize,
}

/// Greedy word-wrap `text` to `width` columns, hard-splitting over-long words. Always returns at least one row.
///
/// ponytail: counts `char`s, not display width — `unicode-width` if CJK glyphs bite.
pub fn wrap_line(text: &str, width: usize) -> Vec<String> {
    // Preserve leading indentation: split it off, wrap the rest, re-attach to row 0.
    let lead = text.len() - text.trim_start_matches(' ').len();
    let (indent, body) = text.split_at(lead);
    let mut rows = vec![];
    let mut cur = String::new();
    for mut word in body.split(' ') {
        while word.chars().count() > width {
            if !cur.is_empty() {
                rows.push(std::mem::take(&mut cur));
            }
            let cut: String = word.chars().take(width).collect();
            word = &word[cut.len()..];
            rows.push(cut);
        }
        let sep = usize::from(!cur.is_empty());
        if !cur.is_empty() && cur.chars().count() + sep + word.chars().count() > width {
            rows.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    rows.push(cur);
    // ponytail: deep indent + narrow panel can push row 0 past `width`; panel clips it.
    if !indent.is_empty() {
        rows[0].insert_str(0, indent);
    }
    rows
}

/// Style-preserving twin of `wrap_line`: word-wrap `spans` to `width`, re-coalescing equal-style runs. Same row invariant.
pub fn wrap_spans(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    // Flatten to (char, style); wrap on chars exactly as `wrap_line` does.
    let chars: Vec<(char, Style)> = spans
        .iter()
        .flat_map(|s| s.content.chars().map(|c| (c, s.style)))
        .collect();

    // Preserve leading indentation (see `wrap_line`).
    let lead = chars.iter().take_while(|(c, _)| *c == ' ').count();
    let (indent, body) = chars.split_at(lead);

    // Split on ' ' (dropping the spaces) into style-carrying words.
    let mut words: Vec<Vec<(char, Style)>> = vec![vec![]];
    for &(c, st) in body {
        if c == ' ' {
            words.push(vec![]);
        } else {
            words
                .last_mut()
                .expect("words is seeded with one bucket")
                .push((c, st));
        }
    }

    let mut rows: Vec<Vec<(char, Style)>> = vec![];
    let mut cur: Vec<(char, Style)> = vec![];
    for mut word in words {
        while word.len() > width {
            if !cur.is_empty() {
                rows.push(std::mem::take(&mut cur));
            }
            let rest = word.split_off(width);
            rows.push(std::mem::take(&mut word));
            word = rest;
        }
        let sep = usize::from(!cur.is_empty());
        if !cur.is_empty() && cur.len() + sep + word.len() > width {
            rows.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push((' ', Style::default()));
        }
        cur.extend(word);
    }
    rows.push(cur);
    // ponytail: deep indent + narrow panel can push row 0 past `width`; panel clips it.
    if !indent.is_empty() {
        let first = rows
            .first_mut()
            .expect("rows always has at least one entry");
        first.splice(0..0, indent.iter().copied());
    }

    rows.into_iter().map(coalesce_spans).collect()
}

/// Merge adjacent equal-style chars back into `Span`s.
fn coalesce_spans(row: Vec<(char, Style)>) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = vec![];
    for (c, st) in row {
        match spans.last_mut() {
            Some(s) if s.style == st => s.content.to_mut().push(c),
            _ => spans.push(Span::styled(c.to_string(), st)),
        }
    }
    spans
}

/// Wrap `text` in bracketed paste (`ESC[200~ … ESC[201~`) for PTY injection: newlines stay inside the
/// markers so a line-reading REPL doesn't submit early. `submit` appends one trailing `\r` (outside the
/// markers) to submit once — review injection wants it, keyboard paste doesn't.
///
/// ponytail: paste-honoring only holds against a real REPL; a bare shell ignores the markers.
pub fn bracketed_paste(text: &str, submit: bool) -> Vec<u8> {
    format!("\x1b[200~{text}\x1b[201~{}", if submit { "\r" } else { "" }).into_bytes()
}

/// A file's `(mtime, len)` change signature, or `None` if it can't be stat'd.
fn stat_sig(path: &str) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}
