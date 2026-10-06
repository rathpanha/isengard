use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_EDITOR_FONT_SIZE: f32 = 14.0;
pub const MIN_FONT_SIZE: f32 = 9.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
/// Cap for each recent list (folders and workspaces).
pub const MAX_RECENT: usize = 8;
/// Cap for persisted workspace sessions (tabs + tree expand).
pub const MAX_SESSIONS: usize = 16;

/// Cursor + scroll for one open text tab (images omit these / use defaults).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileViewState {
    pub cursor_line: u32,
    pub cursor_character: u32,
    pub scroll_x: f32,
    pub scroll_y: f32,
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

/// Open tabs and expanded folders for one workspace / folder key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceSession {
    pub open_files: Vec<OpenFileEntry>,
    pub active: Option<PathBuf>,
    pub expanded: Vec<PathBuf>,
}

/// User-facing settings, persisted as JSON in the platform config directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Code editor font size in pixels.
    pub font_size: f32,
    /// Claude model used by the (upcoming) AI panel.
    pub model: String,
    /// Most recently opened first.
    pub recent_folders: Vec<PathBuf>,
    /// `.isengard-workspace` files, most recently opened first.
    pub recent_workspaces: Vec<PathBuf>,
    /// Last tabs + expanded dirs, keyed by [`crate::workspace::Workspace::session_key`].
    pub sessions: BTreeMap<String, WorkspaceSession>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_EDITOR_FONT_SIZE,
            model: "claude-sonnet-4-5".to_owned(),
            recent_folders: Vec::new(),
            recent_workspaces: Vec::new(),
            sessions: BTreeMap::new(),
        }
    }
}

impl AppConfig {
    /// `<config dir>/isengard/config.json`, e.g. `~/Library/Application Support/isengard/config.json`.
    pub fn default_path() -> Option<PathBuf> {
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
                    },
                )],
                active: Some(PathBuf::from("/code/app/main.rs")),
                expanded: vec![PathBuf::from("/code/app/src")],
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
    fn invalid_or_missing_file_gives_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        assert_eq!(AppConfig::load_from(&path), AppConfig::default());
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(AppConfig::load_from(&path), AppConfig::default());
    }
}
