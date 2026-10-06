//! File-tree icons from the full [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme)
//! set (`assets/icons/material/`, associations in `assets/icons/material-icons.json`).
//!
//! Rendered with `img()` (full-colour SVG), not GPUI Kit's `Icon`/`svg()` —
//! those alpha-mask the SVG and tint with text colour, which would wipe fills.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use gpui_kit::{AnyElement, Image, ImageFormat, IntoElement as _, Styled as _, img, px};
use rust_embed::Embed;
use serde::Deserialize;

#[derive(Embed)]
#[folder = "assets/icons/material/"]
#[include = "*.svg"]
struct MaterialSvgs;

#[derive(Debug, Deserialize)]
struct ThemeManifest {
    #[serde(rename = "fileExtensions")]
    file_extensions: HashMap<String, String>,
    #[serde(rename = "fileNames")]
    file_names: HashMap<String, String>,
    #[serde(rename = "folderNames")]
    folder_names: HashMap<String, String>,
    #[serde(rename = "folderNamesExpanded")]
    folder_names_expanded: HashMap<String, String>,
    file: String,
    folder: String,
    #[serde(rename = "folderExpanded")]
    folder_expanded: String,
    #[serde(rename = "rootFolder")]
    root_folder: String,
    #[serde(rename = "rootFolderExpanded")]
    root_folder_expanded: String,
    light: LightOverrides,
}

#[derive(Debug, Deserialize, Default)]
struct LightOverrides {
    #[serde(default, rename = "fileExtensions")]
    file_extensions: HashMap<String, String>,
    #[serde(default, rename = "fileNames")]
    file_names: HashMap<String, String>,
    #[serde(default, rename = "folderNames")]
    folder_names: HashMap<String, String>,
    #[serde(default, rename = "folderNamesExpanded")]
    folder_names_expanded: HashMap<String, String>,
}

fn manifest() -> &'static ThemeManifest {
    static MANIFEST: LazyLock<ThemeManifest> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../../assets/icons/material-icons.json"))
            .expect("material-icons.json")
    });
    &MANIFEST
}

/// Icon for a file-tree row. Pass `dark` from `cx.theme().is_dark()` so light
/// overrides from the theme apply.
pub fn tree_icon(
    path: &Path,
    is_dir: bool,
    expanded: bool,
    is_root: bool,
    dark: bool,
) -> AnyElement {
    let name = resolve_icon_name(path, is_dir, expanded, is_root, dark);
    img(load_icon(&name))
        .w(px(16.))
        .h(px(16.))
        .flex_shrink_0()
        .into_any_element()
}

fn resolve_icon_name(
    path: &Path,
    is_dir: bool,
    expanded: bool,
    is_root: bool,
    dark: bool,
) -> String {
    let m = manifest();
    let light = !dark;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if is_dir {
        return resolve_folder(m, &file_name, expanded, is_root, light);
    }
    resolve_file(m, &file_name, light)
}

fn resolve_folder(
    m: &ThemeManifest,
    name: &str,
    expanded: bool,
    is_root: bool,
    light: bool,
) -> String {
    if expanded {
        if light && let Some(icon) = m.light.folder_names_expanded.get(name) {
            return icon.clone();
        }
        if let Some(icon) = m.folder_names_expanded.get(name) {
            return icon.clone();
        }
        if is_root {
            return m.root_folder_expanded.clone();
        }
        m.folder_expanded.clone()
    } else {
        if light && let Some(icon) = m.light.folder_names.get(name) {
            return icon.clone();
        }
        if let Some(icon) = m.folder_names.get(name) {
            return icon.clone();
        }
        if is_root {
            return m.root_folder.clone();
        }
        m.folder.clone()
    }
}

fn resolve_file(m: &ThemeManifest, file_name: &str, light: bool) -> String {
    if light && let Some(icon) = m.light.file_names.get(file_name) {
        return icon.clone();
    }
    if let Some(icon) = m.file_names.get(file_name) {
        return icon.clone();
    }

    // VS Code-style: try every suffix after a '.', longest first (`spec.ts` then `ts`).
    for (i, _) in file_name.match_indices('.') {
        let ext = &file_name[i + 1..];
        if ext.is_empty() {
            continue;
        }
        if light && let Some(icon) = m.light.file_extensions.get(ext) {
            return icon.clone();
        }
        if let Some(icon) = m.file_extensions.get(ext) {
            return icon.clone();
        }
    }

    m.file.clone()
}

fn load_icon(name: &str) -> Arc<Image> {
    static CACHE: LazyLock<Mutex<HashMap<String, Arc<Image>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    let mut cache = CACHE.lock().expect("material icon cache");
    if let Some(image) = cache.get(name) {
        return image.clone();
    }

    let file = format!("{name}.svg");
    let bytes = MaterialSvgs::get(&file)
        .map(|f| f.data.into_owned())
        .or_else(|| {
            // Missing association target → generic file glyph.
            MaterialSvgs::get("file.svg").map(|f| f.data.into_owned())
        })
        .unwrap_or_default();

    let image = Arc::new(Image::from_bytes(ImageFormat::Svg, bytes));
    cache.insert(name.to_owned(), image.clone());
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_extensions_file_names_and_folders() {
        assert_eq!(
            resolve_icon_name(Path::new("app.tsx"), false, false, false, true),
            "react_ts"
        );
        assert_eq!(
            resolve_icon_name(Path::new("foo.spec.ts"), false, false, false, true),
            "test-ts"
        );
        assert_eq!(
            resolve_icon_name(Path::new("lib.rs"), false, false, false, true),
            "rust"
        );
        assert_eq!(
            resolve_icon_name(Path::new("package.json"), false, false, false, true),
            "nodejs"
        );
        assert_eq!(
            resolve_icon_name(Path::new("Dockerfile"), false, false, false, true),
            "docker"
        );
        assert_eq!(
            resolve_icon_name(Path::new("src"), true, false, false, true),
            "folder-src"
        );
        assert_eq!(
            resolve_icon_name(Path::new("src"), true, true, false, true),
            "folder-src-open"
        );
        assert_eq!(
            resolve_icon_name(Path::new("my-app"), true, false, true, true),
            "folder-root"
        );
        assert_eq!(
            resolve_icon_name(Path::new("notes.txt"), false, false, false, true),
            "document"
        );
        assert_eq!(
            resolve_icon_name(Path::new("notes.xyz"), false, false, false, true),
            "file"
        );
        // Light override for toml.
        assert_eq!(
            resolve_icon_name(Path::new("Cargo.toml"), false, false, false, false),
            "toml_light"
        );
    }

    #[test]
    fn embeds_resolved_svgs() {
        for name in ["file", "folder", "rust", "react_ts", "folder-src-open"] {
            assert!(
                MaterialSvgs::get(&format!("{name}.svg")).is_some(),
                "missing {name}.svg"
            );
        }
    }
}
