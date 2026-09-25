//! file→styled: read a file and turn it into styled lines plus a plain-text projection.

use std::sync::OnceLock;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// A rendered file: plain-text `content` plus styled lines, all 1:1 by line.
pub(crate) struct Rendered {
    pub(crate) content: String,
    pub(crate) styled: Vec<Line<'static>>,
    /// Per-line: pre-formatted (clip + h-scroll) vs prose (word-wrap), 1:1 with `styled`.
    pub(crate) nowrap: Vec<bool>,
    /// 0-based inclusive source-line range each `styled` row came from. Identity `(i, i)` except
    /// markdown, where a collapsed block's range makes gutter/highlight/`L<n>` match the file (#32).
    pub(crate) source: Vec<(usize, usize)>,
    /// Raw file split into lines — diff-view's `+`/`-` source. `== content.lines()` bar markdown.
    pub(crate) source_lines: Vec<String>,
    /// Terse read error (stderr-bound), set only when the file couldn't be read. `None` on success.
    pub(crate) error: Option<String>,
}

/// Identity map for a verbatim file: row `i` is source line `i`, `source_lines` = `content.lines()`.
fn identity_source(content: &str, n: usize) -> (Vec<(usize, usize)>, Vec<String>) {
    (
        (0..n).map(|i| (i, i)).collect(),
        content.lines().map(str::to_string).collect(),
    )
}

/// Read and render a file: `.md` → styled lines, a known code ext → Nord fg colours, else raw. Errors surface as text, never a panic.
pub(crate) fn render(path: &str) -> Rendered {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("cannot read {path}: {e}");
            let mut r = plain(format!("Couldn't read this file — {e}. Check the path."));
            r.error = Some(msg);
            return r;
        }
    };
    match path
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md" | "markdown") => render_markdown(&raw),
        Some("diff" | "patch") => render_diff(&raw),
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
    let nowrap = vec![false; styled.len()];
    let (source, source_lines) = identity_source(&content, styled.len());
    Rendered {
        content,
        styled,
        nowrap,
        source,
        source_lines,
        error: None,
    }
}

/// Color a unified diff by line prefix — additions green, removals red, hunk headers blue, file headers dim.
/// syntect's bundled Diff syntax leaves these uncolored under the Nord theme, so we do it by first char. `content` stays verbatim.
fn render_diff(raw: &str) -> Rendered {
    let styled = raw
        .lines()
        .map(|l| {
            // `+++`/`---` file headers must beat the `+`/`-` add/remove check.
            let color = if l.starts_with("+++")
                || l.starts_with("---")
                || l.starts_with("diff ")
                || l.starts_with("index ")
            {
                Some(Color::Rgb(120, 120, 120))
            } else if l.starts_with("@@") {
                Some(Color::Rgb(129, 161, 193)) // nord blue
            } else if l.starts_with('+') {
                Some(Color::Rgb(163, 190, 140)) // nord green
            } else if l.starts_with('-') {
                Some(Color::Rgb(224, 108, 117)) // brighter red, more contrast on dark bg than nord11
            } else {
                None
            };
            match color {
                Some(c) => Line::from(Span::styled(l.to_string(), Style::default().fg(c))),
                None => Line::from(Span::raw(l.to_string())),
            }
        })
        .collect::<Vec<_>>();
    let nowrap = vec![true; styled.len()]; // diffs are pre-formatted
    let (source, source_lines) = identity_source(raw, styled.len());
    Rendered {
        content: raw.to_string(),
        styled,
        nowrap,
        source,
        source_lines,
        error: None,
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
            let nowrap = vec![true; styled.len()]; // recognized code is pre-formatted
            let (source, source_lines) = identity_source(raw, styled.len());
            Rendered {
                content: raw.to_string(),
                styled,
                nowrap,
                source,
                source_lines,
                error: None,
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

/// Render markdown per top-level block, tagging each rendered line with its block's source-line
/// range so gutter/highlight/`L<n>` match the file, not the reflowed projection (#32).
///
/// ponytail: `tui-markdown =0.3.9` is a pre-1.0 PoC — `catch_unwind` degrades a pathological doc to
/// raw text (identity ranges, so numbering stays honest); pinned because gutter math keys off its shape.
/// ponytail: a ref-def used across blocks won't resolve after slicing, and a 2+ blank-line gap keeps
/// its blank rows rather than collapsing to one — both accepted for the block-map.
fn render_markdown(md: &str) -> Rendered {
    let source_lines: Vec<String> = md.lines().map(str::to_string).collect();
    let assembled =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assemble_markdown(md)));
    let (styled, source) = assembled.unwrap_or_else(|_| {
        // Degrade to raw text with identity ranges: rendered row i == source line i.
        let styled: Vec<Line<'static>> = md
            .lines()
            .map(|l| Line::from(Span::raw(l.to_string())))
            .collect();
        let source = (0..styled.len()).map(|i| (i, i)).collect();
        (styled, source)
    });
    let content = styled.iter().map(line_text).collect::<Vec<_>>().join("\n");
    let nowrap = classify_nowrap(&styled);
    Rendered {
        content,
        styled,
        nowrap,
        source,
        source_lines,
        error: None,
    }
}

/// Slice markdown into top-level blocks, render each through `tui-markdown`, and tag every resulting
/// line with its block's 0-based source range. Blank source lines gap-fill to blank rows, so every
/// source line has a row.
fn assemble_markdown(md: &str) -> (Vec<Line<'static>>, Vec<(usize, usize)>) {
    let defs = ref_defs(md); // appended to each slice so reference links still resolve
    let line_of = line_indexer(md);
    let lines: Vec<&str> = md.lines().collect();
    let total_lines = lines.len();
    let opts = tui_markdown::Options::new(LauraStyleSheet);
    let mut styled: Vec<Line<'static>> = vec![];
    let mut source: Vec<(usize, usize)> = vec![];
    let mut cursor = 0usize; // next source line still needing a row
    for (b0, b1, kind) in blocks(md) {
        let l0 = line_of(b0);
        let mut l1 = line_of(b1.saturating_sub(1)).max(l0);
        // A block's byte range swallows its trailing blank line; drop it off l1 so the
        // gap-fill loop emits a blank row for it (e.g. the blank before a heading, #39).
        while l1 > l0 && lines.get(l1).is_some_and(|s| s.trim().is_empty()) {
            l1 -= 1;
        }
        while cursor < l0 {
            styled.push(Line::default());
            source.push((cursor, cursor));
            cursor += 1;
        }
        // Each top-level list item is its own slice so every bullet gets its own thread (#45);
        // `align_rows` then maps nested items per line, blank separators included (#70).
        if let BlockKind::List = kind {
            let starts = top_item_lines(&md[b0..b1], l0);
            if !starts.is_empty() {
                for (idx, &s) in starts.iter().enumerate() {
                    let end = starts.get(idx + 1).map_or(l1, |&next| next - 1);
                    // A loose item's range swallows the blank before the next item, which renders
                    // no row; trim it so the item can still map 1:1, then gap-fill it below.
                    let mut e = end;
                    while e > s && lines.get(e).is_some_and(|l| l.trim().is_empty()) {
                        e -= 1;
                    }
                    let slice = format!("{}{defs}", lines[s..=end].join("\n"));
                    let text = tui_markdown::from_str_with_options(&slice, &opts);
                    let rows: Vec<_> = text.lines.iter().map(owned_line).map(refine_line).collect();
                    for (row, range) in align_rows(rows, &lines[s..=e], s) {
                        styled.push(row);
                        source.push(range);
                    }
                    for j in e + 1..=end {
                        styled.push(Line::default());
                        source.push((j, j));
                    }
                }
                cursor = cursor.max(l1 + 1);
                continue; // items rendered per-slice; skip the whole-block path below
            }
            // else: empty item scan → fall through, tagging the whole block (today's honest collapse).
        }
        let slice = format!("{}{defs}", &md[b0..b1]);
        let text = tui_markdown::from_str_with_options(&slice, &opts);
        let rows: Vec<_> = text.lines.iter().map(owned_line).map(refine_line).collect();
        // Prose reflows, so it maps 1:1 only when its shape matches the source (#34, #70). List
        // reaches here only on the fallback: map it like prose.
        if let BlockKind::Prose | BlockKind::List = kind {
            for (row, range) in align_rows(rows, &lines[l0..=l1], l0) {
                styled.push(row);
                source.push(range);
            }
            cursor = cursor.max(l1 + 1);
            continue;
        }
        let n = rows.len();
        for (k, line) in rows.into_iter().enumerate() {
            // Verbatim blocks render one row per source line, so each row owns a single real line.
            let range = match kind {
                BlockKind::Prose | BlockKind::List => (l0, l1), // handled above
                BlockKind::Code => {
                    let s = (l0 + 1 + k).min(l1); // +1 skips the hidden opening fence
                    (s, s)
                }
                BlockKind::Html => {
                    let s = (l0 + k).min(l1); // no fence, offset 0
                    (s, s)
                }
                BlockKind::Table => table_range(k, n, l0, l1),
            };
            styled.push(line);
            source.push(range);
        }
        cursor = cursor.max(l1 + 1);
    }
    while cursor < total_lines {
        styled.push(Line::default());
        source.push((cursor, cursor));
        cursor += 1;
    }
    (styled, source)
}

/// How a top-level block maps source lines to rendered rows: `Prose` (incl. quotes and callouts)
/// maps 1:1 when it renders one row per line, else whole range per row (`align_rows`); `Code`
/// (fenced only) and `Html` render 1:1, one row per source line.
#[derive(Clone, Copy)]
enum BlockKind {
    Prose,
    Code,
    Html,
    List,
    Table,
}

/// Top-level block byte ranges + kind: a depth-0→0 Start/End span (nesting via +1/−1), plus any
/// depth-0 rule. Ref-defs emit no events, so they land in the gaps and get gap-filled.
fn blocks(md: &str) -> Vec<(usize, usize, BlockKind)> {
    let mut out = vec![];
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut kind = BlockKind::Prose;
    for (ev, range) in Parser::new_ext(md, Options::all()).into_offset_iter() {
        match ev {
            Event::Start(tag) => {
                if depth == 0 {
                    start = range.start;
                    kind = match tag {
                        Tag::CodeBlock(k) if k.is_fenced() => BlockKind::Code,
                        Tag::HtmlBlock => BlockKind::Html,
                        Tag::List(_) => BlockKind::List,
                        Tag::Table(_) => BlockKind::Table,
                        _ => BlockKind::Prose,
                    };
                }
                depth += 1;
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    out.push((start, range.end, kind));
                }
            }
            Event::Rule if depth == 0 => out.push((range.start, range.end, BlockKind::Prose)),
            _ => {}
        }
    }
    out
}

/// Source line (offset by `l0`) of each depth-1 item start in a list block — nested items render
/// inside their top-level item's slice and map per line via `align_rows`. Empty on a degraded
/// parse (caller then maps the whole block) (#45, #70).
fn top_item_lines(slice: &str, l0: usize) -> Vec<usize> {
    let line_of = line_indexer(slice);
    let mut out = vec![];
    let mut depth = 0i32;
    for (ev, range) in Parser::new_ext(slice, Options::all()).into_offset_iter() {
        match ev {
            Event::Start(Tag::List(_)) => depth += 1,
            Event::End(TagEnd::List(_)) => depth -= 1,
            Event::Start(Tag::Item) if depth == 1 => out.push(l0 + line_of(range.start)),
            _ => {}
        }
    }
    out
}

/// Source range for rendered table row `k` of `n`. tui-markdown renders `[top-border, header,
/// separator, data…, bottom-border]`; borders fold to a neighbor. Whole-block on shape mismatch (#45).
fn table_range(k: usize, n: usize, l0: usize, l1: usize) -> (usize, usize) {
    let data_rows = l1.saturating_sub(l0 + 1); // l0=header, l0+1=separator, l0+2..=l1=data
    if n != data_rows + 4 {
        return (l0, l1);
    }
    match k {
        0 | 1 => (l0, l0),             // top border + header fold to the header line
        2 => (l0 + 1, l0 + 1),         // separator (`---|---`)
        k if k == n - 1 => (l1, l1),   // bottom border folds to the last data line
        _ => (l0 + k - 1, l0 + k - 1), // data row i=k-3 → source line l0+2+i
    }
}

/// Tag a block's `rows` (rendered from `lines`, starting at source `l0`) with source ranges. Walks
/// both in order: a row pairs 1:1 with a line of the same blank/non-blank shape, and a blank line the
/// renderer dropped (a loose or nested list's separator) gets its own blank row, like the gap-fill
/// (#70). Any other mismatch, such as a reflowed paragraph leaving a line with no row, tags the whole
/// block on every row, like `table_range`.
///
/// ponytail: shape, not content — a quote whose table (+2 rows) and reflowed paragraphs (−1 each)
/// cancel out maps 1:1 wrongly. Compare row text to line text if that shows up.
fn align_rows(
    rows: Vec<Line<'static>>,
    lines: &[&str],
    l0: usize,
) -> Vec<(Line<'static>, (usize, usize))> {
    // A quote's blank line is `>` alone in source. `refine_line` already strips the rendered `>`.
    let blank = |s: &str| {
        s.trim_start_matches(|c: char| c == '>' || c.is_whitespace())
            .is_empty()
    };
    // Row index per source line; `None` = a blank line the renderer dropped.
    let mut pairs: Vec<Option<usize>> = Vec::with_capacity(lines.len());
    let mut k = 0;
    for l in lines {
        if rows
            .get(k)
            .is_some_and(|r| blank(&line_text(r)) == blank(l))
        {
            pairs.push(Some(k));
            k += 1;
        } else if blank(l) {
            pairs.push(None);
        } else {
            break; // a text line with no matching row: the block reflowed
        }
    }
    if pairs.len() < lines.len() || k < rows.len() {
        let whole = (l0, l0 + lines.len().saturating_sub(1));
        return rows.into_iter().map(|r| (r, whole)).collect();
    }
    let mut rows: Vec<Option<Line<'static>>> = rows.into_iter().map(Some).collect();
    pairs
        .into_iter()
        .enumerate()
        .map(|(j, p)| {
            let row = p.and_then(|k| rows[k].take()).unwrap_or_default();
            (row, (l0 + j, l0 + j))
        })
        .collect()
}

/// Reference definitions as blank-line-separated `[label]: dest` lines, appended to each block slice
/// so a ref link still resolves. `\n\n` because a single `\n` folds the def into the prior paragraph.
fn ref_defs(md: &str) -> String {
    let parser = Parser::new_ext(md, Options::all());
    let mut out = String::new();
    for (label, def) in parser.reference_definitions().iter() {
        out.push_str(&format!("\n\n[{label}]: {}", def.dest));
    }
    out
}

/// Byte offset → 0-based source line via a newline prefix-sum.
fn line_indexer(md: &str) -> impl Fn(usize) -> usize {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(md.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    move |byte| starts.partition_point(|&s| s <= byte).saturating_sub(1)
}

/// Classify each markdown line as pre-formatted (clip + h-scroll) vs prose (word-wrap).
///
/// Signals, structural → cosmetic: a **table** row starts with a box-drawing border (tui-markdown
/// draws them, independent of Laura's palette); a **code block** line has *every* non-blank span
/// backgrounded (`code()` is the only backgrounded element — heading/link/blockquote/alert are
/// fg-only); an **HTML block** line is *every*-span `DIM`. "All non-blank spans" (not "any") keeps a
/// prose line with an inline `code`/`<tag>` span wrapping. Blank lines wrap unless a span carries a bg
/// (an empty line inside a fenced block).
///
/// ponytail: code/HTML signals track `tui-markdown =0.3.9`'s style *shape*, like `refine_line`; the
/// has-bg / DIM switches avoid coupling to exact colour values. HTML-via-DIM is the weakest link —
/// if `html()` is restyled off dim, drop HTML auto-nowrap or give `html()` a bg and fold it in.
fn classify_nowrap(styled: &[Line]) -> Vec<bool> {
    styled
        .iter()
        .map(|line| {
            let nonblank = || line.spans.iter().filter(|s| !s.content.trim().is_empty());
            if nonblank().next().is_none() {
                // An empty fenced-code line still carries the code bg; keep it in the box.
                return line.spans.iter().any(|s| s.style.bg.is_some());
            }
            // Table: first visible char is a box-drawing border.
            let is_table = line_text(line)
                .trim_start()
                .chars()
                .next()
                .is_some_and(|c| "┌┬┐├┼┤└┴┘│─".contains(c));
            // Code block: every non-blank span has a background. HTML block: every one is DIM.
            is_table
                || nonblank().all(|s| s.style.bg.is_some())
                || nonblank().all(|s| s.style.add_modifier.contains(Modifier::DIM))
        })
        .collect()
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
pub(crate) const RULE_SENTINEL: &str = "\u{2500}";

/// Inline/fenced code background. One source shared by `code()`, the panel's spotlight/band
/// styling, and the fenced-block rectangle so the chip colour never forks.
pub const CODE_BG: Color = Color::Rgb(45, 45, 45);

/// Heading colour ramp: one blurple hue dimming by level, so headings read as a set.
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
        Style::default().fg(Color::Rgb(180, 180, 180)).bg(CODE_BG)
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
