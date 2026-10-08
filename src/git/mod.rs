//! Minimal Source Control sidebar (CLI `git`).

mod diff_highlight;
mod engine;
mod panel;

pub use diff_highlight::{OverviewMark, line_highlight_ranges, overview_marks};
pub use engine::{discover_repos, file_sides};
pub use panel::{GitPanel, GitPanelEvent};
