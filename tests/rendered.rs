//! `.md` renders with real styling (bold, coloured+indented headings); code files (incl. `.html`) get syntax colour; other types stay raw. Through `Panel::open` + `Panel::layout`.

use std::io::Write;

use anyhow::Result;
use laura::Panel;
use ratatui::style::{Color, Modifier};
use tempfile::Builder;

fn open(suffix: &str, body: &str) -> Result<Panel> {
    let mut file = Builder::new().suffix(suffix).tempfile()?;
    write!(file, "{body}")?;
    file.flush()?;
    let panel = Panel::open(file.path().to_str().unwrap().to_string());
    drop(file);
    Ok(panel)
}

// H1 blurple from LauraStyleSheet's heading ramp.
const H1: Color = Color::Rgb(96, 130, 246);

#[test]
fn html_is_kept_verbatim_and_syntax_coloured() -> Result<()> {
    // HTML opens like a code file: tags preserved 1:1, syntect colour on the source.
    let panel = open(".html", "<h1>Title</h1>\n<p>body text</p>\n")?;
    assert_eq!(
        panel.content, "<h1>Title</h1>\n<p>body text</p>\n",
        "source must be preserved verbatim: {:?}",
        panel.content
    );
    let coloured = panel
        .layout(80)
        .rows
        .iter()
        .flat_map(|r| r.spans.clone())
        .any(|s| matches!(s.style.fg, Some(Color::Rgb(..))));
    assert!(coloured, "html source should get syntax colour");
    Ok(())
}

#[test]
fn markdown_content_is_plain_text() -> Result<()> {
    // The plain-text projection (what reviews quote) drops inline markup.
    let content = open(".md", "# Title\n\nsome **bold** text")?.content;
    assert!(
        content.contains("Title"),
        "missing heading text: {content:?}"
    );
    assert!(content.contains("bold"), "missing bold text: {content:?}");
    assert!(!content.contains("**"), "raw emphasis leaked: {content:?}");
    assert!(!content.contains("<h1>"), "html leaked: {content:?}");
    Ok(())
}

#[test]
fn markdown_bold_renders_as_bold_spans() -> Result<()> {
    let panel = open(".md", "# Title\n\nsome **bold** text")?;
    // Wide width so nothing wraps; find the span carrying "bold".
    let bold = panel
        .layout(80)
        .rows
        .iter()
        .flat_map(|r| r.spans.clone())
        .find(|s| s.content.as_ref() == "bold")
        .expect("a span whose text is exactly the emphasised word");
    assert!(
        bold.style.add_modifier.contains(Modifier::BOLD),
        "bold word should carry the BOLD modifier: {:?}",
        bold.style
    );
    Ok(())
}

#[test]
fn markdown_headings_are_coloured_and_indented() -> Result<()> {
    // H1 flush + gold/bold, literal `#` marker; H2 section indented two columns.
    let panel = open(".md", "# Top\n\n## Sub\n\nbody")?;
    let rows = panel.layout(80).rows;

    let h1 = rows
        .iter()
        .find(|r| r.text().contains("Top"))
        .expect("H1 row");
    assert!(
        h1.text().starts_with("# Top"),
        "H1 keeps its literal marker, no indent: {:?}",
        h1.text()
    );
    assert!(
        h1.spans
            .iter()
            .any(|s| s.style.fg == Some(H1) && s.style.add_modifier.contains(Modifier::BOLD)),
        "H1 should be gold + bold"
    );

    let h2 = rows
        .iter()
        .find(|r| r.text().contains("Sub"))
        .expect("H2 row");
    assert!(
        h2.text().starts_with("  ## Sub"),
        "H2 section indented two columns with its literal marker: {:?}",
        h2.text()
    );
    Ok(())
}

#[test]
fn plain_text_stays_raw() -> Result<()> {
    let content = open(".txt", "# not a heading\n**not bold**")?.content;
    assert_eq!(content, "# not a heading\n**not bold**");
    Ok(())
}

#[test]
fn code_file_is_syntax_coloured_fg_only() -> Result<()> {
    // Rust `fn` picks up a Nord foreground; source preserved verbatim, no span sets a background.
    let panel = open(".rs", "fn main() {\n    let x = 1;\n}\n")?;
    assert_eq!(
        panel.content, "fn main() {\n    let x = 1;\n}\n",
        "source must be preserved verbatim: {:?}",
        panel.content
    );

    let rows = panel.layout(80).rows;
    let spans: Vec<_> = rows.iter().flat_map(|r| r.spans.clone()).collect();

    let kw = spans
        .iter()
        .find(|s| s.content.as_ref() == "fn")
        .expect("a span whose text is exactly the `fn` keyword");
    assert!(
        matches!(kw.style.fg, Some(Color::Rgb(..))),
        "keyword should carry a Nord foreground colour: {:?}",
        kw.style
    );
    assert!(
        spans.iter().all(|s| s.style.bg.is_none()),
        "no code span should set a background — highlighting is foreground-only"
    );
    Ok(())
}

#[test]
fn leading_indentation_survives_wrapping() -> Result<()> {
    // Indented text must keep its leading whitespace (word-wrap used to drop it).
    let panel = open(".rs", "fn main() {\n    let x = 1;\n}\n")?;
    let indented = panel
        .layout(80)
        .rows
        .iter()
        .find(|r| r.text().contains("let x"))
        .map(|r| r.text())
        .expect("the indented body line");
    assert!(
        indented.starts_with("    let x"),
        "leading indent should be preserved: {indented:?}"
    );
    Ok(())
}

#[test]
fn unknown_extension_stays_raw() -> Result<()> {
    // No bundled syntax for `.zzz` → plain, unstyled text.
    let panel = open(".zzz", "keyword fn let const")?;
    assert_eq!(panel.content, "keyword fn let const");
    let styled = panel
        .layout(80)
        .rows
        .iter()
        .flat_map(|r| r.spans.clone())
        .all(|s| s.style.fg.is_none());
    assert!(styled, "unknown types must not be coloured");
    Ok(())
}
