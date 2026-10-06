//! Indent-guide helpers for the flat tree list (VS Code–style vertical lines).

use gpui_kit::component::tree::TreeState;

/// For each visible row, which ancestor columns still have a sibling below
/// (so a vertical guide should continue through this row).
pub fn compute_guide_masks(state: &TreeState) -> Vec<Vec<bool>> {
    let depths = visible_depths(state);
    depths
        .iter()
        .enumerate()
        .map(|(ix, &depth)| {
            (0..depth)
                .map(|col| guide_continues(&depths, ix, col))
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
}
