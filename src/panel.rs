//! A file's review state: an opened file rendered beside the PTY, its cursor, comments, and live reload.

use std::time::SystemTime;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::gitdiff::{self, ChangeKind, DiffOutcome};
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
    /// Per-line: pre-formatted lines clip + h-scroll instead of wrapping. 1:1 with `styled`.
    nowrap: Vec<bool>,
    /// Horizontal scroll offset (chars) applied to nowrap lines; clamped in `scroll_h`.
    pub h_offset: usize,
    /// Selected line, 0-based; where a new comment pins.
    pub cursor: usize,
    /// Agent-directed highlight: 0-based inclusive line range to reverse-video and
    /// anchor the viewport on; `None` = no highlight.
    pub highlight: Option<(usize, usize)>,
    /// `(line, text)` comments; multiple allowed, even several per line.
    pub comments: Vec<(usize, String)>,
    /// Autoscroll: pin the cursor to the last line on every reload (tail/`--follow`).
    pub follow: bool,
    /// Last-seen source signature (mtime, byte len); drives `reload_if_changed`.
    sig: Option<(SystemTime, u64)>,
    /// Terse read error from the last render, or `None` if the file read cleanly. Drives the open-time stderr warning.
    pub read_error: Option<String>,
    /// Per-source-line git-diff marker vs HEAD, 1:1 with `content.lines()`.
    /// Empty when off (markdown, non-repo, clean). Recomputed on open + reload.
    pub changes: Vec<Option<ChangeKind>>,
    /// Latched once when a diff attempt found no `git` binary; drives the open-time
    /// agent warning. (The user-facing toast reads `gitdiff::git_missing`.)
    pub git_missing: bool,
    /// Deleted-line text keyed by the 0-based source line its gap sits *before*.
    /// Populated with `changes` (same git call); consumed only by the diff view.
    removed: Vec<(usize, Vec<String>)>,
    /// #18: render the panel as an inline `+`/`-` diff vs HEAD instead of the file.
    /// Toggled via `set_diff_view`; recomputed data comes from `refresh_diff`.
    pub diff_view: bool,
}

impl Panel {
    /// Read `path` into a panel; a read error becomes visible text, never a panic (the PTY input path must not crash).
    pub fn open(path: String) -> Panel {
        let Rendered {
            content,
            styled,
            indent,
            nowrap,
            error,
        } = render(&path);
        let sig = stat_sig(&path);
        let mut panel = Panel {
            path,
            content,
            styled,
            indent,
            nowrap,
            h_offset: 0,
            cursor: 0,
            highlight: None,
            comments: vec![],
            follow: false,
            sig,
            read_error: error,
            changes: vec![],
            git_missing: false,
            removed: vec![],
            diff_view: false,
        };
        panel.refresh_diff();
        panel
    }

    /// Recompute the per-line git-diff markers vs HEAD. Skipped for markdown (its
    /// `content` is a rendered projection, so source line numbers don't map). A
    /// missing `git` binary latches `git_missing`; any other failure clears markers.
    fn refresh_diff(&mut self) {
        let ext = self.path.rsplit('.').next().map(str::to_ascii_lowercase);
        if matches!(ext.as_deref(), Some("md" | "markdown")) {
            self.changes = vec![];
            self.removed = vec![];
            return;
        }
        match gitdiff::hunks(&self.path) {
            DiffOutcome::Ok(h) => {
                let n = self.line_count();
                self.changes = gitdiff::line_changes(&h, n);
                self.removed = gitdiff::removed_lines(&h, n);
            }
            DiffOutcome::NoGit => {
                self.git_missing = true;
                self.changes = vec![];
                self.removed = vec![];
            }
            DiffOutcome::Unavailable => {
                self.changes = vec![];
                self.removed = vec![];
            }
        }
    }

    /// Enable/disable the inline diff view. Enabling is refused (with a warning to
    /// surface) when there's no diff to show — no `git` binary, or the file is clean
    /// / untracked — because a diff view with no diff is a lie. Returns the warning.
    pub fn set_diff_view(&mut self, on: bool) -> Result<(), String> {
        if !on {
            self.diff_view = false;
            return Ok(());
        }
        if self.git_missing {
            return Err("diff view unavailable — install `git`".into());
        }
        if !self.changes.iter().any(Option::is_some) && self.removed.is_empty() {
            return Err("no changes vs HEAD — nothing to diff".into());
        }
        self.diff_view = true;
        Ok(())
    }

    /// Enable/disable autoscroll; enabling snaps the cursor to the last line now.
    pub fn set_follow(&mut self, on: bool) {
        self.follow = on;
        if on {
            self.cursor = self.line_count() - 1;
        }
    }

    /// Highlight lines `start..=end` (1-based) and scroll them into view. Clamps to
    /// the file and orders the pair; a fully out-of-range request pins to the last line.
    pub fn set_highlight(&mut self, start: u32, end: u32) {
        let last = self.line_count() - 1;
        let a = (start.saturating_sub(1) as usize).min(last);
        let b = (end.saturating_sub(1) as usize).min(last);
        let (lo, hi) = (a.min(b), a.max(b));
        self.highlight = Some((lo, hi));
        // Park the cursor at `hi`: `scroll_offset` reads `cursor == hi` as "fresh highlight" and
        // centers the span. A manual Up/Down moves the cursor off `hi` → plain cursor-follow.
        self.cursor = hi;
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

    /// Scroll pre-formatted lines horizontally by `delta` chars, clamped to `[0, max_width-1]`.
    ///
    /// ponytail: avail-independent clamp — a short nowrap line in a wide panel can over-scroll into
    /// its blank tail, but content can never scroll fully off. Make it avail-aware if the tail annoys.
    pub fn scroll_h(&mut self, delta: isize) {
        let max = self.max_nowrap_width().saturating_sub(1) as isize;
        self.h_offset = (self.h_offset as isize + delta).clamp(0, max) as usize;
    }

    /// Widest nowrap line in chars (0 if none), the horizontal-scroll extent.
    fn max_nowrap_width(&self) -> usize {
        self.styled
            .iter()
            .zip(&self.nowrap)
            .filter(|(_, nw)| **nw)
            .map(|(line, _)| line.spans.iter().map(|s| s.content.chars().count()).sum())
            .max()
            .unwrap_or(0)
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
        if self.diff_view {
            return self.diff_layout(inner_w);
        }
        let total = self.styled.len().max(1);
        let gutter_width = total.to_string().len();
        let tw = inner_w.saturating_sub(gutter_width + 1).max(1);
        let mut rows = vec![];
        let mut starts = vec![];
        for (i, line) in self.styled.iter().enumerate() {
            // A deletion has no surviving line to bar, so emit a dim-red gap row
            // *above* line `i`. Pushed before `starts[i]` so the row sits outside
            // line `i`'s selectable span and the `starts` invariant holds.
            if let Some(ChangeKind::Removed(n)) = self.changes.get(i).copied().flatten() {
                let word = if n == 1 { "line" } else { "lines" };
                let mut label = format!("── {n} {word} removed ");
                let dashes = tw.saturating_sub(label.chars().count());
                label.push_str(&"─".repeat(dashes));
                rows.push(PanelRow {
                    line: i,
                    gutter: None,
                    spans: vec![Span::styled(
                        label,
                        Style::default().fg(Color::Rgb(191, 97, 106)), // nord red
                    )],
                    comment: true,
                });
            }
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
            } else if self.nowrap.get(i).copied().unwrap_or(false) {
                // Pre-formatted: clip to the horizontal window instead of wrapping.
                vec![clip_spans(&line.spans, self.h_offset, avail)]
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

    /// #18: lay the panel out as an inline diff vs HEAD — deleted lines as red `-`
    /// rows above green `+` added/modified lines, unchanged lines plain. Keyed to
    /// current line numbers, so `starts`/gutter numbers stay 1:1 with the source and
    /// scroll/cursor math is unchanged. Data comes from `refresh_diff` (no git here).
    fn diff_layout(&self, inner_w: usize) -> PanelLayout {
        let total = self.styled.len().max(1);
        let gutter_width = total.to_string().len();
        let tw = inner_w.saturating_sub(gutter_width + 1).max(1);
        let green = Style::default().fg(Color::Rgb(163, 190, 140)); // nord green
        let red = Style::default().fg(Color::Rgb(191, 97, 106)); // nord red
        let line_count = self.line_count();
        let lines: Vec<&str> = self.content.lines().collect();
        let mut rows = vec![];
        let mut starts = vec![];
        // Emit the red `-` rows queued *before* source line `at` (deletions key here).
        let removed_at = |rows: &mut Vec<PanelRow>, at: usize| {
            for (idx, texts) in self.removed.iter().filter(|(i, _)| *i == at) {
                for t in texts {
                    for chunk in wrap_line(&format!("-{t}"), tw) {
                        rows.push(PanelRow {
                            line: (*idx).min(line_count - 1),
                            gutter: None,
                            spans: vec![Span::styled(chunk, red)],
                            comment: false,
                        });
                    }
                }
            }
        };
        for i in 0..line_count {
            removed_at(&mut rows, i);
            starts.push(rows.len());
            let (prefix, style) = match self.changes.get(i).copied().flatten() {
                Some(ChangeKind::Added | ChangeKind::Modified) => ('+', Some(green)),
                _ => (' ', None), // context, incl. the surviving line below a deletion gap
            };
            let text = lines.get(i).copied().unwrap_or("");
            for (k, chunk) in wrap_line(&format!("{prefix}{text}"), tw)
                .into_iter()
                .enumerate()
            {
                let span = match style {
                    Some(s) => Span::styled(chunk, s),
                    None => Span::raw(chunk),
                };
                rows.push(PanelRow {
                    line: i,
                    gutter: (k == 0).then_some(i + 1),
                    spans: vec![span],
                    comment: false,
                });
            }
        }
        removed_at(&mut rows, line_count); // trailing EOF deletion
        PanelLayout {
            rows,
            starts,
            gutter_width,
        }
    }

    /// Rows to scroll off the top so the cursor line's *last* wrapped row stays on-screen (its
    /// continuations don't clip). Shared by render and copy so both read the same viewport.
    pub fn scroll_offset(&self, layout: &PanelLayout, view_h: usize) -> usize {
        let last_row = layout.rows.len().saturating_sub(1);
        // Fresh agent highlight (cursor still parked at `hi`): center the span, or top-anchor it
        // when it's taller than the pane (margin saturates to 0) so the reader starts at its top.
        if let Some((lo, hi)) = self.highlight
            && self.cursor == hi
        {
            let start = layout.starts.get(lo).copied().unwrap_or(0);
            let end = line_end_row(layout, hi).unwrap_or(last_row);
            let span = end - start + 1;
            let margin = view_h.saturating_sub(span) / 2;
            return start
                .saturating_sub(margin)
                .min(last_row.saturating_sub(view_h.saturating_sub(1)));
        }
        let cursor_end = line_end_row(layout, self.cursor).unwrap_or(last_row);
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
            nowrap,
            error,
        } = render(&self.path);
        self.content = content;
        self.styled = styled;
        self.indent = indent;
        self.nowrap = nowrap;
        self.sig = sig;
        self.read_error = error;
        // Keep the scroll offset unless the new content is now too narrow to reach it.
        self.h_offset = self.h_offset.min(self.max_nowrap_width().saturating_sub(1));
        self.cursor = if self.follow {
            self.line_count() - 1 // autoscroll: newest line stays selected, so it renders at the bottom
        } else {
            self.cursor.min(self.line_count() - 1)
        };
        // Clamp a stored highlight so a shrunk file doesn't strand it past EOF.
        if let Some((lo, hi)) = self.highlight {
            let last = self.line_count() - 1;
            self.highlight = Some((lo.min(last), hi.min(last)));
        }
        self.refresh_diff();
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

/// The last row belonging to source `line`, scanning forward from its `starts` entry.
///
/// A git-diff gap/deletion row is tagged with the line it precedes but sits *before* that
/// line's `starts` entry (deliberately outside its selectable span — see `layout`/`diff_layout`).
/// So `starts[line + 1] - 1` is not reliably `line`'s own last row: it can land on a gap row
/// queued for `line + 1` instead, off by however many rows that gap spans. Scanning forward from
/// `starts[line]` while rows keep matching `line` sidesteps that regardless of gap size.
fn line_end_row(layout: &PanelLayout, line: usize) -> Option<usize> {
    let mut end = *layout.starts.get(line)?;
    while layout.rows.get(end + 1).is_some_and(|r| r.line == line) {
        end += 1;
    }
    Some(end)
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

/// Clip `spans` to the char window `[h_offset, h_offset+width)` as one row. A right-clip (content
/// past the window) replaces the last visible column with a dim `›`. Reuses the flatten + coalesce
/// machinery of `wrap_spans`; no wrapping.
pub fn clip_spans(spans: &[Span<'static>], h_offset: usize, width: usize) -> Vec<Span<'static>> {
    let chars: Vec<(char, Style)> = spans
        .iter()
        .flat_map(|s| s.content.chars().map(|c| (c, s.style)))
        .collect();
    let total = chars.len();
    let mut window: Vec<(char, Style)> = chars.into_iter().skip(h_offset).take(width).collect();
    if total > h_offset + width
        && let Some(last) = window.last_mut()
    {
        *last = ('›', Style::default().fg(Color::Rgb(90, 90, 90)));
    }
    coalesce_spans(window)
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
