//! A file-backed panel re-reads its source on disk change — no `update` call. Through `Panel::open` + `reload_if_changed`.

use std::io::{Seek, Write};

use anyhow::Result;
use laura::Panel;

#[test]
fn panel_reloads_when_source_changes() -> Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    write!(file, "v1")?;
    file.flush()?;
    let path = file.path().to_str().unwrap().to_string();

    let mut panel = Panel::open(path);
    assert_eq!(panel.content, "v1");

    // Overwrite with a different-length body; the (mtime, len) signature changes.
    file.as_file().set_len(0)?;
    file.rewind()?;
    write!(file, "v2!")?;
    file.flush()?;

    assert!(panel.reload_if_changed(), "changed source should reload");
    assert_eq!(panel.content, "v2!");

    // No change → no spurious reload.
    assert!(
        !panel.reload_if_changed(),
        "unchanged source should not reload"
    );
    Ok(())
}
