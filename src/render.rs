//! file→styled: read a file and turn it into styled lines plus a plain-text projection.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// A rendered file: plain-text `content` plus styled lines and per-line indent, all 1:1 by line.
pub(crate) struct Rendered {
    pub(crate) content: String,
    pub(crate) styled: Vec<Line<'static>>,
    pub(crate) indent: Vec<usize>,
}

/// Read and render a file: `.md` → styled lines, a known code ext → Nord fg colours, else raw. Errors surface as text, never a panic.
///
/// ponytail: `.md` `L<n>` indexes rendered, not source, lines — the payload carries line text so the agent locates by content.
pub(crate) fn render(path: &str) -> Rendered {
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
    let indent = vec![0; styled.len()];
    Rendered {
        content,
        styled,
        indent,
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
                Some(Color::Rgb(191, 97, 106)) // nord red
            } else {
                None
            };
            match color {
                Some(c) => Line::from(Span::styled(l.to_string(), Style::default().fg(c))),
                None => Line::from(Span::raw(l.to_string())),
            }
        })
        .collect::<Vec<_>>();
    let indent = vec![0; styled.len()];
    Rendered {
        content: raw.to_string(),
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
pub(crate) const RULE_SENTINEL: &str = "\u{2500}";

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
