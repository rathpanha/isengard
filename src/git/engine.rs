//! Thin CLI wrapper around system `git` (no libgit).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One dirty / untracked / renamed path in a repo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitChange {
    /// Absolute path to the working-tree file (new path for renames).
    pub path: PathBuf,
    /// Path relative to the repo root (display).
    pub rel_path: String,
    /// Previous path when status is `R` / `C` (needed to discard renames).
    pub old_rel_path: Option<String>,
    /// Single-letter badge: `M` / `A` / `D` / `R` / `?` / `U` / `C`.
    pub status: char,
}

/// Snapshot of one Git worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoStatus {
    pub root: PathBuf,
    pub branch: String,
    pub changes: Vec<GitChange>,
    /// Commits on HEAD not in `@{upstream}` (`0` if no upstream).
    pub ahead: u32,
    /// Commits on `@{upstream}` not in HEAD (`0` if no upstream).
    pub behind: u32,
    /// Set when `git` itself failed for this repo.
    pub error: Option<String>,
}

/// Run `git -C <cwd> <args…>`; stdout on success, combined stderr/stdout on failure.
pub fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    git_inner(cwd, args, true)
}

/// Like [`git`], but keeps trailing whitespace (file blobs from `git show`).
fn git_blob(cwd: &Path, args: &[&str]) -> Result<String, String> {
    git_inner(cwd, args, false)
}

fn git_inner(cwd: &Path, args: &[&str], trim_stdout: bool) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|err| format!("git failed to start: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() {
        return Ok(if trim_stdout {
            stdout.trim_end().to_owned()
        } else {
            stdout.into_owned()
        });
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let msg = if !stderr.trim().is_empty() {
        stderr.trim().to_owned()
    } else if !stdout.trim().is_empty() {
        stdout.trim().to_owned()
    } else {
        format!("git {} failed", args.first().unwrap_or(&"command"))
    };
    Err(msg)
}

/// Old (`HEAD`) and new (worktree) texts for a side-by-side preview.
///
/// Empty old = added/untracked; empty new = deleted. Renames use `old_rel_path`
/// for the HEAD side when present.
pub fn file_sides(
    repo: &Path,
    rel_path: &str,
    status: char,
    old_rel_path: Option<&str>,
) -> Result<(String, String), String> {
    let head_path = old_rel_path
        .filter(|s| !s.is_empty())
        .unwrap_or(rel_path);
    let old = match status {
        '?' => String::new(),
        _ => git_show_head(repo, head_path).unwrap_or_default(),
    };
    let new = match status {
        'D' => String::new(),
        _ => read_worktree(repo, rel_path),
    };
    Ok((old, new))
}

fn git_show_head(repo: &Path, rel_path: &str) -> Result<String, String> {
    // `git show HEAD:path` fails when the path was never committed.
    git_blob(repo, &["show", &format!("HEAD:{rel_path}")])
}

fn read_worktree(repo: &Path, rel_path: &str) -> String {
    let abs = repo.join(rel_path);
    std::fs::read_to_string(&abs).unwrap_or_default()
}

/// Unique Git toplevels covering `folders` (shared `.git` → one entry).
pub fn discover_repos(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut by_canon: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    for folder in folders {
        if !folder.is_dir() {
            continue;
        }
        let Ok(toplevel) = git(folder, &["rev-parse", "--show-toplevel"]) else {
            continue;
        };
        let root = PathBuf::from(toplevel);
        let key = root
            .canonicalize()
            .unwrap_or_else(|_| root.clone());
        by_canon.entry(key).or_insert(root);
    }
    by_canon.into_values().collect()
}

/// Branch name + porcelain status for `root`.
pub fn load_repo_status(root: &Path) -> RepoStatus {
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "HEAD".into());
    let (ahead, behind) = ahead_behind_upstream(root);
    match git(root, &["status", "--porcelain=v1", "-uall"]) {
        Ok(out) => RepoStatus {
            root: root.to_path_buf(),
            branch,
            changes: parse_porcelain(&out, root),
            ahead,
            behind,
            error: None,
        },
        Err(error) => RepoStatus {
            root: root.to_path_buf(),
            branch,
            changes: Vec::new(),
            ahead,
            behind,
            error: Some(error),
        },
    }
}

/// Ahead / behind vs `@{upstream}` (`(0, 0)` if none / error).
pub fn ahead_behind_upstream(root: &Path) -> (u32, u32) {
    let ahead = rev_list_count(root, "@{upstream}..HEAD");
    let behind = rev_list_count(root, "HEAD..@{upstream}");
    (ahead, behind)
}

fn rev_list_count(root: &Path, range: &str) -> u32 {
    match git(root, &["rev-list", "--count", range]) {
        Ok(s) => s.trim().parse().unwrap_or(0),
        Err(_) => 0,
    }
}

/// Load status for every discovered repo under `folders`.
pub fn load_workspace_status(folders: &[PathBuf]) -> Vec<RepoStatus> {
    discover_repos(folders)
        .into_iter()
        .map(|root| load_repo_status(&root))
        .collect()
}

/// Stage `rel_paths` then `git commit -m` (empty selection / message rejected).
pub fn commit_paths(root: &Path, message: &str, rel_paths: &[String]) -> Result<(), String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Commit message is empty".into());
    }
    if rel_paths.is_empty() {
        return Err("No files selected".into());
    }
    let mut args: Vec<&str> = Vec::with_capacity(2 + rel_paths.len());
    args.push("add");
    args.push("--");
    for p in rel_paths {
        args.push(p.as_str());
    }
    git(root, &args)?;
    git(root, &["commit", "-m", message])?;
    Ok(())
}

pub fn pull_ff_only(root: &Path) -> Result<String, String> {
    git(root, &["pull", "--ff-only"])
}

pub fn push(root: &Path) -> Result<String, String> {
    git(root, &["push"])
}

/// Discard one change: untracked → delete; tracked → restore from `HEAD`
/// (index + worktree). Renames restore both old and new paths.
pub fn discard_file(
    root: &Path,
    rel_path: &str,
    status: char,
    old_rel_path: Option<&str>,
) -> Result<(), String> {
    if status == '?' {
        return git(root, &["clean", "-fd", "--", rel_path]).map(|_| ());
    }
    let mut args: Vec<&str> = vec![
        "restore",
        "--source=HEAD",
        "--staged",
        "--worktree",
        "--",
        rel_path,
    ];
    if let Some(old) = old_rel_path.filter(|s| !s.is_empty() && *s != rel_path) {
        args.push(old);
    }
    git(root, &args).map(|_| ())
}

/// Discard every change in the worktree: hard reset tracked + clean untracked.
pub fn discard_all(root: &Path) -> Result<(), String> {
    git(root, &["reset", "--hard", "HEAD"])?;
    git(root, &["clean", "-fd"])?;
    Ok(())
}

/// Non-AI commit subject from change paths (draft only — never auto-commit).
///
/// Examples: `Add foo.rs`, `docs: Update README.md`, `Update a.rs, b.rs (+3 more)`.
pub fn suggest_commit_message(changes: &[GitChange]) -> Option<String> {
    if changes.is_empty() {
        return None;
    }
    let verb = suggest_verb(changes);
    let prefix = conventional_prefix(changes);
    let names: Vec<&str> = changes
        .iter()
        .map(|c| file_stem_name(&c.rel_path))
        .collect();
    const MAX_NAMES: usize = 3;
    let mut body = String::new();
    body.push_str(prefix);
    body.push_str(verb);
    body.push(' ');
    for (i, name) in names.iter().take(MAX_NAMES).enumerate() {
        if i > 0 {
            body.push_str(", ");
        }
        body.push_str(name);
    }
    if names.len() > MAX_NAMES {
        body.push_str(&format!(" (+{} more)", names.len() - MAX_NAMES));
    }
    Some(body)
}

fn suggest_verb(changes: &[GitChange]) -> &'static str {
    let all_new = changes.iter().all(|c| c.status == '?' || c.status == 'A');
    let all_del = changes.iter().all(|c| c.status == 'D');
    if all_new {
        "Add"
    } else if all_del {
        "Remove"
    } else {
        "Update"
    }
}

/// Uniform top-level folder → conventional commit type; otherwise empty.
fn conventional_prefix(changes: &[GitChange]) -> &'static str {
    let mut kind: Option<&'static str> = None;
    for c in changes {
        let next = path_commit_kind(&c.rel_path);
        match kind {
            None => kind = Some(next),
            Some(k) if k == next => {}
            Some(_) => return "",
        }
    }
    match kind {
        Some("docs") => "docs: ",
        Some("test") => "test: ",
        _ => "",
    }
}

fn path_commit_kind(rel: &str) -> &'static str {
    let top = rel.split(['/', '\\']).next().unwrap_or(rel);
    let top = top.to_ascii_lowercase();
    match top.as_str() {
        "docs" | "doc" => "docs",
        "test" | "tests" | "spec" | "specs" => "test",
        _ => "other",
    }
}

fn file_stem_name(rel: &str) -> &str {
    Path::new(rel)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(rel)
}

/// Parse `git status --porcelain=v1` lines into changes.
pub fn parse_porcelain(output: &str, root: &Path) -> Vec<GitChange> {
    let mut changes = Vec::new();
    for line in output.lines() {
        if line.len() < 3 {
            continue;
        }
        let x = line.as_bytes()[0] as char;
        let y = line.as_bytes()[1] as char;
        let rest = &line[3..];
        let (rel, old_rel, status) = if rest.contains(" -> ") {
            // Rename / copy: `old -> new`
            let (old, new) = rest
                .rsplit_once(" -> ")
                .map(|(o, n)| (o, n))
                .unwrap_or(("", rest));
            let st = if x == 'R' || y == 'R' {
                'R'
            } else if x == 'C' || y == 'C' {
                'C'
            } else {
                status_letter(x, y)
            };
            (unquote_path(new), Some(unquote_path(old)), st)
        } else {
            (unquote_path(rest), None, status_letter(x, y))
        };
        if rel.is_empty() {
            continue;
        }
        changes.push(GitChange {
            path: root.join(&rel),
            rel_path: rel,
            old_rel_path: old_rel,
            status,
        });
    }
    changes
}

fn status_letter(x: char, y: char) -> char {
    if x == '?' || y == '?' {
        return '?';
    }
    if x == 'U' || y == 'U' {
        return 'U';
    }
    // Prefer worktree (y), then index (x).
    for ch in [y, x] {
        match ch {
            'M' | 'A' | 'D' | 'R' | 'C' => return ch,
            _ => {}
        }
    }
    if x != ' ' {
        x
    } else if y != ' ' {
        y
    } else {
        'M'
    }
}

fn unquote_path(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        // Porcelain quotes C-style; enough for spaces — leave escapes as-is for v1.
        s[1..s.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    #[test]
    fn parse_porcelain_basic_and_rename() {
        let root = Path::new("/repo");
        let out = "\
M  src/a.rs\n\
 A src/b.rs\n\
?? new.txt\n\
R  old.rs -> renamed.rs\n\
 D gone.rs\n";
        let changes = parse_porcelain(out, root);
        assert_eq!(changes.len(), 5);
        assert_eq!(changes[0].status, 'M');
        assert_eq!(changes[0].rel_path, "src/a.rs");
        assert_eq!(changes[1].status, 'A');
        assert_eq!(changes[2].status, '?');
        assert_eq!(changes[3].status, 'R');
        assert_eq!(changes[3].rel_path, "renamed.rs");
        assert_eq!(changes[3].old_rel_path.as_deref(), Some("old.rs"));
        assert_eq!(changes[3].path, root.join("renamed.rs"));
        assert_eq!(changes[4].status, 'D');
    }

    #[test]
    fn discard_file_untracked_and_modified() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fs::create_dir_all(&root).unwrap();
        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&root)
            .status();
        fs::write(root.join("tracked.txt"), "one\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "tracked.txt"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-m", "init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("tracked.txt"), "two\n").unwrap();
        fs::write(root.join("scratch.txt"), "tmp\n").unwrap();

        discard_file(&root, "scratch.txt", '?', None).unwrap();
        assert!(!root.join("scratch.txt").exists());

        discard_file(&root, "tracked.txt", 'M', None).unwrap();
        assert_eq!(fs::read_to_string(root.join("tracked.txt")).unwrap(), "one\n");
        assert!(load_repo_status(&root).changes.is_empty());
    }

    #[test]
    fn discard_all_clears_tracked_and_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fs::create_dir_all(&root).unwrap();
        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&root)
            .status();
        fs::write(root.join("a.txt"), "a\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "a.txt"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-m", "init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("a.txt"), "changed\n").unwrap();
        fs::write(root.join("b.txt"), "new\n").unwrap();
        discard_all(&root).unwrap();
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "a\n");
        assert!(!root.join("b.txt").exists());
        assert!(load_repo_status(&root).changes.is_empty());
    }

    #[test]
    fn discover_dedupes_shared_toplevel() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fs::create_dir_all(root.join("sub/a")).unwrap();
        fs::create_dir_all(root.join("sub/b")).unwrap();
        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        // Identity for commit (not used, but some git configs require it for later tests).
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&root)
            .status();

        let folders = vec![root.join("sub/a"), root.join("sub/b"), root.clone()];
        let repos = discover_repos(&folders);
        assert_eq!(repos.len(), 1);
        let canon = root.canonicalize().unwrap();
        assert_eq!(repos[0].canonicalize().unwrap(), canon);
    }

    #[test]
    fn load_status_sees_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fs::create_dir_all(&root).unwrap();
        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("hello.txt"), "hi").unwrap();
        let status = load_repo_status(&root);
        assert!(status.error.is_none());
        assert!(
            status
                .changes
                .iter()
                .any(|c| c.rel_path == "hello.txt" && c.status == '?'),
            "{:?}",
            status.changes
        );
    }

    #[test]
    fn commit_paths_requires_message_and_selection() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            commit_paths(tmp.path(), "   ", &["a.txt".into()]).unwrap_err(),
            "Commit message is empty"
        );
        assert_eq!(
            commit_paths(tmp.path(), "ok", &[]).unwrap_err(),
            "No files selected"
        );
    }

    #[test]
    fn commit_paths_stages_only_selected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fs::create_dir_all(&root).unwrap();
        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&root)
            .status();
        fs::write(root.join("a.txt"), "a\n").unwrap();
        fs::write(root.join("b.txt"), "b\n").unwrap();
        commit_paths(&root, "add a", &["a.txt".into()]).unwrap();
        let status = load_repo_status(&root);
        assert!(
            status
                .changes
                .iter()
                .any(|c| c.rel_path == "b.txt" && c.status == '?'),
            "{:?}",
            status.changes
        );
        assert!(
            !status.changes.iter().any(|c| c.rel_path == "a.txt"),
            "{:?}",
            status.changes
        );
    }

    #[test]
    fn file_sides_modified_and_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fs::create_dir_all(&root).unwrap();
        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&root)
            .status();
        fs::write(root.join("a.txt"), "old\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "a.txt"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-m", "init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("a.txt"), "new\n").unwrap();
        fs::write(root.join("b.txt"), "only\n").unwrap();

        let (old, new) = file_sides(&root, "a.txt", 'M', None).unwrap();
        assert_eq!(old, "old\n");
        assert_eq!(new, "new\n");

        let (old, new) = file_sides(&root, "b.txt", '?', None).unwrap();
        assert!(old.is_empty());
        assert_eq!(new, "only\n");
    }

    #[test]
    fn ahead_counts_commits_not_in_upstream() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        fs::create_dir_all(&root).unwrap();
        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&root)
            .status();
        fs::write(root.join("a.txt"), "1\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "a.txt"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-m", "base"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(ahead_behind_upstream(&root), (0, 0), "no upstream yet");

        assert!(
            Command::new("git")
                .args(["checkout", "-b", "feature"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("a.txt"), "2\n").unwrap();
        assert!(
            Command::new("git")
                .args(["commit", "-am", "ahead"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["branch", "--set-upstream-to=master"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
                || Command::new("git")
                    .args(["branch", "--set-upstream-to=main"])
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
        );
        assert_eq!(ahead_behind_upstream(&root), (1, 0));
        let status = load_repo_status(&root);
        assert_eq!(status.ahead, 1);
        assert_eq!(status.behind, 0);

        // Switch to master/main: it is behind feature by 1 once we point
        // upstream the other way — instead commit on base and re-check from feature.
        let base = if root.join(".git/refs/heads/master").exists() {
            "master"
        } else {
            "main"
        };
        assert!(
            Command::new("git")
                .args(["checkout", base])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("a.txt"), "base2\n").unwrap();
        assert!(
            Command::new("git")
                .args(["commit", "-am", "on-base"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["checkout", "feature"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        // feature diverged: 1 ahead (its commit), 1 behind (base's new commit).
        assert_eq!(ahead_behind_upstream(&root), (1, 1));
    }

    #[test]
    fn suggest_message_add_update_docs_and_truncation() {
        assert_eq!(suggest_commit_message(&[]), None);
        let added = vec![GitChange {
            path: PathBuf::from("/r/new.rs"),
            rel_path: "src/new.rs".into(),
            old_rel_path: None,
            status: '?',
        }];
        assert_eq!(
            suggest_commit_message(&added).as_deref(),
            Some("Add new.rs")
        );

        let docs = vec![
            GitChange {
                path: PathBuf::from("/r/docs/a.md"),
                rel_path: "docs/a.md".into(),
                old_rel_path: None,
                status: 'M',
            },
            GitChange {
                path: PathBuf::from("/r/docs/b.md"),
                rel_path: "docs/b.md".into(),
                old_rel_path: None,
                status: 'M',
            },
        ];
        assert_eq!(
            suggest_commit_message(&docs).as_deref(),
            Some("docs: Update a.md, b.md")
        );

        let many: Vec<_> = (0..5)
            .map(|i| GitChange {
                path: PathBuf::from(format!("/r/f{i}.rs")),
                rel_path: format!("f{i}.rs"),
                old_rel_path: None,
                status: 'M',
            })
            .collect();
        assert_eq!(
            suggest_commit_message(&many).as_deref(),
            Some("Update f0.rs, f1.rs, f2.rs (+2 more)")
        );
    }
}
