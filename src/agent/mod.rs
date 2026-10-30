//! Agent panel UI (chat with an external ACP agent). Protocol work lives in
//! `crate::acp`.

pub(crate) mod limits;
mod mention;
mod panel;
mod prompt;
mod slash;

pub use panel::{AgentPanel, AgentPanelEvent, AgentPanelOpen};
pub use prompt::{Attachment, LineRange, image_from_path, line_at_offset};
