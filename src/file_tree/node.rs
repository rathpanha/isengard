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
        if let FsNode::Dir { path, children, .. } = self {
            if children.is_none() {
                *children = Some(Self::load_one_level(path).unwrap_or_else(|err| {
                    log::warn!("failed to read {}: {err}", path.display());
                    Vec::new()
                }));
            }
        }
    }

    /// Flips the expanded state of a directory, loading its children on first expand.
    pub fn toggle_expand(&mut self) {
        if let FsNode::Dir { expanded, .. } = self {
            *expanded = !*expanded;
        }
        if self.is_expanded() {
            self.ensure_loaded();
        }
    }

    pub fn is_expanded(&self) -> bool {
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
    fn lazy_load_sorts_dirs_first_and_skips_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("zeta")).unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join("b.js"), "").unwrap();
        std::fs::write(tmp.path().join("A.json"), "").unwrap();

        let mut root = FsNode::from_path(tmp.path());
        assert!(root.children_mut().is_none(), "children load lazily");

        root.toggle_expand();
        assert!(root.is_expanded());
        let names: Vec<_> = root
            .children_mut()
            .unwrap()
            .iter()
            .map(|n| n.name().to_owned())
            .collect();
        assert_eq!(names, ["zeta", "A.json", "b.js"]);

        root.toggle_expand();
        assert!(!root.is_expanded());
    }
}
