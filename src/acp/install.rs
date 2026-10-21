//! App-local install of the built-in Claude Agent adapter.
//!
//! `npm install --prefix …/isengard/agents/claude` — never `npm -g`. Needs
//! system `node` / `npm` as the runtime only (same rule as Node-based LSPs).

use std::path::{Path, PathBuf};

/// npm package that speaks ACP on top of the Claude Agent SDK.
pub const CLAUDE_PACKAGE: &str = "@agentclientprotocol/claude-agent-acp";
/// Pinned on purpose — upgrade together with the `agent-client-protocol` crate.
pub const CLAUDE_VERSION: &str = "0.94.0";
/// Executable name in `node_modules/.bin` (and on `PATH` for global installs).
pub const CLAUDE_BIN: &str = "claude-agent-acp";

/// Root: `…/isengard/agents`.
pub fn agents_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("isengard")
        .join("agents")
}

fn claude_dir(root: &Path) -> PathBuf {
    root.join("claude")
}

/// `node_modules/.bin/claude-agent-acp` under `root` (may not exist).
pub fn claude_bin_in(root: &Path) -> PathBuf {
    let bin = claude_dir(root).join("node_modules").join(".bin");
    if cfg!(windows) {
        bin.join(format!("{CLAUDE_BIN}.cmd"))
    } else {
        bin.join(CLAUDE_BIN)
    }
}

/// Installs the pinned adapter app-locally (blocking — run off the UI thread).
pub fn install_claude() -> Result<(), String> {
    install_claude_in(&agents_root())
}

pub fn install_claude_in(root: &Path) -> Result<(), String> {
    crate::lsp::npm_install(
        &claude_dir(root),
        &[format!("{CLAUDE_PACKAGE}@{CLAUDE_VERSION}")],
    )?;
    let bin = claude_bin_in(root);
    if bin.is_file() {
        Ok(())
    } else {
        Err(format!(
            "npm install succeeded but `{CLAUDE_BIN}` is missing under node_modules/.bin"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_bin_lives_under_node_modules_bin() {
        let bin = claude_bin_in(Path::new("/data/agents"));
        assert!(bin.starts_with("/data/agents/claude/node_modules/.bin"));
        assert!(bin.to_string_lossy().contains(CLAUDE_BIN));
    }
}
