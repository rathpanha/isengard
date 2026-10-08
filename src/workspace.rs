//! VS Code–style workspaces: one or more root folders, optionally saved to a
//! `.isengard-workspace` file. Pure logic, independent of the UI.

use std::path::{Component, Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::editor::document;

pub const WORKSPACE_EXTENSION: &str = "isengard-workspace";

/// The folders open in a window. A single opened folder is a workspace with one
/// folder and no file; several folders without a file is an "Untitled" workspace.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Workspace {
    folders: Vec<PathBuf>,
    file: Option<PathBuf>,
}

/// On-disk format, modelled on `.code-workspace`. `settings` is reserved for
/// per-workspace settings and is preserved as-is.
#[derive(Debug, Default, Serialize, Deserialize)]
struct WorkspaceFile {
    folders: Vec<FolderEntry>,
    #[serde(default)]
    settings: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FolderEntry {
    path: String,
}

impl Workspace {
    pub fn from_folder(folder: &Path) -> Self {
        Self {
            folders: vec![folder.to_path_buf()],
            file: None,
        }
    }

    pub fn is_workspace_file(path: &Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext == WORKSPACE_EXTENSION)
    }

    pub fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    pub fn is_empty(&self) -> bool {
        self.folders.is_empty()
    }

    pub fn is_multi_root(&self) -> bool {
        self.folders.len() > 1
    }

    /// Several folders that were never saved to a file.
    pub fn is_untitled_multi_root(&self) -> bool {
        self.file.is_none() && self.is_multi_root()
    }

    /// Whether the UI presents this as a workspace (vs. a plain folder).
    pub fn is_named_workspace(&self) -> bool {
        self.file.is_some() || self.is_multi_root()
    }

    /// Adds a root folder; returns `false` if it was already present.
    pub fn add_folder(&mut self, folder: &Path) -> bool {
        if self.folders.iter().any(|f| f == folder) {
            return false;
        }
        self.folders.push(folder.to_path_buf());
        true
    }

    pub fn remove_folder(&mut self, folder: &Path) -> bool {
        let before = self.folders.len();
        self.folders.retain(|f| f != folder);
        self.folders.len() != before
    }

    /// Renames a root folder path in place (e.g. after a disk rename).
    pub fn replace_folder(&mut self, old: &Path, new: &Path) -> bool {
        if old == new || self.folders.iter().any(|f| f == new) {
            return false;
        }
        if let Some(folder) = self.folders.iter_mut().find(|f| *f == old) {
            *folder = new.to_path_buf();
            return true;
        }
        false
    }

    /// Workspace file stem, "Untitled" for an unsaved multi-root workspace,
    /// otherwise the single folder's name.
    pub fn display_name(&self) -> String {
        if let Some(file) = &self.file {
            return file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| document::file_name(file));
        }
        if self.is_multi_root() {
            return "Untitled".to_owned();
        }
        self.folders
            .first()
            .map(|f| document::file_name(f))
            .unwrap_or_default()
    }

    /// Title shown in the window / title bar, e.g. `app` or `demo (Workspace)`.
    pub fn title(&self) -> String {
        if self.is_named_workspace() {
            format!("{} (Workspace)", self.display_name())
        } else {
            self.display_name()
        }
    }

    /// Stable key for persisting open tabs / tree expand in [`crate::config::AppConfig`].
    ///
    /// Workspace file → that file's path; single folder → folder path; untitled
    /// multi-root → `untitled:` + sorted folder paths joined by `\n`.
    pub fn session_key(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        if let Some(file) = &self.file {
            return Some(file.to_string_lossy().into_owned());
        }
        if self.folders.len() == 1 {
            return Some(self.folders[0].to_string_lossy().into_owned());
        }
        let mut folders: Vec<String> = self
            .folders
            .iter()
            .map(|f| f.to_string_lossy().into_owned())
            .collect();
        folders.sort();
        Some(format!("untitled:{}", folders.join("\n")))
    }

    /// The root folder containing `path` (the longest match, for nested roots).
    pub fn root_for(&self, path: &Path) -> Option<&Path> {
        self.folders
            .iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .map(PathBuf::as_path)
    }

    /// `rel/path` with one root, `root-name/rel/path` with several; the full
    /// path for files outside every root.
    pub fn relative_label(&self, path: &Path) -> String {
        let Some(root) = self.root_for(path) else {
            return path.display().to_string();
        };
        let rel = path.strip_prefix(root).unwrap_or(path);
        if self.is_multi_root() {
            Path::new(&document::file_name(root))
                .join(rel)
                .display()
                .to_string()
        } else {
            rel.display().to_string()
        }
    }

    /// Reads a workspace file. Relative folder paths resolve against the file's directory.
    pub fn load(file: &Path) -> anyhow::Result<Self> {
        let text = document::read_text(file)?;
        let parsed: WorkspaceFile = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a valid workspace file", file.display()))?;
        let base = file.parent().unwrap_or(Path::new(""));
        let mut workspace = Self {
            folders: Vec::new(),
            file: Some(file.to_path_buf()),
        };
        for entry in parsed.folders {
            workspace.add_folder(&normalize(&base.join(&entry.path)));
        }
        Ok(workspace)
    }

    /// Writes the workspace to `file` (folder paths relative to it where possible)
    /// and makes it this workspace's file. Existing `settings` are preserved.
    pub fn save_as(&mut self, file: &Path) -> anyhow::Result<()> {
        let settings = document::read_text(file)
            .ok()
            .and_then(|text| serde_json::from_str::<WorkspaceFile>(&text).ok())
            .map(|existing| existing.settings)
            .unwrap_or_default();
        let base = file.parent().unwrap_or(Path::new(""));
        let contents = WorkspaceFile {
            folders: self
                .folders
                .iter()
                .map(|folder| FolderEntry {
                    path: relative_to(folder, base).to_string_lossy().into_owned(),
                })
                .collect(),
            settings,
        };
        document::write_text(file, &serde_json::to_string_pretty(&contents)?)?;
        self.file = Some(file.to_path_buf());
        Ok(())
    }

    /// Re-writes the workspace file, if this workspace has one.
    pub fn save(&mut self) -> anyhow::Result<()> {
        match self.file.clone() {
            Some(file) => self.save_as(&file),
            None => Ok(()),
        }
    }
}

/// Resolves `.` and `..` lexically (without touching the file system, so
/// paths keep the form the user chose, e.g. `/tmp` rather than `/private/tmp`).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` relative to `base` (both absolute), e.g. `../other`. Falls back to
/// `path` when they share no common root (e.g. different Windows drives).
fn relative_to(path: &Path, base: &Path) -> PathBuf {
    let path_parts: Vec<_> = path.components().collect();
    let base_parts: Vec<_> = base.components().collect();
    let common = path_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        return path.to_path_buf();
    }
    let mut rel = PathBuf::new();
    for _ in common..base_parts.len() {
        rel.push("..");
    }
    for part in &path_parts[common..] {
        rel.push(part);
    }
    if rel.as_os_str().is_empty() {
        rel.push(".");
    }
    rel
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(folders: &[&str]) -> Workspace {
        let mut w = Workspace::default();
        for f in folders {
            w.add_folder(Path::new(f));
        }
        w
    }

    #[test]
    fn session_keys() {
        assert_eq!(Workspace::default().session_key(), None);
        assert_eq!(
            ws(&["/code/app"]).session_key().as_deref(),
            Some("/code/app")
        );
        let mut named = ws(&["/code/a", "/code/b"]);
        named.file = Some(PathBuf::from("/code/demo.isengard-workspace"));
        assert_eq!(
            named.session_key().as_deref(),
            Some("/code/demo.isengard-workspace")
        );
        // Untitled multi-root: sorted folder paths.
        assert_eq!(
            ws(&["/code/b", "/code/a"]).session_key().as_deref(),
            Some("untitled:/code/a\n/code/b")
        );
    }

    #[test]
    fn add_and_remove_folders() {
        let mut w = ws(&["/code/a"]);
        assert!(
            !w.add_folder(Path::new("/code/a")),
            "duplicates are ignored"
        );
        assert!(w.add_folder(Path::new("/code/b")));
        assert_eq!(
            w.folders(),
            [PathBuf::from("/code/a"), PathBuf::from("/code/b")]
        );
        assert!(w.remove_folder(Path::new("/code/a")));
        assert!(!w.remove_folder(Path::new("/code/a")));
        assert_eq!(w.folders(), [PathBuf::from("/code/b")]);
    }

    #[test]
    fn names_and_titles() {
        assert_eq!(ws(&["/code/app"]).title(), "app");
        assert_eq!(ws(&["/code/a", "/code/b"]).title(), "Untitled (Workspace)");
        let mut named = ws(&["/code/a"]);
        named.file = Some(PathBuf::from("/code/demo.isengard-workspace"));
        assert_eq!(named.title(), "demo (Workspace)");
        assert_eq!(Workspace::default().title(), "");
    }

    #[test]
    fn replace_folder_updates_root() {
        let mut w = ws(&["/code/a", "/code/b"]);
        assert!(w.replace_folder(Path::new("/code/a"), Path::new("/code/a2")));
        assert_eq!(w.folders(), [PathBuf::from("/code/a2"), PathBuf::from("/code/b")]);
        assert!(!w.replace_folder(Path::new("/code/missing"), Path::new("/code/x")));
        assert!(!w.replace_folder(Path::new("/code/b"), Path::new("/code/a2")));
    }

    #[test]
    fn root_for_prefers_the_deepest_root() {
        let w = ws(&["/code", "/code/app"]);
        assert_eq!(
            w.root_for(Path::new("/code/app/src/x.js")),
            Some(Path::new("/code/app"))
        );
        assert_eq!(
            w.root_for(Path::new("/code/other.js")),
            Some(Path::new("/code"))
        );
        assert_eq!(w.root_for(Path::new("/elsewhere/x.js")), None);
    }

    #[test]
    fn relative_labels() {
        assert_eq!(
            ws(&["/code/app"]).relative_label(Path::new("/code/app/src/x.js")),
            "src/x.js"
        );
        assert_eq!(
            ws(&["/code/app", "/code/lib"]).relative_label(Path::new("/code/lib/y.js")),
            "lib/y.js"
        );
        assert_eq!(
            ws(&["/code/app"]).relative_label(Path::new("/tmp/z.js")),
            "/tmp/z.js"
        );
    }

    #[test]
    fn file_roundtrip_uses_relative_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("project/app")).unwrap();
        std::fs::create_dir_all(root.join("shared")).unwrap();
        let file = root.join("project/demo.isengard-workspace");

        let mut w = ws(&[]);
        w.add_folder(&root.join("project/app"));
        w.add_folder(&root.join("shared"));
        w.save_as(&file).unwrap();
        assert_eq!(w.file(), Some(file.as_path()));

        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(json["folders"][0]["path"], "app");
        assert_eq!(json["folders"][1]["path"], "../shared");

        let loaded = Workspace::load(&file).unwrap();
        assert_eq!(loaded, w);
    }

    #[test]
    fn save_preserves_settings_and_load_keeps_missing_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("w.isengard-workspace");
        std::fs::write(
            &file,
            r#"{ "folders": [{ "path": "gone" }], "settings": { "tabSize": 4 } }"#,
        )
        .unwrap();

        let mut w = Workspace::load(&file).unwrap();
        assert_eq!(
            w.folders(),
            [tmp.path().join("gone")],
            "missing folders are kept"
        );
        w.save().unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(json["settings"]["tabSize"], 4);
    }

    #[test]
    fn invalid_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("bad.isengard-workspace");
        std::fs::write(&file, "not json").unwrap();
        assert!(Workspace::load(&file).is_err());
        assert!(Workspace::load(&tmp.path().join("missing.isengard-workspace")).is_err());
    }

    #[test]
    fn path_helpers() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(
            relative_to(Path::new("/a/b/c"), Path::new("/a/b")),
            PathBuf::from("c")
        );
        assert_eq!(
            relative_to(Path::new("/a/x"), Path::new("/a/b")),
            PathBuf::from("../x")
        );
        assert_eq!(
            relative_to(Path::new("/a/b"), Path::new("/a/b")),
            PathBuf::from(".")
        );
        assert!(Workspace::is_workspace_file(Path::new(
            "x.isengard-workspace"
        )));
        assert!(!Workspace::is_workspace_file(Path::new("x.json")));
    }
}
