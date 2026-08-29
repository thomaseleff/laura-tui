//! A commented panel assembles a PR-style review wrapped in bracketed paste for injection. Through `assemble_review` + `bracketed_paste`, format-only — real-agent paste is a manual check.

use std::io::Write;

use anyhow::Result;
use laura::{Panel, bracketed_paste};

fn panel_with_comments() -> Result<Panel> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "line 0\nline 1\nline 2\nline 3")?;
    file.flush()?;
    let mut panel = Panel::open(file.path().to_str().unwrap().to_string());

    // Two comments on line 2, one on line 0 — out of order to prove sorting.
    panel.move_cursor(2);
    panel.add_comment("tighten this".into());
    panel.add_comment("and this".into());
    panel.move_cursor(-2);
    panel.add_comment("first line note".into());
    // Keep the temp file alive past `open` — content is already read in.
    drop(file);
    Ok(panel)
}

#[test]
fn assemble_review_locks_the_format() -> Result<()> {
    let panel = panel_with_comments()?;
    let expected = format!(
        "[laura review · {}]\n\
         \n\
         overall body\n\
         \n\
         L1  line 0\n\
         \x20     > first line note\n\
         \n\
         L3  line 2\n\
         \x20     > tighten this\n\
         \x20     > and this\n",
        panel.path
    );
    assert_eq!(panel.assemble_review("overall body"), expected);
    Ok(())
}

#[test]
fn empty_overall_omits_the_overall_line() -> Result<()> {
    let panel = panel_with_comments()?;
    let review = panel.assemble_review("");
    // Header sits directly above the first L<n>, with only the one blank line.
    assert!(review.starts_with(&format!("[laura review · {}]\n\nL1  line 0\n", panel.path)));
    assert!(!review.contains("overall"));
    Ok(())
}

#[test]
fn bracketed_paste_keeps_newlines_inside_the_markers() {
    let text = "line a\nline b\nline c";
    let wrapped = bracketed_paste(text);
    let s = String::from_utf8(wrapped).unwrap();

    assert!(
        s.starts_with("\x1b[200~"),
        "must open with paste-start marker"
    );
    assert!(
        s.ends_with("\x1b[201~\r"),
        "trailing \\r must sit outside the close marker"
    );

    // Every embedded newline lands between the open and close markers — no early submit.
    let open = s.find("\x1b[200~").unwrap();
    let close = s.find("\x1b[201~").unwrap();
    for (i, b) in s.bytes().enumerate() {
        if b == b'\n' {
            assert!(
                i > open && i < close,
                "newline at {i} escaped the paste markers"
            );
        }
    }
}
