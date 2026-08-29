//! Pure split-tree ops: `split` nests, `rects` geometry is exact for nested splits, `remove` collapses to the sibling, `remove(PTY)` errors.

use laura::protocol::{Dir, Side};
use laura::{Layout, PTY_PANE, Rect, rects};

#[test]
fn split_nests_and_orders() {
    let mut l = Layout::Pane(PTY_PANE);
    l.split(PTY_PANE, Dir::Horizontal, 40, Side::Second, 1)
        .unwrap();
    l.split(1, Dir::Vertical, 30, Side::Second, 2).unwrap();

    assert_eq!(l.order(), vec![0, 1, 2]);
    assert!(l.contains(0) && l.contains(1) && l.contains(2));

    // Splitting an absent pane errors.
    assert!(l.split(99, Dir::Horizontal, 50, Side::Second, 3).is_err());
}

#[test]
fn rects_geometry_is_exact() {
    let mut l = Layout::Pane(PTY_PANE);
    l.split(PTY_PANE, Dir::Horizontal, 40, Side::Second, 1)
        .unwrap();
    let area = Rect::new(0, 0, 100, 40);
    let map = rects(&l, area);

    assert_eq!(map[&0], Rect::new(0, 0, 40, 40), "first = 40%");
    assert_eq!(map[&1], Rect::new(40, 0, 60, 40), "second = remainder");

    // Nest a vertical split into pane 1; it partitions pane 1's rect only.
    l.split(1, Dir::Vertical, 25, Side::Second, 2).unwrap();
    let map = rects(&l, area);
    assert_eq!(map[&0], Rect::new(0, 0, 40, 40));
    assert_eq!(map[&1], Rect::new(40, 0, 60, 10), "top 25% of pane 1");
    assert_eq!(map[&2], Rect::new(40, 10, 60, 30), "bottom remainder");
}

#[test]
fn remove_collapses_to_sibling() {
    let mut l = Layout::Pane(PTY_PANE);
    l.split(PTY_PANE, Dir::Horizontal, 50, Side::Second, 1)
        .unwrap();
    l.split(1, Dir::Horizontal, 50, Side::Second, 2).unwrap();
    assert_eq!(l.order(), vec![0, 1, 2]);

    l.remove(1).unwrap();
    assert_eq!(l.order(), vec![0, 2], "sibling subtree replaces the split");
    assert!(!l.contains(1));

    // The PTY can't be closed.
    assert!(l.remove(PTY_PANE).is_err());
    // Removing an absent pane errors.
    assert!(l.remove(99).is_err());
}
