//! Line-level ranges for side-by-side diff fills + overview ruler.

use similar::{ChangeTag, TextDiff};

/// One overview-ruler tick as fractions of the document height (`0..=1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverviewMark {
    pub top: f32,
    pub height: f32,
}

/// UTF-8 byte ranges to fill on the old / new sides (deleted / inserted lines).
pub fn line_highlight_ranges(old: &str, new: &str) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let diff = TextDiff::from_lines(old, new);
    let mut old_ranges = Vec::new();
    let mut new_ranges = Vec::new();
    let mut old_off = 0usize;
    let mut new_off = 0usize;
    for change in diff.iter_all_changes() {
        let len = change.value().len();
        match change.tag() {
            ChangeTag::Equal => {
                old_off += len;
                new_off += len;
            }
            ChangeTag::Delete => {
                if len > 0 {
                    old_ranges.push((old_off, old_off + len));
                }
                old_off += len;
            }
            ChangeTag::Insert => {
                if len > 0 {
                    new_ranges.push((new_off, new_off + len));
                }
                new_off += len;
            }
        }
    }
    debug_assert_eq!(old_off, old.len(), "old offset must cover full text");
    debug_assert_eq!(new_off, new.len(), "new offset must cover full text");
    (old_ranges, new_ranges)
}

/// Map byte ranges to overview-ruler marks (document-relative vertical fractions).
///
/// Adjacent ranges merge. If marks would cover (almost) the whole document
/// (e.g. fully added / deleted file), return none — a full-track tick is noise
/// next to the line fills on both conflict and regular diffs.
pub fn overview_marks(text: &str, ranges: &[(usize, usize)]) -> Vec<OverviewMark> {
    let line_count = line_count(text).max(1) as f32;
    let min_h = (1.0 / line_count).max(0.004);
    let mut marks: Vec<OverviewMark> = ranges
        .iter()
        .filter_map(|&(start, end)| {
            if start >= end || start > text.len() {
                return None;
            }
            let end = end.min(text.len());
            let start_line = offset_to_line(text, start);
            let end_line = offset_to_line(text, end.saturating_sub(1)).saturating_add(1);
            let top = (start_line as f32 / line_count).clamp(0.0, 1.0);
            let height = ((end_line - start_line) as f32 / line_count).max(min_h);
            Some(OverviewMark {
                top,
                height: height.min(1.0 - top),
            })
        })
        .collect();
    marks = merge_overview_marks(marks);
    let covered: f32 = marks.iter().map(|m| m.height).sum();
    if covered >= 0.98 {
        return Vec::new();
    }
    marks
}

fn merge_overview_marks(mut marks: Vec<OverviewMark>) -> Vec<OverviewMark> {
    if marks.len() <= 1 {
        return marks;
    }
    marks.sort_by(|a, b| a.top.partial_cmp(&b.top).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::with_capacity(marks.len());
    out.push(marks[0]);
    for m in marks.into_iter().skip(1) {
        let last = out.last_mut().expect("out non-empty");
        let last_end = last.top + last.height;
        if m.top <= last_end + 0.002 {
            let end = (m.top + m.height).max(last_end);
            last.height = (end - last.top).min(1.0 - last.top);
        } else {
            out.push(m);
        }
    }
    out
}

fn line_count(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    // Count lines the same way editors do: trailing `\n` still ends a line.
    let mut n = 1usize;
    for b in text.bytes() {
        if b == b'\n' {
            n += 1;
        }
    }
    if text.ends_with('\n') {
        n -= 1;
    }
    n.max(1)
}

fn offset_to_line(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_changed_lines_only() {
        let old = "a\nb\nc\n";
        let new = "a\nB\nc\n";
        let (o, n) = line_highlight_ranges(old, new);
        assert_eq!(o, vec![(2, 4)]); // "b\n"
        assert_eq!(n, vec![(2, 4)]); // "B\n"
    }

    #[test]
    fn untracked_is_all_insert() {
        let (o, n) = line_highlight_ranges("", "x\ny\n");
        assert!(o.is_empty());
        assert_eq!(n, vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn deleted_is_all_delete() {
        let (o, n) = line_highlight_ranges("x\n", "");
        assert_eq!(o, vec![(0, 2)]);
        assert!(n.is_empty());
    }

    #[test]
    fn overview_mark_for_middle_line() {
        let text = "a\nb\nc\n";
        let marks = overview_marks(text, &[(2, 4)]);
        assert_eq!(marks.len(), 1);
        assert!((marks[0].top - 1.0 / 3.0).abs() < 0.01);
        assert!(marks[0].height > 0.0);
    }

    #[test]
    fn overview_skips_whole_document_change() {
        let text = "x\ny\n";
        let (o, n) = line_highlight_ranges("", text);
        assert!(o.is_empty());
        assert!(overview_marks(text, &n).is_empty());
    }

    #[test]
    fn overview_keeps_partial_hunk() {
        let mut lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        let old = lines.join("\n") + "\n";
        lines[20] = "changed".into();
        let new = lines.join("\n") + "\n";
        let (o, n) = line_highlight_ranges(&old, &new);
        let marks = overview_marks(&new, &n);
        assert_eq!(marks.len(), 1);
        assert!(marks[0].height < 0.1, "expected a short tick, got {}", marks[0].height);
        assert!(o.len() == 1 && n.len() == 1);
    }
}
