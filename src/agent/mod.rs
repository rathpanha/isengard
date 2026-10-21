//! Agent panel UI (chat with an external ACP agent). Protocol work lives in
//! `crate::acp`.

mod mention;
mod panel;
mod prompt;

pub use panel::{AgentPanel, AgentPanelEvent};
pub use prompt::Attachment;
