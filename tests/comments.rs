//! Comments pin to lines in an open panel. Through `Panel::open` + cursor/comment methods.

use std::io::Write;

use anyhow::Result;
use laura::Panel;

#[test]
fn comments_pin_to_their_line() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    writeln!(file, "line 0\nline 1\nline 2\nline 3")?;
    file.flush()?;
    let path = file.path().to_str().unwrap().to_string();

    let mut panel = Panel::open(path);

    panel.move_cursor(2);
    panel.add_comment("first".into());
    panel.move_cursor(-1);
    panel.add_comment("second".into());

    // Stored against the right line with the right text; multiple coexist.
    assert!(panel.comments.contains(&(2, "first".into())));
    assert!(panel.comments.contains(&(1, "second".into())));
    assert_eq!(panel.comments.len(), 2);

    // Cursor clamps at both ends.
    panel.move_cursor(999);
    assert_eq!(panel.cursor, panel.line_count() - 1);
    panel.move_cursor(-999);
    assert_eq!(panel.cursor, 0);

    Ok(())
}
