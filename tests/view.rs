//! The frozen notice drops the file name to fit a narrow terminal.

use laura::view::frozen_notice;

#[test]
fn frozen_notice_shortens_on_a_narrow_terminal() {
    let short = frozen_notice("2026-09-23-protect-review-plan.md", 80);
    assert!(short.contains("Ctrl+R discards inline review"), "{short}");
    assert!(short.chars().count() + 3 <= 80, "{short}");
    assert!(frozen_notice("main.rs", 200).contains("changed on disk"));
}
