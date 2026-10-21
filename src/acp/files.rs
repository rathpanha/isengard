//! Helpers for the agent's `fs/read_text_file` requests.

/// Lines `line..line + limit` of `text` (`line` is 1-based, like ACP).
/// `None` for both returns the whole text unchanged.
pub fn slice_lines(text: &str, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return text.to_owned();
    }
    let start = line.unwrap_or(1).max(1) as usize - 1;
    let take = limit.map_or(usize::MAX, |n| n as usize);
    text.split_inclusive('\n').skip(start).take(take).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_text_without_range() {
        assert_eq!(slice_lines("a\nb\n", None, None), "a\nb\n");
    }

    #[test]
    fn one_based_start_and_limit() {
        let text = "one\ntwo\nthree\nfour";
        assert_eq!(slice_lines(text, Some(2), Some(2)), "two\nthree\n");
        assert_eq!(slice_lines(text, Some(3), None), "three\nfour");
        assert_eq!(slice_lines(text, None, Some(1)), "one\n");
        // Line 0 is treated as the first line; past-the-end is empty.
        assert_eq!(slice_lines(text, Some(0), Some(1)), "one\n");
        assert_eq!(slice_lines(text, Some(10), Some(1)), "");
    }
}
