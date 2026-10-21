//! Line-level ranges for side-by-side diff fills + overview ruler.

use std::ops::Range;

use similar::{ChangeTag, TextDiff};

/// One overview-ruler tick as fractions of the document height (`0..=1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverviewMark {
    pub top: f32,
    pub height: f32,
}

/// Which side of a [`ConflictHunk`] to keep when resolving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkSide {
    /// Current / ours (old pane).
    Old,
    /// Incoming / theirs (new pane).
    New,
}

/// One contiguous Current↔Incoming change (delete/insert run between equals).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictHunk {
    pub old_bytes: Range<usize>,
    pub new_bytes: Range<usize>,
    pub old_lines: Range<usize>,
    pub new_lines: Range<usize>,
}

/// Group line-diff changes into hunks (equal runs are boundaries).
pub fn conflict_hunks(old: &str, new: &str) -> Vec<ConflictHunk> {
    let diff = TextDiff::from_lines(old, new);
    let mut hunks = Vec::new();
    let mut old_off = 0usize;
    let mut new_off = 0usize;
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    let mut open: Option<HunkBuilder> = None;

    for change in diff.iter_all_changes() {
        let value = change.value();
        let len = value.len();
        let lines = count_lines_in_change(value);
        match change.tag() {
            ChangeTag::Equal => {
                if let Some(b) = open.take() {
                    hunks.push(b.finish(old_off, new_off, old_line, new_line));
                }
                old_off += len;
                new_off += len;
                old_line += lines;
                new_line += lines;
            }
            ChangeTag::Delete => {
                let b = open.get_or_insert_with(|| HunkBuilder::start(old_off, new_off, old_line, new_line));
                b.saw_change = true;
                old_off += len;
                old_line += lines;
            }
            ChangeTag::Insert => {
                let b = open.get_or_insert_with(|| HunkBuilder::start(old_off, new_off, old_line, new_line));
                b.saw_change = true;
                new_off += len;
                new_line += lines;
            }
        }
    }
    if let Some(b) = open.take() {
        hunks.push(b.finish(old_off, new_off, old_line, new_line));
    }
    debug_assert_eq!(old_off, old.len());
    debug_assert_eq!(new_off, new.len());
    hunks
}

/// Index of the hunk containing either pane's cursor line (`None` if on equal text).
#[cfg_attr(not(test), allow(dead_code))]
pub fn hunk_index_at_line(hunks: &[ConflictHunk], old_line: usize, new_line: usize) -> Option<usize> {
    hunks.iter().position(|h| {
        (!h.old_lines.is_empty() && h.old_lines.contains(&old_line))
            || (!h.new_lines.is_empty() && h.new_lines.contains(&new_line))
    })
}

/// Build the resolved file from per-hunk Old/New choices (one per hunk).
pub fn apply_hunk_choices(
    old: &str,
    new: &str,
    choices: &[HunkSide],
) -> Result<String, String> {
    let hunks = conflict_hunks(old, new);
    if choices.len() != hunks.len() {
        return Err(format!(
            "Hunk choice count {} != hunk count {}",
            choices.len(),
            hunks.len()
        ));
    }
    let diff = TextDiff::from_lines(old, new);
    let mut out = String::with_capacity(old.len().max(new.len()));
    let mut hunk_i = 0usize;
    let mut old_buf = String::new();
    let mut new_buf = String::new();
    let mut in_hunk = false;

    let flush = |out: &mut String,
                 old_buf: &mut String,
                 new_buf: &mut String,
                 hunk_i: &mut usize,
                 choices: &[HunkSide]| {
        let side = choices.get(*hunk_i).copied().unwrap_or(HunkSide::Old);
        match side {
            HunkSide::Old => out.push_str(old_buf),
            HunkSide::New => out.push_str(new_buf),
        }
        old_buf.clear();
        new_buf.clear();
        *hunk_i += 1;
    };

    for change in diff.iter_all_changes() {
        let value = change.value();
        match change.tag() {
            ChangeTag::Equal => {
                if in_hunk {
                    flush(&mut out, &mut old_buf, &mut new_buf, &mut hunk_i, choices);
                    in_hunk = false;
                }
                out.push_str(value);
            }
            ChangeTag::Delete => {
                in_hunk = true;
                old_buf.push_str(value);
            }
            ChangeTag::Insert => {
                in_hunk = true;
                new_buf.push_str(value);
            }
        }
    }
    if in_hunk {
        flush(&mut out, &mut old_buf, &mut new_buf, &mut hunk_i, choices);
    }
    if hunk_i != hunks.len() {
        return Err(format!(
            "Applied {hunk_i} hunks, expected {}",
            hunks.len()
        ));
    }
    Ok(out)
}

struct HunkBuilder {
    old_bytes_start: usize,
    new_bytes_start: usize,
    old_lines_start: usize,
    new_lines_start: usize,
    saw_change: bool,
}

impl HunkBuilder {
    fn start(old_off: usize, new_off: usize, old_line: usize, new_line: usize) -> Self {
        Self {
            old_bytes_start: old_off,
            new_bytes_start: new_off,
            old_lines_start: old_line,
            new_lines_start: new_line,
            saw_change: false,
        }
    }

    fn finish(
        self,
        old_off: usize,
        new_off: usize,
        old_line: usize,
        new_line: usize,
    ) -> ConflictHunk {
        debug_assert!(self.saw_change);
        ConflictHunk {
            old_bytes: self.old_bytes_start..old_off,
            new_bytes: self.new_bytes_start..new_off,
            old_lines: self.old_lines_start..old_line,
            new_lines: self.new_lines_start..new_line,
        }
    }
}

fn count_lines_in_change(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    let n = s.bytes().filter(|&b| b == b'\n').count();
    if s.ends_with('\n') { n } else { n + 1 }
}

/// Inserted / deleted line counts for a unified text change (agent file summary).
pub fn line_change_counts(old: &str, new: &str) -> (u32, u32) {
    let diff = TextDiff::from_lines(old, new);
    let mut added = 0u32;
    let mut removed = 0u32;
    for change in diff.iter_all_changes() {
        let n = count_lines_in_change(change.value()) as u32;
        match change.tag() {
            ChangeTag::Insert => added = added.saturating_add(n),
            ChangeTag::Delete => removed = removed.saturating_add(n),
            ChangeTag::Equal => {}
        }
    }
    (added, removed)
}

/// UTF-8 byte ranges (start, end) to fill on one side of a diff.
pub type ByteRanges = Vec<(usize, usize)>;

/// UTF-8 byte ranges to fill on the old / new sides (deleted / inserted lines).
pub fn line_highlight_ranges(old: &str, new: &str) -> (ByteRanges, ByteRanges) {
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
        assert_eq!(line_change_counts(old, new), (1, 1));
    }

    #[test]
    fn line_change_counts_create_and_delete() {
        assert_eq!(line_change_counts("", "x\ny\n"), (2, 0));
        assert_eq!(line_change_counts("x\ny\nz\n", ""), (0, 3));
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

    #[test]
    fn conflict_hunks_groups_adjacent_changes() {
        let old = "a\nb\nc\nd\n";
        let new = "a\nB\nC\nd\n";
        let hunks = conflict_hunks(old, new);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_lines, 1..3);
        assert_eq!(hunks[0].new_lines, 1..3);
        assert_eq!(hunk_index_at_line(&hunks, 1, 0), Some(0));
        assert_eq!(hunk_index_at_line(&hunks, 0, 0), None);
    }

    #[test]
    fn apply_hunk_choices_picks_per_hunk() {
        let old = "keep\nold1\nmid\nold2\nend\n";
        let new = "keep\nnew1\nmid\nnew2\nend\n";
        let hunks = conflict_hunks(old, new);
        assert_eq!(hunks.len(), 2);
        let merged = apply_hunk_choices(old, new, &[HunkSide::Old, HunkSide::New]).unwrap();
        assert_eq!(merged, "keep\nold1\nmid\nnew2\nend\n");
    }

    #[test]
    fn apply_insert_only_hunk() {
        let old = "a\nc\n";
        let new = "a\nb\nc\n";
        let hunks = conflict_hunks(old, new);
        assert_eq!(hunks.len(), 1);
        assert!(hunks[0].old_lines.is_empty());
        assert_eq!(
            apply_hunk_choices(old, new, &[HunkSide::New]).unwrap(),
            "a\nb\nc\n"
        );
        assert_eq!(
            apply_hunk_choices(old, new, &[HunkSide::Old]).unwrap(),
            "a\nc\n"
        );
    }
}
