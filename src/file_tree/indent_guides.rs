//! Indent-guide helpers for the flat tree list (VS Code–style vertical lines).

use gpui_kit::component::tree::TreeState;

/// For each visible row, which ancestor columns should show a vertical guide.
///
/// Every ancestor column is drawn through every descendant — including the
/// last child — so the rail runs the full height of a folder's contents.
/// Multi-root only: column 0 stops when the next depth-0 row is another
/// workspace root (no rail linking roots).
pub fn compute_guide_masks(state: &TreeState) -> Vec<Vec<bool>> {
    let depths = visible_depths(state);
    let multi_root = depths.iter().filter(|&&d| d == 0).count() > 1;
    depths
        .iter()
        .enumerate()
        .map(|(ix, &depth)| {
            (0..depth)
                .map(|col| guide_active(&depths, ix, col, multi_root))
                .collect()
        })
        .collect()
}

fn visible_depths(state: &TreeState) -> Vec<usize> {
    let mut depths = Vec::new();
    let mut ix = 0;
    while let Some(entry) = state.entry(ix) {
        depths.push(entry.depth());
        ix += 1;
    }
    depths
}

fn guide_active(depths: &[usize], ix: usize, col: usize, multi_root: bool) -> bool {
    // Normal case: paint this ancestor column on every descendant row.
    if !(multi_root && col == 0) {
        return true;
    }
    // Multi-root col 0: keep the rail only while more content remains under
    // this root; a following depth-0 row is the next workspace folder.
    match depths.get(ix + 1).copied() {
        Some(0) | None => false,
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::guide_active;

    fn masks_for_depths(depths: &[usize]) -> Vec<Vec<bool>> {
        let multi_root = depths.iter().filter(|&&d| d == 0).count() > 1;
        depths
            .iter()
            .enumerate()
            .map(|(ix, &depth)| {
                (0..depth)
                    .map(|col| guide_active(depths, ix, col, multi_root))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn last_child_keeps_parent_guide() {
        // folder, a, b — both children get the parent rail (including last).
        let depths = [0, 1, 1];
        assert!(guide_active(&depths, 1, 0, false));
        assert!(guide_active(&depths, 2, 0, false));
    }

    #[test]
    fn nested_guides_run_full_depth() {
        // root, dir, file, file — innermost col on both files.
        let depths = [0, 1, 2, 2];
        assert_eq!(masks_for_depths(&depths)[2], vec![true, true]);
        assert_eq!(masks_for_depths(&depths)[3], vec![true, true]);
    }

    #[test]
    fn multi_root_skips_cross_root_keeps_nested() {
        // RootA, child, RootB, folder, nested, sibling under B.
        let depths = [0, 1, 0, 1, 2, 1];
        let masks = masks_for_depths(&depths);
        // Sole child under A: next is RootB → no col-0 rail.
        assert_eq!(masks[1], vec![false]);
        // Nested under B: ancestor cols on. Last sibling under B: col0 off (eof).
        assert_eq!(masks[4], vec![true, true]);
        assert_eq!(masks[5], vec![false]);
    }
}
