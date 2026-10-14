//! Language Server Protocol client (IntelliSense).
//!
//! Kit owns completion / hover / definition UI; this module speaks JSON-RPC to
//! servers. Binaries resolve **app-local first** (`…/isengard/lsp/bin`), then
//! system `PATH`. Install downloads into the app data dir (no global `-g` /
//! brew / pip into the user toolchain).

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
pub use install::{
    InstallRecipe, is_app_local, recipe_for_command, run_install, run_uninstall,
};
