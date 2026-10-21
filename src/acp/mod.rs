//! Agent Client Protocol host: run external coding agents (Claude Agent by
//! default) as subprocesses speaking JSON-RPC over stdio — like LSP, but for
//! agents. The agent owns the model and login; Isengard answers its file and
//! permission requests. UI lives in `crate::agent`.

mod client;
mod files;
mod install;
mod registry;
#[cfg(test)]
pub mod testing;

pub use agent_client_protocol::schema::v1 as schema;
pub use client::{
    AgentConnection, AgentEvent, AgentInfo, FileRead, FileWrite, LoginMethod, PermissionAsk,
    SessionOpen,
};
pub use files::slice_lines;
pub use install::install_claude;
pub use registry::{AgentLaunch, CLAUDE_AGENT_NAME, Resolved, resolve};
