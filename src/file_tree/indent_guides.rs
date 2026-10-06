//! Indent-guide helpers for the flat tree list (VS Code–style vertical lines).

use gpui_kit::component::tree::TreeState;

/// For each visible row, which ancestor columns still have a sibling below
/// (so a vertical guide should continue through this row).
///
/// Multi-root workspaces skip column 0: that level is the workspace roots
/// themselves, and we do not draw a rail linking one root folder to another.
/// Guides start under each root (columns 1+).
pub fn compute_guide_masks(state: &TreeState) -> Vec<Vec<bool>> {
    let depths = visible_depths(state);
    let multi_root = depths.iter().filter(|&&d| d == 0).count() > 1;
    depths
        .iter()
        .enumerate()
        .map(|(ix, &depth)| {
            (0..depth)
                .map(|col| {
                    if multi_root && col == 0 {
                        false
                    } else {
                        guide_continues(&depths, ix, col)
                    }
                })
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

/// After row `ix`, is there another visible row at `col` before we rise above it?
fn guide_continues(depths: &[usize], ix: usize, col: usize) -> bool {
    for &depth in &depths[ix + 1..] {
        if depth < col {
            return false;
        }
        if depth == col {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::guide_continues;

    /// Same logic as [`super::compute_guide_masks`] without a `TreeState`.
    fn masks_for_depths(depths: &[usize]) -> Vec<Vec<bool>> {
        let multi_root = depths.iter().filter(|&&d| d == 0).count() > 1;
        depths
            .iter()
            .enumerate()
            .map(|(ix, &depth)| {
                (0..depth)
                    .map(|col| {
                        if multi_root && col == 0 {
                            false
                        } else {
                            guide_continues(depths, ix, col)
                        }
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn sibling_below_keeps_guide() {
        // Flat: folder a (expanded), child a0, then sibling b.
        // depths: a=0, a0=1, b=0
        let depths = vec![0, 1, 0];
        assert!(guide_continues(&depths, 1, 0)); // through a0, line to b
        assert!(!guide_continues(&depths, 2, 0));
        // Last child under a only: a, a0, a1 — no sibling after a0 at depth 0
        let depths = vec![0, 1, 1];
        assert!(!guide_continues(&depths, 1, 0)); // next is depth 1, then end → no depth 0
        assert!(!guide_continues(&depths, 2, 0));
    }

    #[test]
    fn multi_root_skips_workspace_rail_keeps_nested() {
        // RootA, nested under A, RootB, folder under B, child, sibling under B.
        // depths: 0, 1, 0, 1, 2, 1
        let depths = [0, 1, 0, 1, 2, 1];
        let masks = masks_for_depths(&depths);
        // Child of RootA: col 0 suppressed (no line to RootB).
        assert_eq!(masks[1], vec![false]);
        // Nested under RootB (ix=4, depth 2): no workspace rail; nested rail to sibling.
        assert_eq!(masks[4], vec![false, true]);
    }
}
