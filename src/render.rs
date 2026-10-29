//! file→styled: read a file and turn it into styled lines plus a plain-text projection.

use std::cell::{Cell, OnceCell};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd,
};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Highlighter, Theme, ThemeSet};
use syntect::html::{
    IncludeBackground, start_highlighted_html_snippet, styled_line_to_highlighted_html,
};
use syntect::parsing::{Scope, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

thread_local! { static GUARDED: Cell<bool> = const { Cell::new(false) }; }

/// `catch_unwind` that also keeps the panic hook quiet, so ratatui doesn't restore the terminal.
pub(crate) fn guarded<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    let outer = GUARDED.replace(true);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    GUARDED.set(outer);
    r
}

/// True inside `guarded` on this thread; the panic hook reads it.
pub fn in_guard() -> bool {
    GUARDED.get()
}

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
    /// Markdown only: each top-level block (or list item) for the web app (#89). Empty otherwise,
    /// and on a degraded render.
    pub(crate) blocks: Vec<Block>,
}

/// A rendered markdown block: the `styled` rows it covers, its source and, unless it's code or
/// HTML, the file's reference definitions. HTML is built on first use.
pub(crate) struct Block {
    pub(crate) rows: Range<usize>,
    md: String,
    /// The slice's first 0-based source line.
    l0: usize,
    /// Each 0-based source line a row maps to alone, with its first row.
    own: Vec<(usize, usize)>,
    defs: Option<Arc<str>>,
    /// The HTML and its stops.
    html: OnceCell<(String, Vec<(usize, u32)>)>,
}

impl Block {
    /// The block as HTML. A relative image's `src` starts with `\0`, which the caller replaces
    /// with the pane's file route. A panic in `md_html` shows the source as escaped text instead.
    pub(crate) fn html(&self) -> &str {
        &self.built().0
    }

    /// `(row, 1-based line)` for each line inside the block with its own row and its own
    /// `data-line`: a code line, a table body row or a list item. The first row isn't one.
    pub(crate) fn stops(&self) -> &[(usize, u32)] {
        &self.built().1
    }

    fn built(&self) -> &(String, Vec<(usize, u32)>) {
        self.html.get_or_init(|| {
            let md = format!("{}{}", self.md, self.defs.as_deref().unwrap_or(""));
            let lines: Vec<usize> = self.own.iter().map(|&(l, _)| l).collect();
            let (html, anchored) = guarded(|| md_html(&md, self.l0, &lines)).unwrap_or_else(|_| {
                let mut html = String::new();
                pulldown_cmark::html::push_html(
                    &mut html,
                    [
                        Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)),
                        Event::Text(self.md.replace('\0', "\u{FFFD}").into()),
                        Event::End(TagEnd::CodeBlock),
                    ]
                    .into_iter(),
                );
                (html, vec![])
            });
            let stops = self
                .own
                .iter()
                .filter(|&&(_, row)| row != self.rows.start)
                .map(|&(line, row)| (row, line as u32 + 1))
                .filter(|(_, n)| anchored.contains(n))
                .collect();
            (html, stops)
        })
    }
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
pub(crate) fn plain(content: String) -> Rendered {
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
        blocks: vec![],
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
                Some(Color::Rgb(94, 129, 172)) // nord10
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
        blocks: vec![],
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
    let styled = guarded(|| highlight_lines(raw, syntax, syntaxes));
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
                blocks: vec![],
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

/// syntect's bundled light theme, for the web app's markdown code blocks. Bundled by the
/// `default-themes` feature, so `.expect` guards a build-time invariant.
fn light_theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        ThemeSet::load_defaults()
            .themes
            .remove("InspiredGitHub")
            .expect("syntect bundles InspiredGitHub")
    })
}

/// The Nord theme's foreground for a TextMate `scope`, as `#RRGGBB`: what the file view paints it.
/// Callers pass literal scopes, so `.expect` guards a build-time invariant.
pub fn nord_hex(scope: &str) -> String {
    let scope = Scope::new(scope).expect("a literal TextMate scope parses");
    let fg = Highlighter::new(nord_theme())
        .style_for_stack(&[scope])
        .foreground;
    format!("#{:02X}{:02X}{:02X}", fg.r, fg.g, fg.b)
}

/// Render markdown per top-level block, tagging each rendered line with its block's source-line
/// range so gutter/highlight/`L<n>` match the file, not the reflowed projection (#32).
///
/// ponytail: `tui-markdown =0.3.9` is a pre-1.0 PoC — `catch_unwind` degrades a pathological doc to
/// raw text (identity ranges, so numbering stays honest); pinned because gutter math keys off its shape.
/// ponytail: a 2+ blank-line gap keeps its blank rows rather than collapsing to one — accepted for the block-map.
/// ponytail: a footnote whose definition is in another block shows as `[^n]` after slicing.
fn render_markdown(md: &str) -> Rendered {
    let source_lines: Vec<String> = md.lines().map(str::to_string).collect();
    let assembled = guarded(|| assemble_markdown(md));
    let (styled, source, blocks) = assembled.unwrap_or_else(|_| {
        // Degrade to raw text with identity ranges: rendered row i == source line i.
        let styled: Vec<Line<'static>> = md
            .lines()
            .map(|l| Line::from(Span::raw(l.to_string())))
            .collect();
        let source = (0..styled.len()).map(|i| (i, i)).collect();
        (styled, source, vec![])
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
        blocks,
    }
}

/// Slice markdown into top-level blocks, render each through `tui-markdown`, and tag every resulting
/// line with its block's 0-based source range. Blank source lines, and the lines of a block that
/// renders no rows, gap-fill to blank rows; only a code block's hidden fences have no row. Each slice
/// is also a `Block` of the rows it rendered; gap-fill rows belong to none.
fn assemble_markdown(md: &str) -> (Vec<Line<'static>>, Vec<(usize, usize)>, Vec<Block>) {
    let defs: Arc<str> = ref_defs(md).into(); // appended to linked slices so reference links still resolve
    let line_of = line_indexer(md);
    let lines: Vec<&str> = md.lines().collect();
    let total_lines = lines.len();
    let opts = tui_markdown::Options::new(LauraStyleSheet);
    let mut styled: Vec<Line<'static>> = vec![];
    let mut source: Vec<(usize, usize)> = vec![];
    let mut out_blocks: Vec<Block> = vec![];
    let mut push_block =
        |source: &[(usize, usize)], rows: Range<usize>, l0: usize, md: &str, linked: bool| {
            if rows.is_empty() {
                return;
            }
            let mut own: Vec<(usize, usize)> = vec![];
            for r in rows.clone() {
                let (a, b) = source[r];
                if a == b && own.last().is_none_or(|&(l, _)| l != a) {
                    own.push((a, r));
                }
            }
            out_blocks.push(Block {
                rows,
                md: md.to_string(),
                l0,
                own,
                defs: linked.then(|| defs.clone()),
                html: OnceCell::new(),
            });
        };
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
                    let item = lines[s..=end].join("\n");
                    let slice = format!("{item}{defs}");
                    let text = tui_markdown::from_str_with_options(&slice, &opts);
                    let rows: Vec<_> = text.lines.iter().map(owned_line).map(refine_line).collect();
                    let first = styled.len();
                    for (row, range) in align_rows(rows, &lines[s..=e], s) {
                        styled.push(row);
                        source.push(range);
                    }
                    push_block(&source, first..styled.len(), s, &item, true);
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
        // Code and HTML hold no links, and an unclosed one would swallow the defs. An unclosed one
        // also runs to the end of the file: cut its trailing blank lines, which the gap-fill shows (#103).
        let linked = !matches!(kind, BlockKind::Code { .. } | BlockKind::Html);
        let body = if linked {
            md[b0..b1].to_string()
        } else {
            lines[l0..=l1].join("\n")
        };
        let slice = if linked {
            format!("{body}{defs}")
        } else {
            body.clone()
        };
        let fenced_slice;
        let src = match kind {
            BlockKind::Code { .. } => {
                fenced_slice = fence_code(&slice);
                &fenced_slice
            }
            _ => &slice,
        };
        let text = tui_markdown::from_str_with_options(src, &opts);
        let rows: Vec<_> = text.lines.iter().map(owned_line).map(refine_line).collect();
        let first = styled.len();
        // Prose reflows, so it maps 1:1 only when its shape matches the source (#34, #70). List
        // reaches here only on the fallback: map it like prose.
        if let BlockKind::Prose | BlockKind::List = kind {
            for (row, range) in align_rows(rows, &lines[l0..=l1], l0) {
                styled.push(row);
                source.push(range);
            }
            push_block(&source, first..styled.len(), l0, &md[b0..b1], linked);
            if styled.len() > first {
                cursor = cursor.max(l1 + 1);
            }
            continue;
        }
        let n = rows.len();
        for (k, line) in rows.into_iter().enumerate() {
            // Verbatim blocks render one row per source line, so each row owns a single real line.
            let range = match kind {
                BlockKind::Prose | BlockKind::List => (l0, l1), // handled above
                BlockKind::Code { fenced } => {
                    let s = (l0 + usize::from(fenced) + k).min(l1); // skips the hidden opening fence
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
        push_block(&source, first..styled.len(), l0, &body, linked);
        if styled.len() > first {
            cursor = cursor.max(l1 + 1);
        }
    }
    while cursor < total_lines {
        styled.push(Line::default());
        source.push((cursor, cursor));
        cursor += 1;
    }
    (styled, source, out_blocks)
}

/// A code block as an unindented fence with no info string. tui-markdown runs an indented block's,
/// or an indented fence's, lines into one row; a bare fence keeps one row per line.
fn fence_code(slice: &str) -> String {
    let mut body: String = Parser::new(slice)
        .filter_map(|ev| match ev {
            Event::Text(t) => Some(t.into_string()),
            _ => None,
        })
        .collect();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    // Longer than any backtick run in the code, so no line closes the fence.
    let run = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(run.max(2) + 1);
    format!("{fence}\n{body}{fence}\n")
}

/// tui-markdown's options, so block bounds and the browser match the TUI: no smart quotes, no wikilinks.
fn md_opts() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
}

/// One markdown slice as HTML for the web app (#89). The file is untrusted: raw HTML shows as
/// text, only `http(s)` links keep their `href`, a relative or `#heading` link gets a `data-href`,
/// and headings drop `{#id .class}` attributes, so nothing in the file can script the page or
/// restyle Laura's own markup.
///
/// A code line, a table body row or a list item on a 0-based source line in `own` (the lines the
/// TUI gives their own row) carries its own `data-line`. Returns the HTML and those 1-based lines.
fn md_html(slice: &str, l0: usize, own: &[usize]) -> (String, Vec<u32>) {
    // `\0` marks a relative image's file route (below), so the file can't hold one.
    let slice = &slice.replace('\0', "\u{FFFD}");
    let line_of = line_indexer(slice);
    let mut anchored: Vec<u32> = vec![];
    let mut anchor = |line: usize| {
        own.contains(&line).then(|| {
            let n = line as u32 + 1;
            anchored.push(n);
            n
        })
    };
    let mut out: Vec<Event> = vec![];
    // Per open link: its closing tag, or `None` to drop the link's tags.
    let mut links: Vec<Option<Event>> = vec![];
    // An open code, HTML or front matter block: its language, first source line and text.
    let mut code: Option<(Option<&SyntaxReference>, Option<usize>, String)> = None;
    let mut label: Option<String> = None; // a footnote definition's `[n]: `, put in its first paragraph
    let mut img: Option<(String, String)> = None; // an open relative image: its dest and alt text
    let mut aligns: Vec<Alignment> = vec![];
    let (mut in_head, mut cell) = (false, 0);
    for (ev, range) in Parser::new_ext(slice, md_opts()).into_offset_iter() {
        if let Some((syntax, mut first, mut buf)) = code.take() {
            match ev {
                Event::End(TagEnd::CodeBlock | TagEnd::HtmlBlock | TagEnd::MetadataBlock(_)) => {
                    let first = first.unwrap_or(l0);
                    let html = code_html(&buf, syntax, |k| anchor(first + k));
                    out.push(Event::Html(html.into()));
                }
                ev => {
                    if let Event::Text(t) | Event::Html(t) = ev {
                        first.get_or_insert(l0 + line_of(range.start));
                        buf.push_str(&t);
                    }
                    code = Some((syntax, first, buf));
                }
            }
            continue;
        }
        if let Some((dest, mut alt)) = img.take() {
            match ev {
                Event::End(TagEnd::Image) => {
                    let mut h = String::from("<img src=\"\0");
                    escape(&mut h, &dest);
                    h.push_str("\" alt=\"");
                    escape(&mut h, &alt);
                    h.push_str("\">");
                    out.push(Event::Html(h.into()));
                }
                ev => {
                    match ev {
                        Event::Text(t) | Event::Code(t) | Event::InlineHtml(t) => alt.push_str(&t),
                        Event::InlineMath(t) => alt.push_str(&format!("${t}$")),
                        Event::DisplayMath(t) => alt.push_str(&format!("$${t}$$")),
                        Event::SoftBreak | Event::HardBreak => alt.push(' '),
                        _ => {}
                    }
                    img = Some((dest, alt));
                }
            }
            continue;
        }
        if let Some(l) = label.take() {
            if let Event::Start(Tag::Paragraph) = ev {
                out.extend([ev, Event::Text(l.into())]);
                continue;
            }
            out.extend([
                Event::Start(Tag::Paragraph),
                Event::Text(l.into()),
                Event::End(TagEnd::Paragraph),
            ]);
        }
        match ev {
            Event::Html(t) | Event::InlineHtml(t) => out.push(Event::Text(t)),
            Event::Start(
                Tag::HtmlBlock | Tag::MetadataBlock(_) | Tag::CodeBlock(CodeBlockKind::Indented),
            ) => code = Some((None, None, String::new())),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info))) => {
                let token = info.split_whitespace().next().unwrap_or("");
                // An unknown language drops its info string: `push_html` would make it a `class`.
                let syntax = syntax_set()
                    .find_syntax_by_token(token)
                    .filter(|_| !token.is_empty());
                code = Some((syntax, None, String::new()));
            }
            Event::Start(Tag::Item) => match anchor(l0 + line_of(range.start)) {
                Some(n) => out.push(Event::Html(format!(r#"<li data-line="{n}">"#).into())),
                None => out.push(ev),
            },
            Event::Start(Tag::Table(ref a)) => {
                aligns = a.clone();
                out.push(ev);
            }
            Event::Start(Tag::TableHead) | Event::End(TagEnd::TableHead) => {
                in_head = matches!(ev, Event::Start(_));
                out.push(ev);
            }
            // A body row and its cells as `push_html` writes them, plus the row's `data-line`.
            Event::Start(Tag::TableRow) if !in_head => {
                cell = 0;
                out.push(Event::Html(
                    match anchor(l0 + line_of(range.start)) {
                        Some(n) => format!(r#"<tr data-line="{n}">"#),
                        None => "<tr>".to_string(),
                    }
                    .into(),
                ));
            }
            Event::Start(Tag::TableCell) if !in_head => {
                let align = match aligns.get(cell) {
                    Some(Alignment::Left) => r#" style="text-align: left""#,
                    Some(Alignment::Center) => r#" style="text-align: center""#,
                    Some(Alignment::Right) => r#" style="text-align: right""#,
                    _ => "",
                };
                cell += 1;
                out.push(Event::Html(format!("<td{align}>").into()));
            }
            Event::Start(Tag::Heading { level, .. }) => out.push(Event::Start(Tag::Heading {
                level,
                id: None,
                classes: vec![],
                attrs: vec![],
            })),
            Event::Start(Tag::Link {
                link_type,
                ref dest_url,
                ..
            }) => {
                let url = dest_url.to_ascii_lowercase();
                if url.starts_with("http:") || url.starts_with("https:") {
                    links.push(Some(Event::End(TagEnd::Link)));
                    out.push(ev);
                } else if link_type != LinkType::Email
                    && (relative(dest_url) || dest_url.starts_with('#'))
                {
                    let mut a = r#"<a role="link" data-href=""#.to_string();
                    escape(&mut a, dest_url);
                    a.push_str(r#"">"#);
                    links.push(Some(Event::Html("</a>".into())));
                    out.push(Event::Html(a.into()));
                } else {
                    links.push(None);
                }
            }
            Event::End(TagEnd::Link) => out.extend(links.pop().flatten()),
            Event::FootnoteReference(n) => out.push(Event::Text(format!("[{n}]").into())),
            Event::InlineMath(t) => out.push(Event::Text(format!("${t}$").into())),
            Event::DisplayMath(t) => out.push(Event::Text(format!("$${t}$$").into())),
            // The TUI's label, as text, instead of `push_html`'s `<sup>` and `id`.
            Event::Start(Tag::FootnoteDefinition(n)) => label = Some(format!("[{n}]: ")),
            Event::Start(Tag::BlockQuote(Some(k))) => {
                use tui_markdown::{AlertKind as A, StyleSheet as _};
                let k = match k {
                    BlockQuoteKind::Note => A::Note,
                    BlockQuoteKind::Tip => A::Tip,
                    BlockQuoteKind::Important => A::Important,
                    BlockQuoteKind::Warning => A::Warning,
                    BlockQuoteKind::Caution => A::Caution,
                };
                let title = format!(
                    "{} {}",
                    LauraStyleSheet.alert_icon(k),
                    LauraStyleSheet.alert_label(k)
                );
                out.extend([
                    ev,
                    Event::Start(Tag::Paragraph),
                    Event::Start(Tag::Strong),
                    Event::Text(title.into()),
                    Event::End(TagEnd::Strong),
                    Event::End(TagEnd::Paragraph),
                ]);
            }
            Event::Start(Tag::Image { ref dest_url, .. }) if relative(dest_url) => {
                img = Some((dest_url.to_string(), String::new()));
            }
            // Any other image shows its alt text.
            Event::Start(Tag::Image { .. })
            | Event::End(TagEnd::FootnoteDefinition | TagEnd::Image) => {}
            ev => out.push(ev),
        }
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, out.into_iter());
    (html, anchored)
}

/// Whether `dest` is a path beside the file: no scheme, not rooted, not a fragment.
fn relative(dest: &str) -> bool {
    let scheme = dest.split_once(':').is_some_and(|(s, _)| {
        s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    });
    !dest.is_empty() && !scheme && !dest.starts_with(['/', '\\', '#'])
}

/// The 1-based line of the heading whose GitHub slug or `{#id}` is `slug`.
pub(crate) fn heading_line(md: &str, slug: &str) -> Option<u32> {
    let slug = slug.to_lowercase();
    let line_of = line_indexer(md);
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut open: Option<(usize, Option<String>, String)> = None; // start byte, id, text
    for (ev, range) in Parser::new_ext(md, md_opts()).into_offset_iter() {
        match ev {
            Event::Start(Tag::Heading { id, .. }) => {
                open = Some((range.start, id.map(|i| i.to_string()), String::new()));
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, _, text)) = &mut open {
                    text.push_str(&t);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                let Some((start, id, text)) = open.take() else {
                    continue;
                };
                let base: String = text
                    .to_lowercase()
                    .chars()
                    .filter_map(|c| match c {
                        ' ' => Some('-'),
                        c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
                        _ => None,
                    })
                    .collect();
                let n = seen.entry(base.clone()).or_default();
                let slug_n = if *n == 0 { base } else { format!("{base}-{n}") };
                *n += 1;
                // A link such as `#Setup` or `#install` finds a heading or an `{#Install}` id in
                // any case.
                if id.as_deref().is_some_and(|i| i.to_lowercase() == slug) || slug_n == slug {
                    return Some(line_of(start) as u32 + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// A code block's text as HTML, syntax-colored when `syntax` is known, else escaped. Line `k`
/// goes in a `data-line` span when `anchor(k)` gives one; the newline stays outside the span.
fn code_html(
    buf: &str,
    syntax: Option<&SyntaxReference>,
    mut anchor: impl FnMut(usize) -> Option<u32>,
) -> String {
    let colored = syntax.and_then(|syntax| {
        let (open, bg) = start_highlighted_html_snippet(light_theme());
        let mut h = HighlightLines::new(syntax, light_theme());
        let lines = LinesWithEndings::from(buf)
            .map(|line| {
                let regions: Vec<_> = h
                    .highlight_line(line, syntax_set())
                    .ok()?
                    .into_iter()
                    .map(|(s, t)| (s, t.trim_end_matches(['\n', '\r'])))
                    .collect();
                styled_line_to_highlighted_html(&regions, IncludeBackground::IfDifferent(bg)).ok()
            })
            .collect::<Option<Vec<String>>>()?;
        Some((open, lines, "</pre>\n"))
    });
    let (mut out, lines, close) = colored.unwrap_or_else(|| {
        let lines = buf.lines().map(|l| {
            let mut h = String::new();
            escape(&mut h, l);
            h
        });
        (
            "<pre><code>".to_string(),
            lines.collect(),
            "</code></pre>\n",
        )
    });
    for (k, line) in lines.iter().enumerate() {
        match anchor(k) {
            Some(n) => out.push_str(&format!(r#"<span data-line="{n}">{line}</span>"#)),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out.push_str(close);
    out
}

/// `s` as HTML text, safe in an element or a quoted attribute.
pub(crate) fn escape(h: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => h.push_str("&amp;"),
            '<' => h.push_str("&lt;"),
            '>' => h.push_str("&gt;"),
            '"' => h.push_str("&quot;"),
            c => h.push(c),
        }
    }
}

/// How a top-level block maps source lines to rendered rows: `Prose` (incl. quotes and callouts)
/// maps 1:1 when it renders one row per line, else whole range per row (`align_rows`); `Code`
/// and `Html` render 1:1, one row per source line.
#[derive(Clone, Copy)]
enum BlockKind {
    Prose,
    Code { fenced: bool },
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
    for (ev, range) in Parser::new_ext(md, md_opts()).into_offset_iter() {
        match ev {
            Event::Start(tag) => {
                if depth == 0 {
                    // From the line start: an indented code block's range begins after its indent.
                    start = md[..range.start].rfind('\n').map_or(0, |i| i + 1);
                    kind = match tag {
                        Tag::CodeBlock(k) => BlockKind::Code {
                            fenced: k.is_fenced(),
                        },
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
    // pulldown's DefinitionList range runs past the next block's start (#101).
    for i in 1..out.len() {
        out[i - 1].1 = out[i - 1].1.min(out[i].0);
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
    for (ev, range) in Parser::new_ext(slice, md_opts()).into_offset_iter() {
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

/// Reference definitions as blank-line-separated `[label]: <dest>` lines, appended to each linked
/// slice so a ref link still resolves. `\n\n` because a single `\n` folds the def into the prior paragraph.
fn ref_defs(md: &str) -> String {
    let parser = Parser::new_ext(md, md_opts());
    let mut out = String::new();
    for (label, def) in parser.reference_definitions().iter() {
        // `dest` comes back decoded: re-escape it so spaces, `<>`, `&amp;` and an empty dest survive.
        let dest = def
            .dest
            .replace('\\', r"\\")
            .replace('<', r"\<")
            .replace('>', r"\>")
            .replace('&', r"\&");
        out.push_str(&format!("\n\n[{label}]: <{dest}>"));
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
