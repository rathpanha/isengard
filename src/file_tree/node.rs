use std::io;
use std::path::{Path, PathBuf};

/// Entries never shown in the tree.
const IGNORED_NAMES: &[&str] = &[".git", ".DS_Store"];

/// A node in the file tree. Directory children are loaded lazily on first expand.
#[derive(Debug)]
pub enum FsNode {
    File {
        path: PathBuf,
        name: String,
    },
    Dir {
        path: PathBuf,
        name: String,
        /// `None` until the directory has been read.
        children: Option<Vec<FsNode>>,
        expanded: bool,
    },
}

impl FsNode {
    pub fn from_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        if path.is_dir() {
            FsNode::Dir {
                path: path.to_path_buf(),
                name,
                children: None,
                expanded: false,
            }
        } else {
            FsNode::File {
                path: path.to_path_buf(),
                name,
            }
        }
    }

    pub fn name(&self) -> &str {
        match self {
            FsNode::File { name, .. } | FsNode::Dir { name, .. } => name,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            FsNode::File { path, .. } | FsNode::Dir { path, .. } => path,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, FsNode::Dir { .. })
    }

    /// Reads a single directory level: directories first, then files, each sorted
    /// case-insensitively. Unreadable entries are skipped.
    pub fn load_one_level(dir: &Path) -> io::Result<Vec<FsNode>> {
        let mut nodes: Vec<FsNode> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name();
                !IGNORED_NAMES.iter().any(|ignored| name == *ignored)
            })
            .map(|entry| FsNode::from_path(&entry.path()))
            .collect();
        nodes.sort_by(|a, b| {
            b.is_dir()
                .cmp(&a.is_dir())
                .then_with(|| a.name().to_lowercase().cmp(&b.name().to_lowercase()))
        });
        Ok(nodes)
    }

    /// Loads children if this is a directory that has not been read yet.
    pub fn ensure_loaded(&mut self) {
        if let FsNode::Dir { path, children, .. } = self
            && children.is_none()
        {
            *children = Some(Self::load_one_level(path).unwrap_or_else(|err| {
                log::warn!("failed to read {}: {err}", path.display());
                Vec::new()
            }));
        }
    }

    /// Re-reads directory children from disk (keeps expand flag).
    pub fn reload_children(&mut self) {
        let FsNode::Dir { path, children, .. } = self else {
            return;
        };
        *children = Some(Self::load_one_level(path).unwrap_or_else(|err| {
            log::warn!("failed to read {}: {err}", path.display());
            Vec::new()
        }));
    }

    /// Sets a directory's expanded state. Returns `true` if this loaded its children
    /// for the first time (so a view built from the tree must be rebuilt).
    pub fn set_expanded(&mut self, value: bool) -> bool {
        let FsNode::Dir {
            expanded, children, ..
        } = self
        else {
            return false;
        };
        *expanded = value;
        let needs_load = value && children.is_none();
        if needs_load {
            self.ensure_loaded();
        }
        needs_load
    }

    /// Collects paths of directories that are currently expanded.
    /// Stops at a collapsed directory — nested expand flags are not visible.
    pub fn collect_expanded(&self, out: &mut Vec<PathBuf>) {
        if let FsNode::Dir {
            path,
            expanded,
            children,
            ..
        } = self
        {
            if !*expanded {
                return;
            }
            out.push(path.clone());
            if let Some(children) = children {
                for child in children {
                    child.collect_expanded(out);
                }
            }
        }
    }

    /// Expands every ancestor directory along `path` (inclusive if `path` is a dir),
    /// loading children as needed so nested targets become reachable.
    pub fn expand_toward(&mut self, path: &Path) {
        if !path.starts_with(self.path()) && path != self.path() {
            return;
        }
        if self.path() == path {
            if self.is_dir() {
                self.set_expanded(true);
            }
            return;
        }
        if !self.is_dir() {
            return;
        }
        self.set_expanded(true);
        if let Some(children) = self.children_mut() {
            for child in children {
                if path.starts_with(child.path()) {
                    child.expand_toward(path);
                    break;
                }
            }
        }
    }

    /// Finds the node for `path` among this node and its loaded descendants.
    pub fn find_mut(&mut self, path: &Path) -> Option<&mut FsNode> {
        if self.path() == path {
            return Some(self);
        }
        if !path.starts_with(self.path()) {
            return None;
        }
        self.children_mut()?
            .iter_mut()
            .find_map(|child| child.find_mut(path))
    }

    pub fn children(&self) -> Option<&[FsNode]> {
        match self {
            FsNode::Dir { children, .. } => children.as_deref(),
            FsNode::File { .. } => None,
        }
    }

    #[cfg(test)]
    fn is_expanded(&self) -> bool {
        matches!(self, FsNode::Dir { expanded: true, .. })
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<FsNode>> {
        match self {
            FsNode::Dir { children, .. } => children.as_mut(),
            FsNode::File { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_expanded_skips_under_collapsed() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/ui")).unwrap();

        let mut root = FsNode::from_path(tmp.path());
        root.expand_toward(&tmp.path().join("src/ui"));
        root.set_expanded(false); // collapse root; nested flags remain true internally

        let mut expanded = Vec::new();
        root.collect_expanded(&mut expanded);
        assert!(
            expanded.is_empty(),
            "collapsed root must not report nested expands: {expanded:?}"
        );
    }

    #[test]
    fn collect_expanded_and_expand_toward() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/ui")).unwrap();
        std::fs::write(tmp.path().join("src/ui/app.js"), "").unwrap();

        let mut root = FsNode::from_path(tmp.path());
        root.expand_toward(&tmp.path().join("src/ui"));
        let mut expanded = Vec::new();
        root.collect_expanded(&mut expanded);
        assert!(expanded.contains(&tmp.path().to_path_buf()));
        assert!(expanded.contains(&tmp.path().join("src")));
        assert!(expanded.contains(&tmp.path().join("src/ui")));
    }

    #[test]
    fn lazy_load_sorts_dirs_first_and_skips_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("zeta")).unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join("b.js"), "").unwrap();
        std::fs::write(tmp.path().join("A.json"), "").unwrap();

        let mut root = FsNode::from_path(tmp.path());
        assert!(root.children_mut().is_none(), "children load lazily");

        root.set_expanded(true);
        assert!(root.is_expanded());
        let names: Vec<_> = root
            .children_mut()
            .unwrap()
            .iter()
            .map(|n| n.name().to_owned())
            .collect();
        assert_eq!(names, ["zeta", "A.json", "b.js"]);

        root.set_expanded(false);
        assert!(!root.is_expanded());
    }

    #[test]
    fn set_expanded_reports_first_load_and_find_mut_locates_nodes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/ui")).unwrap();
        std::fs::write(tmp.path().join("src/ui/app.js"), "").unwrap();

        let mut root = FsNode::from_path(tmp.path());
        assert!(root.set_expanded(true), "first expand loads children");
        assert!(!root.set_expanded(true), "already loaded");

        let src = root.find_mut(&tmp.path().join("src")).unwrap();
        assert!(src.set_expanded(true));
        let ui = root.find_mut(&tmp.path().join("src/ui")).unwrap();
        assert!(ui.is_dir());
        assert!(root.find_mut(Path::new("/elsewhere")).is_none());
    }
}
