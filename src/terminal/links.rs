//! Detect URLs under the terminal cursor for Ctrl/Cmd+click open.

/// Characters allowed inside a URL after the scheme (conservative; good enough
/// for terminal paste/`curl` output). Trailing punctuation is stripped later.
fn is_url_body(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '-' | '.'
                | '_'
                | '~'
                | ':'
                | '/'
                | '?'
                | '#'
                | '['
                | ']'
                | '@'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '%'
                | '='
        )
}

fn trim_trailing_punct(url: &str) -> &str {
    url.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}'])
}

fn looks_like_url(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("www.")
}

/// Column span of a URL on `line` covering character index `col` (0-based).
///
/// Returns `(start_col, end_col_exclusive, url)`. The URL may start with `www.`;
/// pass it through [`normalize_url`] before opening.
pub fn find_url_span_at(line: &str, col: usize) -> Option<(usize, usize, &str)> {
    if line.is_empty() {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let col = col.min(chars.len().saturating_sub(1));

    let mut start = col;
    while start > 0 && is_url_body(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end + 1 < chars.len() && is_url_body(chars[end + 1]) {
        end += 1;
    }
    end += 1; // exclusive

    let candidate: String = chars[start..end].iter().collect();
    let trimmed = trim_trailing_punct(&candidate);
    if !looks_like_url(trimmed) {
        return None;
    }
    // Shrink end so trailing punct is not part of the span / underline.
    let trim_chars = trimmed.chars().count();
    let end = start + trim_chars;
    let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
    let byte_len: usize = trimmed.chars().map(|c| c.len_utf8()).sum();
    let url = line.get(byte_start..byte_start + byte_len)?;
    Some((start, end, url))
}

/// Find a URL on `line` that covers character index `col` (0-based).
pub fn find_url_at(line: &str, col: usize) -> Option<&str> {
    find_url_span_at(line, col).map(|(_, _, url)| url)
}

/// Ensure `www.…` links get an `https://` scheme for the system opener.
pub fn normalize_url(url: &str) -> String {
    if url.to_ascii_lowercase().starts_with("www.") {
        format!("https://{url}")
    } else {
        url.to_string()
    }
}

/// Open `url` in the default browser (detached; does not block the UI).
pub fn open_url(url: &str) -> std::io::Result<()> {
    open::that_detached(normalize_url(url))
}

/// OS terminal copy/paste chords: macOS `⌘C`/`⌘V`, elsewhere `Ctrl+Shift+C`/`V`.
pub fn is_copy_shortcut(modifiers: &gpui_kit::Modifiers, key: &str) -> bool {
    if !key.eq_ignore_ascii_case("c") {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        modifiers.platform && !modifiers.control && !modifiers.alt && !modifiers.shift
    }
    #[cfg(not(target_os = "macos"))]
    {
        modifiers.control && modifiers.shift && !modifiers.alt && !modifiers.platform
    }
}

pub fn is_paste_shortcut(modifiers: &gpui_kit::Modifiers, key: &str) -> bool {
    if !key.eq_ignore_ascii_case("v") {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        modifiers.platform && !modifiers.control && !modifiers.alt && !modifiers.shift
    }
    #[cfg(not(target_os = "macos"))]
    {
        modifiers.control && modifiers.shift && !modifiers.alt && !modifiers.platform
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_https_under_cursor() {
        let line = "see https://example.com/path for docs";
        let col = line.find("example").unwrap();
        assert_eq!(find_url_at(line, col), Some("https://example.com/path"));
        let (start, end, url) = find_url_span_at(line, col).unwrap();
        assert_eq!(url, "https://example.com/path");
        assert_eq!(&line[start..end], url);
    }

    #[test]
    fn strips_trailing_punct() {
        let line = "go to https://example.com.";
        let col = line.find("https").unwrap();
        assert_eq!(find_url_at(line, col), Some("https://example.com"));
        let (start, end, _) = find_url_span_at(line, col).unwrap();
        assert_eq!(&line[start..end], "https://example.com");
    }

    #[test]
    fn www_normalizes() {
        assert_eq!(normalize_url("www.example.com"), "https://www.example.com");
        assert_eq!(
            normalize_url("https://example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn misses_plain_words() {
        assert!(find_url_at("hello world", 3).is_none());
    }
}
