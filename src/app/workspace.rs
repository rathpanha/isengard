//! Open/switch/persist workspace, recents, titles, workspace menu actions.
use super::*;

impl IsengardApp {
    pub fn open_folder(&mut self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if !folder.is_dir() {
            self.remove_recent_folder(folder, cx);
            notify_error(
                format!("Folder not found: {}", folder.display()),
                window,
                cx,
            );
            return;
        }
        self.request_switch(Switch::Folder(folder.to_path_buf()), window, cx);
    }

    /// Replaces the workspace with the one saved in `file`.
    pub fn open_workspace_file(
        &mut self,
        file: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match Workspace::load(file) {
            Ok(workspace) => self.request_switch(Switch::Workspace(workspace), window, cx),
            Err(err) => {
                if !file.exists() {
                    self.remove_recent_workspace(file, cx);
                }
                notify_error(format!("Could not open workspace: {err:#}"), window, cx);
            }
        }
    }

    /// Adds root folders to the current workspace, keeping open tabs. A saved
    /// workspace file is updated right away.
    pub(super) fn add_folders(&mut self, folders: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let was_empty = self.workspace.is_empty();
        let mut added = false;
        for folder in folders.iter().filter(|f| f.is_dir()) {
            added |= self.workspace.add_folder(folder);
        }
        if !added {
            return;
        }
        if was_empty && !self.workspace.is_multi_root() {
            self.config.add_recent_folder(&self.workspace.folders()[0]);
            self.config.save();
        }
        if was_empty {
            self.switch_agent_panel(window, cx);
        } else {
            self.sync_agent_roots(cx);
        }
        self.persist_workspace(window, cx);
        self.workspace_changed(window, cx);
        self.persist_session(cx);
    }

    /// Re-writes the workspace file after a folder change, if there is one.
    pub(super) fn persist_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(err) = self.workspace.save() {
            notify_error(format!("Could not save workspace: {err:#}"), window, cx);
        }
    }

    /// Re-syncs the tree and titles after `workspace` changed.
    pub(super) fn workspace_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_tree.set_folders(self.workspace.folders(), cx);
        self.refresh_git_panel(window, cx);
        self.restart_fs_watch(window, cx);
        window.set_window_title(&self.window_title());
        self.refresh_menus(cx);
        cx.notify();
    }

    /// Rebuild File/Edit/View for welcome vs folder/workspace (+ tabs).
    pub(super) fn refresh_menus(&self, cx: &mut Context<Self>) {
        menus::update(
            cx,
            &self.app_menu_bar,
            menus::MenuContext {
                has_folder: !self.workspace.is_empty(),
                has_tabs: !self.tabs.is_empty(),
                close_label: self.workspace.close_menu_label(),
            },
        );
    }

    pub(super) fn refresh_git_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folders = self.workspace.folders().to_vec();
        // Few `rev-parse`s — cheap; keys match what the panel will load.
        let labels: HashMap<PathBuf, String> = crate::git::discover_repos(&folders)
            .into_iter()
            .map(|root| {
                let label = root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.display().to_string());
                (root, label)
            })
            .collect();
        self.git
            .update(cx, |panel, cx| panel.set_folders(folders, labels, window, cx));
    }

    pub(super) fn window_title(&self) -> String {
        if self.workspace.is_empty() {
            "Isengard".to_owned()
        } else {
            format!("{} — Isengard", self.workspace.title())
        }
    }

    /// Leaves the current workspace: offers to save an untitled multi-root
    /// workspace, then to save unsaved files, then switches.
    pub(super) fn request_switch(&mut self, switch: Switch, window: &mut Window, cx: &mut Context<Self>) {
        if !self.workspace.is_untitled_multi_root() {
            return self.confirm_unsaved_then(switch, window, cx);
        }
        let after_discard = switch.clone();
        self.open_choice_dialog(
            "Save workspace?",
            "Save the folders in this workspace to a workspace file so you can reopen it later?",
            "Don't Save",
            "Save…",
            move |this, window, cx| this.confirm_unsaved_then(after_discard.clone(), window, cx),
            move |this, window, cx| this.save_workspace_as(Some(switch.clone()), window, cx),
            window,
            cx,
        );
    }

    pub(super) fn confirm_unsaved_then(
        &mut self,
        switch: Switch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_unsaved() {
            return self.apply_switch(switch, window, cx);
        }
        let after_discard = switch.clone();
        let noun = self.workspace.kind_noun();
        let abort = if matches!(switch, Switch::Close) {
            Some(if noun == "workspace" {
                "Save cancelled — workspace still open"
            } else {
                "Save cancelled — folder still open"
            })
        } else {
            Some("Save cancelled — switch aborted")
        };
        self.open_choice_dialog(
            "Unsaved changes",
            format!(
                "Some files have unsaved changes. Save them before closing this {noun}?"
            ),
            "Don't Save",
            "Save All",
            move |this, window, cx| this.apply_switch(after_discard.clone(), window, cx),
            move |this, window, cx| {
                let switch = switch.clone();
                this.request_save_all(
                    move |this, window, cx| this.apply_switch(switch.clone(), window, cx),
                    abort,
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    pub(super) fn apply_switch(&mut self, switch: Switch, window: &mut Window, cx: &mut Context<Self>) {
        // Welcome recent rows unmount on switch; clear any path tooltip that
        // lost its trigger (Kit hide can race a pending show timer).
        window.dismiss_tooltip(cx);
        self.persist_session(cx);
        self.tabs = TabList::default();
        self.workspace = match switch {
            Switch::Folder(folder) => {
                self.config.add_recent_folder(&folder);
                Workspace::from_folder(&folder)
            }
            Switch::Workspace(workspace) => {
                if let Some(file) = workspace.file() {
                    self.config.add_recent_workspace(file);
                }
                workspace
            }
            Switch::Close => Workspace::default(),
        };
        self.lsp
            .reset_workspace(self.workspace.folders().to_vec());
        self.config.save();
        self.focus_handle.focus(window, cx);
        self.workspace_changed(window, cx);
        self.restart_terminal_for_workspace(window, cx);
        self.switch_agent_panel(window, cx);
        self.restore_session(window, cx);
    }

    pub(super) fn save_workspace_as(
        &mut self,
        next: Option<Switch>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = self
            .workspace
            .folders()
            .first()
            .and_then(|folder| folder.parent())
            .map(Path::to_path_buf)
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let name = format!("{}.{WORKSPACE_EXTENSION}", self.workspace.display_name());
        let path = cx.prompt_for_new_path(&dir, Some(&name));
        let abort_close = matches!(next, Some(Switch::Close));
        cx.spawn_in(window, async move |this, window| {
            let path = match path.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => {
                    if abort_close {
                        this.update_in(window, |_, window, cx| {
                            notify_error(
                                "Save cancelled — workspace still open",
                                window,
                                cx,
                            );
                        })
                        .ok();
                    }
                    return;
                }
                Ok(Err(err)) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error(format!("Save dialog failed: {err:#}"), window, cx);
                    })
                    .ok();
                    return;
                }
                Err(_) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error("Save dialog was interrupted", window, cx);
                    })
                    .ok();
                    return;
                }
            };
            this.update_in(window, |this, window, cx| {
                let path = if Workspace::is_workspace_file(&path) {
                    path
                } else {
                    path.with_extension(WORKSPACE_EXTENSION)
                };
                match this.workspace.save_as(&path) {
                    Ok(()) => {
                        this.config.add_recent_workspace(&path);
                        this.config.save();
                        this.workspace_changed(window, cx);
                        notify_success(
                            format!("Saved workspace {}", document::file_name(&path)),
                            window,
                            cx,
                        );
                        if let Some(next) = next {
                            this.confirm_unsaved_then(next, window, cx);
                        }
                    }
                    Err(err) => {
                        notify_error(format!("Could not save workspace: {err:#}"), window, cx)
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn recent_folders(&self) -> &[PathBuf] {
        &self.config.recent_folders
    }

    pub(crate) fn recent_workspaces(&self) -> &[PathBuf] {
        &self.config.recent_workspaces
    }

    pub fn remove_recent_workspace(&mut self, file: &Path, cx: &mut Context<Self>) {
        self.config.remove_recent_workspace(file);
        self.config.save();
        cx.notify();
    }

    pub fn remove_recent_folder(&mut self, folder: &Path, cx: &mut Context<Self>) {
        self.config.remove_recent_folder(folder);
        self.config.save();
        cx.notify();
    }

    /// Opens `path` in a tab. `permanent == false` is a preview tab (italic);
    /// another preview open replaces it. Double-click / edit / `permanent`
    /// pins the tab.
    ///
    /// Preview opens keep keyboard focus on the file tree so Delete / F2 still
    pub(super) fn active_root(&self, cx: &App) -> Option<PathBuf> {
        let folders = self.workspace.folders();
        let hint = self.file_tree.selected_path(cx).or_else(|| {
            self.tabs
                .active()
                .map(|t| t.path.clone())
                .filter(|p| !document::is_untitled(p))
        });
        if let Some(hint) = hint
            && let Some(root) = folders.iter().find(|r| hint.starts_with(r))
        {
            return Some(root.clone());
        }
        folders.first().cloned()
    }

    pub(super) fn on_new_workspace(
        &mut self,
        _: &NewWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Select Folders for Workspace".into()),
        });
        cx.spawn_in(window, async move |this, window| {
            let folders = match paths.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) => return,
                Ok(Err(err)) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error(format!("Open dialog failed: {err:#}"), window, cx);
                    })
                    .ok();
                    return;
                }
                Err(_) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error("Open dialog was interrupted", window, cx);
                    })
                    .ok();
                    return;
                }
            };
            let folders: Vec<PathBuf> = folders.into_iter().filter(|f| f.is_dir()).collect();
            if folders.is_empty() {
                return;
            }
            this.update_in(window, |this, window, cx| {
                this.prompt_save_new_workspace(folders, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Asks where to write the `.isengard-workspace` file, then opens it.
    pub(super) fn prompt_save_new_workspace(
        &mut self,
        folders: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = folders
            .first()
            .and_then(|folder| folder.parent())
            .map(Path::to_path_buf)
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let name = folders
            .first()
            .map(|folder| format!("{}.{WORKSPACE_EXTENSION}", document::file_name(folder)))
            .unwrap_or_else(|| format!("Untitled.{WORKSPACE_EXTENSION}"));
        let path = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |this, window| {
            let path = match path.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => return,
                Ok(Err(err)) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error(format!("Save dialog failed: {err:#}"), window, cx);
                    })
                    .ok();
                    return;
                }
                Err(_) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error("Save dialog was interrupted", window, cx);
                    })
                    .ok();
                    return;
                }
            };
            this.update_in(window, |this, window, cx| {
                let path = if Workspace::is_workspace_file(&path) {
                    path
                } else {
                    path.with_extension(WORKSPACE_EXTENSION)
                };
                let mut workspace = Workspace::default();
                for folder in &folders {
                    workspace.add_folder(folder);
                }
                match workspace.save_as(&path) {
                    Ok(()) => {
                        this.request_switch(Switch::Workspace(workspace), window, cx);
                        notify_success(
                            format!("Created workspace {}", document::file_name(&path)),
                            window,
                            cx,
                        );
                    }
                    Err(err) => {
                        notify_error(format!("Could not create workspace: {err:#}"), window, cx)
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn on_open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(true, window, cx);
    }

    pub(super) fn on_open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(false, window, cx);
    }

    pub(super) fn prompt_and_open(&mut self, directories: bool, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: Some(
                if directories {
                    "Open Folder"
                } else {
                    "Open File"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |this, window| {
            let path = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(err)) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error(format!("Open dialog failed: {err:#}"), window, cx);
                    })
                    .ok();
                    None
                }
                Err(_) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error("Open dialog was interrupted", window, cx);
                    })
                    .ok();
                    None
                }
            };
            let Some(path) = path else {
                return;
            };
            this.update_in(window, |this, window, cx| {
                if directories {
                    this.open_folder(&path, window, cx);
                } else if Workspace::is_workspace_file(&path) {
                    this.open_workspace_file(&path, window, cx);
                } else {
                    this.open_file(&path, true, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn on_open_workspace(
        &mut self,
        _: &OpenWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open Workspace".into()),
        });
        cx.spawn_in(window, async move |this, window| {
            let path = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(err)) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error(format!("Open dialog failed: {err:#}"), window, cx);
                    })
                    .ok();
                    None
                }
                Err(_) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error("Open dialog was interrupted", window, cx);
                    })
                    .ok();
                    None
                }
            };
            let Some(path) = path else {
                return;
            };
            this.update_in(window, |this, window, cx| {
                this.open_workspace_file(&path, window, cx)
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn on_add_folder_to_workspace(
        &mut self,
        _: &AddFolderToWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add Folder to Workspace".into()),
        });
        cx.spawn_in(window, async move |this, window| {
            let paths = match paths.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) => return,
                Ok(Err(err)) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error(format!("Open dialog failed: {err:#}"), window, cx);
                    })
                    .ok();
                    return;
                }
                Err(_) => {
                    this.update_in(window, |_, window, cx| {
                        notify_error("Open dialog was interrupted", window, cx);
                    })
                    .ok();
                    return;
                }
            };
            this.update_in(window, |this, window, cx| {
                this.add_folders(&paths, window, cx)
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn on_save_workspace_as(
        &mut self,
        _: &SaveWorkspaceAs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_empty() {
            notify_error(
                "Open a folder before saving a workspace.",
                window,
                cx,
            );
            return;
        }
        self.save_workspace_as(None, window, cx);
    }

    pub(super) fn on_remove_workspace_folder(
        &mut self,
        action: &RemoveWorkspaceFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.remove_folder(&action.0) {
            self.sync_agent_roots(cx);
            self.persist_workspace(window, cx);
            self.workspace_changed(window, cx);
            self.persist_session(cx);
        }
    }
    pub(super) fn on_close_workspace(
        &mut self,
        _: &CloseWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_empty() && self.tabs.is_empty() {
            return notify_error("Nothing open to close", window, cx);
        }
        // AppMenuBar focuses the prior target then `dispatch_action` (deferred),
        // then dismiss restores focus again. Git chrome buttons are ephemeral —
        // run close on the next tick after parking on our stable handle.
        self.focus_handle.focus(window, cx);
        cx.spawn_in(window, async move |this, window| {
            this.update_in(window, |this, window, cx| {
                this.focus_handle.focus(window, cx);
                this.request_switch(Switch::Close, window, cx);
            })
            .ok();
        })
        .detach();
    }
}
