//! Agent panel UI (chat with an external ACP agent). Protocol work lives in
//! `crate::acp`.

mod limits;
mod mention;
mod panel;
mod prompt;

pub use panel::{AgentPanel, AgentPanelEvent};
pub use prompt::{Attachment, image_from_path};
