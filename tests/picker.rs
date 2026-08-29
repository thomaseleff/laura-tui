//! Pure picker mapping: a 1-based positional keypress maps to a stable pane id, with gaps after closes.

use laura::protocol::{Dir, Side};
use laura::{Layout, PTY_PANE, pane_at};

#[test]
fn positional_label_maps_to_stable_id_with_gaps() {
    // Open panes 0..=4, then close 1, 2, 3 — leaving #0 and #4.
    let mut l = Layout::Pane(PTY_PANE);
    for id in 1..=4 {
        l.split(id - 1, Dir::Horizontal, 50, Side::Second, id)
            .unwrap();
    }
    for id in 1..=3 {
        l.remove(id).unwrap();
    }
    assert_eq!(l.order(), vec![0, 4]);

    // Positional labels are dense (1, 2, …) even though ids have gaps.
    assert_eq!(pane_at(&l, 1), Some(0));
    assert_eq!(pane_at(&l, 2), Some(4));
    assert_eq!(pane_at(&l, 3), None, "past the end");
    assert_eq!(pane_at(&l, 0), None, "labels are 1-based");
}
