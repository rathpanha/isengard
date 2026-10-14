//! Language Server Protocol client (IntelliSense).
//!
//! Kit owns completion / hover / definition UI; this module speaks JSON-RPC to
//! servers on PATH. MVP: rust-analyzer for [`Language::Rust`](crate::editor::language::Language::Rust).

mod bridge;
mod client;
mod host;
mod install;
mod protocol;
mod registry;
mod uri;

pub use client::LspClient;
pub use host::{
    AttachOutcome, CatalogServer, LspEvent, LspHost, MissingServerOffer, ServerDisplayStatus,
    apply_diagnostics, path_for_diagnostics,
};
pub use install::{InstallRecipe, recipe_for_command, run_install, run_uninstall};
