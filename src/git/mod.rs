//! Minimal Source Control sidebar (CLI `git`).

mod diff_highlight;
mod engine;
mod panel;

pub use diff_highlight::{
    ConflictHunk, HunkSide, OverviewMark, apply_hunk_choices, conflict_hunks,
    line_highlight_ranges, overview_marks,
};
pub use engine::{
    ConflictSide, RepoStatus, change_at_path, conflict_file_sides, discover_repos,
    file_sides, load_workspace_status,
};
pub use panel::{GitPanel, GitPanelEvent};
