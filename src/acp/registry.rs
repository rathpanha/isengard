//! Which agent process to launch: `config.json` `agent.command`, else the
//! built-in Claude Agent (app-local install first, then `PATH`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::acp::install::{CLAUDE_BIN, agents_root, claude_bin_in};
use crate::config::AgentSettings;

/// Display name until the agent reports its own `agentInfo.title`.
pub const CLAUDE_AGENT_NAME: &str = "Claude Agent";

/// A resolved agent command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// What to launch for these settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    Launch(AgentLaunch),
    /// Built-in Claude Agent is neither installed app-locally nor on `PATH`.
    ClaudeNotInstalled,
}

pub fn resolve(settings: &AgentSettings) -> Resolved {
    resolve_in(settings, &agents_root(), std::env::var_os("PATH").as_deref())
}

pub fn resolve_in(
    settings: &AgentSettings,
    agents_root: &Path,
    path_var: Option<&std::ffi::OsStr>,
) -> Resolved {
    if let Some(command) = settings.command.as_deref().filter(|c| !c.trim().is_empty()) {
        // Custom agent: spawn errors surface in the panel if it is missing.
        let command = find_on_path(command, path_var).unwrap_or_else(|| PathBuf::from(command));
        return Resolved::Launch(AgentLaunch {
            command,
            args: settings.args.clone(),
            env: settings.env.clone(),
        });
    }
    let local = claude_bin_in(agents_root);
    let command = if local.is_file() {
        Some(local)
    } else {
        find_on_path(CLAUDE_BIN, path_var)
    };
    match command {
        Some(command) => {
            let mut env = BTreeMap::new();
            // Claude Code: default (and force) model for Agent/Explore/Plan subagents.
            if let Some(model) = settings
                .subagent_model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != "inherit")
            {
                env.insert("CLAUDE_CODE_SUBAGENT_MODEL".into(), model.to_owned());
                env.insert("CLAUDE_CODE_SUBAGENT_MODEL_FORCE".into(), "1".into());
            }
            // The adapter is a `#!/usr/bin/env node` script: run it on Bun.
            let (command, args) = crate::runtime::run_on_bun(command, Vec::new());
            Resolved::Launch(AgentLaunch { command, args, env })
        }
        None => Resolved::ClaudeNotInstalled,
    }
}

fn find_on_path(command: &str, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() || command.contains('/') || command.contains('\\') {
        return path.is_file().then(|| path.to_path_buf());
    }
    for dir in std::env::split_paths(path_var?) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        for ext in ["exe", "cmd"] {
            let candidate = dir.join(format!("{command}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn claude_missing_everywhere_is_not_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty-path");
        assert_eq!(
            resolve_in(&AgentSettings::default(), tmp.path(), Some(empty.as_os_str())),
            Resolved::ClaudeNotInstalled
        );
    }

    #[test]
    fn app_local_claude_wins_over_path() {
        let tmp = tempfile::tempdir().unwrap();
        let local = claude_bin_in(&tmp.path().join("agents"));
        touch(&local);
        let path_dir = tmp.path().join("bin");
        touch(&path_dir.join(CLAUDE_BIN));
        let Resolved::Launch(launch) = resolve_in(
            &AgentSettings::default(),
            &tmp.path().join("agents"),
            Some(path_dir.as_os_str()),
        ) else {
            panic!("expected a launch");
        };
        assert_eq!(launch.command, local);
    }

    #[test]
    fn claude_on_path_is_used_when_not_app_local() {
        let tmp = tempfile::tempdir().unwrap();
        let path_dir = tmp.path().join("bin");
        touch(&path_dir.join(CLAUDE_BIN));
        let Resolved::Launch(launch) =
            resolve_in(&AgentSettings::default(), tmp.path(), Some(path_dir.as_os_str()))
        else {
            panic!("expected a launch");
        };
        assert_eq!(launch.command, path_dir.join(CLAUDE_BIN));
    }

    #[test]
    fn custom_command_keeps_args_and_env_even_if_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = AgentSettings {
            command: Some("gemini".into()),
            args: vec!["--experimental-acp".into()],
            env: BTreeMap::from([("A".into(), "1".into())]),
            silent_model: None,
            subagent_model: None,
        };
        let Resolved::Launch(launch) = resolve_in(&settings, tmp.path(), None) else {
            panic!("expected a launch");
        };
        assert_eq!(launch.command, PathBuf::from("gemini"));
        assert_eq!(launch.args, ["--experimental-acp"]);
        assert_eq!(launch.env.get("A").map(String::as_str), Some("1"));
    }

    #[test]
    fn claude_launch_injects_subagent_model_env() {
        let tmp = tempfile::tempdir().unwrap();
        let local = claude_bin_in(&tmp.path().join("agents"));
        touch(&local);
        let settings = AgentSettings {
            subagent_model: Some("haiku".into()),
            ..AgentSettings::default()
        };
        let Resolved::Launch(launch) =
            resolve_in(&settings, &tmp.path().join("agents"), None)
        else {
            panic!("expected a launch");
        };
        assert_eq!(
            launch.env.get("CLAUDE_CODE_SUBAGENT_MODEL").map(String::as_str),
            Some("haiku")
        );
        assert_eq!(
            launch
                .env
                .get("CLAUDE_CODE_SUBAGENT_MODEL_FORCE")
                .map(String::as_str),
            Some("1")
        );
    }
}
