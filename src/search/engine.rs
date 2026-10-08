//! Folder/workspace text search and replace (no external `rg`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use ignore::{WalkBuilder, WalkState};
use regex::{Regex, RegexBuilder};

/// Hard caps so a huge monorepo cannot freeze the UI.
pub const MAX_MATCHES: usize = 10_000;
pub const MAX_FILES_WITH_HITS: usize = 2_000;
/// Skip files larger than this (bytes). Matches common editor search defaults.
pub const MAX_FILE_BYTES: u64 = 1_048_576;
/// Well-known build/deps dirs skipped even without a `.gitignore`.
const SKIP_DIR_NAMES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "__pycache__",
    "vendor",
    ".venv",
    "venv",
    ".tox",
    ".cargo",
    ".svn",
    ".hg",
];
/// Extensions we never bother reading (binary / media / archives).
const SKIP_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "ico", "bmp", "tif", "tiff", "psd", "ai", "svgz", "pdf",
    "zip", "gz", "tgz", "bz2", "xz", "7z", "rar", "jar", "war", "ear", "whl", "dmg", "iso",
    "woff", "woff2", "ttf", "otf", "eot", "mp3", "mp4", "m4a", "wav", "flac", "ogg", "webm",
    "mov", "avi", "mkv", "wasm", "so", "dylib", "dll", "exe", "o", "a", "lib", "class", "pyc",
    "pyo", "db", "sqlite", "sqlite3", "bin", "dat", "pak", "icns", "lockb",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub pattern: String,
    pub replace: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    pub path: PathBuf,
    /// 0-based line index.
    pub line: usize,
    /// 0-based character (Unicode scalar) column of the match start.
    pub col: usize,
    pub line_text: String,
    pub match_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchFileGroup {
    pub path: PathBuf,
    pub matches: Vec<SearchMatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchOutcome {
    pub groups: Vec<SearchFileGroup>,
    pub truncated: bool,
    pub match_count: usize,
    pub file_count: usize,
    /// Set when the pattern cannot be compiled.
    pub error: Option<String>,
}

/// Build the matcher for `query`. Empty pattern → `Ok(None)`.
pub fn compile_regex(query: &SearchQuery) -> Result<Option<Regex>, String> {
    if query.pattern.is_empty() {
        return Ok(None);
    }
    let mut pattern = if query.use_regex {
        query.pattern.clone()
    } else {
        regex::escape(&query.pattern)
    };
    if query.whole_word {
        pattern = format!(r"\b(?:{pattern})\b");
    }
    RegexBuilder::new(&pattern)
        .case_insensitive(!query.case_sensitive)
        .multi_line(false)
        .build()
        .map(Some)
        .map_err(|err| err.to_string())
}

/// Search `roots`, preferring `open_buffers` text over disk when present.
///
/// `cancel` may be set from another thread to abort early (superseded query).
pub fn run_search(
    roots: &[PathBuf],
    query: &SearchQuery,
    open_buffers: &HashMap<PathBuf, String>,
    cancel: &AtomicBool,
) -> SearchOutcome {
    let re = match compile_regex(query) {
        Ok(Some(re)) => re,
        Ok(None) => return SearchOutcome::default(),
        Err(error) => {
            return SearchOutcome {
                error: Some(error),
                ..SearchOutcome::default()
            };
        }
    };

    // Literal prefilter (ASCII case fold): skip files that cannot contain the needle.
    // Whole-word still needs the regex; prefilter only proves substring presence.
    let prefilter: Option<AhoCorasick> = if !query.use_regex && !query.pattern.is_empty() {
        AhoCorasickBuilder::new()
            .ascii_case_insensitive(!query.case_sensitive)
            .build([&query.pattern])
            .ok()
    } else {
        None
    };

    let shared = SharedOutcome::new();
    let mut searched_open: HashSet<PathBuf> = HashSet::new();

    for root in roots {
        if cancel.load(Ordering::Relaxed) || shared.done() {
            break;
        }
        if !root.is_dir() {
            continue;
        }
        for (path, text) in open_buffers {
            if cancel.load(Ordering::Relaxed) || shared.done() {
                break;
            }
            if !path.starts_with(root) || !searched_open.insert(path.clone()) {
                continue;
            }
            let _ = search_text(path, text, &re, prefilter.as_ref(), &shared);
        }

        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let mut builder = WalkBuilder::new(root);
        builder
            .hidden(true)
            .git_ignore(true)
            .require_git(false)
            .git_global(true)
            .git_exclude(true)
            .max_filesize(Some(MAX_FILE_BYTES))
            .threads(threads)
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                !SKIP_DIR_NAMES.iter().any(|d| name.as_ref() == *d)
            });

        builder.build_parallel().run(|| {
            let shared = &shared;
            let re = &re;
            let prefilter = prefilter.as_ref();
            Box::new(move |entry| {
                if cancel.load(Ordering::Relaxed) || shared.done() {
                    return WalkState::Quit;
                }
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => return WalkState::Continue,
                };
                let path = entry.path();
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    return WalkState::Continue;
                }
                if open_buffers.contains_key(path) || should_skip_path(path) {
                    return WalkState::Continue;
                }
                let Ok(bytes) = std::fs::read(path) else {
                    return WalkState::Continue;
                };
                if is_binary(&bytes) {
                    return WalkState::Continue;
                }
                if prefilter.is_some_and(|ac| ac.find(&bytes).is_none()) {
                    return WalkState::Continue;
                }
                let Ok(text) = std::str::from_utf8(&bytes) else {
                    return WalkState::Continue;
                };
                if search_text(path, text, re, prefilter, shared) {
                    WalkState::Quit
                } else {
                    WalkState::Continue
                }
            })
        });
    }

    if !(cancel.load(Ordering::Relaxed) || shared.done()) {
        for (path, text) in open_buffers {
            if cancel.load(Ordering::Relaxed) || shared.done() {
                break;
            }
            if searched_open.contains(path) {
                continue;
            }
            let _ = search_text(path, text, &re, prefilter.as_ref(), &shared);
        }
    }

    shared.into_outcome()
}

struct SharedOutcome {
    groups: Mutex<Vec<SearchFileGroup>>,
    match_count: AtomicUsize,
    file_count: AtomicUsize,
    capped: AtomicBool,
}

impl SharedOutcome {
    fn new() -> Self {
        Self {
            groups: Mutex::new(Vec::new()),
            match_count: AtomicUsize::new(0),
            file_count: AtomicUsize::new(0),
            capped: AtomicBool::new(false),
        }
    }

    fn done(&self) -> bool {
        self.capped.load(Ordering::Relaxed)
    }

    fn into_outcome(self) -> SearchOutcome {
        let groups = self.groups.into_inner().unwrap_or_default();
        let match_count = groups.iter().map(|g| g.matches.len()).sum();
        let file_count = groups.len();
        SearchOutcome {
            truncated: self.capped.load(Ordering::Relaxed),
            match_count,
            file_count,
            groups,
            error: None,
        }
    }
}

/// Returns true when a hard cap was hit (caller should stop).
fn search_text(
    path: &Path,
    text: &str,
    re: &Regex,
    prefilter: Option<&AhoCorasick>,
    shared: &SharedOutcome,
) -> bool {
    if shared.done() {
        return true;
    }
    if prefilter.is_some_and(|ac| ac.find(text.as_bytes()).is_none()) {
        return false;
    }

    let mut file_matches: Vec<SearchMatch> = Vec::new();
    for (line_ix, line) in text.lines().enumerate() {
        for m in re.find_iter(line) {
            if shared.match_count.load(Ordering::Relaxed) >= MAX_MATCHES {
                shared.capped.store(true, Ordering::Relaxed);
                break;
            }
            let col = line[..m.start()].chars().count();
            let match_len = m.as_str().chars().count();
            file_matches.push(SearchMatch {
                path: path.to_path_buf(),
                line: line_ix,
                col,
                line_text: truncate_line(line, 240),
                match_len,
            });
            shared.match_count.fetch_add(1, Ordering::Relaxed);
        }
        if shared.done() {
            break;
        }
    }
    if file_matches.is_empty() {
        return shared.done();
    }

    let mut groups = shared.groups.lock().unwrap_or_else(|e| e.into_inner());
    if shared.file_count.load(Ordering::Relaxed) >= MAX_FILES_WITH_HITS {
        shared.capped.store(true, Ordering::Relaxed);
        return true;
    }
    groups.push(SearchFileGroup {
        path: path.to_path_buf(),
        matches: file_matches,
    });
    let files = shared.file_count.fetch_add(1, Ordering::Relaxed) + 1;
    if files >= MAX_FILES_WITH_HITS || shared.match_count.load(Ordering::Relaxed) >= MAX_MATCHES
    {
        shared.capped.store(true, Ordering::Relaxed);
        return true;
    }
    shared.done()
}

fn should_skip_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| SKIP_EXTENSIONS.iter().any(|s| ext.eq_ignore_ascii_case(s)))
}

fn truncate_line(line: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in line.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8_192).any(|&b| b == 0)
}

/// Replace every match of `query` in `text`. Returns `(new_text, replacement_count)`.
pub fn apply_replace(text: &str, query: &SearchQuery) -> Result<(String, usize), String> {
    let Some(re) = compile_regex(query)? else {
        return Ok((text.to_owned(), 0));
    };
    let mut count = 0usize;
    let replaced = re.replace_all(text, |caps: &regex::Captures| {
        count += 1;
        // Expand `$1` etc. when regex mode; otherwise treat replace as literal.
        if query.use_regex {
            let mut out = String::new();
            caps.expand(&query.replace, &mut out);
            out
        } else {
            query.replace.clone()
        }
    });
    Ok((replaced.into_owned(), count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicBool;

    fn q(pattern: &str) -> SearchQuery {
        SearchQuery {
            pattern: pattern.into(),
            ..SearchQuery::default()
        }
    }

    fn search(
        roots: &[PathBuf],
        query: &SearchQuery,
        buffers: &HashMap<PathBuf, String>,
    ) -> SearchOutcome {
        run_search(roots, query, buffers, &AtomicBool::new(false))
    }

    #[test]
    fn literal_find_and_replace() {
        let text = "foo bar foo\nbaz foo";
        let mut query = q("foo");
        query.replace = "qux".into();
        let (new_text, n) = apply_replace(text, &query).unwrap();
        assert_eq!(n, 3);
        assert_eq!(new_text, "qux bar qux\nbaz qux");
    }

    #[test]
    fn case_sensitive_and_whole_word() {
        let text = "Foo foo food\n";
        let mut query = q("foo");
        query.case_sensitive = true;
        query.whole_word = true;
        let re = compile_regex(&query).unwrap().unwrap();
        let hits: Vec<_> = re.find_iter(text).map(|m| m.as_str()).collect();
        assert_eq!(hits, ["foo"]);
    }

    #[test]
    fn regex_capture_replace() {
        let mut query = q(r"(\w+)=(\w+)");
        query.use_regex = true;
        query.replace = "$2=$1".into();
        let (out, n) = apply_replace("a=b c=d", &query).unwrap();
        assert_eq!(n, 2);
        assert_eq!(out, "b=a d=c");
    }

    #[test]
    fn skips_binary_and_respects_open_buffer() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("a.txt"), "hello world").unwrap();
        fs::write(root.join("b.bin"), [0u8, 1, 2, b'x']).unwrap();

        let outcome = search(&[root.to_path_buf()], &q("hello"), &HashMap::new());
        assert_eq!(outcome.match_count, 1);
        assert_eq!(outcome.file_count, 1);

        let mut buffers = HashMap::new();
        buffers.insert(root.join("a.txt"), "hello hello".into());
        let outcome = search(&[root.to_path_buf()], &q("hello"), &buffers);
        assert_eq!(outcome.match_count, 2);
    }

    #[test]
    fn invalid_regex_returns_error() {
        let mut query = q("(unclosed");
        query.use_regex = true;
        let outcome = search(&[PathBuf::from("/tmp")], &query, &HashMap::new());
        assert!(outcome.error.is_some());
    }

    #[test]
    fn gitignored_file_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join(".gitignore"), "secret.txt\n").unwrap();
        fs::write(root.join("secret.txt"), "needle here").unwrap();
        fs::write(root.join("visible.txt"), "needle here").unwrap();
        let outcome = search(&[root.to_path_buf()], &q("needle"), &HashMap::new());
        assert_eq!(outcome.file_count, 1);
        assert!(outcome.groups[0].path.ends_with("visible.txt"));
    }

    #[test]
    fn skips_node_modules_and_target_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("node_modules/pkg/x.js"), "needle").unwrap();
        fs::write(root.join("target/debug/x.rs"), "needle").unwrap();
        fs::write(root.join("src.txt"), "needle").unwrap();
        let outcome = search(&[root.to_path_buf()], &q("needle"), &HashMap::new());
        assert_eq!(outcome.file_count, 1);
        assert!(outcome.groups[0].path.ends_with("src.txt"));
    }

    #[test]
    fn cancel_aborts_without_requiring_full_walk() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for i in 0..50 {
            fs::write(root.join(format!("f{i}.txt")), "needle here").unwrap();
        }
        let cancel = AtomicBool::new(true);
        let outcome = run_search(
            &[root.to_path_buf()],
            &q("needle"),
            &HashMap::new(),
            &cancel,
        );
        assert!(outcome.error.is_none());
        assert_eq!(outcome.match_count, 0);
    }
}
