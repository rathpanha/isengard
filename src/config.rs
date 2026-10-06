use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const MIN_FONT_SIZE: f32 = 9.0;
pub const MAX_FONT_SIZE: f32 = 32.0;

/// User-facing settings, persisted between runs via eframe storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub font_size: f32,
    pub dark_mode: bool,
    /// Claude model used by the (upcoming) AI panel.
    pub model: String,
    pub last_folder: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            dark_mode: true,
            model: "claude-sonnet-4-5".to_owned(),
            last_folder: None,
        }
    }
}

impl AppConfig {
    pub fn change_font_size(&mut self, delta: f32) {
        self.font_size = (self.font_size + delta).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
    }
}
