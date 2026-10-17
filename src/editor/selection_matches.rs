//! VS Code–style selection matches: other occurrences of the selected text
//! get a soft fill so you can see every copy in the buffer.

use std::ops::Range;

/// Skip huge / empty / whitespace-only selections (same spirit as VS Code).
const MAX_NEEDLE_BYTES: usize = 200;
const MAX_MATCHES: usize = 500;
/// Don't materialize / scan buffers bigger than this (UI-thread cost).
pub const MAX_HAYSTACK_BYTES: usize = 512 * 1024;

/// Whether this selection should drive occurrence highlights.
pub fn should_highlight(needle: &str) -> bool {
    if needle.is_empty() || needle.len() > MAX_NEEDLE_BYTES {
        return false;
    }
    !needle.chars().all(char::is_whitespace)
}

/// Exact (case-sensitive) occurrences of `needle` in `haystack`, excluding the
/// primary selection range. Non-overlapping left-to-right scan.
pub fn find_selection_matches(
    haystack: &str,
    needle: &str,
    primary: Range<usize>,
) -> Vec<Range<usize>> {
    if !should_highlight(needle) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut from = 0;
    while from <= haystack.len() {
        if !haystack.is_char_boundary(from) {
            from += 1;
            continue;
        }
        let Some(rel) = haystack[from..].find(needle) else {
            break;
        };
        let start = from + rel;
        let end = start + needle.len();
        if !haystack.is_char_boundary(end) {
            break;
        }
        if start != primary.start || end != primary.end {
            out.push(start..end);
            if out.len() >= MAX_MATCHES {
                break;
            }
        }
        // Advance by needle length so we don't re-match the same span.
        from = end.max(start + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_other_occurrences_not_primary() {
        let text = "foo bar foo baz foo";
        // Primary is the middle `foo` at 8..11.
        let matches = find_selection_matches(text, "foo", 8..11);
        assert_eq!(matches, vec![0..3, 16..19]);
    }

    #[test]
    fn empty_or_whitespace_skipped() {
        assert!(find_selection_matches("a a", "", 0..0).is_empty());
        assert!(find_selection_matches("a   a", "   ", 1..4).is_empty());
    }

    #[test]
    fn case_sensitive() {
        let text = "Foo foo FOO";
        let matches = find_selection_matches(text, "foo", 4..7);
        assert!(matches.is_empty());
    }
}
