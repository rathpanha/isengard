//! Project-wide find & replace (sidebar view).

mod engine;
mod panel;

pub use engine::{SearchMatch, SearchQuery, apply_replace, run_search};
pub use panel::{SearchPanel, SearchPanelEvent};
