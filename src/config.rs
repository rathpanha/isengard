use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_EDITOR_FONT_SIZE: f32 = 14.0;
pub const MIN_FONT_SIZE: f32 = 9.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
/// Default main window size (also the first-launch fallback).
pub const DEFAULT_WINDOW_WIDTH: f32 = 1280.0;
pub const DEFAULT_WINDOW_HEIGHT: f32 = 800.0;
pub const MIN_WINDOW_WIDTH: f32 = 640.0;
pub const MIN_WINDOW_HEIGHT: f32 = 400.0;
/// Default file-tree sidebar width in the main horizontal split.
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 260.0;
/// Default integrated terminal height in the editor vertical split.
pub const DEFAULT_TERMINAL_HEIGHT: f32 = 200.0;
/// Default Agent panel width on the right of the window.
pub const DEFAULT_AGENT_WIDTH: f32 = 380.0;
/// Cap for each recent list (folders and workspaces).
pub const MAX_RECENT: usize = 8;
/// Cap for persisted workspace sessions (tabs + tree expand).
pub const MAX_SESSIONS: usize = 16;
/// Max bytes of unsaved buffer text stored per tab in `sessions`.
pub const MAX_SESSION_UNSAVED_BYTES: usize = 512 * 1024;

/// Cursor + scroll for one open text tab (images omit these / use defaults).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileViewState {
    pub cursor_line: u32,
    pub cursor_character: u32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    /// Dirty / untitled buffer text (omitted when clean on-disk tabs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsaved_text: Option<String>,
    /// Save-dialog folder for an `untitled:` tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub untitled_save_dir: Option<PathBuf>,
}

/// Which left sidebar strip is selected (Explorer / Source Control / Search).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarPanel {
    #[default]
    Files,
    Search,
    Git,
}

/// Project Search fields (results are recomputed on restore).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSessionState {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pattern: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub replace: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub case_sensitive: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub whole_word: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub use_regex: bool,
}

impl SearchSessionState {
    pub fn is_empty(&self) -> bool {
        self.pattern.is_empty()
            && self.replace.is_empty()
            && !self.case_sensitive
            && !self.whole_word
            && !self.use_regex
    }
}

/// Source Control UI for one git root (draft + selection + commit-box floor).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitRepoSession {
    pub root: PathBuf,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<String>,
    /// Commit box drag floor in px (`0` = auto height only).
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub message_floor_h: f32,
    /// Repo section folded to its header row.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
}

fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

/// One open tab in a session. Plain path strings from older configs still load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenFileEntry {
    Path(PathBuf),
    View {
        path: PathBuf,
        #[serde(flatten)]
        view: FileViewState,
    },
}

impl OpenFileEntry {
    pub fn path(&self) -> &Path {
        match self {
            Self::Path(path) | Self::View { path, .. } => path,
        }
    }

    pub fn view(&self) -> FileViewState {
        match self {
            Self::Path(_) => FileViewState::default(),
            Self::View { view, .. } => view.clone(),
        }
    }

    pub fn with_view(path: PathBuf, view: FileViewState) -> Self {
        Self::View { path, view }
    }
}

fn default_sidebar_width() -> f32 {
    DEFAULT_SIDEBAR_WIDTH
}

fn default_sidebar_visible() -> bool {
    true
}

fn default_terminal_height() -> f32 {
    DEFAULT_TERMINAL_HEIGHT
}

/// Open tabs and expanded folders for one workspace / folder key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceSession {
    pub open_files: Vec<OpenFileEntry>,
    pub active: Option<PathBuf>,
    pub expanded: Vec<PathBuf>,
    /// Integrated terminal tab cwds (shells are respawned on restore).
    pub terminal_cwds: Vec<PathBuf>,
    /// Index into `terminal_cwds` for the selected tab.
    pub terminal_active: usize,
    /// Whether the terminal panel was visible for this workspace.
    pub terminal_visible: bool,
    /// File-tree sidebar width (px) in `main-split`.
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    /// Whether the file-tree sidebar is shown.
    #[serde(default = "default_sidebar_visible")]
    pub sidebar_visible: bool,
    /// Integrated terminal panel height (px) in `editor-term-split`.
    #[serde(default = "default_terminal_height")]
    pub terminal_height: f32,
    /// Whether the right-hand Agent panel was shown.
    pub agent_visible: bool,
    /// Agent panel width (px) in `agent-split`.
    #[serde(default = "default_agent_width")]
    pub agent_width: f32,
    /// Agent chats started in this folder/workspace, newest first. The agent
    /// stores the transcripts; this is only Isengard's index of which belong here.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub agent_chats: Vec<AgentChat>,
    /// Chat reopened when the panel connects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_active: Option<String>,
    /// Composer Mode / Model / Effort / … last picked in this folder/workspace.
    #[serde(skip_serializing_if = "AgentPrefs::is_empty")]
    pub agent_prefs: AgentPrefs,
    /// Explorer / Source Control / Search strip selection.
    #[serde(default)]
    pub sidebar_panel: SidebarPanel,
    /// Project Search query + toggles (hit list is rebuilt on restore).
    #[serde(default, skip_serializing_if = "SearchSessionState::is_empty")]
    pub search: SearchSessionState,
    /// Commit drafts / selections / box height per git root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git_repos: Vec<GitRepoSession>,
}

/// Agent composer choices re-applied to every chat the panel opens (new,
/// loaded, resumed). Keyed by ACP config option id; values the agent no longer
/// offers are skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentPrefs {
    /// Legacy ACP `modes` id, for agents without a Mode config option.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, AgentPrefValue>,
    /// Model in use before the OpusPlan Switch turned on — restored when it
    /// turns off (survives restarts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_before_opusplan: Option<String>,
}

impl AgentPrefs {
    pub fn is_empty(&self) -> bool {
        self.mode.is_none() && self.config.is_empty() && self.model_before_opusplan.is_none()
    }
}

/// Select value id or boolean toggle (ACP `session/set_config_option`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AgentPrefValue {
    Bool(bool),
    Value(String),
}

/// Cap for remembered agent chats per folder/workspace.
pub const MAX_AGENT_CHATS: usize = 50;

/// One row in a persisted end-of-turn **N Files** card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFileChange {
    pub path: PathBuf,
    pub added: u32,
    pub removed: u32,
    #[serde(default)]
    pub deleted: bool,
}

/// Files the agent touched in one user turn (restored after `session/load`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFileChangesCard {
    /// 0-based index among `User` bubbles in the transcript.
    pub turn: u32,
    pub files: Vec<AgentFileChange>,
}

/// One agent chat (ACP session) in the per-workspace index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentChat {
    /// ACP `sessionId`.
    pub id: String,
    pub title: String,
    /// Last prompt time, Unix seconds.
    pub updated: u64,
    /// End-of-turn file-change cards (ACP replay has no equivalent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_changes: Vec<AgentFileChangesCard>,
}

fn default_agent_width() -> f32 {
    DEFAULT_AGENT_WIDTH
}

impl Default for WorkspaceSession {
    fn default() -> Self {
        Self {
            open_files: Vec::new(),
            active: None,
            expanded: Vec::new(),
            terminal_cwds: Vec::new(),
            terminal_active: 0,
            terminal_visible: false,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_visible: true,
            terminal_height: DEFAULT_TERMINAL_HEIGHT,
            agent_visible: false,
            agent_width: DEFAULT_AGENT_WIDTH,
            agent_chats: Vec::new(),
            agent_active: None,
            agent_prefs: AgentPrefs::default(),
            sidebar_panel: SidebarPanel::default(),
            search: SearchSessionState::default(),
            git_repos: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_tab_size() -> u32 {
    2
}

/// Per-language LSP overrides under `lsp.languages.<id>` (ids from
/// [`crate::lsp::registry::config_key`], e.g. `rust`, `python`, `typescript`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LspLanguageSettings {
    /// `false` disables this language. Omitted → on when a default server exists
    /// (or when `command` is set for a language without a built-in default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Replace the built-in executable (app-local `lsp/bin` then `PATH`,
    /// unless absolute).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Replace the built-in argv (use `[]` for no args).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

/// Language servers are not inside the app binary — Install downloads into
/// `data_dir/isengard/lsp/`, or point at PATH binaries (defaults are common
/// command names).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LspSettings {
    /// Master switch for all language servers.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Run `textDocument/formatting` before writing a disk-backed tab.
    #[serde(default = "default_true")]
    pub format_on_save: bool,
    /// Per-language enable / command / args overrides.
    #[serde(default)]
    pub languages: BTreeMap<String, LspLanguageSettings>,
    /// Process keys (usually binary names) the user has installed / used —
    /// shown in the status bar even when idle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known: Vec<String>,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            format_on_save: true,
            languages: BTreeMap::new(),
            known: Vec::new(),
        }
    }
}

/// Update checking. Releases are looked up on GitHub at most once a day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Check for a newer release at startup (Isengard > Check for Updates…
    /// always works).
    #[serde(default = "default_true")]
    pub check: bool,
    /// Unix seconds of the last completed check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<u64>,
    /// Version the user chose "Skip this version" for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_version: Option<String>,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check: true,
            last_check: None,
            skipped_version: None,
        }
    }
}

/// External ACP agent for the Agent panel. Empty → built-in Claude Agent
/// (app-local install, then `claude-agent-acp` on `PATH`). The agent owns the
/// model and login; Isengard never stores API keys.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    /// Executable speaking ACP over stdio (absolute, or looked up on `PATH`),
    /// e.g. `"gemini"` with `args: ["--experimental-acp"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// ACP Model option value id for silent jobs (Improve / chat title /
    /// Git Suggest). `None` → inherit the open chat’s model. Invalid / stale
    /// ids are ignored at prompt time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silent_model: Option<String>,
    /// Default model for Claude Code subagents (`CLAUDE_CODE_SUBAGENT_MODEL`).
    /// `None` → Claude’s built-in order (Explore may use a different model).
    /// Also sets `CLAUDE_CODE_SUBAGENT_MODEL_FORCE=1` so Explore/Plan follow it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_model: Option<String>,
}

/// User-facing settings, persisted as JSON in the platform config directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Code editor font size in pixels.
    pub font_size: f32,
    /// Editor tab width (spaces). Overridable per workspace file.
    #[serde(default = "default_tab_size")]
    pub tab_size: u32,
    /// Agent panel launch command — see [`AgentSettings`].
    #[serde(default)]
    pub agent: AgentSettings,
    /// Last Agent composer choices anywhere; seeds folders/workspaces that
    /// have none of their own yet.
    #[serde(default, skip_serializing_if = "AgentPrefs::is_empty")]
    pub agent_prefs: AgentPrefs,
    /// Language Server Protocol (IntelliSense) — see [`LspSettings`].
    #[serde(default)]
    pub lsp: LspSettings,
    /// Automatic update checks — see [`UpdateSettings`].
    #[serde(default)]
    pub updates: UpdateSettings,
    /// Last windowed (or restore-from-maximized) width in pixels.
    pub window_width: f32,
    /// Last windowed (or restore-from-maximized) height in pixels.
    pub window_height: f32,
    /// Last window origin X, if known (absent → center on open).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_x: Option<f32>,
    /// Last window origin Y, if known (absent → center on open).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_y: Option<f32>,
    /// Whether the window was maximized when last saved.
    #[serde(default)]
    pub window_maximized: bool,
    /// Most recently opened first.
    pub recent_folders: Vec<PathBuf>,
    /// `.isengard-workspace` files, most recently opened first.
    pub recent_workspaces: Vec<PathBuf>,
    /// Last tabs + tree + terminal layout, keyed by workspace session key.
    pub sessions: BTreeMap<String, WorkspaceSession>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_EDITOR_FONT_SIZE,
            tab_size: default_tab_size(),
            agent: AgentSettings::default(),
            agent_prefs: AgentPrefs::default(),
            lsp: LspSettings::default(),
            updates: UpdateSettings::default(),
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            window_x: None,
            window_y: None,
            window_maximized: false,
            recent_folders: Vec::new(),
            recent_workspaces: Vec::new(),
            sessions: BTreeMap::new(),
        }
    }
}

impl AppConfig {
    /// `<config dir>/isengard/config.json`, e.g. `~/Library/Application Support/isengard/config.json`.
    pub fn default_path() -> Option<PathBuf> {
        // UI tests drive the real app; they must never read or overwrite the
        // user's config (load/save become defaults/no-ops).
        if cfg!(test) {
            return None;
        }
        dirs::config_dir().map(|dir| dir.join("isengard").join("config.json"))
    }

    pub fn load() -> Self {
        Self::default_path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    /// Loads settings, falling back to defaults if the file is missing or invalid.
    pub fn load_from(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                log::warn!("ignoring invalid config {}: {err}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        if let Some(path) = Self::default_path()
            && let Err(err) = self.save_to(&path)
        {
            log::warn!("failed to save config: {err:#}");
        }
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn change_font_size(&mut self, delta: f32) {
        self.font_size = (self.font_size + delta).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
    }

    /// Clamp and store window geometry. Returns whether anything changed.
    pub fn set_window_geometry(
        &mut self,
        width: f32,
        height: f32,
        x: f32,
        y: f32,
        maximized: bool,
    ) -> bool {
        let width = width.max(MIN_WINDOW_WIDTH);
        let height = height.max(MIN_WINDOW_HEIGHT);
        let next_x = Some(x);
        let next_y = Some(y);
        if (self.window_width - width).abs() < 0.5
            && (self.window_height - height).abs() < 0.5
            && self.window_x == next_x
            && self.window_y == next_y
            && self.window_maximized == maximized
        {
            return false;
        }
        self.window_width = width;
        self.window_height = height;
        self.window_x = next_x;
        self.window_y = next_y;
        self.window_maximized = maximized;
        true
    }

    /// Size used when opening the main window (clamped).
    pub fn window_size(&self) -> (f32, f32) {
        (
            self.window_width.max(MIN_WINDOW_WIDTH),
            self.window_height.max(MIN_WINDOW_HEIGHT),
        )
    }

    pub fn add_recent_folder(&mut self, folder: &Path) {
        push_recent(&mut self.recent_folders, folder);
    }

    pub fn remove_recent_folder(&mut self, folder: &Path) {
        self.recent_folders.retain(|p| p != folder);
        self.remove_session_for_path(folder);
    }

    pub fn add_recent_workspace(&mut self, file: &Path) {
        push_recent(&mut self.recent_workspaces, file);
    }

    pub fn remove_recent_workspace(&mut self, file: &Path) {
        self.recent_workspaces.retain(|p| p != file);
        self.remove_session_for_path(file);
    }

    pub fn put_session(&mut self, key: String, session: WorkspaceSession) {
        if key.is_empty() {
            return;
        }
        self.sessions.insert(key.clone(), session);
        while self.sessions.len() > MAX_SESSIONS {
            let victim = self
                .sessions
                .keys()
                .filter(|k| *k != &key)
                .find(|k| {
                    let p = Path::new(k.as_str());
                    !self.recent_folders.iter().any(|f| f == p)
                        && !self.recent_workspaces.iter().any(|f| f == p)
                        && !k.starts_with("untitled:")
                })
                .cloned()
                .or_else(|| {
                    self.sessions
                        .keys()
                        .find(|k| *k != &key)
                        .cloned()
                });
            // ponytail: prefer evicting keys not in recent lists; never the one just written.
            if let Some(k) = victim {
                self.sessions.remove(&k);
            } else {
                break;
            }
        }
    }

    pub fn session(&self, key: &str) -> Option<&WorkspaceSession> {
        self.sessions.get(key)
    }

    fn remove_session_for_path(&mut self, path: &Path) {
        let key = path.to_string_lossy().into_owned();
        self.sessions.remove(&key);
    }
}

/// Moves `path` to the front of `list`, dropping the oldest beyond [`MAX_RECENT`].
fn push_recent(list: &mut Vec<PathBuf>, path: &Path) {
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(MAX_RECENT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_folders_are_deduped_and_capped() {
        let mut config = AppConfig::default();
        for i in 0..MAX_RECENT + 2 {
            config.add_recent_folder(Path::new(&format!("/p{i}")));
        }
        config.add_recent_folder(Path::new("/p3"));

        assert_eq!(config.recent_folders.len(), MAX_RECENT);
        assert_eq!(config.recent_folders[0], Path::new("/p3"));
        assert_eq!(
            config
                .recent_folders
                .iter()
                .filter(|p| *p == Path::new("/p3"))
                .count(),
            1
        );
        assert_eq!(
            config.recent_folders[1],
            Path::new(&format!("/p{}", MAX_RECENT + 1))
        );
    }

    #[test]
    fn recent_workspaces_are_tracked_separately() {
        let mut config = AppConfig::default();
        config.add_recent_folder(Path::new("/code/app"));
        config.add_recent_workspace(Path::new("/code/demo.isengard-workspace"));
        config.add_recent_workspace(Path::new("/code/other.isengard-workspace"));
        config.add_recent_workspace(Path::new("/code/demo.isengard-workspace"));
        assert_eq!(config.recent_folders, [PathBuf::from("/code/app")]);
        assert_eq!(
            config.recent_workspaces,
            [
                PathBuf::from("/code/demo.isengard-workspace"),
                PathBuf::from("/code/other.isengard-workspace")
            ]
        );
        config.remove_recent_workspace(Path::new("/code/other.isengard-workspace"));
        assert_eq!(config.recent_workspaces.len(), 1);
        assert!(!config
            .sessions
            .contains_key("/code/other.isengard-workspace"));
    }

    #[test]
    fn sessions_roundtrip_and_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        let mut config = AppConfig::default();
        for i in 0..MAX_SESSIONS + 3 {
            config.put_session(
                format!("/extra/{i:02}"),
                WorkspaceSession {
                    open_files: vec![OpenFileEntry::Path(PathBuf::from(format!(
                        "/extra/{i:02}/f"
                    )))],
                    ..Default::default()
                },
            );
        }
        assert_eq!(config.sessions.len(), MAX_SESSIONS);
        config.put_session(
            "/code/app".into(),
            WorkspaceSession {
                open_files: vec![OpenFileEntry::with_view(
                    PathBuf::from("/code/app/main.rs"),
                    FileViewState {
                        cursor_line: 3,
                        cursor_character: 5,
                        scroll_x: 0.0,
                        scroll_y: -40.0,
                        ..Default::default()
                    },
                )],
                active: Some(PathBuf::from("/code/app/main.rs")),
                expanded: vec![PathBuf::from("/code/app/src")],
                ..Default::default()
            },
        );
        assert!(config.sessions.len() <= MAX_SESSIONS);
        assert!(config.session("/code/app").is_some());
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        let entry = &loaded.session("/code/app").unwrap().open_files[0];
        assert_eq!(entry.path(), Path::new("/code/app/main.rs"));
        assert_eq!(entry.view().cursor_line, 3);
        assert_eq!(entry.view().scroll_y, -40.0);
    }

    #[test]
    fn lsp_settings_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        let mut config = AppConfig::default();
        config.lsp.languages.insert(
            "python".into(),
            LspLanguageSettings {
                enabled: Some(true),
                command: Some("pyright-langserver".into()),
                args: Some(vec!["--stdio".into()]),
            },
        );
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        assert!(loaded.lsp.enabled);
        let py = loaded.lsp.languages.get("python").unwrap();
        assert_eq!(py.command.as_deref(), Some("pyright-langserver"));
        assert_eq!(py.args.as_ref().unwrap(), &["--stdio".to_string()]);
    }

    #[test]
    fn agent_settings_roundtrip_and_legacy_model_key() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        let mut config = AppConfig::default();
        config.agent.command = Some("gemini".into());
        config.agent.args = vec!["--experimental-acp".into()];
        config.agent.silent_model = Some("haiku".into());
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        assert_eq!(loaded.agent, config.agent);
        assert_eq!(loaded.agent.silent_model.as_deref(), Some("haiku"));

        // Older configs carried an unused `model` key; it must not reset settings.
        std::fs::write(&path, r#"{"font_size":16.0,"model":"claude-sonnet-4-5"}"#).unwrap();
        let legacy = AppConfig::load_from(&path);
        assert_eq!(legacy.font_size, 16.0);
        assert_eq!(legacy.agent, AgentSettings::default());
    }

    #[test]
    fn agent_prefs_roundtrip_global_and_per_session() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        let mut prefs = AgentPrefs {
            mode: Some("plan".into()),
            ..Default::default()
        };
        prefs
            .config
            .insert("model".into(), AgentPrefValue::Value("opus".into()));
        prefs
            .config
            .insert("fast".into(), AgentPrefValue::Bool(true));
        let mut config = AppConfig {
            agent_prefs: prefs.clone(),
            ..Default::default()
        };
        let session = WorkspaceSession {
            agent_prefs: prefs.clone(),
            ..Default::default()
        };
        config.put_session("/tmp/a".into(), session);
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        assert_eq!(loaded.agent_prefs, prefs);
        assert_eq!(loaded.session("/tmp/a").unwrap().agent_prefs, prefs);
        // Empty prefs are omitted from the file.
        let json = serde_json::to_string(&WorkspaceSession::default()).unwrap();
        assert!(!json.contains("agent_prefs"));
    }

    #[test]
    fn session_without_agent_fields_uses_defaults() {
        let session: WorkspaceSession = serde_json::from_str(r#"{"open_files":[]}"#).unwrap();
        assert!(!session.agent_visible);
        assert_eq!(session.agent_width, DEFAULT_AGENT_WIDTH);
        assert!(session.agent_chats.is_empty());
        assert_eq!(session.agent_active, None);
        assert_eq!(session.sidebar_panel, SidebarPanel::Files);
        assert!(session.search.is_empty());
        assert!(session.git_repos.is_empty());
    }

    #[test]
    fn session_sidebar_search_git_unsaved_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        let mut config = AppConfig::default();
        config.put_session(
            "/code/app".into(),
            WorkspaceSession {
                sidebar_panel: SidebarPanel::Git,
                search: SearchSessionState {
                    pattern: "foo".into(),
                    case_sensitive: true,
                    ..Default::default()
                },
                git_repos: vec![GitRepoSession {
                    root: PathBuf::from("/code/app"),
                    message: "wip".into(),
                    selected: vec!["a.rs".into()],
                    message_floor_h: 120.0,
                    collapsed: true,
                }],
                open_files: vec![OpenFileEntry::with_view(
                    PathBuf::from("/code/app/a.rs"),
                    FileViewState {
                        unsaved_text: Some("edited".into()),
                        untitled_save_dir: None,
                        ..Default::default()
                    },
                )],
                ..Default::default()
            },
        );
        config.save_to(&path).unwrap();
        let session = AppConfig::load_from(&path).session("/code/app").unwrap().clone();
        assert_eq!(session.sidebar_panel, SidebarPanel::Git);
        assert_eq!(session.search.pattern, "foo");
        assert!(session.search.case_sensitive);
        assert_eq!(session.git_repos[0].message, "wip");
        assert_eq!(
            session.open_files[0].view().unsaved_text.as_deref(),
            Some("edited")
        );
    }

    #[test]
    fn agent_chats_roundtrip_per_session() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        let mut config = AppConfig::default();
        config.put_session(
            "/code/app".into(),
            WorkspaceSession {
                agent_chats: vec![AgentChat {
                    id: "s1".into(),
                    title: "Fix the build".into(),
                    updated: 1_700_000_000,
                    file_changes: vec![AgentFileChangesCard {
                        turn: 0,
                        files: vec![AgentFileChange {
                            path: PathBuf::from("/code/app/src/main.rs"),
                            added: 2,
                            removed: 1,
                            deleted: false,
                        }],
                    }],
                }],
                agent_active: Some("s1".into()),
                ..Default::default()
            },
        );
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        let session = loaded.session("/code/app").unwrap();
        assert_eq!(session.agent_chats[0].title, "Fix the build");
        assert_eq!(session.agent_active.as_deref(), Some("s1"));
        assert_eq!(session.agent_chats[0].file_changes[0].files[0].added, 2);
    }

    #[test]
    fn agent_chat_without_file_changes_still_loads() {
        let chat: AgentChat = serde_json::from_str(
            r#"{"id":"s1","title":"Hi","updated":1}"#,
        )
        .unwrap();
        assert!(chat.file_changes.is_empty());
    }

    #[test]
    fn open_file_entry_accepts_legacy_path_string() {
        let json = r#"{"open_files":["/a/b.rs"],"active":"/a/b.rs","expanded":[]}"#;
        let session: WorkspaceSession = serde_json::from_str(json).unwrap();
        assert_eq!(session.open_files[0].path(), Path::new("/a/b.rs"));
        assert_eq!(session.open_files[0].view(), FileViewState::default());
    }

    #[test]
    fn remove_recent_drops_session() {
        let mut config = AppConfig::default();
        config.add_recent_folder(Path::new("/code/app"));
        config.put_session("/code/app".into(), WorkspaceSession::default());
        config.remove_recent_folder(Path::new("/code/app"));
        assert!(config.session("/code/app").is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested/config.json");
        let mut config = AppConfig::default();
        config.add_recent_folder(Path::new("/code/app"));
        config.save_to(&path).unwrap();
        assert_eq!(AppConfig::load_from(&path), config);
    }

    #[test]
    fn window_geometry_persists_and_legacy_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        let mut config = AppConfig::default();
        assert!(config.set_window_geometry(1600., 900., 12., 34., true));
        assert!(!config.set_window_geometry(1600., 900., 12., 34., true));
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        assert_eq!(loaded.window_width, 1600.);
        assert_eq!(loaded.window_height, 900.);
        assert_eq!(loaded.window_x, Some(12.));
        assert_eq!(loaded.window_y, Some(34.));
        assert!(loaded.window_maximized);

        // Older configs without window_* fields still load.
        std::fs::write(&path, r#"{"font_size":14.0,"model":"x"}"#).unwrap();
        let legacy = AppConfig::load_from(&path);
        assert_eq!(legacy.window_width, DEFAULT_WINDOW_WIDTH);
        assert_eq!(legacy.window_height, DEFAULT_WINDOW_HEIGHT);
        assert!(!legacy.window_maximized);
    }

    #[test]
    fn invalid_or_missing_file_gives_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        assert_eq!(AppConfig::load_from(&path), AppConfig::default());
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(AppConfig::load_from(&path), AppConfig::default());
    }
}
