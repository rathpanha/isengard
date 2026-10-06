use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MIN_FONT_SIZE: f32 = 9.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
pub const MAX_RECENT_FOLDERS: usize = 8;

/// User-facing settings, persisted between runs via eframe storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub font_size: f32,
    pub dark_mode: bool,
    /// Claude model used by the (upcoming) AI panel.
    pub model: String,
    /// Most recently opened first.
    pub recent_folders: Vec<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            dark_mode: true,
            model: "claude-sonnet-4-5".to_owned(),
            recent_folders: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn change_font_size(&mut self, delta: f32) {
        self.font_size = (self.font_size + delta).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
    }

    /// Moves `folder` to the front of the recent list, dropping the oldest beyond the limit.
    pub fn add_recent_folder(&mut self, folder: &Path) {
        self.remove_recent_folder(folder);
        self.recent_folders.insert(0, folder.to_path_buf());
        self.recent_folders.truncate(MAX_RECENT_FOLDERS);
    }

    pub fn remove_recent_folder(&mut self, folder: &Path) {
        self.recent_folders.retain(|p| p != folder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_folders_are_deduped_and_capped() {
        let mut config = AppConfig::default();
        for i in 0..MAX_RECENT_FOLDERS + 2 {
            config.add_recent_folder(Path::new(&format!("/p{i}")));
        }
        config.add_recent_folder(Path::new("/p3"));

        assert_eq!(config.recent_folders.len(), MAX_RECENT_FOLDERS);
        assert_eq!(config.recent_folders[0], Path::new("/p3"));
        assert_eq!(config.recent_folders.iter().filter(|p| *p == Path::new("/p3")).count(), 1);
        assert_eq!(config.recent_folders[1], Path::new(&format!("/p{}", MAX_RECENT_FOLDERS + 1)));
    }
}
