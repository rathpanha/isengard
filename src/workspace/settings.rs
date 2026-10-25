//! Overlay `.isengard-workspace` `settings` onto [`AppConfig`].
//!
//! Recognized keys (VS Code–style and flat Isengard aliases). Unknown keys
//! stay in the file untouched.

use serde_json::{Map, Value};

use crate::config::AppConfig;

/// Merge known workspace settings into `config` (workspace wins).
pub fn apply_to_config(settings: &Map<String, Value>, config: &mut AppConfig) {
    if let Some(n) = number_setting(settings, &["editor.fontSize", "font_size"]) {
        config.font_size = n.clamp(crate::config::MIN_FONT_SIZE, crate::config::MAX_FONT_SIZE);
    }
    if let Some(n) = u32_setting(settings, &["editor.tabSize", "tabSize", "tab_size"]) {
        config.tab_size = n.clamp(1, 8);
    }
    if let Some(on) = bool_setting(
        settings,
        &[
            "editor.formatOnSave",
            "format_on_save",
            "lsp.format_on_save",
        ],
    ) {
        config.lsp.format_on_save = on;
    }
}

fn number_setting(settings: &Map<String, Value>, keys: &[&str]) -> Option<f32> {
    for key in keys {
        let Some(v) = settings.get(*key) else {
            continue;
        };
        match v {
            Value::Number(n) => return n.as_f64().map(|f| f as f32),
            Value::String(s) => return s.parse().ok(),
            _ => continue,
        }
    }
    None
}

fn u32_setting(settings: &Map<String, Value>, keys: &[&str]) -> Option<u32> {
    for key in keys {
        let Some(v) = settings.get(*key) else {
            continue;
        };
        match v {
            Value::Number(n) => return n.as_u64().map(|u| u as u32),
            Value::String(s) => return s.parse().ok(),
            _ => continue,
        }
    }
    None
}

fn bool_setting(settings: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    for key in keys {
        let Some(v) = settings.get(*key) else {
            continue;
        };
        match v {
            Value::Bool(b) => return Some(*b),
            Value::String(s) => match s.as_str() {
                "true" => return Some(true),
                "false" => return Some(false),
                _ => continue,
            },
            _ => continue,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overlays_vscode_and_flat_keys() {
        let mut config = AppConfig::default();
        let base_font = config.font_size;
        let mut map = Map::new();
        map.insert("editor.fontSize".into(), json!(18));
        map.insert("tabSize".into(), json!(4));
        map.insert("editor.formatOnSave".into(), json!(false));
        apply_to_config(&map, &mut config);
        assert_eq!(config.font_size, 18.0);
        assert_eq!(config.tab_size, 4);
        assert!(!config.lsp.format_on_save);
        assert_ne!(config.font_size, base_font);
    }
}
