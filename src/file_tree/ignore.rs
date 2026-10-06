//! Per-workspace-root `.gitignore` matching (VS Code–style: show, but dim).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Compiled gitignore rules for each workspace root folder.
#[derive(Default)]
pub struct GitIgnoreIndex {
    by_root: HashMap<PathBuf, Gitignore>,
}

impl GitIgnoreIndex {
    /// Rebuilds matchers for `roots` (root `.gitignore` plus nested ones found
    /// by a shallow walk). Call after folder set / tree expand so new nested
    /// ignore files are picked up.
    pub fn rebuild(&mut self, roots: &[PathBuf]) {
        self.by_root.clear();
        for root in roots {
            if !root.is_dir() {
                continue;
            }
            self.by_root.insert(root.clone(), compile_for_root(root));
        }
    }

    /// `true` if `path` is ignored by the workspace root that contains it.
    pub fn is_ignored(&self, path: &Path) -> bool {
        let Some(root) = self
            .by_root
            .keys()
            .filter(|r| path.starts_with(r))
            .max_by_key(|r| r.as_os_str().len())
        else {
            return false;
        };
        let Some(gi) = self.by_root.get(root) else {
            return false;
        };
        let Ok(rel) = path.strip_prefix(root) else {
            return false;
        };
        if rel.as_os_str().is_empty() {
            return false;
        }
        gi.matched_path_or_any_parents(rel, path.is_dir())
            .is_ignore()
    }
}

fn compile_for_root(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    add_gitignore_files(root, &mut builder, 0);
    builder.build().unwrap_or_else(|err| {
        log::warn!("gitignore for {}: {err}", root.display());
        Gitignore::empty()
    })
}

/// Walks directories under `root` adding every `.gitignore`. Caps depth so a
/// huge monorepo doesn't stall open; nested rules deeper than the cap still
/// apply once those dirs are expanded and we refresh.
fn add_gitignore_files(dir: &Path, builder: &mut GitignoreBuilder, depth: u32) {
    const MAX_DEPTH: u32 = 8;
    let gi = dir.join(".gitignore");
    if gi.is_file() {
        let _ = builder.add(&gi);
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if name == ".git" || name == "node_modules" || name == "target" {
            continue;
        }
        // Still descend into dirs that may be ignored — their .gitignore can
        // un-ignore children; VS Code reads those files too.
        add_gitignore_files(&path, builder, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_gitignored_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join(".gitignore"), "secret.txt\nbuild/\n").unwrap();
        std::fs::write(root.join("secret.txt"), "").unwrap();
        std::fs::write(root.join("keep.txt"), "").unwrap();
        std::fs::create_dir(root.join("build")).unwrap();
        std::fs::write(root.join("build/out.js"), "").unwrap();

        let mut index = GitIgnoreIndex::default();
        index.rebuild(&[root.to_path_buf()]);

        assert!(index.is_ignored(&root.join("secret.txt")));
        assert!(index.is_ignored(&root.join("build")));
        assert!(index.is_ignored(&root.join("build/out.js")));
        assert!(!index.is_ignored(&root.join("keep.txt")));
        assert!(!index.is_ignored(root));
    }

    #[test]
    fn nested_gitignore_is_applied() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("pkg")).unwrap();
        std::fs::write(root.join("pkg/.gitignore"), "local.only\n").unwrap();
        std::fs::write(root.join("pkg/local.only"), "").unwrap();
        std::fs::write(root.join("pkg/ok.txt"), "").unwrap();

        let mut index = GitIgnoreIndex::default();
        index.rebuild(&[root.to_path_buf()]);

        assert!(index.is_ignored(&root.join("pkg/local.only")));
        assert!(!index.is_ignored(&root.join("pkg/ok.txt")));
    }
}
