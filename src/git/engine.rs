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

/// In-progress Git operation that needs the user (or the terminal) to finish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoOp {
    Merge,
    Rebase,
    CherryPick,
}

impl RepoOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Merge => "Merge",
            Self::Rebase => "Rebase",
            Self::CherryPick => "Cherry-pick",
        }
    }
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
    /// Merge / rebase / cherry-pick in progress (if any).
    pub op: Option<RepoOp>,
    /// Set when `git` itself failed for this repo.
    pub error: Option<String>,
}

impl RepoStatus {
    pub fn conflict_count(&self) -> usize {
        self.changes.iter().filter(|c| c.status == 'U').count()
    }

    pub fn has_conflicts(&self) -> bool {
        self.conflict_count() > 0
    }

    /// Pull / Push / Discard must not run over an unfinished op or conflicts.
    pub fn blocks_sync_and_discard(&self) -> bool {
        self.op.is_some() || self.has_conflicts()
    }

    /// Rebase / cherry-pick finish with `--continue`, not our Commit button.
    pub fn blocks_commit(&self) -> bool {
        matches!(self.op, Some(RepoOp::Rebase) | Some(RepoOp::CherryPick))
    }

    pub fn blocks_suggest(&self) -> bool {
        self.op.is_some() || self.has_conflicts()
    }
}

/// Sidebar danger line for an in-progress op / unmerged paths.
pub fn conflict_banner(op: Option<RepoOp>, conflict_count: usize) -> Option<String> {
    let Some(op) = op else {
        if conflict_count == 0 {
            return None;
        }
        // Unmerged paths without MERGE_HEAD (unusual) — still warn.
        return Some(format!(
            "{conflict_count} conflict{} — open files to edit markers",
            if conflict_count == 1 { "" } else { "s" }
        ));
    };
    let noun = if conflict_count == 1 {
        "conflict"
    } else {
        "conflicts"
    };
    let tail = match op {
        RepoOp::Merge => {
            if conflict_count == 0 {
                "ready to commit".into()
            } else {
                format!("{conflict_count} {noun}")
            }
        }
        RepoOp::Rebase | RepoOp::CherryPick => {
            if conflict_count == 0 {
                "continue from the terminal".into()
            } else {
                format!("{conflict_count} {noun}")
            }
        }
    };
    Some(format!("{} in progress — {tail}", op.label()))
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        if !trim_stdout {
            return Ok(stdout.into_owned());
        }
        let out = stdout.trim_end();
        if !out.is_empty() {
            return Ok(out.to_owned());
        }
        // `git push` writes "Everything up-to-date" / "To …" on stderr.
        let err = stderr.trim_end();
        if !err.is_empty() {
            return Ok(err.to_owned());
        }
        return Ok(String::new());
    }
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

/// Current (stage 2 / ours) and Incoming (stage 3 / theirs) for a conflict.
///
/// Missing stage → empty string (deleted on that side).
pub fn conflict_file_sides(repo: &Path, rel_path: &str) -> Result<(String, String), String> {
    let ours = git_show_stage(repo, 2, rel_path).unwrap_or_default();
    let theirs = git_show_stage(repo, 3, rel_path).unwrap_or_default();
    if ours.is_empty() && theirs.is_empty() {
        return Err(format!("No conflict stages for {rel_path}"));
    }
    Ok((ours, theirs))
}

fn git_show_head(repo: &Path, rel_path: &str) -> Result<String, String> {
    // `git show HEAD:path` fails when the path was never committed.
    git_blob(repo, &["show", &format!("HEAD:{rel_path}")])
}

fn git_show_stage(repo: &Path, stage: u8, rel_path: &str) -> Result<String, String> {
    git_blob(repo, &["show", &format!(":{stage}:{rel_path}")])
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
    let op = detect_repo_op(root);
    match git(root, &["status", "--porcelain=v1", "-uall"]) {
        Ok(out) => RepoStatus {
            root: root.to_path_buf(),
            branch,
            changes: parse_porcelain(&out, root),
            ahead,
            behind,
            op,
            error: None,
        },
        Err(error) => RepoStatus {
            root: root.to_path_buf(),
            branch,
            changes: Vec::new(),
            ahead,
            behind,
            op,
            error: Some(error),
        },
    }
}

/// `MERGE_HEAD` / rebase state / `CHERRY_PICK_HEAD` via `git rev-parse --git-path`.
pub fn detect_repo_op(root: &Path) -> Option<RepoOp> {
    if git_path_exists(root, "MERGE_HEAD") {
        return Some(RepoOp::Merge);
    }
    if git_path_exists(root, "CHERRY_PICK_HEAD") {
        return Some(RepoOp::CherryPick);
    }
    if git_path_exists(root, "REBASE_HEAD")
        || git_path_exists(root, "rebase-merge")
        || git_path_exists(root, "rebase-apply")
    {
        return Some(RepoOp::Rebase);
    }
    None
}

fn git_path_exists(root: &Path, name: &str) -> bool {
    let Ok(raw) = git(root, &["rev-parse", "--git-path", name]) else {
        return false;
    };
    let path = PathBuf::from(raw.trim());
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    path.exists()
}

/// Default merge commit subject from `.git/MERGE_MSG` (comments stripped).
pub fn read_merge_msg(root: &Path) -> Option<String> {
    let Ok(raw) = git(root, &["rev-parse", "--git-path", "MERGE_MSG"]) else {
        return None;
    };
    let path = PathBuf::from(raw.trim());
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return None;
    };
    let body: String = text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let body = body.trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_owned())
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

/// Find the dirty/conflict entry for an absolute worktree path, if any.
pub fn change_at_path(
    statuses: &[RepoStatus],
    file: &Path,
) -> Option<(PathBuf, GitChange)> {
    for status in statuses {
        if let Some(change) = status.changes.iter().find(|c| c.path == file) {
            return Some((status.root.clone(), change.clone()));
        }
    }
    None
}

/// Stage `rel_paths` then `git commit -m` (empty selection / message rejected).
///
/// Refuses if any selected worktree file still contains conflict markers.
pub fn commit_paths(root: &Path, message: &str, rel_paths: &[String]) -> Result<(), String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Commit message is empty".into());
    }
    if rel_paths.is_empty() {
        return Err("No files selected".into());
    }
    for rel in rel_paths {
        if path_has_conflict_markers(root, rel) {
            return Err(format!(
                "Unresolved conflict markers in {rel} — Accept Current/Incoming or edit the file"
            ));
        }
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

/// Which side to keep for a whole-file conflict resolve (`git checkout --ours|--theirs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictSide {
    /// Current branch / HEAD (`--ours` during merge).
    Current,
    /// Incoming branch (`--theirs` during merge).
    Incoming,
}

impl ConflictSide {
    fn checkout_arg(self) -> &'static str {
        match self {
            Self::Current => "--ours",
            Self::Incoming => "--theirs",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::Incoming => "Incoming",
        }
    }
}

/// Whole-file resolve: `git checkout --ours|--theirs -- path` then `git add -- path`.
pub fn accept_conflict_side(
    root: &Path,
    rel_path: &str,
    side: ConflictSide,
) -> Result<(), String> {
    if detect_repo_op(root).is_none() {
        return Err("No merge/rebase/cherry-pick in progress".into());
    }
    git(
        root,
        &["checkout", side.checkout_arg(), "--", rel_path],
    )?;
    git(root, &["add", "--", rel_path])?;
    Ok(())
}

/// Write a fully merged conflict file and `git add` it (hunk / manual resolve).
pub fn write_resolved_conflict(
    root: &Path,
    rel_path: &str,
    content: &str,
) -> Result<(), String> {
    if detect_repo_op(root).is_none() {
        return Err("No merge/rebase/cherry-pick in progress".into());
    }
    if has_conflict_markers(content) {
        return Err("Resolved content still contains conflict markers".into());
    }
    let abs = root.join(rel_path);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&abs, content).map_err(|e| format!("Write {rel_path}: {e}"))?;
    git(root, &["add", "--", rel_path])?;
    Ok(())
}

/// True if `text` looks like it still has unresolved Git conflict markers.
pub fn has_conflict_markers(text: &str) -> bool {
    let mut saw_start = false;
    let mut saw_mid = false;
    for line in text.lines() {
        // Markers are typically at column 0; allow optional leading whitespace.
        let t = line.trim_start();
        if t.starts_with("<<<<<<<") {
            saw_start = true;
        } else if saw_start && t.starts_with("=======") {
            saw_mid = true;
        } else if saw_start && saw_mid && t.starts_with(">>>>>>>") {
            return true;
        }
    }
    false
}

fn path_has_conflict_markers(root: &Path, rel_path: &str) -> bool {
    let abs = root.join(rel_path);
    match std::fs::read_to_string(&abs) {
        Ok(text) => has_conflict_markers(&text),
        // Missing / binary — don't block commit on unreadable; git add will decide.
        Err(_) => false,
    }
}

pub fn pull_ff_only(root: &Path) -> Result<String, String> {
    if detect_repo_op(root).is_some() {
        return Err("Cannot pull while a merge/rebase/cherry-pick is in progress".into());
    }
    git(root, &["pull", "--ff-only"])
}

pub fn push(root: &Path) -> Result<String, String> {
    if detect_repo_op(root).is_some() {
        return Err("Cannot push while a merge/rebase/cherry-pick is in progress".into());
    }
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
    if status == 'U' {
        return Err("Cannot discard a conflicted file — open it and edit markers".into());
    }
    if detect_repo_op(root).is_some() {
        return Err("Cannot discard while a merge/rebase/cherry-pick is in progress".into());
    }
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
    if detect_repo_op(root).is_some() {
        return Err("Cannot discard all while a merge/rebase/cherry-pick is in progress".into());
    }
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

/// Soft ceiling so an AI Suggest turn does not swallow a multi‑MB diff.
const MAX_COMMIT_SUGGEST_CONTEXT: usize = 32_000;

/// Status + `git diff HEAD` for selected paths (capped), for AI Suggest prompts.
pub fn commit_suggest_context(repo: &Path, rel_paths: &[String]) -> String {
    if rel_paths.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut status_args: Vec<&str> = Vec::with_capacity(3 + rel_paths.len());
    status_args.extend_from_slice(&["status", "--short", "--"]);
    for p in rel_paths {
        status_args.push(p.as_str());
    }
    if let Ok(status) = git(repo, &status_args)
        && !status.is_empty()
    {
        out.push_str(&status);
        out.push_str("\n\n");
    }
    let mut diff_args: Vec<&str> = Vec::with_capacity(3 + rel_paths.len());
    diff_args.extend_from_slice(&["diff", "HEAD", "--"]);
    for p in rel_paths {
        diff_args.push(p.as_str());
    }
    if let Ok(diff) = git(repo, &diff_args)
        && !diff.is_empty()
    {
        out.push_str(&diff);
    }
    truncate_chars(&out, MAX_COMMIT_SUGGEST_CONTEXT)
}

/// Recent subject lines (newest first) so the agent can match repo style.
pub fn recent_commit_subjects(repo: &Path, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let n = limit.to_string();
    match git(repo, &["log", &format!("-{n}"), "--pretty=format:%s"]) {
        Ok(out) if !out.is_empty() => out.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

/// Prompt for any connected ACP agent (Claude or otherwise).
pub fn build_commit_suggest_prompt(repo: &Path, rel_paths: &[String]) -> String {
    let subjects = recent_commit_subjects(repo, 8);
    let recent = if subjects.is_empty() {
        "(none)".to_owned()
    } else {
        subjects
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let paths = rel_paths
        .iter()
        .map(|p| format!("- {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    let context = commit_suggest_context(repo, rel_paths);
    format!(
        "Write a git commit message for the selected changes below.\n\n\
         Rules:\n\
         - Reply with ONLY the commit message (subject line, optional short body).\n\
         - No quotes, no markdown fences, no preamble or explanation.\n\
         - Match the style of recent commits when possible.\n\
         - Imperative mood; keep the subject concise.\n\
         - Do not use tools.\n\n\
         Recent commits:\n{recent}\n\n\
         Selected paths:\n{paths}\n\n\
         Diff / status:\n{context}\n"
    )
}

/// Strip model fluff (fences, quotes) into a commit-box draft.
pub fn scrub_commit_message(raw: &str) -> String {
    let mut s = raw.trim().to_owned();
    if s.starts_with("```") {
        let mut lines = s.lines();
        let _ = lines.next();
        let body: Vec<&str> = lines.collect();
        let mut joined = body.join("\n");
        if let Some(stripped) = joined.strip_suffix("```") {
            joined = stripped.trim_end().to_owned();
        }
        s = joined.trim().to_owned();
    }
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s = s[1..s.len() - 1].trim().to_owned();
    }
    // Drop a leading "Commit message:" style label on the first line.
    if let Some(rest) = s
        .strip_prefix("Commit message:")
        .or_else(|| s.strip_prefix("commit message:"))
    {
        s = rest.trim().to_owned();
    }
    truncate_chars(&s, 4_000)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
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
///
/// Unmerged paths (`U`, both-added `AA`, both-deleted `DD`) sort first.
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
            let st = if is_unmerged(x, y) {
                'U'
            } else if x == 'R' || y == 'R' {
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
    changes.sort_by_key(|c| if c.status == 'U' { 0u8 } else { 1 });
    changes
}

fn is_unmerged(x: char, y: char) -> bool {
    // Porcelain unmerged: UU/AU/UA/DU/UD plus both-added AA / both-deleted DD.
    x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D')
}

fn status_letter(x: char, y: char) -> char {
    if x == '?' || y == '?' {
        return '?';
    }
    if is_unmerged(x, y) {
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
    fn has_conflict_markers_detects_standard_block() {
        assert!(!has_conflict_markers("plain\n"));
        assert!(!has_conflict_markers("<<<<<<< only start\n"));
        assert!(has_conflict_markers(
            "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature\n"
        ));
        assert!(has_conflict_markers(
            "  <<<<<<< HEAD\nours\n  =======\ntheirs\n  >>>>>>> feature\n"
        ));
    }

    #[test]
    fn accept_current_clears_conflict_and_commit_rejects_markers() {
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
        assert!(
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["config", "user.name", "Test"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("note.txt"), "base\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "note.txt"])
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
        assert!(
            Command::new("git")
                .args(["checkout", "-b", "other"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("note.txt"), "other\n").unwrap();
        assert!(
            Command::new("git")
                .args(["commit", "-am", "other"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["checkout", "-"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("note.txt"), "main\n").unwrap();
        assert!(
            Command::new("git")
                .args(["commit", "-am", "main"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            !Command::new("git")
                .args(["merge", "other"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(path_has_conflict_markers(&root, "note.txt"));
        let (ours, theirs) = conflict_file_sides(&root, "note.txt").unwrap();
        assert_eq!(ours, "main\n");
        assert_eq!(theirs, "other\n");
        assert!(
            commit_paths(&root, "merge", &["note.txt".into()])
                .unwrap_err()
                .contains("Unresolved conflict markers")
        );
        accept_conflict_side(&root, "note.txt", ConflictSide::Current).unwrap();
        assert!(!path_has_conflict_markers(&root, "note.txt"));
        assert_eq!(fs::read_to_string(root.join("note.txt")).unwrap(), "main\n");
        let status = load_repo_status(&root);
        assert!(!status.has_conflicts());
        assert_eq!(status.op, Some(RepoOp::Merge));
    }

    #[test]
    fn conflict_banner_copy() {
        assert_eq!(
            conflict_banner(Some(RepoOp::Merge), 2).as_deref(),
            Some("Merge in progress — 2 conflicts")
        );
        assert_eq!(
            conflict_banner(Some(RepoOp::Merge), 1).as_deref(),
            Some("Merge in progress — 1 conflict")
        );
        assert_eq!(
            conflict_banner(Some(RepoOp::Merge), 0).as_deref(),
            Some("Merge in progress — ready to commit")
        );
        assert_eq!(
            conflict_banner(Some(RepoOp::Rebase), 0).as_deref(),
            Some("Rebase in progress — continue from the terminal")
        );
        assert_eq!(
            conflict_banner(None, 1).as_deref(),
            Some("1 conflict — open files to edit markers")
        );
        assert_eq!(conflict_banner(None, 0), None);
    }

    #[test]
    fn parse_porcelain_unmerged_sorts_first() {
        let root = Path::new("/repo");
        let out = "\
M  ok.rs\n\
UU conflict.rs\n\
AA both_added.rs\n\
?? scratch.txt\n\
DD both_deleted.rs\n";
        let changes = parse_porcelain(out, root);
        assert_eq!(changes.len(), 5);
        assert!(changes[..3].iter().all(|c| c.status == 'U'));
        let u_paths: Vec<&str> = changes[..3].iter().map(|c| c.rel_path.as_str()).collect();
        assert!(u_paths.contains(&"conflict.rs"));
        assert!(u_paths.contains(&"both_added.rs"));
        assert!(u_paths.contains(&"both_deleted.rs"));
        assert_eq!(changes[3].status, 'M');
        assert_eq!(changes[4].status, '?');
    }

    #[test]
    fn detect_merge_op_and_merge_msg() {
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
        // Avoid depending on the machine's git identity.
        assert!(
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["config", "user.name", "Test"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("note.txt"), "base\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "note.txt"])
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
        assert!(
            Command::new("git")
                .args(["checkout", "-b", "other"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("note.txt"), "other\n").unwrap();
        assert!(
            Command::new("git")
                .args(["commit", "-am", "other"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["checkout", "-"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("note.txt"), "main\n").unwrap();
        assert!(
            Command::new("git")
                .args(["commit", "-am", "main"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let merge = Command::new("git")
            .args(["merge", "other"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(!merge.status.success(), "expected conflict");
        assert_eq!(detect_repo_op(&root), Some(RepoOp::Merge));
        let status = load_repo_status(&root);
        assert_eq!(status.op, Some(RepoOp::Merge));
        assert!(status.has_conflicts());
        assert!(status.blocks_sync_and_discard());
        assert!(!status.blocks_commit());
        let msg = read_merge_msg(&root).expect("MERGE_MSG");
        assert!(msg.to_ascii_lowercase().contains("merge"));
        assert!(
            discard_file(&root, "note.txt", 'U', None).is_err(),
            "discard conflict must fail"
        );
        assert!(discard_all(&root).is_err(), "discard-all mid-merge must fail");
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
    fn scrub_commit_message_strips_fences_and_labels() {
        assert_eq!(
            scrub_commit_message("```\nfix: handle empty list\n```"),
            "fix: handle empty list"
        );
        assert_eq!(
            scrub_commit_message("Commit message: Add widget"),
            "Add widget"
        );
        assert_eq!(scrub_commit_message("\"docs: tweak README\""), "docs: tweak README");
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

    #[test]
    fn push_noop_returns_everything_up_to_date() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let bare = tmp.path().join("bare.git");
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
        assert!(
            Command::new("git")
                .args(["commit", "--allow-empty", "-m", "init"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["init", "--bare"])
                .arg(&bare)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["remote", "add", "origin"])
                .arg(&bare)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["push", "-u", "origin", "HEAD"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let msg = push(&root).unwrap();
        assert_eq!(msg.trim(), "Everything up-to-date");
    }
}
