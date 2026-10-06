use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::{
    ActiveTheme as _, IconName, Selectable as _, Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonGroup, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
    input::{Editor, EditorState, InputEvent, TabSize},
    menu::AppMenuBar,
    notification::Notification,
    resizable::{h_resizable, resizable_panel},
    status_bar::StatusBar,
    tab::{Tab, TabBar},
    text::TextView,
    tree::TreeEvent,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use serde::Deserialize;

use crate::config::{AppConfig, DEFAULT_EDITOR_FONT_SIZE};
use crate::editor::document;
use crate::editor::language::Language;
use crate::editor::tabs::TabList;
use crate::theme;
use crate::ui::components::center_dialog;
use crate::ui::file_tree::FileTreePanel;
use crate::ui::welcome;
use crate::workspace::{WORKSPACE_EXTENSION, Workspace};

actions!(
    isengard,
    [
        OpenFolder,
        OpenFile,
        OpenWorkspace,
        AddFolderToWorkspace,
        SaveWorkspaceAs,
        CloseWorkspace,
        Save,
        SaveAll,
        CloseTab,
        NextTab,
        PrevTab,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        TogglePreview,
        About,
        Quit,
    ]
);

/// Removes one root folder from the workspace (tree context menu on a root).
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct RemoveWorkspaceFolder(pub PathBuf);

/// What to switch the window to once the user has confirmed leaving the
/// current workspace (VS Code closes all editors when switching).
#[derive(Clone)]
enum Switch {
    Folder(PathBuf),
    Workspace(Workspace),
    Close,
}

/// Registers global key bindings. `secondary` is Cmd on macOS and Ctrl elsewhere.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-o", OpenFolder, None),
        KeyBinding::new("secondary-s", Save, None),
        KeyBinding::new("secondary-alt-s", SaveAll, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("secondary-=", IncreaseFontSize, None),
        KeyBinding::new("secondary--", DecreaseFontSize, None),
        KeyBinding::new("secondary-0", ResetFontSize, None),
        KeyBinding::new("secondary-shift-v", TogglePreview, None),
        KeyBinding::new("secondary-q", Quit, None),
    ]);
    // Fallback when the app view is not on the focus path (e.g. while a dialog is open).
    cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
}

struct EditorTab {
    path: PathBuf,
    body: TabBody,
    is_modified: bool,
    /// VS Code–style preview: replaced by the next single-click open until pinned.
    preview: bool,
}

enum TabBody {
    Text {
        language: Language,
        state: Entity<EditorState>,
        /// Rendered view (Markdown → TextView, SVG → img) instead of the source editor.
        rendered_preview: bool,
        _subscriptions: Vec<Subscription>,
    },
    Image,
}

impl EditorTab {
    fn label(&self) -> String {
        document::file_name(&self.path)
    }

    fn language(&self) -> Option<Language> {
        match &self.body {
            TabBody::Text { language, .. } => Some(*language),
            TabBody::Image => None,
        }
    }

    fn is_image(&self) -> bool {
        matches!(self.body, TabBody::Image)
    }

    fn editor_state(&self) -> Option<&Entity<EditorState>> {
        match &self.body {
            TabBody::Text { state, .. } => Some(state),
            TabBody::Image => None,
        }
    }

    fn rendered_preview(&self) -> bool {
        match &self.body {
            TabBody::Text {
                rendered_preview, ..
            } => *rendered_preview,
            TabBody::Image => false,
        }
    }

    fn supports_rendered_preview(&self) -> bool {
        self.language()
            .is_some_and(Language::supports_rendered_preview)
    }
}

pub struct IsengardApp {
    config: AppConfig,
    focus_handle: FocusHandle,
    /// The open folders; the file tree mirrors `workspace.folders()`.
    workspace: Workspace,
    file_tree: FileTreePanel,
    tabs: TabList<EditorTab>,
    app_menu_bar: Entity<AppMenuBar>,
    /// Set once the user chose to quit despite unsaved changes.
    allow_quit: bool,
    _subscriptions: Vec<Subscription>,
}

impl IsengardApp {
    pub fn new(
        config: AppConfig,
        initial_path: Option<PathBuf>,
        app_menu_bar: Entity<AppMenuBar>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let file_tree = FileTreePanel::new(cx);
        let tree_subscription =
            cx.subscribe(file_tree.state(), |this, _, event: &TreeEvent, cx| {
                this.file_tree.handle_event(event, cx);
            });

        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.confirm_quit(window, cx))
                .unwrap_or(true)
        });

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let mut app = Self {
            config,
            focus_handle,
            workspace: Workspace::default(),
            file_tree,
            tabs: TabList::default(),
            app_menu_bar,
            allow_quit: false,
            _subscriptions: vec![tree_subscription],
        };

        // Otherwise start on the welcome screen.
        match initial_path {
            Some(path) if Workspace::is_workspace_file(&path) => {
                app.open_workspace_file(&path, window, cx)
            }
            Some(path) if path.is_dir() => app.open_folder(&path, window, cx),
            Some(path) if path.is_file() => {
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    app.open_folder(parent, window, cx);
                }
                app.open_file(&path, true, window, cx);
            }
            _ => {}
        }
        app
    }

    // ---- Folders and files -------------------------------------------------

    /// Replaces the workspace with `folder` (asking first if work would be lost).
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
    fn add_folders(&mut self, folders: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
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
        self.persist_workspace(window, cx);
        self.workspace_changed(window, cx);
    }

    /// Re-writes the workspace file after a folder change, if there is one.
    fn persist_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(err) = self.workspace.save() {
            notify_error(format!("Could not save workspace: {err:#}"), window, cx);
        }
    }

    /// Re-syncs the tree and titles after `workspace` changed.
    fn workspace_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_tree.set_folders(self.workspace.folders(), cx);
        window.set_window_title(&self.window_title());
        cx.notify();
    }

    fn window_title(&self) -> String {
        if self.workspace.is_empty() {
            "Isengard".to_owned()
        } else {
            format!("{} — Isengard", self.workspace.title())
        }
    }

    /// Leaves the current workspace: offers to save an untitled multi-root
    /// workspace, then to save unsaved files, then switches.
    fn request_switch(&mut self, switch: Switch, window: &mut Window, cx: &mut Context<Self>) {
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

    fn confirm_unsaved_then(
        &mut self,
        switch: Switch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_unsaved() {
            return self.apply_switch(switch, window, cx);
        }
        let after_discard = switch.clone();
        self.open_choice_dialog(
            "Unsaved changes",
            "Some files have unsaved changes. Save them before closing this workspace?",
            "Don't Save",
            "Save All",
            move |this, window, cx| this.apply_switch(after_discard.clone(), window, cx),
            move |this, window, cx| match this.save_all(cx) {
                Ok(()) => this.apply_switch(switch.clone(), window, cx),
                Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
            },
            window,
            cx,
        );
    }

    fn apply_switch(&mut self, switch: Switch, window: &mut Window, cx: &mut Context<Self>) {
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
        self.config.save();
        self.focus_handle.focus(window, cx);
        self.workspace_changed(window, cx);
    }

    /// Asks where to save the workspace file, saves it, then continues with `next`.
    fn save_workspace_as(
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
        cx.spawn_in(window, async move |this, window| {
            let path = path.await.ok()?.ok()??;
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
                        window.push_notification(
                            format!("Saved workspace {}", document::file_name(&path)),
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
            .ok()
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
    fn open_file(
        &mut self,
        path: &Path,
        permanent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.tabs.position(|t| t.path == path) {
            if permanent {
                self.pin_tab(ix, cx);
            }
            self.activate_tab(ix, window, cx);
            return;
        }

        if !permanent {
            self.retire_preview_tab(window, cx);
        }

        if document::is_image(path) {
            if !path.is_file() {
                return notify_error(
                    format!("Image not found: {}", path.display()),
                    window,
                    cx,
                );
            }
            self.tabs.push(EditorTab {
                path: path.to_path_buf(),
                body: TabBody::Image,
                is_modified: false,
                preview: !permanent,
            });
            self.file_tree.select_path(path, cx);
            self.focus_handle.focus(window, cx);
            cx.notify();
            return;
        }

        let text = match document::read_text(path) {
            Ok(text) => text,
            Err(err) => return notify_error(format!("Could not open file: {err:#}"), window, cx),
        };

        let language = Language::from_path(path);
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.highlighter_name())
                .line_number(true)
                .indent_guides(true)
                .soft_wrap(false)
                .tab_size(TabSize {
                    tab_size: 2,
                    hard_tabs: false,
                })
                .default_value(text)
        });
        let subscriptions = vec![
            cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let id = state.entity_id();
                    if let Some(tab) = this.tabs.find_mut(|t| {
                        t.editor_state()
                            .is_some_and(|s| s.entity_id() == id)
                    }) {
                        tab.is_modified = true;
                        // Editing pins a preview tab (VS Code behaviour).
                        tab.preview = false;
                    }
                    cx.notify();
                }
            }),
            // Re-render on cursor moves so the status bar's Ln/Col stays current.
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];

        self.tabs.push(EditorTab {
            path: path.to_path_buf(),
            body: TabBody::Text {
                language,
                state,
                rendered_preview: false,
                _subscriptions: subscriptions,
            },
            is_modified: false,
            preview: !permanent,
        });
        self.file_tree.select_path(path, cx);
        self.focus_active_editor(window, cx);
        cx.notify();
    }

    fn pin_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(ix)
            && tab.preview
        {
            tab.preview = false;
            cx.notify();
        }
    }

    /// Drops a clean preview tab, or pins it if dirty, before opening another preview.
    fn retire_preview_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.position(|t| t.preview) else {
            return;
        };
        let dirty = self.tabs.get(ix).is_some_and(|t| t.is_modified);
        if dirty {
            self.pin_tab(ix, cx);
        } else if let Some(path) = self.tabs.get(ix).map(|t| t.path.clone()) {
            self.close_tab(&path, window, cx);
        }
    }

    fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.activate(ix) {
            if let Some(path) = self.tabs.active().map(|t| t.path.clone()) {
                self.file_tree.select_path(&path, cx);
            }
            self.focus_active_editor(window, cx);
            cx.notify();
        }
    }

    fn focus_active_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.tabs.active().and_then(|t| t.editor_state()) {
            Some(state) => state.update(cx, |state, cx| state.focus(window, cx)),
            None => self.focus_handle.focus(window, cx),
        }
    }

    fn save_tab(&mut self, path: &Path, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let Some(tab) = self.tabs.find_mut(|t| t.path == path) else {
            return Ok(());
        };
        let Some(state) = tab.editor_state() else {
            return Ok(());
        };
        let text = state.read(cx).value();
        document::write_text(&tab.path, &text)?;
        tab.is_modified = false;
        cx.notify();
        Ok(())
    }

    fn save_all(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let modified: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| t.is_modified)
            .map(|t| t.path.clone())
            .collect();
        for path in modified {
            self.save_tab(&path, cx)?;
        }
        Ok(())
    }

    fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.is_modified)
    }

    /// Closes the tab for `path`, asking first if it has unsaved changes.
    fn request_close(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|t| t.path == path) else {
            return;
        };
        if tab.is_modified {
            self.confirm_close_tab(path.to_path_buf(), window, cx);
        } else {
            self.close_tab(path, window, cx);
        }
    }

    fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.position(|t| t.path == path) {
            self.tabs.remove(ix);
            self.focus_active_editor(window, cx);
            cx.notify();
        }
    }

    // ---- Dialogs -----------------------------------------------------------

    /// Opens a confirmation dialog with Cancel, a secondary choice, and the
    /// primary (safe default) choice. Choices run on the app after the dialog closes.
    #[allow(clippy::too_many_arguments)]
    fn open_choice_dialog(
        &self,
        title: &'static str,
        message: impl Into<SharedString>,
        secondary_label: &'static str,
        primary_label: &'static str,
        on_secondary: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        on_primary: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity().downgrade();
        let message: SharedString = message.into();
        let on_secondary = Rc::new(on_secondary);
        let on_primary = Rc::new(on_primary);
        window.open_dialog(cx, move |dialog, window, _| {
            let (app_secondary, app_primary) = (app.clone(), app.clone());
            let (on_secondary, on_primary) = (on_secondary.clone(), on_primary.clone());
            center_dialog(
                dialog.title(title).child(message.clone()).footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("dialog-cancel")
                                .cursor_pointer()
                                .outline()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("dialog-secondary")
                                .cursor_pointer()
                                .outline()
                                .label(secondary_label)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = app_secondary
                                        .update(cx, |this, cx| on_secondary(this, window, cx));
                                }),
                        )
                        .child(
                            Button::new("dialog-primary")
                                .cursor_pointer()
                                .primary()
                                .label(primary_label)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = app_primary
                                        .update(cx, |this, cx| on_primary(this, window, cx));
                                }),
                        ),
                ),
                window,
            )
        });
    }

    fn confirm_close_tab(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path_discard = path.clone();
        self.open_choice_dialog(
            "Unsaved changes",
            format!(
                "Save changes to {} before closing?",
                document::file_name(&path)
            ),
            "Don't Save",
            "Save",
            move |this, window, cx| this.close_tab(&path_discard, window, cx),
            move |this, window, cx| match this.save_tab(&path, cx) {
                Ok(()) => this.close_tab(&path, window, cx),
                Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
            },
            window,
            cx,
        );
    }

    /// Returns `true` if the app may close now; otherwise asks the user first.
    fn confirm_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.allow_quit || !self.has_unsaved() {
            return true;
        }
        self.open_choice_dialog(
            "Quit Isengard?",
            "Some files have unsaved changes.",
            "Quit Without Saving",
            "Save All & Quit",
            |this, _, cx| this.quit(cx),
            |this, window, cx| match this.save_all(cx) {
                Ok(()) => this.quit(cx),
                Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
            },
            window,
            cx,
        );
        false
    }

    fn quit(&mut self, cx: &mut Context<Self>) {
        self.allow_quit = true;
        cx.quit();
    }

    // ---- Actions -----------------------------------------------------------

    fn on_open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(true, window, cx);
    }

    fn on_open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(false, window, cx);
    }

    fn prompt_and_open(&mut self, directories: bool, window: &mut Window, cx: &mut Context<Self>) {
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
            let path = paths.await.ok()?.ok()??.into_iter().next()?;
            this.update_in(window, |this, window, cx| {
                if directories {
                    this.open_folder(&path, window, cx);
                } else if Workspace::is_workspace_file(&path) {
                    this.open_workspace_file(&path, window, cx);
                } else {
                    this.open_file(&path, true, window, cx);
                }
            })
            .ok()
        })
        .detach();
    }

    fn on_open_workspace(
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
            let path = paths.await.ok()?.ok()??.into_iter().next()?;
            this.update_in(window, |this, window, cx| {
                this.open_workspace_file(&path, window, cx)
            })
            .ok()
        })
        .detach();
    }

    fn on_add_folder_to_workspace(
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
            let paths = paths.await.ok()?.ok()??;
            this.update_in(window, |this, window, cx| {
                this.add_folders(&paths, window, cx)
            })
            .ok()
        })
        .detach();
    }

    fn on_save_workspace_as(
        &mut self,
        _: &SaveWorkspaceAs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_empty() {
            notify_error(
                "Open a folder before saving a workspace.".into(),
                window,
                cx,
            );
            return;
        }
        self.save_workspace_as(None, window, cx);
    }

    fn on_remove_workspace_folder(
        &mut self,
        action: &RemoveWorkspaceFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.remove_folder(&action.0) {
            self.persist_workspace(window, cx);
            self.workspace_changed(window, cx);
        }
    }

    fn on_close_workspace(
        &mut self,
        _: &CloseWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.is_empty() || !self.tabs.is_empty() {
            self.request_switch(Switch::Close, window, cx);
        }
    }

    fn on_save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().map(|t| t.path.clone()) else {
            return;
        };
        match self.save_tab(&path, cx) {
            Ok(()) => window.push_notification(format!("Saved {}", document::file_name(&path)), cx),
            Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
        }
    }

    fn on_save_all(&mut self, _: &SaveAll, window: &mut Window, cx: &mut Context<Self>) {
        match self.save_all(cx) {
            Ok(()) => window.push_notification("Saved all files", cx),
            Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
        }
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.tabs.active().map(|t| t.path.clone()) {
            self.request_close(&path, window, cx);
        }
    }

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle(true);
        self.focus_active_editor(window, cx);
        cx.notify();
    }

    fn on_prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle(false);
        self.focus_active_editor(window, cx);
        cx.notify();
    }

    fn on_increase_font(
        &mut self,
        _: &IncreaseFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.change_font_size(1.0);
        self.apply_config(window, cx);
    }

    fn on_decrease_font(
        &mut self,
        _: &DecreaseFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.change_font_size(-1.0);
        self.apply_config(window, cx);
    }

    fn on_reset_font(&mut self, _: &ResetFontSize, window: &mut Window, cx: &mut Context<Self>) {
        self.config.font_size = DEFAULT_EDITOR_FONT_SIZE;
        self.apply_config(window, cx);
    }

    fn on_toggle_preview(
        &mut self,
        _: &TogglePreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        if !tab.supports_rendered_preview() {
            return;
        }
        let next = !tab.rendered_preview();
        self.set_rendered_preview(next, window, cx);
    }

    fn set_rendered_preview(
        &mut self,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.tabs.active_index() else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        let TabBody::Text {
            language,
            rendered_preview,
            ..
        } = &mut tab.body
        else {
            return;
        };
        if !language.supports_rendered_preview() || *rendered_preview == preview {
            return;
        }
        *rendered_preview = preview;
        if preview {
            self.focus_handle.focus(window, cx);
        } else {
            self.focus_active_editor(window, cx);
        }
        cx.notify();
    }

    fn apply_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        theme::apply(&self.config, Some(window), cx);
        self.config.save();
        cx.notify();
    }

    fn on_about(&mut self, _: &About, window: &mut Window, cx: &mut Context<Self>) {
        window.open_dialog(cx, |dialog, window, _| {
            center_dialog(
                dialog
                    .title("About Isengard")
                    .child(format!("Version {}", env!("CARGO_PKG_VERSION")))
                    .child("A code editor built with GPUI Kit."),
                window,
            )
        });
    }

    fn on_quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_quit(window, cx) {
            self.quit(cx);
        }
    }

    // ---- Rendering ---------------------------------------------------------

    /// The welcome screen replaces the workspace until a folder or file is open.
    fn show_welcome(&self) -> bool {
        self.workspace.is_empty() && self.tabs.is_empty()
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let title = self.window_title();
        let bar = if cfg!(target_os = "macos") {
            h_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(title),
                )
        } else {
            h_flex()
                .flex_1()
                .items_center()
                .child(h_flex().flex_1().child(self.app_menu_bar.clone()))
        };
        TitleBar::new().child(bar).into_any_element()
    }

    fn render_editor_area(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(active) = self.tabs.active() else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("Select a file to start editing")
                .into_any_element();
        };

        let tab_bar = TabBar::new("editor-tabs")
            .selected_index(self.tabs.active_index().unwrap_or_default())
            .children(self.tabs.iter().enumerate().map(|(ix, tab)| {
                let path = tab.path.clone();
                let id = SharedString::from(format!("close-tab-{}", tab.path.display()));
                let preview = tab.preview;
                let modified = tab.is_modified;
                let dirty_color = cx.theme().blue;
                Tab::new()
                    .label(tab.label())
                    .cursor_pointer()
                    .when(preview, |tab| tab.italic())
                    .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                        // Double-click the tab pins a preview (VS Code).
                        if ev.click_count() >= 2 {
                            this.pin_tab(ix, cx);
                        }
                        this.activate_tab(ix, window, cx);
                    }))
                    .suffix(
                        h_flex()
                            .items_center()
                            .gap_1()
                            // Same 12px from the tab's right border as the label's left padding.
                            .mr_2()
                            .when(modified, |this| {
                                this.child(
                                    // Block-style dirty mark (DESIGN.md: no rounded corners).
                                    div()
                                        .size(px(6.))
                                        .flex_shrink_0()
                                        .bg(dirty_color),
                                )
                            })
                            .child(
                                Button::new(id)
                                    .ghost()
                                    .cursor_pointer()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .tooltip("Close")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.request_close(&path, window, cx);
                                    })),
                            ),
                    )
            }));

        let show_preview_toolbar = active.supports_rendered_preview();
        let rendered_preview = active.rendered_preview();
        let preview_toolbar = show_preview_toolbar.then(|| {
            let preview = rendered_preview;
            h_flex()
                .w_full()
                .items_center()
                .justify_end()
                .gap_2()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    ButtonGroup::new("rendered-view-mode")
                        .outline()
                        .xsmall()
                        .child(
                            Button::new("rendered-edit")
                                .label("Edit")
                                .cursor_pointer()
                                .selected(!preview),
                        )
                        .child(
                            Button::new("rendered-preview")
                                .label("Preview")
                                .cursor_pointer()
                                .selected(preview),
                        )
                        .on_click(cx.listener(move |this, selected: &Vec<usize>, window, cx| {
                            let want_preview = selected.contains(&1);
                            this.set_rendered_preview(want_preview, window, cx);
                        })),
                )
        });

        let content = if active.is_image() {
            let path = active.path.clone();
            let id = SharedString::from(format!("image-preview-{}", path.display()));
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .bg(cx.theme().sidebar)
                .child(
                    img(path)
                        .id(id)
                        .object_fit(ObjectFit::Contain)
                        .max_w_full()
                        .max_h_full(),
                )
                .into_any_element()
        } else if rendered_preview {
            let language = active.language().expect("text tab");
            let text = active
                .editor_state()
                .expect("text tab")
                .read(cx)
                .value();
            match language {
                Language::Svg => {
                    let id = SharedString::from(format!("svg-preview-{}", active.path.display()));
                    let image = Arc::new(Image::from_bytes(
                        ImageFormat::Svg,
                        text.as_bytes().to_vec(),
                    ));
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_4()
                        .bg(cx.theme().sidebar)
                        .child(
                            img(image)
                                .id(id)
                                .object_fit(ObjectFit::Contain)
                                .max_w_full()
                                .max_h_full(),
                        )
                        .into_any_element()
                }
                _ => {
                    // Markdown (and any future TextView-based preview).
                    let id =
                        SharedString::from(format!("md-preview-{}", active.path.display()));
                    div()
                        .size_full()
                        .p_4()
                        .overflow_hidden()
                        .child(
                            TextView::markdown(id, text)
                                .scrollable(true)
                                .selectable(true),
                        )
                        .into_any_element()
                }
            }
        } else {
            let state = active.editor_state().expect("text tab");
            Editor::new(state)
                .bordered(false)
                .p_0()
                .h_full()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(cx.theme().mono_font_size)
                .into_any_element()
        };

        v_flex()
            .size_full()
            .child(tab_bar)
            .children(preview_toolbar)
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }

    /// The status bar describes the active file, so it only exists while one is open.
    fn render_status_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tab = self.tabs.active()?;
        let path = self.workspace.relative_label(&tab.path);
        let right = if let Some(state) = tab.editor_state() {
            let cursor = state.read(cx).cursor_position();
            h_flex()
                .gap_4()
                .child(format!(
                    "Ln {}, Col {}",
                    cursor.line + 1,
                    cursor.character + 1
                ))
                .child(tab.language().unwrap().display_name())
                .into_any_element()
        } else {
            div().child("Image").into_any_element()
        };
        let bar = StatusBar::new()
            .left(div().child(path))
            .right(right)
            .text_xs();
        Some(bar.into_any_element())
    }
}

impl Render for IsengardApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.show_welcome() {
            welcome::render(self, cx).into_any_element()
        } else if self.file_tree.is_open() {
            let app = cx.entity().downgrade();
            let tree = self.file_tree.render(
                move |path, permanent, window, cx| {
                    _ = app.update(cx, |this, cx| {
                        this.open_file(&path, permanent, window, cx)
                    });
                },
                cx,
            );
            h_resizable("main-split")
                .child(
                    resizable_panel()
                        .size(px(260.))
                        .child(div().size_full().min_w_0().overflow_hidden().child(tree)),
                )
                .child(resizable_panel().child(self.render_editor_area(cx)))
                .into_any_element()
        } else {
            self.render_editor_area(cx).into_any_element()
        };

        v_flex()
            .id("isengard")
            .key_context("Isengard")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(cx.theme().font_family.clone())
            .on_action(cx.listener(Self::on_open_folder))
            .on_action(cx.listener(Self::on_open_file))
            .on_action(cx.listener(Self::on_open_workspace))
            .on_action(cx.listener(Self::on_add_folder_to_workspace))
            .on_action(cx.listener(Self::on_save_workspace_as))
            .on_action(cx.listener(Self::on_remove_workspace_folder))
            .on_action(cx.listener(Self::on_close_workspace))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_all))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .on_action(cx.listener(Self::on_increase_font))
            .on_action(cx.listener(Self::on_decrease_font))
            .on_action(cx.listener(Self::on_reset_font))
            .on_action(cx.listener(Self::on_toggle_preview))
            .on_action(cx.listener(Self::on_about))
            .on_action(cx.listener(Self::on_quit))
            .child(self.render_title_bar(cx))
            .child(div().flex_1().min_h_0().child(body))
            .children(self.render_status_bar(cx))
    }
}

fn notify_error(message: String, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::error(message), cx);
}
