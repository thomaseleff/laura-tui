//! A file's review state: an opened file rendered beside the PTY, its cursor, comments, and live reload.

use std::time::SystemTime;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::gitdiff::{self, ChangeKind, DiffOutcome};
use crate::render::{CODE_BG, RULE_SENTINEL, Rendered, render};

/// An opened file rendered beside the PTY. State logic lives here, off the render loop, so tests can build one directly.
pub struct Panel {
    pub path: String,
    /// Plain-text projection reviews and line counting work off, so `L<n>` quotes clean lines even when the display is styled.
    pub content: String,
    /// Styled display spans, 1:1 with `content.lines()` so a gutter number equals the review's `L<n>`.
    styled: Vec<Line<'static>>,
    /// Per-line: pre-formatted lines clip + h-scroll instead of wrapping. 1:1 with `styled`.
    nowrap: Vec<bool>,
    /// 0-based inclusive SOURCE line range each rendered row came from, 1:1 with `styled`.
    /// Identity `(i, i)` for code/diff/plain; a block's range for a collapsed markdown paragraph.
    /// Gutter/highlight/`L<n>` key off this so they match the file's real line numbers (#32).
    source: Vec<(usize, usize)>,
    /// The raw file split into lines — what diff-view renders as a `+`/`-` patch.
    /// `== content.lines()` for non-markdown; the un-joined source for markdown.
    source_lines: Vec<String>,
    /// Horizontal scroll offset (chars) applied to nowrap lines; clamped in `scroll_h`.
    pub h_offset: usize,
    /// Selected line, 0-based; where a new comment pins.
    pub cursor: usize,
    /// Agent-directed highlight: 0-based inclusive line range to spotlight (its rows stay
    /// full-color, the rest of the pane dims) and anchor the viewport on; `None` = no highlight.
    pub highlight: Option<(usize, usize)>,
    /// Review threads, keyed by rendered-line index (1:1 with `styled`); cleared on submit (#45).
    pub threads: Vec<Thread>,
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
    /// The source changed on disk while notes were open, so the pane holds its in-memory snapshot
    /// (a reload would move the lines the notes pin to). Cleared once the notes clear (#45).
    pub source_changed: bool,
}

/// Who wrote a note. `Agent` carries its label; the human is always `User`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Author {
    User,
    Agent(String),
}

/// One note in a thread — an author and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub author: Author,
    pub body: String,
}

impl Note {
    fn user(body: String) -> Note {
        Note {
            author: Author::User,
            body,
        }
    }
}

/// A thread pinned to a rendered-line index; `assemble_review` maps it through `source` to file lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub line: usize,
    /// Card show/hide and gutter-caret shape. Starts expanded.
    pub collapsed: bool,
    pub root: Note,
    /// Newest last, one level deep.
    pub replies: Vec<Note>,
}

impl Thread {
    fn new(line: usize, root: Note) -> Thread {
        Thread {
            line,
            collapsed: false,
            root,
            replies: vec![],
        }
    }

    /// Newest note: the last reply, else the root.
    fn last(&self) -> &Note {
        self.replies.last().unwrap_or(&self.root)
    }

    fn last_mut(&mut self) -> &mut Note {
        self.replies.last_mut().unwrap_or(&mut self.root)
    }

    /// Root then replies, in order.
    fn notes(&self) -> impl Iterator<Item = &Note> {
        std::iter::once(&self.root).chain(&self.replies)
    }
}

/// One decision shared by the `c` keypress prefill and its commit, so both agree.
enum UserIntent {
    New,
    Edit,
    Reply,
}

impl Panel {
    /// Read `path` into a panel; a read error becomes visible text, never a panic (the PTY input path must not crash).
    pub fn open(path: String) -> Panel {
        let Rendered {
            content,
            styled,
            nowrap,
            source,
            source_lines,
            error,
        } = render(&path);
        let sig = stat_sig(&path);
        let mut panel = Panel {
            path,
            content,
            styled,
            nowrap,
            source,
            source_lines,
            h_offset: 0,
            cursor: 0,
            highlight: None,
            threads: vec![],
            follow: false,
            sig,
            read_error: error,
            changes: vec![],
            git_missing: false,
            removed: vec![],
            diff_view: false,
            source_changed: false,
        };
        panel.refresh_diff();
        panel
    }

    /// Recompute per-source-line git-diff markers vs HEAD, in source-line space (what `line_changes`
    /// gives); `layout` maps them through `source` to rendered rows (#32). Missing `git` latches
    /// `git_missing`; any other failure clears markers.
    fn refresh_diff(&mut self) {
        match gitdiff::hunks(&self.path) {
            DiffOutcome::Ok(h) => {
                let n = self.source_lines.len();
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

    /// Highlight the rows covering source lines `start..=end` (1-based) and scroll them into view.
    /// Lights *every* rendered row whose source range intersects the request, so a collapsed block
    /// (table/list/alert — many rows sharing one range) lights whole, not just its first row.
    /// A fully out-of-range request pins to the last row.
    pub fn set_highlight(&mut self, start: u32, end: u32) {
        let s0 = start.min(end).saturating_sub(1) as usize;
        let s1 = start.max(end).saturating_sub(1) as usize;
        // First and last rows intersecting [s0, s1]. `position`/`rposition` (not a single lookup per
        // bound) is what keeps a multi-row block whole: both its bounds would otherwise collapse to
        // the block's first row, since every row of it carries the same source range.
        let hit = |&(a, b): &(usize, usize)| a <= s1 && b >= s0;
        let (lo, hi) = match (
            self.source.iter().position(hit),
            self.source.iter().rposition(hit),
        ) {
            (Some(lo), Some(hi)) => (lo, hi),
            _ => {
                let last = self.source.len().saturating_sub(1);
                (last, last)
            }
        };
        self.highlight = Some((lo, hi));
        // Park the cursor at `hi`: `scroll_offset` reads `cursor == hi` as "fresh highlight" and
        // centers the span. A manual Up/Down moves the cursor off `hi` → plain cursor-follow.
        self.cursor = hi;
    }

    /// Clear the agent-directed highlight; the pane returns to normal (undimmed) rendering.
    /// No viewport change — cursor/scroll stay put.
    pub fn clear_highlight(&mut self) {
        self.highlight = None;
    }

    /// Row `i`'s gutter change: the highest-ranked change over its source range (Modified > Added >
    /// Removed, so a mixed markdown block reads as edited). Folds to `changes[i]` for identity files.
    fn row_change(&self, i: usize) -> Option<ChangeKind> {
        let (a, b) = *self.source.get(i)?;
        (a..=b)
            .filter_map(|s| self.changes.get(s).copied().flatten())
            .max_by_key(|k| match k {
                ChangeKind::Modified => 2,
                ChangeKind::Added => 1,
                ChangeKind::Removed(_) => 0,
            })
    }

    /// Deleted-line count surfacing above row `i`: deletions in its source range, counted on the
    /// range's first row only so a collapsed block shows one gap.
    fn removed_in_row(&self, i: usize) -> Option<usize> {
        let (a, b) = *self.source.get(i)?;
        if i > 0 && self.source.get(i - 1) == Some(&(a, b)) {
            return None; // a continuation row of the same block — gap already emitted
        }
        let n: usize = (a..=b)
            .filter_map(|s| match self.changes.get(s).copied().flatten() {
                Some(ChangeKind::Removed(k)) => Some(k),
                _ => None,
            })
            .sum();
        (n > 0).then_some(n)
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

    /// The thread on rendered line `line`, if any.
    pub fn thread_at(&self, line: usize) -> Option<&Thread> {
        self.threads.iter().find(|t| t.line == line)
    }

    fn thread_at_mut(&mut self, line: usize) -> Option<&mut Thread> {
        self.threads.iter_mut().find(|t| t.line == line)
    }

    /// How many threads — gates the title and `S`, and the gutter shows them all.
    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }

    /// Toggle collapse of the thread on `line`'s canonical row (no-op if none).
    pub fn toggle_collapsed(&mut self, line: usize) {
        let row = self.canonical_row(line);
        if let Some(t) = self.thread_at_mut(row) {
            t.collapsed = !t.collapsed;
        }
    }

    /// Set every thread's collapse to `v` (bulk `r`).
    pub fn set_all_collapsed(&mut self, v: bool) {
        for t in &mut self.threads {
            t.collapsed = v;
        }
    }

    /// What `c` does on the cursor line: new thread, edit your own newest note, or reply.
    fn user_intent(&self) -> UserIntent {
        match self.thread_at(self.canonical_row(self.cursor)) {
            None => UserIntent::New,
            Some(t) if matches!(t.last().author, Author::User) => UserIntent::Edit,
            Some(_) => UserIntent::Reply,
        }
    }

    /// `c` seam: expand the cursor's thread and return the edit-prefill seed (edited note, else empty).
    pub fn begin_comment(&mut self) -> String {
        let row = self.canonical_row(self.cursor);
        if let Some(t) = self.thread_at_mut(row) {
            t.collapsed = false;
        }
        match self.user_intent() {
            UserIntent::Edit => self
                .thread_at(row)
                .map(|t| t.last().body.clone())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Commit the human's draft on Enter (contextual new / edit / reply).
    pub fn author_note(&mut self, text: String) {
        if text.trim().is_empty() {
            return; // no empty threads/replies; empty edit is a no-op, keeps the existing body. Esc cancels.
        }
        let row = self.canonical_row(self.cursor);
        match self.user_intent() {
            UserIntent::New => self.threads.push(Thread::new(row, Note::user(text))),
            UserIntent::Edit => {
                self.thread_at_mut(row).unwrap().last_mut().body = text;
            }
            UserIntent::Reply => {
                self.thread_at_mut(row)
                    .unwrap()
                    .replies
                    .push(Note::user(text));
            }
        }
    }

    /// The agent's write path. `line` is a 1-based file line; `body` starts a thread or appends a
    /// reply. `Err` on an empty body, and `Err` past EOF so a stale line can't clamp onto another thread.
    pub fn agent_note(
        &mut self,
        line: u32,
        body: Option<String>,
        author: Option<String>,
    ) -> Result<(), String> {
        let body = body
            .filter(|b| !b.trim().is_empty())
            .ok_or("comment needs a body")?;
        let n = self.source_lines.len();
        if (line.saturating_sub(1) as usize) >= n {
            return Err(format!("line {line} is past the file's {n} lines"));
        }
        let row = self.source_line_to_row(line);
        let name = author.unwrap_or_else(|| "agent".into());
        let note = Note {
            author: Author::Agent(name),
            body,
        };
        match self.thread_at_mut(row) {
            Some(t) => t.replies.push(note),
            None => self.threads.push(Thread::new(row, note)),
        }
        Ok(())
    }

    /// First rendered row whose source range holds 1-based file `line`; clamps to the last row when
    /// out of range. The single-point form of `set_highlight`'s intersect (agent may send `line: 0`).
    fn source_line_to_row(&self, line: u32) -> usize {
        let s = line.saturating_sub(1) as usize;
        self.source
            .iter()
            .position(|&(a, b)| a <= s && b >= s)
            .unwrap_or_else(|| self.source.len().saturating_sub(1))
    }

    /// A rendered `row` mapped to its block's first row — where both write paths key threads, so a
    /// comment on any row of a folded block hits one shared thread (#45).
    fn canonical_row(&self, row: usize) -> usize {
        match self.source.get(row) {
            Some(&(a, _)) => self.source_line_to_row(a as u32 + 1),
            None => row,
        }
    }

    /// The thread on the cursor's canonical row, if any (what `r`/`c` key off).
    pub fn cursor_thread(&self) -> Option<&Thread> {
        self.thread_at(self.canonical_row(self.cursor))
    }

    /// Threads at-or-above vs strictly below the cursor's canonical row — the title's ↑/↓ counter,
    /// so `a + b` equals the thread count (#45).
    pub fn thread_split(&self) -> (usize, usize) {
        let row = self.canonical_row(self.cursor);
        let below = self.threads.iter().filter(|t| t.line > row).count();
        (self.threads.len() - below, below)
    }

    /// Rendered row of the nearest thread after (`fwd`) or before the cursor — the `n`/`N` target,
    /// wrapping around the ends. None only with no threads.
    pub fn thread_jump(&self, fwd: bool) -> Option<usize> {
        let row = self.canonical_row(self.cursor);
        let lines = self.threads.iter().map(|t| t.line);
        if fwd {
            lines
                .clone()
                .filter(|&l| l > row)
                .min()
                .or_else(|| lines.min())
        } else {
            lines
                .clone()
                .filter(|&l| l < row)
                .max()
                .or_else(|| lines.max())
        }
    }

    /// `Shift+S`: build the review, hand it to `send`, and clear the threads only once it's sent —
    /// a failed send keeps them so the user can retry (#72).
    pub fn submit_review(
        &mut self,
        overall: &str,
        send: impl FnOnce(&str) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        send(&self.assemble_review(overall))?;
        self.threads.clear();
        Ok(())
    }

    /// PR-style review for PTY injection: each thread under a 1-based source header (`L<n>` or
    /// `L<a>-<b>` for a block), author-labeled; `overall` omitted when empty (#45).
    pub fn assemble_review(&self, overall: &str) -> String {
        let mut out = format!("[laura review · {}]\n", self.path);
        // A frozen snapshot's line numbers are against the reviewed content, not disk — warn the agent.
        if self.source_changed {
            out.push_str(
                "⚠ file changed on disk since this inline review — line numbers are against the snapshot\n",
            );
        }
        if !overall.is_empty() {
            out.push_str(&format!("\n{overall}\n"));
        }
        let mut threads: Vec<&Thread> = self.threads.iter().collect();
        threads.sort_by_key(|t| t.line);
        for t in threads {
            out.push('\n');
            // Header is the row's source range: `L<n>` for a 1:1 line, `L<a>-<b>` for a block (#32).
            let (a, b) = self.source.get(t.line).copied().unwrap_or((t.line, t.line));
            let hdr = if a == b {
                format!("L{}", a + 1)
            } else {
                format!("L{}-{}", a + 1, b + 1)
            };
            // Raw source line, not the rendered row: a table/list thread carries `| c | 3 |`, not box glyphs.
            match self.source_lines.get(a) {
                Some(text) => out.push_str(&format!("{hdr}  {text}\n")),
                None => out.push_str(&format!("{hdr}\n")),
            }
            for note in t.notes() {
                let label = match &note.author {
                    Author::User => "user",
                    Author::Agent(n) => n.as_str(),
                };
                out.push_str(&format!("      > [{label}] {}\n", note.body));
            }
        }
        out
    }

    /// Card rows for the expanded thread on line `i` (empty when none/collapsed): a rounded gray box
    /// grown to the widest note; human notes are inline-code chips, agent notes plain fg.
    /// Emitted at column 0 — `render_panel` prefixes the decoration, so `pad_w` is only a wrap budget.
    fn thread_rows(&self, i: usize, inner_w: usize) -> Vec<PanelRow> {
        let Some(t) = self.thread_at(i).filter(|t| !t.collapsed) else {
            return vec![];
        };
        let gutter_width = self.source_lines.len().max(1).to_string().len();
        let pad_w = gutter_width + 3; // number + sep + change + sep — card's left edge sits under the caret
        let cap = inner_w.saturating_sub(pad_w + 4).max(12); // two border + two padding columns
        let border = Style::default().fg(Color::Rgb(120, 120, 120)); // rounded box gray
        let plain = Style::default().fg(Color::Rgb(171, 178, 191)); // one dark fg
        let chip = Style::default().fg(Color::Rgb(180, 180, 180)).bg(CODE_BG);

        let width = |spans: &[Span<'static>]| -> usize {
            spans.iter().map(|s| s.content.chars().count()).sum()
        };

        // Inner note lines: wrap the whole `{name} · body` as one unit; continuations align under the
        // note's start (connector width only), not under the body.
        let mut inner: Vec<Vec<Span<'static>>> = vec![];
        for (idx, note) in t.notes().enumerate() {
            let is_user = matches!(note.author, Author::User);
            let name = match &note.author {
                Author::User => "You",
                // Title-case the unnamed-agent fallback so it reads as a role beside "You";
                // real supplied names render verbatim. The chat payload keeps lowercase "agent".
                Author::Agent(n) if n == "agent" => "Agent",
                Author::Agent(n) => n.as_str(),
            };
            let connector = if idx == 0 { "" } else { "└─ " };
            let cw = connector.chars().count();
            let chip_pad = if is_user { 2 } else { 0 }; // the chip pads one space each side
            let full = format!("{name} · {}", note.body);
            for (j, seg) in wrap_line(&full, cap.saturating_sub(cw + chip_pad))
                .into_iter()
                .enumerate()
            {
                let mut spans = vec![];
                let lead = if j == 0 {
                    connector.to_string()
                } else {
                    " ".repeat(cw)
                };
                if !lead.is_empty() {
                    spans.push(Span::styled(lead, border));
                }
                spans.push(if is_user {
                    Span::styled(format!(" {seg} "), chip) // whole note as an inline-code chip
                } else {
                    Span::styled(seg, plain)
                });
                inner.push(spans);
            }
        }

        // Draw the rounded box, growing to the widest inner line. `line: i` so scroll/copy keep the
        // card with its source line; `comment: true` so cosmetics skip it and render_panel indents it.
        let w = inner.iter().map(|s| width(s)).max().unwrap_or(0);
        let mk = |spans: Vec<Span<'static>>| PanelRow {
            line: i,
            gutter: None,
            spans,
            comment: true,
            change: None,
            review: None,
            nowrap: false,
        };
        let mut rows = vec![mk(vec![Span::styled(
            format!("╭{}╮", "─".repeat(w + 2)),
            border,
        )])];
        for line in inner {
            let used = width(&line);
            let mut spans = vec![Span::styled("│ ".to_string(), border)];
            spans.extend(line);
            spans.push(Span::styled(format!("{} │", " ".repeat(w - used)), border));
            rows.push(mk(spans));
        }
        rows.push(mk(vec![Span::styled(
            format!("╰{}╯", "─".repeat(w + 2)),
            border,
        )]));
        rows
    }

    /// Lay the panel into visual rows for an `inner_w`-wide area. Wrapping here (not the widget) keeps rows 1:1 so scroll, scrollbar, and `L<n>` stay exact.
    pub fn layout(&self, inner_w: usize) -> PanelLayout {
        if self.diff_view {
            return self.diff_layout(inner_w);
        }
        // Gutter is sized off the largest *source* line number: a collapsed block makes the top
        // number exceed `styled.len()`.
        let total = self.source_lines.len().max(1);
        let gutter_width = total.to_string().len();
        // 5-cell gutter decoration (number · sep · change · sep · caret · sep · body); see render_panel.
        let tw = inner_w.saturating_sub(gutter_width + 5).max(1);
        let mut rows = vec![];
        let mut starts = vec![];
        for (i, line) in self.styled.iter().enumerate() {
            // A deletion has no surviving line to bar, so emit a dim-red gap row
            // *above* line `i`. Pushed before `starts[i]` so the row sits outside
            // line `i`'s selectable span and the `starts` invariant holds.
            if let Some(n) = self.removed_in_row(i) {
                let word = if n == 1 { "line" } else { "lines" };
                let mut label = format!("── {n} {word} removed ");
                // +2 so the separator still reaches the pane edge from the caret-aligned indent (+3, not +5).
                let dashes = (tw + 2).saturating_sub(label.chars().count());
                label.push_str(&"─".repeat(dashes));
                rows.push(PanelRow {
                    line: i,
                    gutter: None,
                    spans: vec![Span::styled(
                        label,
                        Style::default().fg(Color::Rgb(191, 97, 106)), // nord red
                    )],
                    comment: true,
                    change: None,
                    review: None,
                    nowrap: false,
                });
            }
            starts.push(rows.len());
            let nowrap = self.nowrap.get(i).copied().unwrap_or(false);
            // A thematic-break sentinel stretches full-width; everything else word-wraps.
            let chunks = if line.spans.len() == 1 && line.spans[0].content.as_ref() == RULE_SENTINEL
            {
                vec![vec![Span::styled("─".repeat(tw), line.spans[0].style)]]
            } else if nowrap {
                // Pre-formatted: clip to the horizontal window instead of wrapping.
                vec![clip_spans(&line.spans, self.h_offset, tw)]
            } else {
                wrap_spans(&line.spans, tw)
            };
            for (k, chunk) in chunks.into_iter().enumerate() {
                rows.push(PanelRow {
                    line: i,
                    // Gutter shows the block's *first* source line, not the rendered index.
                    gutter: (k == 0).then_some(self.source[i].0 + 1),
                    spans: chunk,
                    comment: false,
                    nowrap,
                    // Bar runs every wrapped row of the changed paragraph; row_change folds
                    // the block's source range, identical for each row of line i (#35).
                    change: self.row_change(i),
                    // Caret only on the line's first row; carries collapse shape.
                    review: (k == 0)
                        .then(|| self.thread_at(i))
                        .flatten()
                        .map(|t| t.collapsed),
                });
            }
            // Card rows for an expanded thread land after the line's last (wrapped) row.
            rows.extend(self.thread_rows(i, inner_w));
        }
        // Source-independent cosmetics, applied once to the finished rows (both testable through
        // `layout`): box fenced code into a rectangle, then half-cap inline code. Neither adds or
        // removes rows, so `starts` stays valid.
        box_fenced(&mut rows);
        cap_inline(&mut rows);
        PanelLayout {
            rows,
            starts,
            gutter_width,
        }
    }

    /// #18: lay the panel out as an inline diff vs HEAD — deleted lines as red `-` rows above green
    /// `+` added/modified lines, unchanged plain. Iterates the raw `source_lines`, so `changes`/
    /// `removed` index straight in and markdown shows a readable patch, not an overlay on prose (#32).
    ///
    /// ponytail: markdown's cursor is rendered-row space while these rows key to source lines, so the
    /// diff-view cursor anchor is approximate — a read mode, block-close.
    fn diff_layout(&self, inner_w: usize) -> PanelLayout {
        let total = self.source_lines.len().max(1);
        let gutter_width = total.to_string().len();
        // Match `layout`'s 5-cell decoration budget; diff view renders those cells as blanks.
        let tw = inner_w.saturating_sub(gutter_width + 5).max(1);
        let green = Style::default().fg(Color::Rgb(163, 190, 140)); // nord green
        let red = Style::default().fg(Color::Rgb(191, 97, 106)); // nord red
        let line_count = self.source_lines.len().max(1);
        let lines: Vec<&str> = self.source_lines.iter().map(String::as_str).collect();
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
                            change: None,
                            review: None,
                            nowrap: false,
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
                    change: None,
                    review: None,
                    nowrap: false,
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
        if !self.threads.is_empty() {
            self.source_changed = true; // freeze: hold the snapshot the notes pin to, don't touch self.sig
            return false; // stays "dirty" so we keep noticing until the notes clear (submit/Ctrl+R)
        }
        let Rendered {
            content,
            styled,
            nowrap,
            source,
            source_lines,
            error,
        } = render(&self.path);
        self.content = content;
        self.styled = styled;
        self.nowrap = nowrap;
        self.source = source;
        self.source_lines = source_lines;
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
        self.source_changed = false;
        true
    }

    /// `Ctrl+R`: drop pending notes and refresh the pane to the on-disk file.
    pub fn discard_and_reload(&mut self) {
        self.threads.clear();
        self.sig = None;
        self.reload_if_changed();
    }
}

/// One visual row of a laid-out panel. `gutter` is `Some(n)` on a line's first row, `None` on continuations/comments; `comment` marks a dimmed `<- …` row.
pub struct PanelRow {
    pub line: usize,
    pub gutter: Option<usize>,
    /// Styled, width-fit spans for this visual row (leading indent included).
    pub spans: Vec<Span<'static>>,
    pub comment: bool,
    /// Gutter-bar change, folded from the row's source range; `None` on continuation/comment/gap
    /// rows and in diff-view. The renderer keys the bar off this (#32).
    pub change: Option<ChangeKind>,
    /// Review caret on the line's first row: `Some(collapsed)` = a thread here, shape carries
    /// collapse (`▸`/`▾`) (#45).
    pub review: Option<bool>,
    /// Pre-formatted row (table/code/diff): `cap_inline` skips it so caps don't drift alignment (#45).
    pub nowrap: bool,
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

/// A fenced-code row: ≥1 non-blank span, every non-blank one carrying the `code()` bg box. Mirrors
/// `render::classify_nowrap`'s code-block rule — plain code *files* are fg-only, so they don't match.
fn is_fenced_row(spans: &[Span<'static>]) -> bool {
    let mut any = false;
    for s in spans.iter().filter(|s| !s.content.trim().is_empty()) {
        any = true;
        if s.style.bg.is_none() {
            return false;
        }
    }
    any
}

/// Box each contiguous run of fenced-code rows into a uniform rectangle: pad every row's code bg to
/// the run's widest content plus a 1-cell inner space on each side. Hugs the code, not the pane edge.
///
/// ponytail: padding runs after nowrap rows are clipped to `avail`, so a boxed run can overflow
/// `avail` by ~2 cells; the Paragraph clips it. h-scroll extent reads source width, so it's unaffected.
fn box_fenced(rows: &mut [PanelRow]) {
    let width = |r: &PanelRow| {
        r.spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum::<usize>()
    };
    let mut i = 0;
    while i < rows.len() {
        // Card rows carry a pre-sized rounded box; boxing them would double-wrap it.
        if rows[i].comment || !is_fenced_row(&rows[i].spans) {
            i += 1;
            continue;
        }
        let end = i + rows[i..]
            .iter()
            .take_while(|r| is_fenced_row(&r.spans))
            .count();
        let w = rows[i..end].iter().map(width).max().unwrap_or(0);
        for r in &mut rows[i..end] {
            // Pad cells carry the row's own code style, so they dim/band exactly like the block.
            let style = r
                .spans
                .iter()
                .find(|s| !s.content.trim().is_empty())
                .map_or_else(Style::default, |s| s.style);
            let used = width(r);
            let mut boxed = vec![Span::styled(" ", style)];
            boxed.append(&mut r.spans);
            boxed.push(Span::styled(" ".repeat(w - used + 1), style));
            r.spans = boxed;
        }
        i = end;
    }
}

/// Wrap each maximal run of inline-code (bg) spans on a non-fenced row with full-cell end caps
/// (`█`, fg = code bg on the default bg) so the chip carries a solid cell of colour before and after
/// its glyphs — reads as padding, not the blank half-cell a half-block cap leaves on its outer side.
/// Plain Unicode block element — no Nerd font needed.
///
/// ponytail: a cap can push a fully-wrapped prose row up to 2 cells past `avail` and the last right
/// cap may clip at the pane edge — rare (inline code exactly at a wrap boundary); the Paragraph clips.
fn cap_inline(rows: &mut [PanelRow]) {
    let cap = Style::default().fg(CODE_BG);
    for r in rows.iter_mut() {
        // Card note rows carry inline-code chips inside a pre-sized border; capping them overflows it.
        // Nowrap rows (tables/code) are column-aligned — a cap widens a cell and drifts the borders (#45).
        if r.comment
            || r.nowrap
            || is_fenced_row(&r.spans)
            || !r.spans.iter().any(|s| s.style.bg.is_some())
        {
            continue;
        }
        let mut out: Vec<Span<'static>> = vec![];
        let mut in_run = false;
        for s in r.spans.drain(..) {
            match (s.style.bg.is_some(), in_run) {
                (true, false) => {
                    out.push(Span::styled("\u{2588}", cap)); // █ left cap (full-cell chip edge)
                    in_run = true;
                }
                (false, true) => {
                    out.push(Span::styled("\u{2588}", cap)); // █ right cap
                    in_run = false;
                }
                _ => {}
            }
            out.push(s);
        }
        if in_run {
            out.push(Span::styled("\u{2588}", cap));
        }
        r.spans = out;
    }
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

    // Split on ' ' into style-carrying words, remembering each separator space's own style so an
    // intra-chip space keeps the code bg (contiguous chip) while a prose space between two chips
    // stays plain. `sep` is the style of the space that *precedes* the word (unused on word 0).
    let mut words: Vec<(Style, Vec<(char, Style)>)> = vec![(Style::default(), vec![])];
    for &(c, st) in body {
        if c == ' ' {
            words.push((st, vec![]));
        } else {
            words
                .last_mut()
                .expect("words is seeded with one bucket")
                .1
                .push((c, st));
        }
    }

    let mut rows: Vec<Vec<(char, Style)>> = vec![];
    let mut cur: Vec<(char, Style)> = vec![];
    for (sep_style, mut word) in words {
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
            cur.push((' ', sep_style)); // keep the source space's style (code bg stays contiguous)
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
