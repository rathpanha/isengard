//! Which agent process to launch for a panel: workspace-remembered slot
//! (Claude / Cursor) overlaid on global silent/subagent settings, or a custom
//! `config.json` `agent.command` (tests / gemini / …).
//!
//! Built-in Cursor uses slot `"cursor-agent"` (app-local `…/agents/cursor/`
//! first, then `PATH`; args default to `["acp"]`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::acp::install::{
    CLAUDE_BIN, CURSOR_BIN, agents_root, claude_bin_in, cursor_bin_in,
};
use crate::config::AgentSettings;

/// Display name until the agent reports its own `agentInfo.title`.
pub const CLAUDE_AGENT_NAME: &str = "Claude Agent";
/// Display name for the built-in Cursor Agent before `initialize`.
pub const CURSOR_AGENT_NAME: &str = "Cursor Agent";

/// Config `agent.command` value that selects the built-in Cursor Agent.
pub const CURSOR_COMMAND: &str = "cursor-agent";

/// Built-in agents the panel picker / switcher offers (not custom `agent.command`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinAgent {
    Claude,
    Cursor,
}

impl BuiltinAgent {
    pub fn slot_id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Cursor => CURSOR_COMMAND,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => CLAUDE_AGENT_NAME,
            Self::Cursor => CURSOR_AGENT_NAME,
        }
    }

    /// Material Icon Theme stem (`claude`, `cursor`).
    pub fn icon_stem(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Cursor => "cursor",
        }
    }
}

/// Claude / Cursor when `agent.command` is empty or `"cursor-agent"`; else custom.
pub fn builtin_agent(settings: &AgentSettings) -> Option<BuiltinAgent> {
    match settings
        .command
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        None => Some(BuiltinAgent::Claude),
        Some(CURSOR_COMMAND) => Some(BuiltinAgent::Cursor),
        Some(_) => None,
    }
}

/// Apply a built-in choice onto existing settings (keeps silent / subagent model).
pub fn with_builtin(settings: &AgentSettings, agent: BuiltinAgent) -> AgentSettings {
    with_agent_slot(settings, agent.slot_id())
}

/// Overlay a workspace slot id onto global agent settings (silent / subagent kept).
/// Does not read or write `config.json` `agent.command` for Claude / Cursor —
/// those are chosen per folder/workspace.
pub fn with_agent_slot(settings: &AgentSettings, slot: &str) -> AgentSettings {
    let mut next = settings.clone();
    match slot.trim() {
        "" | "claude" => {
            next.command = None;
            if next.args == ["acp".to_owned()] {
                next.args.clear();
            }
        }
        CURSOR_COMMAND => {
            next.command = Some(CURSOR_COMMAND.into());
        }
        other => {
            next.command = Some(other.to_owned());
        }
    }
    next
}

/// Launch settings + whether the empty-slot Claude/Cursor picker should show.
///
/// Built-in selection is per workspace (`remembered`). Global `agent.command`
/// is only used when it names a **custom** agent (tests / gemini / …).
pub fn launch_for_workspace(
    global: &AgentSettings,
    remembered: Option<&str>,
) -> (AgentSettings, bool) {
    if let Some(slot) = remembered.map(str::trim).filter(|s| !s.is_empty()) {
        return (with_agent_slot(global, slot), false);
    }
    if builtin_agent(global).is_none() {
        // Custom global command — no picker.
        return (global.clone(), false);
    }
    // Ignore leftover Claude/Cursor `agent.command` in config; ask once per folder.
    let mut placeholder = global.clone();
    placeholder.command = None;
    if placeholder.args == ["acp".to_owned()] {
        placeholder.args.clear();
    }
    (placeholder, true)
}

/// Slot id for chat isolation / panel cache (`claude`, `cursor-agent`, …).
pub fn agent_slot_id(settings: &AgentSettings) -> String {
    match builtin_agent(settings) {
        Some(agent) => agent.slot_id().into(),
        None => settings
            .command
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .unwrap_or("claude")
            .to_owned(),
    }
}

/// In-memory panel map key: workspace session key + agent slot.
pub fn panel_key(workspace_key: &str, settings: &AgentSettings) -> String {
    format!("{workspace_key}\x1e{}", agent_slot_id(settings))
}

/// Split [`panel_key`] into `(workspace_session_key, agent_slot)`.
pub fn split_panel_key(key: &str) -> Option<(&str, &str)> {
    key.split_once('\x1e')
}

/// A resolved agent command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// The built-in Claude adapter: its sessions list the `opusplan` Model alias.
    pub claude: bool,
}

/// What to launch for these settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    Launch(AgentLaunch),
    /// Built-in Claude Agent is neither installed app-locally nor on `PATH`.
    ClaudeNotInstalled,
    /// Built-in Cursor Agent is neither installed app-locally nor on `PATH`.
    CursorNotInstalled,
}

pub fn resolve(settings: &AgentSettings) -> Resolved {
    resolve_in(settings, &agents_root(), std::env::var_os("PATH").as_deref())
}

pub fn resolve_in(
    settings: &AgentSettings,
    agents_root: &Path,
    path_var: Option<&std::ffi::OsStr>,
) -> Resolved {
    if wants_cursor(settings) {
        return resolve_cursor(settings, agents_root, path_var);
    }
    if let Some(command) = settings.command.as_deref().filter(|c| !c.trim().is_empty()) {
        // Custom agent: spawn errors surface in the panel if it is missing.
        let command = find_on_path(command, path_var).unwrap_or_else(|| PathBuf::from(command));
        return Resolved::Launch(AgentLaunch {
            command,
            args: settings.args.clone(),
            env: settings.env.clone(),
            claude: false,
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
            Resolved::Launch(AgentLaunch {
                command,
                args,
                env,
                claude: true,
            })
        }
        None => Resolved::ClaudeNotInstalled,
    }
}

fn wants_cursor(settings: &AgentSettings) -> bool {
    settings
        .command
        .as_deref()
        .map(str::trim)
        .is_some_and(|c| c == CURSOR_COMMAND)
}

fn resolve_cursor(
    settings: &AgentSettings,
    agents_root: &Path,
    path_var: Option<&std::ffi::OsStr>,
) -> Resolved {
    let local = cursor_bin_in(agents_root);
    let command = if local.is_file() {
        Some(local)
    } else {
        find_on_path(CURSOR_BIN, path_var).or_else(|| find_on_path("agent", path_var))
    };
    match command {
        Some(command) => {
            let args = if settings.args.is_empty() {
                vec!["acp".into()]
            } else {
                settings.args.clone()
            };
            Resolved::Launch(AgentLaunch {
                command,
                args,
                env: settings.env.clone(),
                claude: false,
            })
        }
        None => Resolved::CursorNotInstalled,
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
    use crate::acp::install::CURSOR_BIN;

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
        assert!(!launch.claude, "a custom agent is not the Claude adapter");
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
        assert!(launch.claude, "the built-in adapter gets the Claude session settings");
    }

    #[test]
    fn cursor_command_defaults_args_to_acp_and_prefers_app_local() {
        let tmp = tempfile::tempdir().unwrap();
        let agents = tmp.path().join("agents");
        let local = cursor_bin_in(&agents);
        touch(&local);
        let path_dir = tmp.path().join("bin");
        touch(&path_dir.join(CURSOR_BIN));
        let settings = AgentSettings {
            command: Some(CURSOR_COMMAND.into()),
            ..AgentSettings::default()
        };
        let Resolved::Launch(launch) =
            resolve_in(&settings, &agents, Some(path_dir.as_os_str()))
        else {
            panic!("expected a launch");
        };
        assert_eq!(launch.command, local);
        assert_eq!(launch.args, ["acp"]);
    }

    #[test]
    fn cursor_missing_is_not_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty-path");
        let settings = AgentSettings {
            command: Some(CURSOR_COMMAND.into()),
            ..AgentSettings::default()
        };
        assert_eq!(
            resolve_in(&settings, tmp.path(), Some(empty.as_os_str())),
            Resolved::CursorNotInstalled
        );
    }

    #[test]
    fn builtin_agent_and_with_builtin_round_trip() {
        assert_eq!(builtin_agent(&AgentSettings::default()), Some(BuiltinAgent::Claude));
        let cursor = with_builtin(&AgentSettings::default(), BuiltinAgent::Cursor);
        assert_eq!(builtin_agent(&cursor), Some(BuiltinAgent::Cursor));
        assert_eq!(agent_slot_id(&cursor), CURSOR_COMMAND);
        let mut leftover = cursor.clone();
        leftover.args = vec!["acp".into()];
        let back = with_builtin(&leftover, BuiltinAgent::Claude);
        assert!(back.command.is_none());
        assert!(back.args.is_empty(), "drop Cursor default args for Claude");
        let custom = AgentSettings {
            command: Some("gemini".into()),
            ..AgentSettings::default()
        };
        assert_eq!(builtin_agent(&custom), None);
    }

    #[test]
    fn launch_for_workspace_picks_until_remembered() {
        let global = AgentSettings::default();
        let (settings, pick) = launch_for_workspace(&global, None);
        assert!(pick);
        assert!(settings.command.is_none());
        let (cursor, pick) = launch_for_workspace(&global, Some(CURSOR_COMMAND));
        assert!(!pick);
        assert_eq!(cursor.command.as_deref(), Some(CURSOR_COMMAND));
        // Leftover config command must not skip the picker.
        let stale = AgentSettings {
            command: Some(CURSOR_COMMAND.into()),
            ..AgentSettings::default()
        };
        let (_, pick) = launch_for_workspace(&stale, None);
        assert!(pick, "ignore global cursor-agent until workspace remembers");
        let custom = AgentSettings {
            command: Some("gemini".into()),
            ..AgentSettings::default()
        };
        let (_, pick) = launch_for_workspace(&custom, None);
        assert!(!pick, "custom global command skips picker");
    }
}
