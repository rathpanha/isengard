//! Minimal Source Control sidebar (CLI `git`).

mod diff_highlight;
mod engine;
mod panel;

pub use diff_highlight::{
    ConflictHunk, HunkSide, OverviewMark, apply_hunk_choices, conflict_hunks,
    line_change_counts, line_highlight_ranges, overview_marks,
};
pub use engine::{
    ConflictSide, RepoStatus, build_commit_suggest_prompt, change_at_path, conflict_file_sides,
    discover_repos, file_sides, load_workspace_status, scrub_commit_message,
    snapshot_dirty_contents, worktree_deltas_since_turn,
};
pub use panel::{GitPanel, GitPanelEvent};
