//! Per-file indentation detection (VS Code `detectIndentation` style).

use gpui_kit::component::input::TabSize;

const SCAN_LINES: usize = 1000;

/// Guess the indent of `text` from its first lines: tabs if most indented lines
/// start with a tab, else the most common step between consecutive space-indented
/// lines (2/4/8). `None` when nothing is indented (caller falls back to config).
///
/// ponytail: majority / delta-histogram heuristic, no `.editorconfig`; upgrade
/// path is to read `.editorconfig` before detecting.
pub fn detect_indent(text: &str) -> Option<TabSize> {
    let (mut tabs, mut spaces) = (0usize, 0usize);
    // Only 2 / 4 / 8 count: an odd step (6, 3…) is a messy file, not an indent style.
    let mut deltas = [0usize; 9];
    let mut prev = 0usize;
    for line in text.lines().take(SCAN_LINES) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tabs += 1;
            continue;
        }
        let width = line.len() - line.trim_start_matches(' ').len();
        if width > 0 {
            spaces += 1;
        }
        let delta = width.abs_diff(prev);
        if matches!(delta, 2 | 4 | 8) {
            deltas[delta] += 1;
        }
        prev = width;
    }
    if tabs > spaces {
        return Some(TabSize {
            tab_size: 4,
            hard_tabs: true,
        });
    }
    // First max wins, so ties prefer the smaller step.
    let (step, count) = (2..=8)
        .map(|d| (d, deltas[d]))
        .fold((0, 0), |best, cur| if cur.1 > best.1 { cur } else { best });
    (count > 0).then_some(TabSize {
        tab_size: step,
        hard_tabs: false,
    })
}

#[cfg(test)]
mod tests {
    use super::detect_indent;

    #[test]
    fn detects_spaces_tabs_and_none() {
        let two = detect_indent("a {\n  b {\n    c\n  }\n}\n").unwrap();
        assert_eq!((two.tab_size, two.hard_tabs), (2, false));
        let four = detect_indent("def f():\n    if x:\n        y\n").unwrap();
        assert_eq!((four.tab_size, four.hard_tabs), (4, false));
        let tab = detect_indent("func f() {\n\tif x {\n\t\ty\n\t}\n}\n").unwrap();
        assert!(tab.hard_tabs);
        assert!(detect_indent("a\nb\n").is_none());
        assert!(detect_indent("f {\n      x\n}\n").is_none(), "odd step is not a style");
        assert!(detect_indent("").is_none());
    }
}
