//! The pane HTML that the web app's `pane` and `rows` frames carry: a file pane's rows and markdown
//! blocks, their spans as styled HTML, and an HTML page's tagged lines.

use std::fmt::Write as _;
use std::ops::Range;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::panel::PanelLayout;
use crate::panel::cursor::{on_card, on_stop};
use crate::render::{Block, escape};
use crate::view::GUTTER_SPANS;
use crate::{ChangeKind, Panel, PanelRow};

/// `html` with ` data-laura-line="N"` after each start tag's name, N the 1-based line of its `<`,
/// so a click on an element reports the line the TUI shows its tag on. Comments, CDATA and
/// raw text aren't scanned for tags. `tail` goes at the end, or before an unclosed comment, tag,
/// raw-text element or unclosed template, past nested ones, which would hide it.
/// ponytail: a hand scanner, not a parser; a malformed page only misplaces a click's line. Swap in
/// a tokenizer with source positions if that bites.
pub(super) fn tag_lines(html: &str, tail: &str) -> String {
    let b = html.as_bytes();
    // The index of `pat` at or after `from`, any case.
    let find = |from: usize, pat: &str| {
        b.get(from..)?
            .windows(pat.len())
            .position(|w| w.eq_ignore_ascii_case(pat.as_bytes()))
            .map(|k| from + k)
    };
    // Whether the template open at `from` closes, past the nested templates inside it.
    let closes = |mut from: usize| {
        let mut depth = 1;
        while let Some(j) = find(from, "template") {
            from = j + 8;
            if !b
                .get(from)
                .is_some_and(|c| c.is_ascii_whitespace() || matches!(c, b'/' | b'>'))
            {
                continue;
            }
            if b[..j].ends_with(b"</") {
                depth -= 1;
            } else if b[..j].ends_with(b"<") {
                depth += 1;
            }
            if depth == 0 {
                return true;
            }
        }
        false
    };
    let mut out = String::with_capacity(html.len() + html.len() / 4);
    let (mut i, mut copied, mut line, mut counted, mut cut) = (0, 0, 1, 0, None);
    while let Some(k) = b[i..].iter().position(|&c| c == b'<') {
        i += k;
        // Where `i` lands in `out`.
        let at = out.len() + i - copied;
        let rest = &html[i..];
        // `None`: the rest of the page is a comment, a tag, text or a template's content, so `tail`
        // goes at `at`.
        let next = if rest.starts_with("<!--") {
            // A browser ends `<!-->` and `<!--->` at their own `>`.
            find(i + 2, "-->").map(|e| e + 3)
        } else if rest.starts_with("<![CDATA[") {
            find(i + 9, "]]>").map(|e| e + 3)
        } else if b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            let end = b[i + 1..]
                .iter()
                .position(|c| c.is_ascii_whitespace() || matches!(c, b'/' | b'>'))
                .map_or(b.len(), |e| i + 1 + e);
            line += html[counted..i].matches('\n').count();
            counted = i;
            out.push_str(&html[copied..end]);
            let _ = write!(out, r#" data-laura-line="{line}""#);
            copied = end;
            let name = html[i + 1..end].to_ascii_lowercase();
            // The tag's `>`, past quoted values: a `<` or `>` in `title="a>b <i>"` is text. A quote
            // opens only after `=`, so `title=it's` is a plain value.
            let (mut quote, mut eq) = (None, false);
            i = end;
            while let Some(&c) = b.get(i) {
                match quote {
                    None if c == b'>' => break,
                    None if eq && (c == b'"' || c == b'\'') => quote = Some(c),
                    Some(q) if c == q => quote = None,
                    _ => {}
                }
                eq = quote.is_none() && (c == b'=' || (eq && c.is_ascii_whitespace()));
                i += 1;
            }
            match name.as_str() {
                _ if i == b.len() => None,
                "plaintext" => None,
                "template" if !closes(i) => None,
                "script" | "style" | "textarea" | "title" | "xmp" | "noscript" | "iframe"
                | "noembed" | "noframes" => find(i, &format!("</{name}")),
                _ => Some(i),
            }
        } else {
            Some(i + 1)
        };
        let Some(next) = next else {
            cut = Some(at);
            break;
        };
        i = next;
    }
    out.push_str(&html[copied..]);
    out.insert_str(cut.unwrap_or(out.len()), tail);
    out
}

/// The gutter's first cell, blank, so the cursor's bar never touches a wide line number.
const GAP: &str = r#"<span class="g"> "#;

/// One layout row. `data-line` is the 1-based source line the browser sends back; `data-row` the
/// rendered line, which copy joins on. The gutter, after [`GAP`], can't be selected. `hl` marks a highlighted row,
/// which the browser scrolls to on `recenter`; `cur` marks the cursor's row.
pub(super) fn row_html(panel: &Panel, row: &PanelRow, line: &Line) -> String {
    let highlighted = !row.comment
        && panel
            .highlight
            .is_some_and(|(lo, hi)| (lo..=hi).contains(&row.line));
    // `panel_lines` marks the cursor's row with a background; `.cur` paints it, so `.hl` wins.
    let cur = line.style.bg.is_some();
    let change = change_attr(row.change);
    let mut h = String::with_capacity(160);
    let _ = write!(
        h,
        r#"<div class="row{}{}{}" data-line="{}" data-row="{}"{change}{}>{GAP}"#,
        // Only removed-lines rows reach here as comment rows: the browser draws the cards.
        if row.comment { " gap" } else { "" },
        if highlighted { " hl" } else { "" },
        if cur { " cur" } else { "" },
        panel.row_source_line(row),
        row.line,
        style_attr(Style {
            bg: None,
            ..line.style
        })
    );
    let gutter = GUTTER_SPANS.min(line.spans.len());
    spans_html(&mut h, &line.spans[..gutter]);
    h.push_str("</span>");
    spans_html(&mut h, &line.spans[gutter..]);
    h.push_str("</div>");
    h
}

/// A row's or a block's tint, which `style.css` paints.
fn change_attr(change: Option<ChangeKind>) -> &'static str {
    match change {
        Some(ChangeKind::Added) => r#" data-change="added""#,
        Some(ChangeKind::Modified) => r#" data-change="modified""#,
        Some(ChangeKind::Removed(_)) => r#" data-change="removed""#,
        None => "",
    }
}

/// A markdown pane as one unit per rendered block (#89): the removed-lines rows inside it go above
/// it; the browser draws the thread cards below. A block's `data-line` is its first source line. Each of the
/// block's [`Block::stops`] carries its own `data-line`. An `hl` block's `data-at` places the
/// highlight within it, as a fraction of its rows. A block's `data-row` starts
/// with `b`, so copy never joins it to a row. `base` is the pane's file route URL for the file's
/// folder, which a relative image's `src` starts with.
pub(super) fn block_units(
    panel: &Panel,
    layout: &PanelLayout,
    lines: &[Line],
    focused: bool,
    base: &str,
) -> (Vec<String>, Option<usize>) {
    let rows = |span: Range<usize>, units: &mut Vec<String>, cursor: &mut Option<usize>| {
        for j in span {
            let row = &layout.rows[j];
            // A thread's card rows: the browser draws the card from `threads`.
            if row.comment && j > layout.starts[row.line] && !layout.eof.contains(&j) {
                continue;
            }
            if !row.comment && row.line == panel.cursor {
                *cursor = Some(units.len());
            }
            units.push(row_html(panel, row, &lines[j]));
        }
    };
    let mut units = vec![];
    let mut cursor = None;
    let mut next = 0;
    for b in panel.blocks() {
        let Some(&first) = layout.starts.get(b.rows.start) else {
            continue;
        };
        rows(next..first, &mut units, &mut cursor);
        // The EOF gap tags the last line but shows below the last block, from the tail `rows` call.
        let end = layout.rows[first..]
            .iter()
            .position(|r| r.line >= b.rows.end)
            .map_or(layout.rows.len(), |k| first + k)
            .min(layout.eof.start);
        let mut gutter: Vec<Span> = lines[first]
            .spans
            .iter()
            .take(GUTTER_SPANS)
            .cloned()
            .collect();
        let mut change: Option<ChangeKind> = None;
        for (i, (row, line)) in layout.rows[first..end]
            .iter()
            .zip(&lines[first..end])
            .enumerate()
        {
            if !row.comment {
                // The highlighted number shows if any row of the block has one.
                if panel
                    .highlight
                    .is_some_and(|(lo, hi)| (lo..=hi).contains(&row.line))
                    && let (Some(n), Some(s)) = (gutter.get_mut(0), line.spans.first())
                {
                    n.style = s.style;
                }
                // The block takes one tint, its rows' highest-ranked change.
                change = std::cmp::max_by_key(change, row.change, |k| k.map(ChangeKind::rank));
            } else if first + i > layout.starts[row.line] {
                continue;
            } else {
                units.push(row_html(panel, row, line));
            }
        }
        let md = mark_cursor_stop(panel, b, b.html().replace('\0', base));
        let has_cursor = b.rows.contains(&panel.cursor);
        let cur = focused && has_cursor && !on_card(panel, b) && !on_stop(panel, b);
        let hl = panel
            .highlight
            .is_some_and(|(lo, hi)| lo < b.rows.end && hi >= b.rows.start);
        let cls = if cur {
            " cur"
        } else if panel.highlight.is_some() && !hl {
            " dim"
        } else {
            ""
        };
        if has_cursor {
            cursor = Some(units.len());
        }
        // The highlight's midpoint, so the browser centers it rather than the whole block.
        let at = match panel.highlight {
            Some((lo, hi)) if hl => {
                let mid = (lo.max(b.rows.start) + hi.min(b.rows.end - 1)) as f64 / 2.0 + 0.5;
                let at = (mid - b.rows.start as f64) / b.rows.len() as f64;
                format!(r#" data-at="{at:.3}""#)
            }
            _ => String::new(),
        };
        let mut h = String::with_capacity(md.len() + 200);
        let _ = write!(
            h,
            r#"<div class="blk{cls}{}" data-line="{}" data-row="b{}"{at}{}>{GAP}"#,
            if hl { " hl" } else { "" },
            panel.row_source_line(&layout.rows[first]),
            b.rows.start,
            change_attr(change),
        );
        spans_html(&mut h, &gutter);
        h.push_str(r#"</span><div class="md">"#);
        h.push_str(&md);
        h.push_str("</div></div>");
        units.push(h);
        next = end;
    }
    rows(next..layout.rows.len(), &mut units, &mut cursor);
    (units, cursor)
}

/// `md`, a block's HTML, with `class="cur"` on the one of the block's [`Block::stops`] that has
/// the cursor. Marked focused or not, so the browser's scroll target doesn't change with focus.
/// The file's text can't fake ` data-line="N">`: every `>` in it is escaped.
fn mark_cursor_stop(panel: &Panel, b: &Block, md: String) -> String {
    let Some(&(_, n)) = b.stops().iter().find(|&&(row, _)| row == panel.cursor) else {
        return md;
    };
    md.replacen(
        &format!(r#" data-line="{n}">"#),
        &format!(r#" data-line="{n}" class="cur">"#),
        1,
    )
}

fn spans_html(h: &mut String, spans: &[Span]) {
    for s in spans {
        let _ = write!(h, "<span{}>", style_attr(s.style));
        escape(h, &s.content);
        h.push_str("</span>");
    }
}

fn style_attr(st: Style) -> String {
    let m = st.add_modifier;
    let (mut fg, mut bg) = (st.fg.and_then(color), st.bg.and_then(color));
    if m.contains(Modifier::REVERSED) {
        (fg, bg) = (
            Some(bg.unwrap_or("var(--pane)".into())),
            Some(fg.unwrap_or("var(--fg)".into())),
        );
    }
    let mut css = String::new();
    if let Some(c) = fg {
        let _ = write!(css, "color:{c};");
    }
    if let Some(c) = bg {
        let _ = write!(css, "background:{c};");
    }
    for (flag, rule) in [
        (Modifier::BOLD, "font-weight:bold;"),
        (Modifier::DIM, "opacity:.5;"),
        (Modifier::ITALIC, "font-style:italic;"),
        (Modifier::UNDERLINED, "text-decoration:underline;"),
    ] {
        if m.contains(flag) {
            css.push_str(rule);
        }
    }
    if css.is_empty() {
        css
    } else {
        format!(r#" style="{css}""#)
    }
}

/// The RGB colors Laura's file pane rows carry, made for a dark terminal, and what the light web
/// app shows instead: the Nord theme's code colors, then `view.rs`' and `render.rs`' own.
const LIGHT: &[([u8; 3], &str)] = &[
    ([0x2e, 0x34, 0x40], "#1d1f24"), // nord0
    ([0x3b, 0x42, 0x52], "#f2f3f6"), // nord1
    ([0x4c, 0x56, 0x6a], "#a0a4ad"), // nord3
    ([0xd8, 0xde, 0xe9], "#1d1f24"), // nord4: code text, a highlighted row's number
    ([0xe5, 0xe9, 0xf0], "#1d1f24"), // nord5
    ([0xec, 0xef, 0xf4], "#1d1f24"), // nord6
    ([0x8f, 0xbc, 0xbb], "#953800"), // nord7: types
    ([0x88, 0xc0, 0xd0], "#0550ae"), // nord8: functions
    ([0x81, 0xa1, 0xc1], "#cf222e"), // nord9: keywords
    ([0x5e, 0x81, 0xac], "#0550ae"), // nord10
    ([0xbf, 0x61, 0x6a], "#bf3a32"), // nord11
    ([0xd0, 0x87, 0x70], "#953800"), // nord12
    ([0xeb, 0xcb, 0x8b], "#9a5a09"), // nord13: warnings
    ([0xa3, 0xbe, 0x8c], "#1d7d4c"), // nord14: strings
    ([0xb4, 0x8e, 0xad], "#8250df"), // nord15: numbers
    ([0x61, 0x6e, 0x88], "#6e7781"), // the Nord theme's comments
    ([0x38, 0xa6, 0x60], "#1d7d4c"), // view.rs ADDED
    ([0xb3, 0x59, 0x6b], "#bf3a32"), // view.rs REMOVED
    ([0x52, 0x5c, 0x6e], "#e2e4f6"), // view.rs CODE_ON_BAND
    ([0x2d, 0x2d, 0x2d], "#f2f3f6"), // render.rs CODE_BG
    ([0xab, 0xb2, 0xbf], "#1d1f24"), // a comment card's text
    ([0xb4, 0xb4, 0xb4], "#1d1f24"), // inline code
    ([0x78, 0xb4, 0xf0], "#4f5bd5"), // links
    ([0xe0, 0x6c, 0x75], "#bf3a32"), // the diff view's removed text
];

/// RGB as hex, through [`LIGHT`]; the 16 ANSI colors as `style.css`'s palette.
fn color(c: Color) -> Option<String> {
    let ansi = |i: u8| Some(format!("var(--ansi-{i})"));
    match c {
        Color::Reset => None,
        Color::Rgb(r, g, b) => Some(
            LIGHT
                .iter()
                .find(|(rgb, _)| *rgb == [r, g, b])
                .map_or_else(|| format!("#{r:02x}{g:02x}{b:02x}"), |(_, l)| l.to_string()),
        ),
        Color::Indexed(i) if i < 16 => ansi(i),
        // ponytail: Laura's own styles use RGB or the 16 colors; map the 256-color cube if one appears.
        Color::Indexed(_) => None,
        Color::Black => ansi(0),
        Color::Red => ansi(1),
        Color::Green => ansi(2),
        Color::Yellow => ansi(3),
        Color::Blue => ansi(4),
        Color::Magenta => ansi(5),
        Color::Cyan => ansi(6),
        Color::Gray => ansi(7),
        Color::DarkGray => ansi(8),
        Color::LightRed => ansi(9),
        Color::LightGreen => ansi(10),
        Color::LightYellow => ansi(11),
        Color::LightBlue => ansi(12),
        Color::LightMagenta => ansi(13),
        Color::LightCyan => ansi(14),
        Color::White => ansi(15),
    }
}
