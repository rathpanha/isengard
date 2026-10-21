//! `@` mention query parsing + workspace file/folder listing for the composer.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// Soft caps so `@` in a huge repo stays snappy.
pub const MAX_MENTION_WALK: usize = 2_000;
/// Cap for fuzzy search only — root / browse listings show every child
/// (minus [`SKIP_DIRS`]).
pub const MAX_MENTION_HITS: usize = 8;
/// Safety ceiling when listing a single directory (root or dive-in).
pub const MAX_DIR_LISTING: usize = 200;

/// Build/deps dirs omitted from `@` even when they sit in the workspace root.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "__pycache__",
    "vendor",
    ".venv",
    "venv",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionQuery {
    /// Byte offset of `@` in the composer text.
    pub at: usize,
    /// Text after `@` up to the cursor (no whitespace).
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionHit {
    pub path: PathBuf,
    /// Path relative to a workspace root (or the file name).
    pub label: String,
    pub is_dir: bool,
}

/// Active `@mention` just before `cursor` (UTF-8 byte offset), if any.
pub fn mention_at_cursor(text: &str, cursor: usize) -> Option<MentionQuery> {
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];
    let at = before.rfind('@')?;
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
    Some(MentionQuery {
        at,
        query: query.to_owned(),
    })
}

/// Replace `@query` at `at..cursor` with nothing; returns new text + caret.
pub fn strip_mention(text: &str, at: usize, cursor: usize) -> (String, usize) {
    let cursor = cursor.min(text.len());
    let at = at.min(cursor);
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..at]);
    out.push_str(&text[cursor..]);
    (out, at)
}

/// Workspace files/folders matching `query` (case-insensitive substring on the
/// relative path). Empty query → **all** top-level entries under each root
/// (except [`SKIP_DIRS`]). A query that names an existing directory lists that
/// directory's children. Otherwise fuzzy-search, capped at `limit`.
pub fn list_mentions(roots: &[PathBuf], query: &str, limit: usize) -> Vec<MentionHit> {
    let limit = limit.max(1).min(MAX_MENTION_HITS);
    let query = query.trim_matches('/');
    let query_lower = query.to_ascii_lowercase();

    if query.is_empty() {
        // No 8-hit trim here — root browse must show every folder/file.
        return list_root_children(roots, MAX_DIR_LISTING);
    }

    if let Some((root, dir)) = resolve_dir(roots, query) {
        return list_dir_children(&root, &dir, MAX_DIR_LISTING);
    }

    // Fuzzy contains on relative paths (capped).
    let mut hits = Vec::new();
    let mut walked = 0usize;
    for root in roots {
        if !root.exists() {
            continue;
        }
        let walker = WalkBuilder::new(root)
            .hidden(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(true)
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                !(entry.file_type().is_some_and(|t| t.is_dir())
                    && SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s)))
            })
            .build();
        for entry in walker.flatten() {
            walked += 1;
            if walked > MAX_MENTION_WALK || hits.len() >= MAX_MENTION_WALK {
                break;
            }
            let path = entry.path();
            if path == root {
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let label = display_label(roots, root, rel);
            let label_lower = label.to_ascii_lowercase();
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            let matches = label_lower.contains(&query_lower)
                || label_lower
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.contains(&query_lower));
            if !matches {
                continue;
            }
            hits.push(Scored {
                rank: score(&label_lower, &query_lower),
                hit: MentionHit {
                    path: path.to_path_buf(),
                    label,
                    is_dir,
                },
            });
        }
    }
    hits.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.hit.label.cmp(&b.hit.label)));
    hits.into_iter()
        .take(limit)
        .map(|s| s.hit)
        .collect()
}

/// Relative path used when diving into a folder (`@label/`).
pub fn mention_rel_label(roots: &[PathBuf], path: &Path) -> String {
    for root in roots {
        if let Ok(rel) = path.strip_prefix(root) {
            return display_label(roots, root, rel);
        }
    }
    file_name(path)
}

fn resolve_dir(roots: &[PathBuf], query: &str) -> Option<(PathBuf, PathBuf)> {
    let query = query.trim_matches('/');
    for root in roots {
        // Multi-root labels are `rootName/rel`; strip that prefix when present.
        let rel = if roots.len() > 1 {
            let prefix = format!("{}/", file_name(root));
            query.strip_prefix(&prefix).unwrap_or(query)
        } else {
            query
        };
        let candidate = root.join(rel);
        if candidate.is_dir() {
            return Some((root.clone(), candidate));
        }
    }
    None
}

fn list_dir_children(root: &Path, dir: &Path, limit: usize) -> Vec<MentionHit> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| {
        (
            !e.file_type().map(|t| t.is_dir()).unwrap_or(false),
            e.file_name(),
        )
    });
    let roots = [root.to_path_buf()];
    let mut hits = Vec::new();
    for entry in entries {
        if hits.len() >= limit {
            break;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s)) {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let label = display_label(&roots, root, rel);
        hits.push(MentionHit {
            path,
            label,
            is_dir,
        });
    }
    hits
}

fn display_label(roots: &[PathBuf], root: &Path, rel: &Path) -> String {
    let rel = rel.to_string_lossy().replace('\\', "/");
    if roots.len() > 1 {
        format!("{}/{}", file_name(root), rel)
    } else {
        rel
    }
}

struct Scored {
    rank: u8,
    hit: MentionHit,
}

fn score(label_lower: &str, query_lower: &str) -> u8 {
    let name = label_lower.rsplit('/').next().unwrap_or(label_lower);
    if name == query_lower {
        0
    } else if name.starts_with(query_lower) {
        1
    } else if label_lower.starts_with(query_lower) {
        2
    } else if name.contains(query_lower) {
        3
    } else {
        4
    }
}

fn list_root_children(roots: &[PathBuf], limit: usize) -> Vec<MentionHit> {
    let mut hits = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| {
            (
                !e.file_type().map(|t| t.is_dir()).unwrap_or(false),
                e.file_name(),
            )
        });
        for entry in entries {
            if hits.len() >= limit {
                return hits;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s)) {
                continue;
            }
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let label = display_label(roots, root, rel);
            hits.push(MentionHit {
                path,
                label,
                is_dir,
            });
        }
    }
    hits
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn mention_requires_boundary_before_at() {
        assert!(mention_at_cursor("hello @fi", 9).is_some());
        assert!(mention_at_cursor("@fi", 3).is_some());
        assert!(mention_at_cursor("user@fi", 7).is_none());
        assert!(mention_at_cursor("see @a b", 8).is_none());
    }

    #[test]
    fn strip_removes_at_query() {
        let (text, caret) = strip_mention("look @src/a.rs please", 5, 14);
        assert_eq!(text, "look  please");
        assert_eq!(caret, 5);
    }

    #[test]
    fn list_empty_query_shows_root_children() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("README.md"), "x").unwrap();
        fs::create_dir(dir.path().join("target")).unwrap();
        let hits = list_mentions(&[dir.path().to_path_buf()], "", 10);
        let labels: Vec<_> = hits.iter().map(|h| h.label.as_str()).collect();
        assert!(labels.contains(&"src"), "{labels:?}");
        assert!(labels.contains(&"README.md"), "{labels:?}");
        assert!(!labels.iter().any(|l| l.contains("target")), "{labels:?}");
    }

    #[test]
    fn list_empty_query_is_not_clipped_to_search_cap() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..12 {
            fs::create_dir(dir.path().join(format!("dir{i:02}"))).unwrap();
        }
        let hits = list_mentions(&[dir.path().to_path_buf()], "", MAX_MENTION_HITS);
        assert!(
            hits.len() >= 12,
            "root listing must exceed fuzzy cap of {MAX_MENTION_HITS}, got {}",
            hits.len()
        );
    }

    #[test]
    fn list_filters_by_query() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("other.txt"), "x").unwrap();
        let hits = list_mentions(&[dir.path().to_path_buf()], "main", 10);
        assert!(
            hits.iter().any(|h| h.label.ends_with("main.rs")),
            "{hits:?}"
        );
        assert!(!hits.iter().any(|h| h.label.contains("other")), "{hits:?}");
    }

    #[test]
    fn list_dir_query_browses_children() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("lib.rs"), "").unwrap();
        fs::write(dir.path().join("top.txt"), "").unwrap();
        let hits = list_mentions(&[dir.path().to_path_buf()], "src", 10);
        assert!(hits.iter().any(|h| h.label == "src/lib.rs"), "{hits:?}");
        assert!(!hits.iter().any(|h| h.label == "top.txt"), "{hits:?}");
    }
}
