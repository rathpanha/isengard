//! `@` mention query parsing + workspace file/folder listing for the composer.

use std::path::{Path, PathBuf};

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
    "graphify-out",
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

/// Replace `@query` at `at..cursor` with `insert`; returns new text + caret.
pub fn replace_mention(text: &str, at: usize, cursor: usize, insert: &str) -> (String, usize) {
    let cursor = cursor.min(text.len());
    let at = at.min(cursor);
    let mut out = String::with_capacity(text.len() + insert.len());
    out.push_str(&text[..at]);
    out.push_str(insert);
    out.push_str(&text[cursor..]);
    (out, at + insert.len())
}

/// Absolute paths for `@rel` tokens in `text` (whitespace-bounded, same
/// boundary rules as [`mention_at_cursor`]). Order = first appearance; deduped.
pub fn paths_in_text(roots: &[PathBuf], text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let boundary = i == 0 || bytes[i - 1].is_ascii_whitespace();
            if boundary {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
                    end += 1;
                }
                if end > start {
                    let raw = &text[start..end];
                    // Prefer the raw token (`@a.rs`); only strip trailing
                    // punctuation when that fails (`@a.rs.`).
                    let path = resolve_mention_path(roots, raw).or_else(|| {
                        let trimmed = raw.trim_end_matches(|c: char| {
                            matches!(c, ',' | ';' | ':' | ')' | ']' | '}' | '.' | '!' | '?')
                        });
                        (trimmed != raw)
                            .then(|| resolve_mention_path(roots, trimmed))
                            .flatten()
                    });
                    if let Some(path) = path
                        && seen.insert(path.clone())
                    {
                        out.push(path);
                    }
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Resolve an `@` token (relative label) to a file or directory under `roots`.
pub fn resolve_mention_path(roots: &[PathBuf], query: &str) -> Option<PathBuf> {
    let query = query.trim_matches('/');
    if query.is_empty() {
        return None;
    }
    for root in roots {
        if file_name(root).eq_ignore_ascii_case(query) {
            return Some(root.clone());
        }
        let rel = if roots.len() > 1 {
            let prefix = format!("{}/", file_name(root));
            match query.strip_prefix(&prefix) {
                Some("") => return Some(root.clone()),
                Some(rest) => rest,
                None => query,
            }
        } else {
            query
        };
        let candidate = root.join(rel);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Workspace files/folders matching `query` (case-insensitive substring on the
/// relative path).
///
/// - Empty query → **workspace roots** + nested folders + root-level files
///   (except [`SKIP_DIRS`]).
/// - Query ending in `/` that names a directory (including a workspace root)
///   → browse that directory's children (→/Tab dive inserts the trailing slash).
/// - Otherwise fuzzy-search roots + tree, capped at `limit`.
///
/// Uses a plain FS walk (not `gitignore`) so nested folders the user can see
/// in the tree are always reachable via `@` — noisy build dirs stay in
/// [`SKIP_DIRS`]. Workspace roots themselves are mentionable (multi-root
/// folders like `royal5g-system` are not children of any walk).
pub fn list_mentions(roots: &[PathBuf], query: &str, limit: usize) -> Vec<MentionHit> {
    let limit = limit.clamp(1, MAX_MENTION_HITS);
    let browse = query.ends_with('/');
    let query = query.trim_matches('/');
    let query_lower = query.to_ascii_lowercase();

    if query.is_empty() {
        // No 8-hit trim — initial `@` must show roots + nested folders.
        return list_initial_mentions(roots, MAX_DIR_LISTING);
    }

    // Trailing `/` means "list this folder" (dive). Bare `@src` stays fuzzy so
    // nested `src/agent` etc. remain reachable without browsing.
    if browse && let Some((root, dir)) = resolve_dir(roots, query) {
        return list_dir_children(roots, &root, &dir, MAX_DIR_LISTING);
    }

    // Fuzzy: workspace roots first, then each root's tree with a fair budget
    // so a huge primary folder cannot starve secondary roots (the user can
    // `@` a non-primary root but never find files inside it).
    let mut hits = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        let label = file_name(root);
        let label_lower = label.to_ascii_lowercase();
        if label_lower.contains(&query_lower) {
            hits.push(Scored {
                rank: score(&label_lower, &query_lower),
                hit: MentionHit {
                    path: root.clone(),
                    label,
                    is_dir: true,
                },
            });
        }
    }
    let live: Vec<_> = roots.iter().filter(|r| r.exists()).collect();
    // Typing `royal5g-system/…` scopes the walk to that root with the full budget.
    let scoped: Vec<_> = live
        .iter()
        .copied()
        .filter(|root| {
            let name = file_name(root).to_ascii_lowercase();
            query_lower == name || query_lower.starts_with(&format!("{name}/"))
        })
        .collect();
    let search_roots: &[_] = if scoped.is_empty() { &live } else { &scoped };
    let per_root = if scoped.len() == 1 {
        MAX_MENTION_WALK
    } else {
        (MAX_MENTION_WALK / search_roots.len().max(1)).max(400)
    };
    for root in search_roots {
        let mut walked = 0usize;
        walk_one_root(root, |root, path, is_dir, _depth| {
            walked += 1;
            if walked > per_root {
                return false;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                return true;
            };
            let label = display_label(roots, root, rel);
            let label_lower = label.to_ascii_lowercase();
            let matches = label_lower.contains(&query_lower)
                || label_lower
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.contains(&query_lower));
            if matches {
                hits.push(Scored {
                    rank: score(&label_lower, &query_lower),
                    hit: MentionHit {
                        path: path.to_path_buf(),
                        label,
                        is_dir,
                    },
                });
            }
            true
        });
    }
    hits.sort_by(|a, b| {
        a.rank
            .cmp(&b.rank)
            .then_with(|| b.hit.is_dir.cmp(&a.hit.is_dir))
            .then_with(|| a.hit.label.cmp(&b.hit.label))
    });
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
    let path = resolve_mention_path(roots, query)?;
    if !path.is_dir() {
        return None;
    }
    let root = roots
        .iter()
        .find(|r| path.starts_with(r))
        .cloned()
        .unwrap_or_else(|| path.clone());
    Some((root, path))
}

fn list_dir_children(
    roots: &[PathBuf],
    root: &Path,
    dir: &Path,
    limit: usize,
) -> Vec<MentionHit> {
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
        let label = display_label(roots, root, rel);
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
    if rel.is_empty() {
        return file_name(root);
    }
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

/// Depth-first walk of one workspace root. Skips [`SKIP_DIRS`] by name only.
/// `visit` returns `false` to stop this root early.
fn walk_one_root(root: &Path, mut visit: impl FnMut(&Path, &Path, bool, usize) -> bool) {
    if !root.exists() {
        return;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        // Dirs first so nested folders are visited early under the walk cap.
        entries.sort_by_key(|e| {
            (
                !e.file_type().map(|t| t.is_dir()).unwrap_or(false),
                e.file_name(),
            )
        });
        let mut child_dirs = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir && SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s)) {
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let depth = rel.components().count();
            if !visit(root, &path, is_dir, depth) {
                return;
            }
            if is_dir {
                child_dirs.push(path);
            }
        }
        for d in child_dirs.into_iter().rev() {
            stack.push(d);
        }
    }
}

/// Empty `@`: workspace roots, then nested folders (fair share per root), then
/// root-level files.
fn list_initial_mentions(roots: &[PathBuf], limit: usize) -> Vec<MentionHit> {
    let live: Vec<_> = roots.iter().filter(|r| r.exists()).cloned().collect();
    let mut hits: Vec<_> = live
        .iter()
        .map(|root| MentionHit {
            path: root.clone(),
            label: file_name(root),
            is_dir: true,
        })
        .collect();
    let remaining = limit.saturating_sub(hits.len());
    let per_root_dirs = (remaining / live.len().max(1)).max(1);
    let mut dirs = Vec::new();
    let mut root_files = Vec::new();
    for root in &live {
        let mut taken = 0usize;
        walk_one_root(root, |root, path, is_dir, depth| {
            let Ok(rel) = path.strip_prefix(root) else {
                return true;
            };
            let label = display_label(roots, root, rel);
            let hit = MentionHit {
                path: path.to_path_buf(),
                label,
                is_dir,
            };
            if is_dir {
                if taken >= per_root_dirs {
                    return false;
                }
                dirs.push((depth, hit));
                taken += 1;
            } else if depth == 1 {
                root_files.push(hit);
            }
            true
        });
    }
    dirs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.label.cmp(&b.1.label)));
    root_files.sort_by(|a, b| a.label.cmp(&b.label));
    hits.extend(dirs.into_iter().map(|(_, h)| h));
    hits.extend(root_files);
    hits.truncate(limit);
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
    fn replace_can_strip_or_insert() {
        let (stripped, caret) = replace_mention("look @src/a.rs please", 5, 14, "");
        assert_eq!(stripped, "look  please");
        assert_eq!(caret, 5);
        let (text, caret) = replace_mention("look @src please", 5, 9, "@src/a.rs ");
        assert_eq!(text, "look @src/a.rs  please");
        assert_eq!(caret, 5 + "@src/a.rs ".len());
    }

    #[test]
    fn paths_in_text_finds_workspace_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "x").unwrap();
        let roots = [dir.path().to_path_buf()];
        let paths = paths_in_text(&roots, "see @src/a.rs and @src please");
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert!(paths[0].ends_with("src/a.rs"));
        assert!(paths[1].ends_with("src") && paths[1].is_dir());
        assert!(paths_in_text(&roots, "user@src/a.rs").is_empty());
        let dotted = paths_in_text(&roots, "see @src/a.rs.");
        assert_eq!(dotted.len(), 1);
        assert!(dotted[0].ends_with("src/a.rs"));
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
    fn list_empty_query_includes_nested_folders() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::create_dir(dir.path().join("src").join("agent")).unwrap();
        fs::write(dir.path().join("src").join("main.rs"), "").unwrap();
        let hits = list_mentions(&[dir.path().to_path_buf()], "", 50);
        let labels: Vec<_> = hits.iter().map(|h| h.label.as_str()).collect();
        assert!(labels.contains(&"src"), "{labels:?}");
        assert!(labels.contains(&"src/agent"), "{labels:?}");
        // Nested files stay out of the empty list (browse or type to find them).
        assert!(!labels.iter().any(|l| l.ends_with("main.rs")), "{labels:?}");
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
    fn list_dir_query_browses_children_with_trailing_slash() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("lib.rs"), "").unwrap();
        fs::write(dir.path().join("top.txt"), "").unwrap();
        let hits = list_mentions(&[dir.path().to_path_buf()], "src/", 10);
        assert!(hits.iter().any(|h| h.label == "src/lib.rs"), "{hits:?}");
        assert!(!hits.iter().any(|h| h.label == "top.txt"), "{hits:?}");
    }

    #[test]
    fn bare_dir_query_fuzzy_matches_folder_not_only_children() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::create_dir(dir.path().join("src").join("agent")).unwrap();
        let hits = list_mentions(&[dir.path().to_path_buf()], "agent", 10);
        assert!(
            hits.iter().any(|h| h.label == "src/agent" && h.is_dir),
            "{hits:?}"
        );
    }

    #[test]
    fn nested_folder_query_matches_in_this_repo() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let hits = list_mentions(std::slice::from_ref(&root), "mention", MAX_MENTION_HITS);
        assert!(
            hits.iter().any(|h| {
                let l = h.label.replace('\\', "/");
                l.contains("agent/mention") || (h.is_dir && l.ends_with("agent"))
            }),
            "expected nested agent/mention hit, got {hits:?}"
        );
        let empty = list_mentions(&[root], "", MAX_DIR_LISTING);
        assert!(
            empty.iter().any(|h| h.is_dir && h.label.contains('/')),
            "empty @ should list nested dirs, got {:?}",
            empty.iter().map(|h| &h.label).collect::<Vec<_>>()
        );
    }

    /// gitignore must not hide nested folders from `@` (plain FS walk).
    #[test]
    fn nested_folder_visible_even_when_gitignored() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::create_dir(dir.path().join("src").join("secret")).unwrap();
        fs::write(dir.path().join(".gitignore"), "src/\n").unwrap();
        let hits = list_mentions(&[dir.path().to_path_buf()], "secret", 10);
        assert!(
            hits.iter().any(|h| h.label == "src/secret" && h.is_dir),
            "gitignored nested dir must still match, got {hits:?}"
        );
        let empty = list_mentions(&[dir.path().to_path_buf()], "", 50);
        assert!(
            empty.iter().any(|h| h.label == "src/secret"),
            "empty @ must list gitignored nested dir, got {:?}",
            empty.iter().map(|h| &h.label).collect::<Vec<_>>()
        );
    }

    /// Multi-root workspace folders are roots — not walk children — and must
    /// match when the user types `@royal5g-system`.
    #[test]
    fn multi_root_folder_name_is_mentionable() {
        let dir = tempfile::tempdir().unwrap();
        let graph = dir.path().join("royal5g-graph");
        let system = dir.path().join("royal5g-system");
        fs::create_dir(&graph).unwrap();
        fs::create_dir(&system).unwrap();
        fs::write(system.join("readme.md"), "x").unwrap();
        let roots = vec![graph.clone(), system.clone()];

        let empty = list_mentions(&roots, "", 50);
        assert!(
            empty.iter().any(|h| h.label == "royal5g-system" && h.path == system),
            "empty @ must list workspace roots, got {:?}",
            empty.iter().map(|h| &h.label).collect::<Vec<_>>()
        );

        let hits = list_mentions(&roots, "royal5g-system", 8);
        assert!(
            hits.iter().any(|h| h.label == "royal5g-system" && h.is_dir && h.path == system),
            "filter must hit the workspace root itself, got {hits:?}"
        );

        let browsed = list_mentions(&roots, "royal5g-system/", 8);
        assert!(
            browsed.iter().any(|h| h.label.ends_with("readme.md")),
            "trailing slash browses inside the root, got {browsed:?}"
        );
    }

    /// A huge primary root must not starve fuzzy search in a secondary root.
    #[test]
    fn fuzzy_reaches_secondary_root_when_primary_is_huge() {
        let dir = tempfile::tempdir().unwrap();
        let primary = dir.path().join("primary");
        let secondary = dir.path().join("secondary");
        fs::create_dir(&primary).unwrap();
        fs::create_dir(&secondary).unwrap();
        for i in 0..(MAX_MENTION_WALK + 100) {
            fs::write(primary.join(format!("pad{i}.txt")), "x").unwrap();
        }
        fs::write(secondary.join("needle.rs"), "fn x() {}").unwrap();
        let roots = vec![primary, secondary];
        let hits = list_mentions(&roots, "needle", 8);
        assert!(
            hits.iter().any(|h| h.label.contains("needle")),
            "secondary-root file must match despite huge primary, got {hits:?}"
        );
        let scoped = list_mentions(&roots, "secondary/needle", 8);
        assert!(
            scoped.iter().any(|h| h.label.contains("needle")),
            "scoped query must find secondary file, got {scoped:?}"
        );
    }
}
