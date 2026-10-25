//! `/` slash-command query parsing + filtering for the agent composer.
//!
//! Commands come from ACP `available_commands_update` (Claude Agent, …).

/// Cap so a long agent catalog stays scrollable but small.
pub const MAX_SLASH_HITS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashQuery {
    /// Byte offset of `/` in the composer text.
    pub at: usize,
    /// Text after `/` up to the cursor (no whitespace).
    pub query: String,
}

/// Active `/command` just before `cursor` (UTF-8 byte offset), if any.
pub fn slash_at_cursor(text: &str, cursor: usize) -> Option<SlashQuery> {
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];
    let at = before.rfind('/')?;
    if at > 0 {
        let prev = before[..at].chars().next_back()?;
        if !prev.is_whitespace() {
            return None;
        }
    }
    let query = &before[at + 1..];
    if query.chars().any(|c| c.is_whitespace() || c == '\n') {
        return None;
    }
    Some(SlashQuery {
        at,
        query: query.to_owned(),
    })
}

/// Filter advertised commands by name/description; order preserved, capped.
pub fn filter_commands<'a, T>(
    commands: &'a [T],
    query: &str,
    limit: usize,
    name: impl Fn(&T) -> &str,
    description: impl Fn(&T) -> &str,
) -> Vec<&'a T> {
    let q = query.trim().trim_start_matches('/').to_ascii_lowercase();
    let mut out = Vec::new();
    for cmd in commands {
        let n = name(cmd);
        let d = description(cmd);
        let n_l = n.to_ascii_lowercase();
        let d_l = d.to_ascii_lowercase();
        let hit = if q.is_empty() {
            true
        } else {
            n_l.contains(&q) || d_l.contains(&q)
        };
        if hit {
            out.push(cmd);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

/// Replace `/query` at `at..cursor` with `insert`; returns new text + caret.
pub fn replace_slash(text: &str, at: usize, cursor: usize, insert: &str) -> (String, usize) {
    let cursor = cursor.min(text.len());
    let at = at.min(cursor);
    let mut out = String::with_capacity(text.len() + insert.len());
    out.push_str(&text[..at]);
    out.push_str(insert);
    out.push_str(&text[cursor..]);
    (out, at + insert.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_at_start_and_after_space() {
        let q = slash_at_cursor("/com", 4).unwrap();
        assert_eq!(q.at, 0);
        assert_eq!(q.query, "com");
        let q = slash_at_cursor("hi /help", 8).unwrap();
        assert_eq!(q.at, 3);
        assert_eq!(q.query, "help");
    }

    #[test]
    fn slash_ignores_urls_and_paths() {
        assert!(slash_at_cursor("https://x", 9).is_none());
        assert!(slash_at_cursor("a/b", 3).is_none());
    }

    #[test]
    fn filter_matches_name_or_description() {
        struct C {
            name: &'static str,
            description: &'static str,
        }
        let cmds = [
            C {
                name: "compact",
                description: "Clear context",
            },
            C {
                name: "help",
                description: "Show help",
            },
            C {
                name: "cost",
                description: "Token usage",
            },
        ];
        let hits = filter_commands(&cmds, "hel", 12, |c| c.name, |c| c.description);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "help");
        let hits = filter_commands(&cmds, "token", 12, |c| c.name, |c| c.description);
        assert_eq!(hits[0].name, "cost");
        let hits = filter_commands(&cmds, "", 12, |c| c.name, |c| c.description);
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn replace_slash_inserts_command() {
        let (out, caret) = replace_slash("/com", 0, 4, "/compact ");
        assert_eq!(out, "/compact ");
        assert_eq!(caret, "/compact ".len());
    }
}
