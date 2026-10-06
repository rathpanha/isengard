use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_EDITOR_FONT_SIZE: f32 = 14.0;
pub const MIN_FONT_SIZE: f32 = 9.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
/// Cap for each recent list (folders and workspaces).
pub const MAX_RECENT: usize = 8;

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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_EDITOR_FONT_SIZE,
            model: "claude-sonnet-4-5".to_owned(),
            recent_folders: Vec::new(),
            recent_workspaces: Vec::new(),
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
    }

    pub fn add_recent_workspace(&mut self, file: &Path) {
        push_recent(&mut self.recent_workspaces, file);
    }

    pub fn remove_recent_workspace(&mut self, file: &Path) {
        self.recent_workspaces.retain(|p| p != file);
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
