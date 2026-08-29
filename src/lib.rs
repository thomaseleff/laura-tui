//! Host a shell/agent in a PTY and parse its output into a vt100 grid.

pub mod protocol;

pub use protocol::Message;

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use anyhow::Result;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Answer ConPTY's `ESC[6n` cursor query; it withholds child output until replied. Fixed `1;1` — nothing tracks the real cursor.
pub fn dsr_reply(chunk: &[u8]) -> Option<&'static [u8]> {
    chunk
        .windows(4)
        .any(|w| w == b"\x1b[6n")
        .then_some(b"\x1b[1;1R")
}

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
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));

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
        }
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

/// Per-tab counter for unique socket names; no clock/rng needed.
static TAB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One workspace tab: a PTY, its optional panel, and its own `LAURA_TAB` socket. Per-tab sockets isolate tabs by addressing (protocol.rs).
pub struct Tab {
    pub pty: PtyTab,
    pub panel: Option<Panel>,
    pub name: String,
    /// Set by `Open{focus}`; the run loop consumes it to focus the panel once.
    pub pending_focus: bool,
    /// Tab hosts an agent (declared via `laura ready`); gates review injection.
    pub agent: bool,
    rx: Receiver<Message>,
    pty_size: (u16, u16),
}

impl Tab {
    /// Mint a unique socket, serve it, point `cmd`'s `LAURA_TAB` at it, spawn the PTY.
    pub fn spawn(mut cmd: CommandBuilder, rows: u16, cols: u16) -> Result<Tab> {
        let n = TAB_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("laura-{}-{}.sock", std::process::id(), n);
        let rx = protocol::serve(&name)?;
        cmd.env("LAURA_TAB", &name);
        let pty = PtyTab::spawn(cmd, rows, cols)?;
        Ok(Tab {
            pty,
            panel: None,
            name,
            pending_focus: false,
            agent: false,
            rx,
            pty_size: (rows, cols),
        })
    }

    /// Drain queued socket messages into panel state, then live-reload the panel.
    pub fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Message::Open { path, focus } => {
                    self.panel = Some(Panel::open(path));
                    self.pending_focus = focus;
                }
                Message::Close => self.panel = None,
                Message::Ready => self.agent = true,
                Message::Update { .. } => {} // reserved; not yet emitted
            }
        }
        if let Some(p) = self.panel.as_mut() {
            p.reload_if_changed();
        }
    }

    /// Resize the PTY only when its draw area changed, else the shell wraps wrong.
    pub fn resize_to(&mut self, rows: u16, cols: u16) {
        if (rows, cols) != self.pty_size {
            self.pty_size = (rows, cols);
            self.pty.resize(rows, cols);
        }
    }
}

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
    /// Last-seen source signature (mtime, byte len); drives `reload_if_changed`.
    sig: Option<(SystemTime, u64)>,
}

impl Panel {
    /// Read `path` into a panel; a read error becomes visible text, never a panic (the PTY input path must not crash).
    pub fn open(path: String) -> Panel {
        let Rendered {
            content,
            styled,
            indent,
        } = render(&path);
        let sig = stat_sig(&path);
        Panel {
            path,
            content,
            styled,
            indent,
            cursor: 0,
            comments: vec![],
            sig,
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
        } = render(&self.path);
        self.content = content;
        self.styled = styled;
        self.indent = indent;
        self.sig = sig;
        self.cursor = self.cursor.min(self.line_count() - 1);
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

/// Wrap `text` in bracketed paste (`ESC[200~ … ESC[201~`) for PTY injection: newlines stay inside the markers so a line-reading REPL doesn't submit early; one trailing `\r` outside submits once.
///
/// ponytail: paste-honoring only holds against a real REPL; a bare shell ignores the markers.
pub fn bracketed_paste(text: &str) -> Vec<u8> {
    format!("\x1b[200~{text}\x1b[201~\r").into_bytes()
}

/// A rendered file: plain-text `content` plus styled lines and per-line indent, all 1:1 by line.
struct Rendered {
    content: String,
    styled: Vec<Line<'static>>,
    indent: Vec<usize>,
}

/// Read and render a file: `.md` → styled lines, a known code ext → Nord fg colours, else raw. Errors surface as text, never a panic.
///
/// ponytail: `.md` `L<n>` indexes rendered, not source, lines — the payload carries line text so the agent locates by content.
fn render(path: &str) -> Rendered {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return plain(format!("cannot read {path}: {e}")),
    };
    match path
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md" | "markdown") => render_markdown(&raw),
        Some(ext) => render_code(&raw, ext),
        None => plain(raw),
    }
}

/// Already-plain text as unstyled, unindented lines (1:1 with `content.lines()`).
fn plain(content: String) -> Rendered {
    let styled = content
        .lines()
        .map(|l| Line::from(Span::raw(l.to_string())))
        .collect::<Vec<_>>();
    let indent = vec![0; styled.len()];
    Rendered {
        content,
        styled,
        indent,
    }
}

/// Syntax-highlight by extension (Nord, fg-only). `content` stays verbatim source. Falls back to `plain` for unrecognized extensions.
fn render_code(raw: &str, ext: &str) -> Rendered {
    let syntaxes = syntax_set();
    let Some(syntax) = syntaxes.find_syntax_by_extension(ext) else {
        return plain(raw.to_string());
    };
    if syntax.name == "Plain Text" {
        return plain(raw.to_string());
    }
    // syntect over untrusted bytes: degrade a highlight panic to raw text, don't crash.
    let styled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        highlight_lines(raw, syntax, syntaxes)
    }));
    match styled {
        Ok(Some(styled)) => {
            let indent = vec![0; styled.len()];
            Rendered {
                content: raw.to_string(),
                styled,
                indent,
            }
        }
        _ => plain(raw.to_string()),
    }
}

/// Highlight each source line into fg-only spans (1:1 with `content.lines()`). `None` on syntect error.
fn highlight_lines(
    raw: &str,
    syntax: &syntect::parsing::SyntaxReference,
    syntaxes: &SyntaxSet,
) -> Option<Vec<Line<'static>>> {
    let mut highlighter = HighlightLines::new(syntax, nord_theme());
    let mut out = Vec::new();
    for line in LinesWithEndings::from(raw) {
        let ranges = highlighter.highlight_line(line, syntaxes).ok()?;
        let spans = ranges
            .iter()
            .map(|(style, text)| {
                let fg = style.foreground;
                Span::styled(
                    text.trim_end_matches(['\n', '\r']).to_string(),
                    Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b)),
                )
            })
            .filter(|s| !s.content.is_empty())
            .collect::<Vec<_>>();
        out.push(Line::from(spans));
    }
    Some(out)
}

/// syntect's bundled syntaxes, loaded once. Newline variant matches `LinesWithEndings`.
fn syntax_set() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The bundled Nord theme, parsed once. `include_str!`, so `.expect` guards a build-time invariant.
fn nord_theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let mut cursor = std::io::Cursor::new(include_str!("../assets/nord.tmTheme"));
        ThemeSet::load_from_reader(&mut cursor)
            .expect("bundled nord.tmTheme is a valid TextMate theme")
    })
}

/// Render markdown to styled lines via `tui-markdown`, deriving indent from heading depth. `content` is the styling-stripped projection.
///
/// ponytail: `tui-markdown` `=0.3.9` is a pre-1.0 PoC — `catch_unwind` degrades a pathological doc to raw text; pinned because its line/style shape isn't semver-stable and gutter math keys off it.
fn render_markdown(md: &str) -> Rendered {
    let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let opts = tui_markdown::Options::new(LauraStyleSheet);
        tui_markdown::from_str_with_options(md, &opts)
    }));
    let Ok(text) = rendered else {
        return plain(md.to_string());
    };
    let styled: Vec<Line<'static>> = text.lines.iter().map(owned_line).map(refine_line).collect();
    let content = styled.iter().map(line_text).collect::<Vec<_>>().join("\n");
    let indent = heading_indents(&styled);
    Rendered {
        content,
        styled,
        indent,
    }
}

/// Clone a line into `'static`, folding line-level style into each span so wrapping preserves colour.
fn owned_line(line: &Line) -> Line<'static> {
    let base = line.style;
    let spans = line
        .spans
        .iter()
        .map(|s| Span::styled(s.content.to_string(), base.patch(s.style)))
        .collect::<Vec<_>>();
    Line::from(spans)
}

/// A line's plain text (spans concatenated, styling dropped).
fn line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Marks a thematic break; `layout` stretches it full-width so `---` renders as a rule.
const RULE_SENTINEL: &str = "\u{2500}";

/// Heading colour ramp: one blurple hue dimming by level, so headings read as a set. `heading_level` sniffs a heading by this colour + bold.
const HEADING_RAMP: [Color; 4] = [
    Color::Rgb(96, 130, 246), // H1
    Color::Rgb(82, 111, 209), // H2
    Color::Rgb(69, 94, 177),  // H3
    Color::Rgb(60, 81, 153),  // H4+
];

/// Fix what `tui-markdown` hard-codes past the `StyleSheet`: drop the `>` blockquote gutter, return the light-blue list number to body colour, swap `---` for a full-width sentinel. Runs after `owned_line`, so dropping spans never loses colour.
///
/// ponytail: keys off tui-markdown's exact `Span::raw(">")` and `.light_blue()` — pinned `=0.3.9`; revisit on any bump.
fn refine_line(line: Line<'static>) -> Line<'static> {
    if line.spans.len() == 1 && line.spans[0].content.as_ref() == "---" {
        return Line::from(Span::styled(
            RULE_SENTINEL,
            Style::default().fg(Color::Rgb(90, 90, 90)),
        ));
    }
    let mut spans = line.spans;
    // Leading ">" prefix spans (one per nesting level) + the separating space.
    if spans.first().is_some_and(|s| s.content.as_ref() == ">") {
        let keep = spans
            .iter()
            .position(|s| s.content.as_ref() != ">")
            .unwrap_or(spans.len());
        spans.drain(..keep);
        if spans.first().is_some_and(|s| s.content.as_ref() == " ") {
            spans.remove(0);
        }
    }
    for s in &mut spans {
        if s.style.fg == Some(Color::LightBlue) {
            s.style.fg = None;
        }
    }
    Line::from(spans)
}

/// Per-line indent mirroring heading depth, until the next heading.
///
/// ponytail: capped at 3 levels so a deep `####` can't run a narrow panel out of width.
fn heading_indents(lines: &[Line]) -> Vec<usize> {
    const STEP: usize = 2;
    const CAP: usize = 3;
    let mut cur = 0usize;
    lines
        .iter()
        .map(|line| {
            if let Some(level) = heading_level(line) {
                cur = (level.min(CAP) - 1) * STEP;
            }
            cur
        })
        .collect()
}

/// A heading line's level (1-based), else `None`. Detects a heading by style (bold + `HEADING_RAMP` colour), then counts leading `#`s.
fn heading_level(line: &Line) -> Option<usize> {
    let first = line.spans.first()?;
    let is_heading = first.style.add_modifier.contains(Modifier::BOLD)
        && first.style.fg.is_some_and(|c| HEADING_RAMP.contains(&c));
    if !is_heading {
        return None;
    }
    let hashes = line_text(line).chars().take_while(|c| *c == '#').count();
    (hashes > 0).then_some(hashes)
}

/// Panel styling for `tui-markdown`: muted palette, monochrome heading ramp, gray code, soft-blue link, italic blockquote, hidden code fences.
#[derive(Clone)]
struct LauraStyleSheet;

impl tui_markdown::StyleSheet for LauraStyleSheet {
    fn heading(&self, level: u8) -> Style {
        let idx = (level as usize)
            .saturating_sub(1)
            .min(HEADING_RAMP.len() - 1);
        Style::default()
            .fg(HEADING_RAMP[idx])
            .add_modifier(Modifier::BOLD)
    }

    fn code(&self) -> Style {
        Style::default()
            .fg(Color::Rgb(180, 180, 180))
            .bg(Color::Rgb(45, 45, 45))
    }

    // ponytail: color-only — Ratatui has no dotted underline, so the soft blue carries the link.
    fn link(&self) -> Style {
        Style::default().fg(Color::Rgb(120, 180, 240))
    }

    fn blockquote(&self) -> Style {
        Style::default()
            .fg(Color::Rgb(150, 150, 150))
            .add_modifier(Modifier::ITALIC)
    }

    fn alert(&self, kind: tui_markdown::AlertKind) -> Style {
        use tui_markdown::AlertKind::*;
        let c = match kind {
            Note => Color::Rgb(120, 150, 190),
            Tip => Color::Rgb(120, 180, 140),
            Important => Color::Rgb(160, 140, 190),
            Warning => Color::Rgb(200, 180, 120),
            Caution => Color::Rgb(200, 130, 120),
        };
        Style::default().fg(c)
    }

    // Structure via bold only; drop the default cyan so the header stays monochrome.
    fn table_header(&self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }

    fn code_block_fence(&self) -> &str {
        ""
    }
}

/// A file's `(mtime, len)` change signature, or `None` if it can't be stat'd.
fn stat_sig(path: &str) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
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
