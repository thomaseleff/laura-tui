//! A terminal smaller than 40 × 10 shows one line in place of the workspace.

use laura::layout::workspace_fits;
use ratatui::layout::Rect;

#[test]
fn the_workspace_needs_40_columns_and_10_rows() {
    assert!(!workspace_fits(Rect::new(0, 0, 39, 10)));
    assert!(!workspace_fits(Rect::new(0, 0, 40, 9)));
    assert!(workspace_fits(Rect::new(0, 0, 40, 10)));
}
