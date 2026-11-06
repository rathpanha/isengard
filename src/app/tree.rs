//! File-tree inline create/rename/delete and path remaps.
use super::tab::TabBody;
use super::*;

/// What Cut / Copy left for Paste in the file tree.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TreeClipboard {
    path: PathBuf,
    cut: bool,
}

impl IsengardApp {
    pub(super) fn begin_inline_rename(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_in_workspace(&path) {
            return;
        }
        let suggested = document::file_name(&path);
        self.begin_tree_edit(TreeEditTarget::Rename(path), suggested, window, cx);
    }

    pub(super) fn begin_inline_new_file(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !dir.is_dir() || !self.path_in_workspace(&dir) {
            return;
        }
        let suggested = document::unique_new_file_path(&dir)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled.txt".into());
        self.begin_tree_edit(
            TreeEditTarget::NewFile { parent: dir },
            suggested,
            window,
            cx,
        );
    }

    pub(super) fn begin_inline_new_folder(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !dir.is_dir() || !self.path_in_workspace(&dir) {
            return;
        }
        let suggested = document::unique_new_folder_path(&dir)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "New Folder".into());
        self.begin_tree_edit(
            TreeEditTarget::NewFolder { parent: dir },
            suggested,
            window,
            cx,
        );
    }

    pub(super) fn begin_tree_edit(
        &mut self,
        target: TreeEditTarget,
        suggested: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Drop any in-progress edit first (drops Escape intercept + create row).
        self.tree_edit = None;
        self.file_tree.set_creating(None, cx);

        match &target {
            TreeEditTarget::Rename(path) => self.file_tree.select_path(path, cx),
            TreeEditTarget::NewFile { parent } => {
                self.file_tree
                    .set_creating(Some((parent.clone(), false)), cx);
            }
            TreeEditTarget::NewFolder { parent } => {
                self.file_tree
                    .set_creating(Some((parent.clone(), true)), cx);
            }
        }

        let input = cx.new(|cx| InputState::new(window, cx).default_value(suggested));
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        let events =
            cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        this.commit_tree_edit(window, cx);
                    }
                    _ => {}
                },
            );
        let weak = cx.weak_entity();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if event.keystroke.key.as_str() != "escape" {
                return;
            }
            let Some(entity) = weak.upgrade() else {
                return;
            };
            entity.update(cx, |this, cx| {
                if this.tree_edit.is_some() {
                    this.cancel_tree_edit(window, cx);
                    cx.stop_propagation();
                }
            });
        });
        self.tree_edit = Some(TreeInlineEdit {
            target,
            input,
            _events: events,
            _keys: keys,
        });
        cx.notify();
    }

    pub(super) fn commit_tree_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.tree_edit.take() else {
            return;
        };
        let name = edit.input.read(cx).value().to_string();
        let opened_new_file = matches!(edit.target, TreeEditTarget::NewFile { .. });
        let result = match &edit.target {
            TreeEditTarget::Rename(path) => self.rename_path(path, &name, window, cx),
            TreeEditTarget::NewFile { parent } => {
                if name.trim().is_empty() {
                    self.file_tree.set_creating(None, cx);
                    self.focus_file_tree(window, cx);
                    cx.notify();
                    return;
                }
                self.create_file_in(parent, &name, window, cx)
            }
            TreeEditTarget::NewFolder { parent } => {
                if name.trim().is_empty() {
                    self.file_tree.set_creating(None, cx);
                    self.focus_file_tree(window, cx);
                    cx.notify();
                    return;
                }
                self.create_folder_in(parent, &name, window, cx)
            }
        };
        if let Err(err) = result {
            notify_error(err, window, cx);
            // Keep the same field so Escape-intercept / Enter still work.
            if matches!(
                edit.target,
                TreeEditTarget::NewFile { .. } | TreeEditTarget::NewFolder { .. }
            ) {
                let is_folder = matches!(edit.target, TreeEditTarget::NewFolder { .. });
                let parent = match &edit.target {
                    TreeEditTarget::NewFile { parent } | TreeEditTarget::NewFolder { parent } => {
                        parent.clone()
                    }
                    TreeEditTarget::Rename(_) => unreachable!(),
                };
                self.file_tree.set_creating(Some((parent, is_folder)), cx);
            }
            edit.input.update(cx, |state, cx| {
                state.focus(window, cx);
                state.select_all(window, cx);
            });
            self.tree_edit = Some(edit);
            cx.notify();
            return;
        }
        self.file_tree.set_creating(None, cx);
        // New file already focused the editor; rename / new folder keep the tree.
        if !opened_new_file {
            self.focus_file_tree(window, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_tree_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tree_edit.take().is_none() {
            return;
        }
        self.file_tree.set_creating(None, cx);
        self.focus_file_tree(window, cx);
        cx.notify();
    }

    pub(super) fn path_in_workspace(&self, path: &Path) -> bool {
        self.workspace
            .folders()
            .iter()
            .any(|root| path == root.as_path() || path.starts_with(root))
    }

    pub(super) fn create_file_in(
        &mut self,
        dir: &Path,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let name = document::validate_entry_name(name).map_err(|e| e.to_string())?;
        let path = dir.join(name);
        if path.exists() {
            return Err(format!("\"{name}\" already exists."));
        }
        std::fs::write(&path, "").map_err(|err| format!("Could not create file: {err}"))?;
        self.file_tree.set_creating(None, cx);
        self.file_tree.refresh_dir(dir, cx);
        self.file_tree.select_path(&path, cx);
        self.open_file(&path, true, window, cx);
        self.persist_session(cx);
        cx.notify();
        Ok(())
    }

    pub(super) fn create_folder_in(
        &mut self,
        dir: &Path,
        name: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let name = document::validate_entry_name(name).map_err(|e| e.to_string())?;
        let path = dir.join(name);
        if path.exists() {
            return Err(format!("\"{name}\" already exists."));
        }
        std::fs::create_dir(&path).map_err(|err| format!("Could not create folder: {err}"))?;
        self.file_tree.set_creating(None, cx);
        self.file_tree.refresh_dir(dir, cx);
        self.file_tree.select_path(&path, cx);
        self.persist_session(cx);
        cx.notify();
        Ok(())
    }

    pub(super) fn rename_path(
        &mut self,
        path: &Path,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.path_in_workspace(path) {
            return Err("Path is not in the workspace.".into());
        }
        let name = document::validate_entry_name(name).map_err(|e| e.to_string())?;
        let Some(parent) = path.parent() else {
            return Err("Cannot rename this path.".into());
        };
        let new_path = parent.join(name);
        if new_path == path {
            return Ok(());
        }
        if new_path.exists() {
            return Err(format!("\"{name}\" already exists."));
        }
        std::fs::rename(path, &new_path).map_err(|err| format!("Rename failed: {err}"))?;
        self.remap_open_paths(path, &new_path, window, cx);
        if self.workspace.replace_folder(path, &new_path) {
            self.persist_workspace(window, cx);
            self.workspace_changed(window, cx);
        } else {
            self.file_tree.refresh_dir(parent, cx);
        }
        self.file_tree.select_path(&new_path, cx);
        self.persist_session(cx);
        cx.notify();
        Ok(())
    }

    /// Moves `from` into the folder `to_dir` (tree drag-and-drop).
    pub(super) fn move_path(
        &mut self,
        from: &Path,
        to_dir: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.path_in_workspace(from) || !self.path_in_workspace(to_dir) {
            return Err("Path is not in the workspace.".into());
        }
        if self.workspace.folders().iter().any(|root| root == from) {
            return Err("Workspace folders cannot be moved.".into());
        }
        if !to_dir.is_dir() {
            return Err("Drop target is not a folder.".into());
        }
        if !crate::ui::file_tree::move_allowed(from, to_dir) {
            return Err("Cannot move a folder into itself.".into());
        }
        let Some(name) = from.file_name() else {
            return Err("Cannot move this path.".into());
        };
        let new_path = to_dir.join(name);
        if new_path.exists() {
            return Err(format!(
                "\"{}\" already exists in {}.",
                name.to_string_lossy(),
                document::file_name(to_dir)
            ));
        }
        std::fs::rename(from, &new_path).map_err(|err| format!("Move failed: {err}"))?;
        self.remap_open_paths(from, &new_path, window, cx);
        if let Some(parent) = from.parent() {
            self.file_tree.refresh_dir(parent, cx);
        }
        self.file_tree.refresh_dir(to_dir, cx);
        self.file_tree.select_path(&new_path, cx);
        self.persist_session(cx);
        cx.notify();
        Ok(())
    }

    pub(super) fn on_toggle_dir(
        &mut self,
        action: &ToggleDir,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_tree
            .set_dir_expanded(&action.path, action.expand, cx);
        self.persist_session(cx);
    }

    pub(super) fn on_move_path(
        &mut self,
        action: &MovePath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tree_edit.is_some() {
            self.cancel_tree_edit(window, cx);
        }
        if let Err(err) = self.move_path(&action.from, &action.to_dir, window, cx) {
            notify_error(err, window, cx);
        }
    }

    /// Remembers `path` for Paste; a cut is only a move once pasted.
    pub(super) fn set_tree_clipboard(
        &mut self,
        path: PathBuf,
        cut: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_in_workspace(&path) {
            return;
        }
        if cut && self.workspace.folders().contains(&path) {
            return notify_error("Workspace folders cannot be moved.", window, cx);
        }
        self.tree_clipboard = Some(TreeClipboard {
            path: path.clone(),
            cut,
        });
        self.file_tree.set_clipboard(Some((path, cut)), cx);
        cx.notify();
    }

    /// Pastes the tree clipboard into `target` (a folder) or `target`'s parent.
    pub(super) fn paste_path(
        &mut self,
        target: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(clip) = self.tree_clipboard.clone() else {
            return Err("Nothing to paste.".into());
        };
        let dir = if target.is_dir() {
            target
        } else {
            target.parent().ok_or("Cannot paste here.")?
        };
        if !clip.path.exists() {
            self.tree_clipboard = None;
            self.file_tree.set_clipboard(None, cx);
            return Err(format!(
                "{} no longer exists.",
                document::file_name(&clip.path)
            ));
        }
        if clip.cut {
            // Pasting a cut back where it came from changes nothing.
            if clip.path.parent() == Some(dir) {
                return Ok(());
            }
            self.move_path(&clip.path, dir, window, cx)?;
            self.tree_clipboard = None;
            self.file_tree.set_clipboard(None, cx);
            Ok(())
        } else {
            self.copy_path(&clip.path, dir, cx)
        }
    }

    /// Copies `from` into the folder `to_dir` under a name that is not taken yet
    /// (`a copy.txt`, `a copy 2.txt`, …) — never overwrites.
    pub(super) fn copy_path(
        &mut self,
        from: &Path,
        to_dir: &Path,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.path_in_workspace(from) || !self.path_in_workspace(to_dir) {
            return Err("Path is not in the workspace.".into());
        }
        if !to_dir.is_dir() {
            return Err("Paste target is not a folder.".into());
        }
        if to_dir.starts_with(from) {
            return Err("Cannot copy a folder into itself.".into());
        }
        let Some(name) = from.file_name() else {
            return Err("Cannot copy this path.".into());
        };
        let new_path = unique_copy_path(to_dir, name, from.is_dir());
        if let Err(err) = copy_recursive(from, &new_path) {
            // `new_path` did not exist before, so clearing it is safe.
            let _ = if new_path.is_dir() {
                std::fs::remove_dir_all(&new_path)
            } else {
                std::fs::remove_file(&new_path)
            };
            return Err(format!("Copy failed: {err}"));
        }
        self.file_tree.refresh_dir(to_dir, cx);
        self.file_tree.select_path(&new_path, cx);
        self.persist_session(cx);
        cx.notify();
        Ok(())
    }

    pub(super) fn on_cut_path(
        &mut self,
        action: &CutPath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_tree_clipboard(action.0.clone(), true, window, cx);
    }

    pub(super) fn on_copy_path(
        &mut self,
        action: &CopyPath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_tree_clipboard(action.0.clone(), false, window, cx);
    }

    pub(super) fn on_paste_path(
        &mut self,
        action: &PastePath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(err) = self.paste_path(&action.0, window, cx) {
            notify_error(err, window, cx);
        }
    }

    pub(super) fn on_cut_selected_tree_item(
        &mut self,
        _: &CutSelectedTreeItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.file_tree.selected_path(cx) {
            self.set_tree_clipboard(path, true, window, cx);
        }
    }

    pub(super) fn on_copy_selected_tree_item(
        &mut self,
        _: &CopySelectedTreeItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.file_tree.selected_path(cx) {
            self.set_tree_clipboard(path, false, window, cx);
        }
    }

    /// Paste into the selection's folder, else the first workspace root.
    pub(super) fn on_paste_into_tree_selection(
        &mut self,
        _: &PasteIntoTreeSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self
            .file_tree
            .selected_path(cx)
            .or_else(|| self.workspace.folders().first().cloned())
        else {
            return;
        };
        if let Err(err) = self.paste_path(&target, window, cx) {
            notify_error(err, window, cx);
        }
    }

    pub(super) fn delete_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if !self.path_in_workspace(path) {
            return;
        }
        let open: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| t.path == path || t.path.starts_with(path))
            .map(|t| t.path.clone())
            .collect();
        if !open.is_empty() {
            self.close_tabs(&open, window, cx);
        }
        if let Err(err) = trash::delete(path) {
            return notify_error(format!("Move to Trash failed: {err}"), window, cx);
        }
        let was_root = self.workspace.remove_folder(path);
        if was_root {
            self.persist_workspace(window, cx);
            self.workspace_changed(window, cx);
        } else if let Some(parent) = path.parent() {
            self.file_tree.refresh_dir(parent, cx);
        }
        notify_success("Moved to Trash", window, cx);
        self.persist_session(cx);
        cx.notify();
    }

    /// Retargets open tabs whose path equals `from` or lives under it.
    pub(super) fn remap_open_paths(
        &mut self,
        from: &Path,
        to: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let indexes: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.path == from || t.path.starts_with(from))
            .map(|(ix, _)| ix)
            .collect();
        for ix in indexes {
            let (old_path, new_path, attach) = {
                let Some(tab) = self.tabs.get_mut(ix) else {
                    continue;
                };
                let new_path = if tab.path == from {
                    to.to_path_buf()
                } else if let Ok(rest) = tab.path.strip_prefix(from) {
                    to.join(rest)
                } else {
                    continue;
                };
                let old_path = tab.path.clone();
                tab.path = new_path.clone();
                let attach = if let TabBody::Text {
                    language, state, ..
                } = &mut tab.body
                {
                    let lang = Language::from_path(&new_path);
                    *language = lang;
                    let name = lang.highlighter_name();
                    state.update(cx, |state, cx| {
                        state.set_highlighter(name, cx);
                    });
                    Some((lang, state.clone()))
                } else {
                    None
                };
                (old_path, new_path, attach)
            };
            self.lsp.did_close(&old_path);
            if let Some((language, state)) = attach {
                self.lsp_attach_tab(&new_path, language, &state, window, cx);
            }
        }
    }

    /// Returns `true` if the app may close now; otherwise asks the user first.
    pub(super) fn on_rename_path(
        &mut self,
        action: &RenamePath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_inline_rename(action.0.clone(), window, cx);
    }

    pub(super) fn on_delete_path(
        &mut self,
        action: &DeletePath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_path(&action.0, window, cx);
    }

    pub(super) fn on_reveal_path(
        &mut self,
        action: &RevealPath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(err) = reveal_in_file_manager(&action.0) {
            notify_error(
                format!("Could not open the file manager: {err}"),
                window,
                cx,
            );
        }
    }

    pub(super) fn on_new_file_in(
        &mut self,
        action: &NewFileIn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_inline_new_file(action.0.clone(), window, cx);
    }

    pub(super) fn on_new_folder_in(
        &mut self,
        action: &NewFolderIn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_inline_new_folder(action.0.clone(), window, cx);
    }

    pub(super) fn on_rename_selected_tree_item(
        &mut self,
        _: &RenameSelectedTreeItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.file_tree.selected_path(cx) {
            self.begin_inline_rename(path, window, cx);
        }
    }

    pub(super) fn on_delete_selected_tree_item(
        &mut self,
        _: &DeleteSelectedTreeItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.file_tree.selected_path(cx) {
            self.delete_path(&path, window, cx);
        }
    }
}

/// Open the OS file manager on `path`: a folder is opened, a file is selected
/// inside its parent where the platform supports it.
fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{} no longer exists", path.display()));
    }
    let open_dir = |dir: &Path| open::that_detached(dir).map_err(|e| e.to_string());
    if path.is_dir() {
        return open_dir(path);
    }
    let parent = path.parent().unwrap_or(path);
    let spawned = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .is_ok()
    } else if cfg!(target_os = "windows") {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        std::process::Command::new("explorer")
            .arg(arg)
            .spawn()
            .is_ok()
    } else {
        url::Url::from_file_path(path).is_ok_and(|uri| {
            std::process::Command::new("dbus-send")
                .args([
                    "--session",
                    "--dest=org.freedesktop.FileManager1",
                    "--type=method_call",
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1.ShowItems",
                ])
                .arg(format!("array:string:{uri}"))
                .arg("string:")
                .status()
                .is_ok_and(|s| s.success())
        })
    };
    if spawned { Ok(()) } else { open_dir(parent) }
}

/// `dir/name` when free, else `dir/<stem> copy[ N].<ext>`.
fn unique_copy_path(dir: &Path, name: &std::ffi::OsStr, is_dir: bool) -> PathBuf {
    let direct = dir.join(name);
    if !direct.exists() {
        return direct;
    }
    let as_path = Path::new(name);
    let (stem, ext) = if is_dir {
        (name.to_string_lossy().into_owned(), None)
    } else {
        (
            as_path
                .file_stem()
                .unwrap_or(name)
                .to_string_lossy()
                .into_owned(),
            as_path
                .extension()
                .map(|e| e.to_string_lossy().into_owned()),
        )
    };
    let mut n = 1;
    loop {
        let suffix = if n == 1 {
            " copy".to_string()
        } else {
            format!(" copy {n}")
        };
        let candidate = match &ext {
            Some(ext) => format!("{stem}{suffix}.{ext}"),
            None => format!("{stem}{suffix}"),
        };
        let path = dir.join(candidate);
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}

/// Copies a file, symlink (unix) or folder tree; `to` must not exist.
fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else if meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
        }
        #[cfg(not(unix))]
        {
            std::fs::copy(from, to).map(|_| ())
        }
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}
