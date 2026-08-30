//! The plain-text projection and row/indent structure that `L<n>` and scroll read off — verbatim for code, markup-stripped for `.md`, heading-indented. Through `Panel::open` + `Panel::layout`. Styling (colour/bold/bg) is display-only pixels, not asserted here.

use std::io::Write;

use anyhow::Result;
use laura::Panel;
use tempfile::Builder;

fn open(suffix: &str, body: &str) -> Result<Panel> {
    let mut file = Builder::new().suffix(suffix).tempfile()?;
    write!(file, "{body}")?;
    file.flush()?;
    let panel = Panel::open(file.path().to_str().unwrap().to_string());
    drop(file);
    Ok(panel)
}

#[test]
fn html_is_kept_verbatim() -> Result<()> {
    // HTML opens like a code file: source preserved 1:1 in the plain-text projection.
    let panel = open(".html", "<h1>Title</h1>\n<p>body text</p>\n")?;
    assert_eq!(
        panel.content, "<h1>Title</h1>\n<p>body text</p>\n",
        "source must be preserved verbatim: {:?}",
        panel.content
    );
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
fn markdown_headings_are_indented() -> Result<()> {
    // H1 flush with its literal `#` marker; H2 section indented two columns — the row structure scroll reads.
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
fn code_file_is_kept_verbatim() -> Result<()> {
    // Source is preserved verbatim in the projection reviews quote.
    let panel = open(".rs", "fn main() {\n    let x = 1;\n}\n")?;
    assert_eq!(
        panel.content, "fn main() {\n    let x = 1;\n}\n",
        "source must be preserved verbatim: {:?}",
        panel.content
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
fn diff_projection_is_verbatim_and_colored() -> Result<()> {
    use ratatui::style::Color;

    let src = "@@ -1 +1 @@\n-    old = 1\n+    new = 2\n";
    let panel = open(".diff", src)?;
    // Source preserved verbatim for `L<n>` quoting.
    assert_eq!(panel.content, src, "diff source must be verbatim");

    // Add/remove lines carry green/red foreground (styling a user can see).
    let rows = panel.layout(80).rows;
    let color = |needle: &str| {
        rows.iter()
            .find(|r| r.text().contains(needle))
            .and_then(|r| r.spans.iter().find(|s| !s.content.trim().is_empty()))
            .and_then(|s| s.style.fg)
    };
    assert_eq!(
        color("new = 2"),
        Some(Color::Rgb(163, 190, 140)),
        "addition is green"
    );
    assert_eq!(
        color("old = 1"),
        Some(Color::Rgb(191, 97, 106)),
        "removal is red"
    );
    Ok(())
}

#[test]
fn unknown_extension_stays_raw() -> Result<()> {
    // No bundled syntax for `.zzz` → plain text in the projection.
    let panel = open(".zzz", "keyword fn let const")?;
    assert_eq!(panel.content, "keyword fn let const");
    Ok(())
}
