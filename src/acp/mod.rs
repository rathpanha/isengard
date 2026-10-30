//! Agent Client Protocol host: run external coding agents (Claude Agent by
//! default) as subprocesses speaking JSON-RPC over stdio — like LSP, but for
//! agents. The agent owns the model and login; Isengard answers its file and
//! permission requests. UI lives in `crate::agent`.

mod client;
mod cursor_ext;
mod files;
mod install;
mod registry;
#[cfg(test)]
pub mod testing;

pub use agent_client_protocol::schema::v1 as schema;
pub use client::{
    AgentConnection, AgentEvent, AgentInfo, CreatePlanAsk, FileRead, FileWrite, LoginMethod,
    PermissionAsk, SessionOpen, SilentConfigValue,
};
pub use cursor_ext::{CursorCreatePlanOutcome, CursorPlanTodoStatus};
pub use files::slice_lines;
pub use install::{install_claude, install_cursor};
pub use registry::{
    AgentLaunch, BuiltinAgent, CLAUDE_AGENT_NAME, CURSOR_AGENT_NAME, CURSOR_COMMAND, Resolved,
    agent_slot_id, builtin_agent, launch_for_workspace, panel_key, resolve, split_panel_key,
    with_builtin,
};
