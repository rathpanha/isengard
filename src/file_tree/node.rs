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
            self.open_single_dir_chain();
        }
        needs_load
    }

    /// A directory whose only entry is another directory is shown on one row
    /// (see [`Self::compact`]), so open that child right away — and its own
    /// single child, and so on — or the row could not know how far to extend.
    fn open_single_dir_chain(&mut self) {
        if let Some([only]) = self.children_mut().map(Vec::as_mut_slice)
            && only.is_dir()
        {
            only.set_expanded(true);
        }
    }

    /// Follows a chain of directories that each hold exactly one loaded
    /// directory (`co` → `istad` → `gateway`). Returns the label for the single
    /// row that stands for the chain and the deepest directory (the row's
    /// identity). A source-root folder (`java`, `kotlin`, …) never merges with
    /// its child; below one, names join with `.` like a Java package.
    pub fn compact(&self) -> (String, &FsNode) {
        let mut names = vec![self.name()];
        let mut cur = self;
        while !is_source_root(cur.name())
            && let Some([only]) = cur.children()
            && only.is_dir()
            && only.children().is_some()
        {
            names.push(only.name());
            cur = only;
        }
        let mut in_package = self
            .path()
            .ancestors()
            .skip(1)
            .filter_map(|a| a.file_name())
            .any(|n| is_source_root(&n.to_string_lossy()));
        let mut label = String::from(names[0]);
        let mut prev = names[0];
        for name in &names[1..] {
            in_package |= is_source_root(prev);
            let dotted = in_package && is_identifier(prev) && is_identifier(name);
            label.push(if dotted { '.' } else { '/' });
            label.push_str(name);
            prev = name;
        }
        (label, cur)
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

    /// Re-reads an already-loaded directory without losing the loaded/expanded
    /// state of subdirectories that still exist. No-op while unloaded (nothing
    /// shows it yet) — used by the file watcher, which must never expand or
    /// collapse anything.
    pub fn refresh_children(&mut self) {
        let FsNode::Dir { path, children, .. } = self else {
            return;
        };
        let Some(old) = children.take() else {
            return;
        };
        let mut old_dirs: std::collections::HashMap<PathBuf, FsNode> = old
            .into_iter()
            .filter(FsNode::is_dir)
            .map(|node| (node.path().to_path_buf(), node))
            .collect();
        let fresh = Self::load_one_level(path).unwrap_or_else(|err| {
            log::warn!("failed to read {}: {err}", path.display());
            Vec::new()
        });
        *children = Some(
            fresh
                .into_iter()
                .map(|node| match old_dirs.remove(node.path()) {
                    Some(kept) if node.is_dir() => kept,
                    _ => node,
                })
                .collect(),
        );
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

fn is_source_root(name: &str) -> bool {
    matches!(name, "java" | "kotlin" | "scala" | "groovy")
}

fn is_identifier(name: &str) -> bool {
    !name.starts_with(|c: char| c.is_ascii_digit())
        && !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
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
    fn refresh_children_keeps_subdir_state_and_picks_up_changes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/ui")).unwrap();
        std::fs::write(tmp.path().join("a.js"), "").unwrap();

        let mut root = FsNode::from_path(tmp.path());
        root.expand_toward(&tmp.path().join("src/ui"));

        std::fs::remove_file(tmp.path().join("a.js")).unwrap();
        std::fs::write(tmp.path().join("b.js"), "").unwrap();
        root.refresh_children();

        let names: Vec<_> = root
            .children()
            .unwrap()
            .iter()
            .map(|n| n.name().to_owned())
            .collect();
        assert_eq!(names, ["src", "b.js"]);
        let mut expanded = Vec::new();
        root.collect_expanded(&mut expanded);
        assert!(
            expanded.contains(&tmp.path().join("src/ui")),
            "{expanded:?}"
        );

        // Unloaded dirs stay unloaded.
        let mut fresh = FsNode::from_path(tmp.path());
        fresh.refresh_children();
        assert!(fresh.children().is_none());
    }

    #[test]
    fn set_expanded_reports_first_load_and_find_mut_locates_nodes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/ui")).unwrap();
        std::fs::write(tmp.path().join("src/ui/app.js"), "").unwrap();

        let mut root = FsNode::from_path(tmp.path());
        assert!(root.set_expanded(true), "first expand loads children");
        assert!(!root.set_expanded(true), "already loaded");

        // `src` is the root's only entry, so it was opened with the root.
        let src = root.find_mut(&tmp.path().join("src")).unwrap();
        assert!(!src.set_expanded(true));
        let ui = root.find_mut(&tmp.path().join("src/ui")).unwrap();
        assert!(ui.is_dir());
        assert!(root.find_mut(Path::new("/elsewhere")).is_none());
    }

    #[test]
    fn single_dir_chains_compact_into_one_package_row() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("src/main/java/co/istad/gateway");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("App.java"), "").unwrap();
        std::fs::create_dir_all(tmp.path().join("src/main/resources")).unwrap();

        let mut root = FsNode::from_path(tmp.path());
        root.expand_toward(&pkg);

        // `main` has two children (java + resources): no merge there.
        let main = root.find_mut(&tmp.path().join("src/main")).unwrap();
        assert_eq!(main.compact().0, "main");
        // The source root stands alone; the package chain below it is dotted.
        let java = root.find_mut(&tmp.path().join("src/main/java")).unwrap();
        let (label, deepest) = java.compact();
        assert_eq!(label, "java");
        assert_eq!(deepest.path(), tmp.path().join("src/main/java"));
        let co = root.find_mut(&tmp.path().join("src/main/java/co")).unwrap();
        let (label, deepest) = co.compact();
        assert_eq!(label, "co.istad.gateway");
        assert_eq!(deepest.path(), pkg);
        // `src` has a single child, `main`, which is not a leaf chain end.
        let src = root.find_mut(&tmp.path().join("src")).unwrap();
        assert_eq!(src.compact().0, "src/main");
    }

    #[test]
    fn chain_stops_where_a_file_or_sibling_appears() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b/c")).unwrap();
        std::fs::write(tmp.path().join("a/b/note.txt"), "").unwrap();
        let mut root = FsNode::from_path(tmp.path());
        root.expand_toward(&tmp.path().join("a/b/c"));
        let a = root.find_mut(&tmp.path().join("a")).unwrap();
        assert_eq!(a.compact().0, "a/b");
    }
}
