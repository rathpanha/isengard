use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Callback after a single tab save (path may change when materializing untitled).
type AfterSave = Rc<dyn Fn(&mut IsengardApp, PathBuf, &mut Window, &mut Context<IsengardApp>)>;
/// Callback after Save All / a save queue finishes.
type AfterSaveAll = Rc<dyn Fn(&mut IsengardApp, &mut Window, &mut Context<IsengardApp>)>;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Selectable as _, Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonGroup, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
    input::{
        Editor, EditorState, InputEvent, InputState, Position, RangeDecoration,
        RangeDecorationCollection, RangeDecorationStyle, RopeExt as _, TabSize,
    },
    list::ListItem,
    menu::{AppMenuBar, ContextMenuExt as _},
    resizable::{ResizableState, h_resizable, resizable_panel, v_resizable},
    status_bar::StatusBar,
    tab::Tab,
    text::TextView,
    tree::TreeEvent,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use serde::Deserialize;

use crate::config::{
    AppConfig, DEFAULT_EDITOR_FONT_SIZE, DEFAULT_SIDEBAR_WIDTH, DEFAULT_TERMINAL_HEIGHT,
    FileViewState, OpenFileEntry, WorkspaceSession,
};
use crate::editor::document;
use crate::editor::language::Language;
use crate::editor::tabs::TabList;
use crate::file_tree::tree_icon;
use crate::search::{
    SearchMatch, SearchPanel, SearchPanelEvent, SearchQuery, apply_replace, run_search,
};
use crate::terminal::TerminalPanel;
use crate::theme;
use crate::ui::components::{center_dialog, notify_error, notify_success};
use crate::ui::file_tree::{FileTreePanel, TreeEditTarget};
use crate::ui::welcome;
use crate::workspace::{WORKSPACE_EXTENSION, Workspace};

#[derive(Clone, Copy)]
struct WindowGeometrySnapshot {
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    maximized: bool,
    fullscreen: bool,
}

fn window_geometry_snapshot(window: &Window) -> WindowGeometrySnapshot {
    let wb = window.window_bounds();
    let bounds = wb.get_bounds();
    WindowGeometrySnapshot {
        width: bounds.size.width.into(),
        height: bounds.size.height.into(),
        x: bounds.origin.x.into(),
        y: bounds.origin.y.into(),
        maximized: matches!(wb, WindowBounds::Maximized(_)),
        fullscreen: matches!(wb, WindowBounds::Fullscreen(_)),
    }
}

actions!(
    isengard,
    [
        NewFile,
        NewWorkspace,
        OpenFolder,
        OpenFile,
        OpenWorkspace,
        AddFolderToWorkspace,
        SaveWorkspaceAs,
        CloseWorkspace,
        Save,
        SaveAll,
        CloseTab,
        CloseAllTabs,
        NextTab,
        PrevTab,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        TogglePreview,
        ToggleSidebar,
        /// Show the project Search sidebar (`⌘⇧F` / `Ctrl+Shift+F`).
        FindInFiles,
        /// Switch the sidebar to the file tree.
        ShowFileTree,
        ToggleTerminal,
        NewTerminal,
        /// Fired by the terminal panel after tabs change (close / need persist).
        TerminalTabsChanged,
        /// Rename the tree selection (F2 while Tree focused).
        RenameSelectedTreeItem,
        /// Move the tree selection to Trash (Delete/Backspace while Tree focused).
        DeleteSelectedTreeItem,
        About,
        Quit,
    ]
);

/// Active inline name edit in the file tree (Enter/Blur commits, Escape cancels).
struct TreeInlineEdit {
    target: crate::ui::file_tree::TreeEditTarget,
    input: Entity<InputState>,
    _events: Subscription,
    _keys: Subscription,
}

/// Removes one root folder from the workspace (tree context menu on a root).
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct RemoveWorkspaceFolder(pub PathBuf);

/// Renames a tree path (context menu).
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct RenamePath(pub PathBuf);

/// Moves a tree path to the OS Trash (context menu).
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct DeletePath(pub PathBuf);

/// Creates a new file inside this directory (tree context menu).
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct NewFileIn(pub PathBuf);

/// Creates a new folder inside this directory (tree context menu).
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct NewFolderIn(pub PathBuf);

/// Closes the tab for this path (tab context menu; may not be the active tab).
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct CloseTabAt(pub PathBuf);

/// Closes every tab except the one for this path.
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct CloseOtherTabs(pub PathBuf);

/// What to switch the window to once the user has confirmed leaving the
/// current workspace (VS Code closes all editors when switching).
#[derive(Clone)]
enum Switch {
    Folder(PathBuf),
    Workspace(Workspace),
    Close,
}

/// Which view fills the left sidebar (same panel width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarView {
    Files,
    Search,
}

/// Folder list for the multi-root “Open Terminal in…” dialog.
///
/// Kit dialogs keep a focus trap on their own handle, so `on_key_down` on the
/// list never fires. We intercept keystrokes for the life of this entity instead.
struct TerminalCwdPicker {
    folders: Vec<PathBuf>,
    selected: usize,
    hide_on_cancel: bool,
    app: WeakEntity<IsengardApp>,
    _keys: Subscription,
}

impl TerminalCwdPicker {
    fn new(
        folders: Vec<PathBuf>,
        hide_on_cancel: bool,
        app: WeakEntity<IsengardApp>,
        cx: &mut Context<Self>,
    ) -> Self {
        let entity = cx.weak_entity();
        // ponytail: dialog focus trap eats on_key_down; intercept until Kit
        // exposes the dialog focus handle to content.
        let _keys = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(entity) = entity.upgrade() else {
                return;
            };
            entity.update(cx, |this, cx| {
                match event.keystroke.key.as_str() {
                    "up" => {
                        if this.selected > 0 {
                            this.selected -= 1;
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }
                    "down" => {
                        if this.selected + 1 < this.folders.len() {
                            this.selected += 1;
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }
                    "enter" => {
                        this.confirm(window, cx);
                        cx.stop_propagation();
                    }
                    "escape" => {
                        this.cancel(window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            });
        });
        Self {
            folders,
            selected: 0,
            hide_on_cancel,
            app,
            _keys,
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = self.folders.get(self.selected).cloned() else {
            return;
        };
        window.close_dialog(cx);
        _ = self.app.update(cx, |this, cx| {
            this.add_terminal_in(folder, window, cx);
        });
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        if self.hide_on_cancel {
            _ = self.app.update(cx, |this, cx| {
                this.terminal.update(cx, |term, cx| {
                    term.clear_sessions(cx);
                    term.hide(cx);
                });
                this.persist_session(cx);
                cx.notify();
            });
        }
    }
}

impl Render for TerminalCwdPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected;
        v_flex()
            .id("terminal-cwd-picker")
            .w_full()
            .gap_1()
            .children(self.folders.iter().enumerate().map(|(ix, folder)| {
                let name = document::file_name(folder);
                ListItem::new(ix)
                    .w_full()
                    .selected(ix == selected)
                    .cursor_pointer()
                    .accessibility_label(name.clone())
                    .child(div().px_2().py_1().child(name))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.selected = ix;
                        this.confirm(window, cx);
                    }))
            }))
    }
}

/// Registers global key bindings. `secondary` is Cmd on macOS and Ctrl elsewhere.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-n", NewFile, None),
        KeyBinding::new("secondary-o", OpenFolder, None),
        KeyBinding::new("secondary-s", Save, None),
        KeyBinding::new("secondary-alt-s", SaveAll, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("secondary-=", IncreaseFontSize, None),
        KeyBinding::new("secondary--", DecreaseFontSize, None),
        KeyBinding::new("secondary-0", ResetFontSize, None),
        // Yield to terminal paste (`Ctrl+Shift+V` / `⌘⇧V` elsewhere) when focused.
        KeyBinding::new("secondary-shift-v", TogglePreview, Some("!Terminal")),
        KeyBinding::new("secondary-b", ToggleSidebar, None),
        KeyBinding::new("secondary-shift-f", FindInFiles, None),
        KeyBinding::new("ctrl-`", ToggleTerminal, None),
        KeyBinding::new("ctrl-shift-`", NewTerminal, None),
        // Tree selection only — do not steal Delete/F2 from the editor.
        KeyBinding::new("f2", RenameSelectedTreeItem, Some("Tree")),
        KeyBinding::new("delete", DeleteSelectedTreeItem, Some("Tree")),
        KeyBinding::new("backspace", DeleteSelectedTreeItem, Some("Tree")),
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
    /// Default folder when saving an in-memory tab (workspace New File).
    untitled_save_dir: Option<PathBuf>,
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
        document::tab_label(&self.path)
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
    search: Entity<SearchPanel>,
    sidebar_view: SidebarView,
    /// Bumps on each query change so stale background searches are ignored.
    search_generation: u64,
    /// Flipped to abort an in-flight `run_search` when a newer query arrives.
    search_cancel: Arc<AtomicBool>,
    /// Fill decorations for project-search hits in the active preview editor.
    /// Dropping the handle does not clear them — we `dispose` on replace.
    search_hit_decorations: Option<RangeDecorationCollection>,
    tabs: TabList<EditorTab>,
    app_menu_bar: Entity<AppMenuBar>,
    /// Bottom integrated terminal (lazy; created on first show).
    terminal: Entity<TerminalPanel>,
    /// Per-workspace layout (restored from [`WorkspaceSession`]).
    sidebar_width: f32,
    sidebar_visible: bool,
    terminal_height: f32,
    /// Set once the user chose to quit despite unsaved changes.
    allow_quit: bool,
    /// Suppresses session writes while batch-restoring tabs.
    restoring_session: bool,
    /// Debounce token for persisting window bounds while resizing.
    window_geometry_generation: u64,
    /// Inline rename / new-file / new-folder field over a tree row (not a modal).
    tree_edit: Option<TreeInlineEdit>,
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
                this.persist_session(cx);
            });

        let search = cx.new(|cx| SearchPanel::new(window, cx));
        let search_subscription =
            cx.subscribe_in(&search, window, |this, _, event: &SearchPanelEvent, window, cx| {
                match event {
                    SearchPanelEvent::QueryChanged(query) => {
                        this.schedule_search(query.clone(), cx);
                    }
                    SearchPanelEvent::OpenMatch(hit) => {
                        this.open_search_match(hit, window, cx);
                    }
                    SearchPanelEvent::ReplaceInFile(path) => {
                        this.replace_in_file(path.clone(), window, cx);
                    }
                    SearchPanelEvent::ReplaceAll => {
                        this.confirm_replace_all(window, cx);
                    }
                }
            });

        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| {
                this.persist_window_geometry(window);
                this.persist_session(cx);
                this.confirm_quit(window, cx)
            })
            .unwrap_or(true)
        });

        let bounds_sub = cx.observe_window_bounds(window, |this, window, cx| {
            this.schedule_persist_window_geometry(window, cx);
        });

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let terminal = cx.new(|_| TerminalPanel::new(&[]));

        let mut app = Self {
            config,
            focus_handle,
            workspace: Workspace::default(),
            file_tree,
            search,
            sidebar_view: SidebarView::Files,
            search_generation: 0,
            search_cancel: Arc::new(AtomicBool::new(false)),
            search_hit_decorations: None,
            tabs: TabList::default(),
            app_menu_bar,
            terminal,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_visible: true,
            terminal_height: DEFAULT_TERMINAL_HEIGHT,
            allow_quit: false,
            restoring_session: false,
            window_geometry_generation: 0,
            tree_edit: None,
            _subscriptions: vec![tree_subscription, search_subscription, bounds_sub],
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
        self.persist_session(cx);
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
            move |this, window, cx| {
                let switch = switch.clone();
                this.request_save_all(
                    move |this, window, cx| this.apply_switch(switch.clone(), window, cx),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    fn apply_switch(&mut self, switch: Switch, window: &mut Window, cx: &mut Context<Self>) {
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
        self.config.save();
        self.focus_handle.focus(window, cx);
        self.workspace_changed(window, cx);
        self.restart_terminal_for_workspace(window, cx);
        self.restore_session(window, cx);
    }

    fn restart_terminal_for_workspace(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Drop shells from the previous workspace; `restore_session` respawns
        // saved tabs for the new key (if any).
        self.terminal.update(cx, |term, cx| {
            term.clear_sessions(cx);
            term.hide(cx);
        });
    }

    /// Debounced write of window size/position into `config.json`.
    fn schedule_persist_window_geometry(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.window_geometry_generation = self.window_geometry_generation.wrapping_add(1);
        let geometry_gen = self.window_geometry_generation;
        // Capture geometry now — by the time the timer fires the Window borrow is gone.
        let snapshot = window_geometry_snapshot(window);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            this.update(cx, |this, _| {
                if this.window_geometry_generation != geometry_gen {
                    return;
                }
                if this.apply_window_geometry_snapshot(snapshot) {
                    this.config.save();
                }
            })
            .ok()
        })
        .detach();
    }

    fn persist_window_geometry(&mut self, window: &Window) {
        if self.apply_window_geometry_snapshot(window_geometry_snapshot(window)) {
            self.config.save();
        }
    }

    fn apply_window_geometry_snapshot(&mut self, snap: WindowGeometrySnapshot) -> bool {
        // Skip fullscreen: restore size is already in Maximized/Windowed variants.
        if snap.fullscreen {
            return false;
        }
        self.config.set_window_geometry(
            snap.width,
            snap.height,
            snap.x,
            snap.y,
            snap.maximized,
        )
    }

    /// Writes the current tabs + cursor/scroll + expanded dirs + terminals under this workspace's key.
    fn persist_session(&mut self, cx: &App) {
        if self.restoring_session {
            return;
        }
        let Some(key) = self.workspace.session_key() else {
            return;
        };
        let open_files = self
            .tabs
            .iter()
            .filter(|tab| !document::is_untitled(&tab.path))
            .map(|tab| {
                let view = tab
                    .editor_state()
                    .map(|state| {
                        let state = state.read(cx);
                        let cursor = state.cursor_position();
                        let scroll = state.scroll_offset();
                        FileViewState {
                            cursor_line: cursor.line,
                            cursor_character: cursor.character,
                            scroll_x: scroll.x.into(),
                            scroll_y: scroll.y.into(),
                        }
                    })
                    .unwrap_or_default();
                OpenFileEntry::with_view(tab.path.clone(), view)
            })
            .collect();
        let terminal = self.terminal.read(cx);
        let active = self
            .tabs
            .active()
            .filter(|t| !document::is_untitled(&t.path))
            .map(|t| t.path.clone());
        let session = WorkspaceSession {
            open_files,
            active,
            expanded: self.file_tree.expanded_paths(),
            terminal_cwds: terminal.session_cwds(),
            terminal_active: terminal.active_index(),
            terminal_visible: terminal.is_visible(),
            sidebar_width: self.sidebar_width,
            sidebar_visible: self.sidebar_visible,
            terminal_height: self.terminal_height,
        };
        self.config.put_session(key, session);
        self.config.save();
    }

    fn reset_layout_defaults(&mut self) {
        self.sidebar_width = DEFAULT_SIDEBAR_WIDTH;
        self.sidebar_visible = true;
        self.terminal_height = DEFAULT_TERMINAL_HEIGHT;
    }

    /// Reopens saved tabs (with cursor/scroll), expands dirs, and restores terminals.
    fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.workspace.session_key() else {
            self.reset_layout_defaults();
            return;
        };
        let Some(session) = self.config.session(&key).cloned() else {
            self.reset_layout_defaults();
            return;
        };

        self.restoring_session = true;
        self.sidebar_width = if session.sidebar_width >= 100. {
            session.sidebar_width
        } else {
            DEFAULT_SIDEBAR_WIDTH
        };
        self.sidebar_visible = session.sidebar_visible;
        self.terminal_height = if session.terminal_height >= 100. {
            session.terminal_height
        } else {
            DEFAULT_TERMINAL_HEIGHT
        };
        self.file_tree.expand_paths(&session.expanded, cx);
        for entry in &session.open_files {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            self.open_file(path, true, window, cx);
            self.apply_file_view(path, &entry.view(), window, cx);
        }
        if let Some(active) = &session.active
            && let Some(ix) = self.tabs.position(|t| t.path == *active)
        {
            self.activate_tab(ix, window, cx);
            if let Some(entry) = session.open_files.iter().find(|e| e.path() == active.as_path())
            {
                // Re-apply scroll after activate; cursor move-to can shift the viewport.
                self.apply_file_view(active, &entry.view(), window, cx);
            }
        }
        self.terminal.update(cx, |term, cx| {
            term.restore_sessions(
                &session.terminal_cwds,
                session.terminal_active,
                session.terminal_visible,
                window,
                cx,
            );
        });
        self.restoring_session = false;
        self.persist_session(cx);
    }

    fn apply_file_view(
        &self,
        path: &Path,
        view: &FileViewState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self
            .tabs
            .iter()
            .find(|t| t.path == path)
            .and_then(|t| t.editor_state())
            .cloned()
        else {
            return;
        };
        state.update(cx, |state, cx| {
            // gpui prelude's `Position` is CSS; the editor uses LSP Position from Kit.
            state.set_cursor_position(
                gpui_kit::component::input::Position {
                    line: view.cursor_line,
                    character: view.cursor_character,
                },
                window,
                cx,
            );
            state.set_scroll_offset(point(px(view.scroll_x), px(view.scroll_y)), cx);
        });
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
    ///
    /// Preview opens keep keyboard focus on the file tree so Delete / F2 still
    /// work; permanent opens focus the editor.
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
            if self.tabs.activate(ix) {
                self.file_tree.select_path(path, cx);
                self.persist_session(cx);
                cx.notify();
            }
            self.focus_after_tree_open(permanent, window, cx);
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
                untitled_save_dir: None,
            });
            self.file_tree.select_path(path, cx);
            self.focus_after_tree_open(permanent, window, cx);
            self.persist_session(cx);
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
            untitled_save_dir: None,
        });
        self.file_tree.select_path(path, cx);
        self.focus_after_tree_open(permanent, window, cx);
        self.persist_session(cx);
        cx.notify();
    }

    fn focus_file_tree(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_tree
            .state()
            .update(cx, |state, cx| state.focus(window, cx));
    }

    fn focus_after_tree_open(
        &self,
        permanent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if permanent {
            self.focus_active_editor(window, cx);
        } else {
            self.focus_file_tree(window, cx);
        }
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
            self.persist_session(cx);
            cx.notify();
        }
    }

    fn focus_active_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.tabs.active().and_then(|t| t.editor_state()) {
            Some(state) => state.update(cx, |state, cx| state.focus(window, cx)),
            None => self.focus_handle.focus(window, cx),
        }
    }

    /// Writes an on-disk tab. Untitled buffers must go through [`Self::request_save`].
    fn save_tab(&mut self, path: &Path, cx: &mut Context<Self>) -> anyhow::Result<()> {
        if document::is_untitled(path) {
            anyhow::bail!("untitled buffer needs a path");
        }
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

    /// Saves `path`, prompting for a location when the buffer is untitled.
    /// `after` runs with the final on-disk path on success.
    fn request_save(
        &mut self,
        path: PathBuf,
        after: impl Fn(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if document::is_untitled(&path) {
            self.prompt_save_untitled(path, Rc::new(after), window, cx);
            return;
        }
        match self.save_tab(&path, cx) {
            Ok(()) => after(self, path, window, cx),
            Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
        }
    }

    fn prompt_save_untitled(
        &mut self,
        old_path: PathBuf,
        after: AfterSave,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = self.tabs.iter().find(|t| t.path == old_path);
        let dir = tab
            .and_then(|t| t.untitled_save_dir.clone())
            .or_else(|| self.new_file_directory(cx))
            .or_else(|| self.workspace.folders().first().cloned())
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let suggested = if tab.is_some_and(|t| t.untitled_save_dir.is_some()) {
            document::unique_new_file_path(&dir)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("{}.txt", document::tab_label(&old_path)))
        } else {
            format!("{}.txt", document::tab_label(&old_path))
        };
        let prompt = cx.prompt_for_new_path(&dir, Some(&suggested));
        cx.spawn_in(window, async move |this, window| {
            let new_path = prompt.await.ok()?.ok()??;
            this.update_in(window, |this, window, cx| {
                match this.materialize_untitled(&old_path, new_path.clone(), window, cx) {
                    Ok(()) => after(this, new_path, window, cx),
                    Err(err) => notify_error(format!("Save failed: {err:#}"), window, cx),
                }
            })
            .ok()
        })
        .detach();
    }

    /// Writes an untitled buffer to `new_path` and rebinds the tab.
    fn materialize_untitled(
        &mut self,
        old_path: &Path,
        new_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let text = {
            let Some(tab) = self.tabs.iter().find(|t| t.path == *old_path) else {
                anyhow::bail!("tab was closed");
            };
            let Some(state) = tab.editor_state() else {
                anyhow::bail!("not a text tab");
            };
            state.read(cx).value()
        };
        document::write_text(&new_path, &text)?;

        if let Some(existing) = self.tabs.position(|t| t.path == new_path) {
            if let Some(old_ix) = self.tabs.position(|t| t.path == *old_path) {
                self.tabs.remove(old_ix);
            }
            let ix = self
                .tabs
                .position(|t| t.path == new_path)
                .unwrap_or(existing);
            self.activate_tab(ix, window, cx);
        } else if let Some(tab) = self.tabs.find_mut(|t| t.path == *old_path) {
            tab.path = new_path.clone();
            tab.is_modified = false;
            tab.untitled_save_dir = None;
            if let TabBody::Text {
                language, state, ..
            } = &mut tab.body
            {
                let lang = Language::from_path(&new_path);
                *language = lang;
                let name = lang.highlighter_name();
                state.update(cx, |state, cx| {
                    state.set_highlighter(name, cx);
                });
            }
        }

        if self.workspace.is_empty()
            && let Some(parent) = new_path.parent().filter(|p| p.is_dir())
        {
            self.add_folders(&[parent.to_path_buf()], window, cx);
        } else if let Some(parent) = new_path.parent() {
            self.file_tree.refresh_dir(parent, cx);
        }
        self.file_tree.select_path(&new_path, cx);
        self.persist_session(cx);
        cx.notify();
        Ok(())
    }

    /// Saves every dirty tab (prompts for each untitled), then runs `after`.
    fn request_save_all(
        &mut self,
        after: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dirty: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| t.is_modified)
            .map(|t| t.path.clone())
            .collect();
        self.request_save_queue(dirty, Rc::new(after), window, cx);
    }

    fn request_save_queue(
        &mut self,
        mut paths: Vec<PathBuf>,
        after: AfterSaveAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        while let Some(path) = paths.first().cloned() {
            let still_dirty = self
                .tabs
                .iter()
                .any(|t| t.path == path && t.is_modified);
            if !still_dirty {
                paths.remove(0);
                continue;
            }
            if document::is_untitled(&path) {
                let rest = paths[1..].to_vec();
                let after = after.clone();
                self.prompt_save_untitled(
                    path,
                    Rc::new(move |this, _, window, cx| {
                        this.request_save_queue(rest.clone(), after.clone(), window, cx);
                    }),
                    window,
                    cx,
                );
                return;
            }
            if let Err(err) = self.save_tab(&path, cx) {
                return notify_error(format!("Save failed: {err:#}"), window, cx);
            }
            paths.remove(0);
        }
        after(self, window, cx);
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

    /// Closes many tabs at once; one confirmation if any are dirty.
    fn request_close_many(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let dirty = paths.iter().any(|path| {
            self.tabs
                .iter()
                .any(|t| t.path == *path && t.is_modified)
        });
        if !dirty {
            self.close_tabs(&paths, window, cx);
            return;
        }
        let paths_discard = paths.clone();
        self.open_choice_dialog(
            "Unsaved changes",
            "Some files have unsaved changes. Save them before closing?",
            "Don't Save",
            "Save All",
            move |this, window, cx| this.close_tabs(&paths_discard, window, cx),
            move |this, window, cx| {
                let paths = paths.clone();
                this.request_save_all(
                    move |this, window, cx| this.close_tabs(&paths, window, cx),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tabs(&[path.to_path_buf()], window, cx);
    }

    fn close_tabs(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        // Remove from the end so earlier indexes stay valid while we scan.
        let mut indexes: Vec<usize> = paths
            .iter()
            .filter_map(|path| self.tabs.position(|t| t.path == *path))
            .collect();
        indexes.sort_unstable();
        indexes.dedup();
        for ix in indexes.into_iter().rev() {
            self.tabs.remove(ix);
        }
        self.focus_active_editor(window, cx);
        self.persist_session(cx);
        cx.notify();
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
                            // Secondary is always the discard path today
                            // ("Don't Save", "Quit Without Saving").
                            Button::new("dialog-secondary")
                                .cursor_pointer()
                                .danger()
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
                document::tab_label(&path)
            ),
            "Don't Save",
            "Save",
            move |this, window, cx| this.close_tab(&path_discard, window, cx),
            move |this, window, cx| {
                this.request_save(
                    path.clone(),
                    |this, saved, window, cx| this.close_tab(&saved, window, cx),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    fn begin_inline_rename(
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

    fn begin_inline_new_file(
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

    fn begin_inline_new_folder(
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

    fn begin_tree_edit(
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
        let events = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    this.commit_tree_edit(window, cx);
                }
                _ => {}
            }
        });
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

    fn commit_tree_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn cancel_tree_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tree_edit.take().is_none() {
            return;
        }
        self.file_tree.set_creating(None, cx);
        self.focus_file_tree(window, cx);
        cx.notify();
    }

    fn path_in_workspace(&self, path: &Path) -> bool {
        self.workspace
            .folders()
            .iter()
            .any(|root| path == root.as_path() || path.starts_with(root))
    }

    fn create_file_in(
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

    fn create_folder_in(
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

    fn rename_path(
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
        self.remap_open_paths(path, &new_path, cx);
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

    fn delete_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
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
    fn remap_open_paths(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        let indexes: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.path == from || t.path.starts_with(from))
            .map(|(ix, _)| ix)
            .collect();
        for ix in indexes {
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
            tab.path = new_path.clone();
            if let TabBody::Text {
                language, state, ..
            } = &mut tab.body
            {
                let lang = Language::from_path(&new_path);
                *language = lang;
                let name = lang.highlighter_name();
                state.update(cx, |state, cx| {
                    state.set_highlighter(name, cx);
                });
            }
        }
    }

    /// Returns `true` if the app may close now; otherwise asks the user first.
    fn confirm_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.allow_quit || !self.has_unsaved() {
            self.persist_session(cx);
            return true;
        }
        self.open_choice_dialog(
            "Quit Isengard?",
            "Some files have unsaved changes.",
            "Quit Without Saving",
            "Save All & Quit",
            |this, _, cx| this.quit(cx),
            |this, window, cx| {
                this.request_save_all(|this, _, cx| this.quit(cx), window, cx);
            },
            window,
            cx,
        );
        false
    }

    fn quit(&mut self, cx: &mut Context<Self>) {
        self.persist_session(cx);
        self.allow_quit = true;
        cx.quit();
    }

    // ---- Actions -----------------------------------------------------------

    fn on_new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        let save_dir = if self.workspace.is_empty() {
            None
        } else {
            match self.new_file_directory(cx) {
                Some(dir) => Some(dir),
                None => {
                    return notify_error(
                        "No folder available for a new file.",
                        window,
                        cx,
                    );
                }
            }
        };
        self.open_untitled(save_dir, window, cx);
    }

    /// In-memory buffer until Save; `save_dir` seeds the save dialog in a workspace.
    fn open_untitled(
        &mut self,
        save_dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut id = 1u64;
        let path = loop {
            let candidate = document::untitled_path(id);
            if !self.tabs.iter().any(|t| t.path == candidate) {
                break candidate;
            }
            id += 1;
        };
        let language = Language::from_path(Path::new("untitled.txt"));
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
                .default_value("")
        });
        let subscriptions = vec![
            cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let entity_id = state.entity_id();
                    if let Some(tab) = this.tabs.find_mut(|t| {
                        t.editor_state()
                            .is_some_and(|s| s.entity_id() == entity_id)
                    }) {
                        tab.is_modified = true;
                        tab.preview = false;
                    }
                    cx.notify();
                }
            }),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];
        self.tabs.push(EditorTab {
            path,
            body: TabBody::Text {
                language,
                state,
                rendered_preview: false,
                _subscriptions: subscriptions,
            },
            is_modified: false,
            preview: false,
            untitled_save_dir: save_dir,
        });
        self.focus_active_editor(window, cx);
        cx.notify();
    }

    /// Directory for a new file on Save: tree selection (or its parent), else the
    /// active root (multi-root) / sole root.
    fn new_file_directory(&self, cx: &App) -> Option<PathBuf> {
        let folders = self.workspace.folders();
        if folders.is_empty() {
            return None;
        }
        if let Some(selected) = self.file_tree.selected_path(cx) {
            let dir = if selected.is_dir() {
                selected
            } else {
                selected
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(Path::to_path_buf)
                    .unwrap_or(selected)
            };
            if folders.iter().any(|root| dir.starts_with(root)) {
                return Some(dir);
            }
        }
        if let Some(path) = self.tabs.active().map(|t| &t.path)
            && !document::is_untitled(path)
            && let Some(parent) = path.parent()
            && folders.iter().any(|root| parent.starts_with(root))
        {
            return Some(parent.to_path_buf());
        }
        self.active_root(cx)
    }

    /// Root that contains the tree selection or active file; otherwise the first root.
    fn active_root(&self, cx: &App) -> Option<PathBuf> {
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

    fn on_new_workspace(
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
            let folders = paths.await.ok()?.ok()??;
            let folders: Vec<PathBuf> = folders.into_iter().filter(|f| f.is_dir()).collect();
            if folders.is_empty() {
                return None;
            }
            this.update_in(window, |this, window, cx| {
                this.prompt_save_new_workspace(folders, window, cx);
            })
            .ok()
        })
        .detach();
    }

    /// Asks where to write the `.isengard-workspace` file, then opens it.
    fn prompt_save_new_workspace(
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
            let path = path.await.ok()?.ok()??;
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
            .ok()
        })
        .detach();
    }

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
                "Open a folder before saving a workspace.",
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
            self.persist_session(cx);
        }
    }

    fn on_rename_path(
        &mut self,
        action: &RenamePath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_inline_rename(action.0.clone(), window, cx);
    }

    fn on_delete_path(
        &mut self,
        action: &DeletePath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_path(&action.0, window, cx);
    }

    fn on_new_file_in(
        &mut self,
        action: &NewFileIn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_inline_new_file(action.0.clone(), window, cx);
    }

    fn on_new_folder_in(
        &mut self,
        action: &NewFolderIn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_inline_new_folder(action.0.clone(), window, cx);
    }

    fn on_rename_selected_tree_item(
        &mut self,
        _: &RenameSelectedTreeItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.file_tree.selected_path(cx) {
            self.begin_inline_rename(path, window, cx);
        }
    }

    fn on_delete_selected_tree_item(
        &mut self,
        _: &DeleteSelectedTreeItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.file_tree.selected_path(cx) {
            self.delete_path(&path, window, cx);
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
        self.request_save(path, |_, _, _, _| {}, window, cx);
    }

    fn on_save_all(&mut self, _: &SaveAll, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_unsaved() {
            return;
        }
        self.request_save_all(|_, _, _| {}, window, cx);
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.tabs.active().map(|t| t.path.clone()) {
            self.request_close(&path, window, cx);
        }
    }

    fn on_close_tab_at(
        &mut self,
        action: &CloseTabAt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_close(&action.0, window, cx);
    }

    fn on_close_other_tabs(
        &mut self,
        action: &CloseOtherTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keep = &action.0;
        // Activate the right-clicked tab so it stays selected after others close.
        if let Some(ix) = self.tabs.position(|t| t.path == *keep) {
            self.tabs.activate(ix);
        }
        let paths: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| t.path != *keep)
            .map(|t| t.path.clone())
            .collect();
        self.request_close_many(paths, window, cx);
    }

    fn on_close_all_tabs(
        &mut self,
        _: &CloseAllTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<PathBuf> = self.tabs.iter().map(|t| t.path.clone()).collect();
        self.request_close_many(paths, window, cx);
    }

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle(true);
        self.focus_active_editor(window, cx);
        self.persist_session(cx);
        cx.notify();
    }

    fn on_prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle(false);
        self.focus_active_editor(window, cx);
        self.persist_session(cx);
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

    fn on_toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.file_tree.is_open() {
            return;
        }
        self.sidebar_visible = !self.sidebar_visible;
        self.persist_session(cx);
        cx.notify();
    }

    fn on_find_in_files(
        &mut self,
        _: &FindInFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_empty() {
            return;
        }
        self.sidebar_view = SidebarView::Search;
        self.sidebar_visible = true;
        self.persist_session(cx);
        self.search.update(cx, |panel, cx| panel.focus_find(window, cx));
        cx.notify();
    }

    fn on_show_file_tree(
        &mut self,
        _: &ShowFileTree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_empty() {
            return;
        }
        self.sidebar_view = SidebarView::Files;
        self.sidebar_visible = true;
        self.persist_session(cx);
        self.focus_file_tree(window, cx);
        cx.notify();
    }

    fn open_buffer_texts(&self, cx: &App) -> HashMap<PathBuf, String> {
        let mut map = HashMap::new();
        for tab in self.tabs.iter() {
            if document::is_untitled(&tab.path) {
                continue;
            }
            if let Some(state) = tab.editor_state() {
                map.insert(tab.path.clone(), state.read(cx).value().to_string());
            }
        }
        map
    }

    fn schedule_search(&mut self, query: SearchQuery, cx: &mut Context<Self>) {
        // Abort any walk still chewing through the previous pattern.
        self.search_cancel.store(true, Ordering::Relaxed);
        self.search_generation = self.search_generation.wrapping_add(1);
        let search_gen = self.search_generation;
        if query.pattern.is_empty() {
            self.search
                .update(cx, |panel, cx| panel.clear_results(cx));
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = cancel.clone();
        self.search
            .update(cx, |panel, cx| panel.set_searching(true, cx));
        let roots = self.workspace.folders().to_vec();
        let buffers = self.open_buffer_texts(cx);
        cx.spawn(async move |this, cx| {
            // Debounce typing; generation check drops superseded timers.
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let still = this
                .update(cx, |this, _| this.search_generation == search_gen)
                .ok()?;
            if !still {
                return None;
            }
            let outcome = cx
                .background_spawn(async move {
                    run_search(&roots, &query, &buffers, &cancel)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.search_generation != search_gen {
                    return;
                }
                let labels: HashMap<PathBuf, String> = outcome
                    .groups
                    .iter()
                    .map(|g| {
                        (
                            g.path.clone(),
                            this.workspace.relative_label(&g.path),
                        )
                    })
                    .collect();
                this.search
                    .update(cx, |panel, cx| panel.set_outcome(outcome, labels, cx));
            })
            .ok()
        })
        .detach();
    }

    fn open_search_match(
        &mut self,
        hit: &SearchMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file(&hit.path, false, window, cx);
        let siblings: Vec<SearchMatch> = self
            .search
            .read(cx)
            .groups()
            .iter()
            .find(|g| g.path == hit.path)
            .map(|g| g.matches.clone())
            .unwrap_or_else(|| vec![hit.clone()]);
        let Some(state) = self
            .tabs
            .iter()
            .find(|t| t.path == hit.path)
            .and_then(|t| t.editor_state())
            .cloned()
        else {
            return;
        };
        if let Some(prev) = self.search_hit_decorations.take() {
            prev.dispose(cx);
        }
        let fill = cx.theme().warning.opacity(0.35);
        let hit_line = hit.line as u32;
        let hit_col = hit.col as u32;
        let hit_end = (hit.col + hit.match_len) as u32;
        let collection = state.update(cx, |state, cx| {
            let decorations: Vec<RangeDecoration> = siblings
                .iter()
                .map(|m| {
                    let start = state.text().position_to_offset(&Position {
                        line: m.line as u32,
                        character: m.col as u32,
                    });
                    let end = state.text().position_to_offset(&Position {
                        line: m.line as u32,
                        character: (m.col + m.match_len) as u32,
                    });
                    RangeDecoration::new(start..end)
                        .with_style(RangeDecorationStyle::Fill)
                        .with_color(fill)
                })
                .collect();
            let collection = state.create_range_decorations_collection(decorations, cx);
            let start = state.text().position_to_offset(&Position {
                line: hit_line,
                character: hit_col,
            });
            let end = state.text().position_to_offset(&Position {
                line: hit_line,
                character: hit_end,
            });
            // Selects the clicked hit and scrolls it into view (`move_to` inside).
            state.set_selected_range(start..end, cx);
            state.focus(window, cx);
            collection
        });
        self.search_hit_decorations = Some(collection);
    }

    fn replace_in_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = self.search.read(cx).query(cx);
        if query.pattern.is_empty() {
            return;
        }
        if let Err(err) = self.apply_replace_to_path(&path, &query, window, cx) {
            return notify_error(format!("Replace failed: {err:#}"), window, cx);
        }
        self.rescan_search(cx);
    }

    fn confirm_replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.search.read(cx);
        let files = panel.file_count();
        let matches = panel.match_count();
        if files == 0 || matches == 0 {
            return;
        }
        self.open_choice_dialog(
            "Replace All?",
            format!("Replace {matches} occurrences across {files} files?"),
            "Don't Replace",
            "Replace All",
            |_, _, _| {},
            |this, window, cx| this.replace_all(window, cx),
            window,
            cx,
        );
    }

    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.read(cx).query(cx);
        let paths: Vec<PathBuf> = self
            .search
            .read(cx)
            .groups()
            .iter()
            .map(|g| g.path.clone())
            .collect();
        for path in paths {
            if let Err(err) = self.apply_replace_to_path(&path, &query, window, cx) {
                return notify_error(format!("Replace failed: {err:#}"), window, cx);
            }
        }
        self.rescan_search(cx);
    }

    fn apply_replace_to_path(
        &mut self,
        path: &Path,
        query: &SearchQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let open_state = self
            .tabs
            .iter()
            .find(|t| t.path == *path)
            .and_then(|t| t.editor_state().cloned());
        if let Some(state) = open_state {
            let text = state.read(cx).value().to_string();
            let (new_text, count) =
                apply_replace(&text, query).map_err(anyhow::Error::msg)?;
            if count == 0 {
                return Ok(());
            }
            state.update(cx, |state, cx| {
                state.set_value(new_text, window, cx);
            });
            if let Some(tab) = self.tabs.find_mut(|t| t.path == *path) {
                tab.is_modified = true;
                tab.preview = false;
            }
            cx.notify();
            return Ok(());
        }
        let text = document::read_text(path)?;
        let (new_text, count) = apply_replace(&text, query).map_err(anyhow::Error::msg)?;
        if count > 0 {
            document::write_text(path, &new_text)?;
        }
        Ok(())
    }

    fn rescan_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).query(cx);
        self.schedule_search(query, cx);
    }

    /// Explorer / Search toggle strip above the active sidebar view.
    fn render_sidebar_chrome(&self, cx: &mut Context<Self>) -> AnyElement {
        let files = self.sidebar_view == SidebarView::Files;
        h_flex()
            .id("sidebar-view-toggle")
            .w_full()
            .items_center()
            .justify_start()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                Button::new("sidebar-files")
                    .ghost()
                    .cursor_pointer()
                    .small()
                    .icon(LucideIcon::FolderTree)
                    .tooltip("Explorer")
                    .selected(files)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.on_show_file_tree(&ShowFileTree, window, cx);
                    })),
            )
            .child(
                Button::new("sidebar-search")
                    .ghost()
                    .cursor_pointer()
                    .small()
                    .icon(LucideIcon::Search)
                    .tooltip("Search")
                    .selected(!files)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.on_find_in_files(&FindInFiles, window, cx);
                    })),
            )
            .into_any_element()
    }

    fn on_toggle_terminal(
        &mut self,
        _: &ToggleTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal.read(cx).is_visible() {
            self.terminal.update(cx, |term, cx| term.hide(cx));
            self.persist_session(cx);
            self.focus_active_editor(window, cx);
            cx.notify();
            return;
        }
        self.open_terminal(window, cx);
    }

    fn on_new_terminal(
        &mut self,
        _: &NewTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_terminal(window, cx);
    }

    fn on_terminal_tabs_changed(
        &mut self,
        _: &TerminalTabsChanged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.persist_session(cx);
        if !self.terminal.read(cx).has_sessions() {
            self.focus_active_editor(window, cx);
        }
        cx.notify();
    }

    /// Show the panel: restore existing sessions without asking, or create the first.
    /// No-op on the welcome screen (needs at least one workspace folder).
    fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.folders().is_empty() {
            return;
        }
        if self.terminal.read(cx).has_sessions() {
            self.terminal
                .update(cx, |term, cx| term.show(window, cx));
            self.persist_session(cx);
            cx.notify();
            return;
        }
        self.create_terminal(window, cx);
    }

    /// Always spawn a new terminal tab (asks for root when multi-root).
    fn create_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folders = self.workspace.folders().to_vec();
        if folders.is_empty() {
            return;
        }
        if folders.len() > 1 {
            self.pick_terminal_cwd(folders, false, window, cx);
            return;
        }
        let cwd = crate::terminal::pty::default_cwd(&folders);
        self.add_terminal_in(cwd, window, cx);
    }

    fn add_terminal_in(
        &mut self,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal
            .update(cx, |term, cx| term.add_session(cwd, window, cx));
        self.persist_session(cx);
        cx.notify();
    }

    /// Dialog listing workspace roots. `hide_on_cancel` is for workspace switches
    /// where the old session cwd is already invalid.
    fn pick_terminal_cwd(
        &self,
        folders: Vec<PathBuf>,
        hide_on_cancel: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity().downgrade();
        let muted = cx.theme().muted_foreground;
        // Create once — the dialog builder may run every paint.
        let picker = cx.new(|cx| {
            TerminalCwdPicker::new(folders, hide_on_cancel, app, cx)
        });
        let picker_for_dialog = picker.clone();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let picker_cancel = picker_for_dialog.clone();
            let picker_open = picker_for_dialog.clone();
            center_dialog(
                dialog
                    .title("Open Terminal in…")
                    .child(
                        v_flex()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(
                                        "Choose a workspace folder for the shell. ↑↓ to move, Enter to open.",
                                    ),
                            )
                            .child(picker_for_dialog.clone()),
                    )
                    .footer(
                        DialogFooter::new()
                            .gap_2()
                            .child(
                                Button::new("terminal-cwd-cancel")
                                    .cursor_pointer()
                                    .outline()
                                    .label("Cancel")
                                    .on_click(move |_, window, cx| {
                                        picker_cancel.update(cx, |picker, cx| {
                                            picker.cancel(window, cx);
                                        });
                                    }),
                            )
                            .child(
                                Button::new("terminal-cwd-open")
                                    .cursor_pointer()
                                    .primary()
                                    .label("Open")
                                    .on_click(move |_, window, cx| {
                                        picker_open.update(cx, |picker, cx| {
                                            picker.confirm(window, cx);
                                        });
                                    }),
                            ),
                    ),
                window,
            )
        });
    }

    fn remember_sidebar_width(&mut self, state: &Entity<ResizableState>, cx: &App) {
        let sizes = state.read(cx).sizes();
        if let Some(&w) = sizes.first() {
            let w = f32::from(w).clamp(100., 800.);
            if (self.sidebar_width - w).abs() > 0.5 {
                self.sidebar_width = w;
                self.persist_session(cx);
            }
        }
    }

    fn remember_terminal_height(&mut self, state: &Entity<ResizableState>, cx: &App) {
        let sizes = state.read(cx).sizes();
        if let Some(&h) = sizes.get(1) {
            let h = f32::from(h).clamp(100., 800.);
            if (self.terminal_height - h).abs() > 0.5 {
                self.terminal_height = h;
                self.persist_session(cx);
            }
        }
    }

    fn on_about(&mut self, _: &About, window: &mut Window, cx: &mut Context<Self>) {
        window.open_dialog(cx, |dialog, window, _| {
            center_dialog(
                dialog
                    .title("About Isengard")
                    .child(format!("Version {}", env!("CARGO_PKG_VERSION")))
                    .child("The already configured editor.")
                    .child("Dark mode, one font, no preference archaeology."),
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
        TitleBar::new()
            // Linux CSD close calls `remove_window()` unless we handle it here
            // (`on_window_should_close` is not invoked for that button).
            .on_close_window(cx.listener(|this, _, window, cx| {
                if this.confirm_quit(window, cx) {
                    this.quit(cx);
                }
            }))
            .child(bar)
            .into_any_element()
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

        let active_ix = self.tabs.active_index().unwrap_or_default();
        let tab_count = self.tabs.iter().count();
        // TabBar::children requires bare `Tab` (Into<Tab>), so ContextMenu cannot
        // wrap a tab there — compose Kit `Tab`s in divs for the right-click menu.
        // (Tab itself is not an Element, so ContextMenu cannot wrap it either.)
        let tab_bar = h_flex()
            .id("editor-tabs")
            .w_full()
            .flex_shrink_0()
            .overflow_x_scroll()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tab_bar)
            .children(self.tabs.iter().enumerate().map(|(ix, tab)| {
                let path = tab.path.clone();
                let path_menu = path.clone();
                let id = SharedString::from(format!("close-tab-{}", tab.path.display()));
                let tab_id = SharedString::from(format!("tab-{}", tab.path.display()));
                let preview = tab.preview;
                let modified = tab.is_modified;
                let dirty_color = cx.theme().blue;
                let close_others_disabled = tab_count <= 1;
                let selected = ix == active_ix;
                div()
                    .id(ElementId::Name(tab_id))
                    .context_menu(move |menu, _, _| {
                        menu.menu("Close", Box::new(CloseTabAt(path_menu.clone())))
                            .menu_with_disabled(
                                "Close Others",
                                Box::new(CloseOtherTabs(path_menu.clone())),
                                close_others_disabled,
                            )
                            .menu("Close All", Box::new(CloseAllTabs))
                    })
                    .child(
                        Tab::new()
                            // Icon+label as children (not Tab::prefix): Kit's
                            // inner left pad insets the icon; prefix would sit
                            // outside that pad and leave a 12px gap before the name.
                            .aria_label(tab.label())
                            .child(
                                // Same icon↔label gap as file-tree rows.
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(tree_icon(
                                        &path,
                                        false,
                                        false,
                                        false,
                                        cx.theme().is_dark(),
                                    ))
                                    .child(tab.label()),
                            )
                            .selected(selected)
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
                                            // Block-style dirty mark (docs/design.md: no rounded corners).
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

    fn render_status_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let term_visible = self.terminal.read(cx).is_visible();
        let has_folders = self.file_tree.is_open();
        let sidebar_visible = self.sidebar_visible;

        let left = h_flex()
            .gap_2()
            .items_center()
            .when(has_folders, |this| {
                this.child(
                    Button::new("toggle-sidebar")
                        .ghost()
                        .cursor_pointer()
                        .small()
                        .icon(if sidebar_visible {
                            IconName::PanelLeftClose
                        } else {
                            IconName::PanelLeftOpen
                        })
                        .tooltip(if sidebar_visible {
                            "Hide Sidebar"
                        } else {
                            "Show Sidebar"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.on_toggle_sidebar(&ToggleSidebar, window, cx)
                        })),
                )
            })
            .when_some(self.tabs.active(), |this, tab| {
                this.child(if document::is_untitled(&tab.path) {
                    tab.label()
                } else {
                    self.workspace.relative_label(&tab.path)
                })
            });

        let right = h_flex()
            .gap_4()
            .items_center()
            .when_some(self.tabs.active(), |this, tab| {
                if let Some(state) = tab.editor_state() {
                    let cursor = state.read(cx).cursor_position();
                    this.child(format!(
                        "Ln {}, Col {}",
                        cursor.line + 1,
                        cursor.character + 1
                    ))
                    .child(tab.language().unwrap().display_name())
                } else {
                    this.child("Image")
                }
            })
            .when(has_folders, |this| {
                this.child(
                    Button::new("toggle-terminal")
                        .ghost()
                        .cursor_pointer()
                        .small()
                        .icon(IconName::SquareTerminal)
                        .tooltip(if term_visible {
                            "Minimize Terminal"
                        } else {
                            "Show Terminal"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.on_toggle_terminal(&ToggleTerminal, window, cx)
                        })),
                )
            });

        StatusBar::new()
            .left(left)
            .right(right)
            .text_xs()
            .into_any_element()
    }

    /// Editor column, optionally stacked with the terminal under it (not under the tree).
    fn render_editor_with_terminal(&self, cx: &mut Context<Self>) -> AnyElement {
        let editor = self.render_editor_area(cx);
        self.stack_with_terminal(editor, cx)
    }

    fn stack_with_terminal(&self, top: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        if !self.terminal.read(cx).is_visible() {
            return top;
        }
        let height = px(self.terminal_height.max(100.));
        let app = cx.entity().downgrade();
        // Key by workspace so Kit's ResizableState doesn't leak sizes across folders.
        let split_id = self
            .workspace
            .session_key()
            .unwrap_or_else(|| "none".into());
        v_resizable(SharedString::from(format!("editor-term-split-{split_id}")))
            .on_resize(move |state, _, cx| {
                let _ = app.update(cx, |this, cx| {
                    this.remember_terminal_height(state, cx);
                });
            })
            .child(resizable_panel().child(top))
            .child(
                resizable_panel()
                    .size(height)
                    .child(self.terminal.clone()),
            )
            .into_any_element()
    }
}

impl Render for IsengardApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = if self.show_welcome() {
            welcome::render(self, cx).into_any_element()
        } else if self.file_tree.is_open() && self.sidebar_visible {
            let chrome = self.render_sidebar_chrome(cx);
            let body = match self.sidebar_view {
                SidebarView::Files => {
                    let app = cx.entity().downgrade();
                    let edit = self
                        .tree_edit
                        .as_ref()
                        .map(|edit| (&edit.target, &edit.input));
                    self.file_tree.render(
                        edit,
                        move |path, permanent, window, cx| {
                            _ = app.update(cx, |this, cx| {
                                this.open_file(&path, permanent, window, cx)
                            });
                        },
                        cx,
                    )
                }
                SidebarView::Search => self.search.clone().into_any_element(),
            };
            let sidebar = v_flex()
                .size_full()
                .min_w_0()
                .child(chrome)
                .child(div().flex_1().min_h_0().min_w_0().overflow_hidden().child(body));
            let width = px(self.sidebar_width.max(100.));
            let app = cx.entity().downgrade();
            let split_id = self
                .workspace
                .session_key()
                .unwrap_or_else(|| "none".into());
            h_resizable(SharedString::from(format!("main-split-{split_id}")))
                .on_resize(move |state, _, cx| {
                    let _ = app.update(cx, |this, cx| {
                        this.remember_sidebar_width(state, cx);
                    });
                })
                .child(
                    resizable_panel()
                        .size(width)
                        .child(div().size_full().min_w_0().overflow_hidden().child(sidebar)),
                )
                .child(resizable_panel().child(self.render_editor_with_terminal(cx)))
                .into_any_element()
        } else {
            self.render_editor_with_terminal(cx)
        };

        v_flex()
            .id("isengard")
            .key_context("Isengard")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(cx.theme().font_family.clone())
            .on_action(cx.listener(Self::on_new_file))
            .on_action(cx.listener(Self::on_new_workspace))
            .on_action(cx.listener(Self::on_open_folder))
            .on_action(cx.listener(Self::on_open_file))
            .on_action(cx.listener(Self::on_open_workspace))
            .on_action(cx.listener(Self::on_add_folder_to_workspace))
            .on_action(cx.listener(Self::on_save_workspace_as))
            .on_action(cx.listener(Self::on_remove_workspace_folder))
            .on_action(cx.listener(Self::on_rename_path))
            .on_action(cx.listener(Self::on_delete_path))
            .on_action(cx.listener(Self::on_new_file_in))
            .on_action(cx.listener(Self::on_new_folder_in))
            .on_action(cx.listener(Self::on_rename_selected_tree_item))
            .on_action(cx.listener(Self::on_delete_selected_tree_item))
            .on_action(cx.listener(Self::on_close_workspace))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_all))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_close_tab_at))
            .on_action(cx.listener(Self::on_close_other_tabs))
            .on_action(cx.listener(Self::on_close_all_tabs))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .on_action(cx.listener(Self::on_increase_font))
            .on_action(cx.listener(Self::on_decrease_font))
            .on_action(cx.listener(Self::on_reset_font))
            .on_action(cx.listener(Self::on_toggle_preview))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_find_in_files))
            .on_action(cx.listener(Self::on_show_file_tree))
            .on_action(cx.listener(Self::on_toggle_terminal))
            .on_action(cx.listener(Self::on_new_terminal))
            .on_action(cx.listener(Self::on_terminal_tabs_changed))
            .on_action(cx.listener(Self::on_about))
            .on_action(cx.listener(Self::on_quit))
            .child(self.render_title_bar(cx))
            .child(div().flex_1().min_h_0().child(main))
            .when(!self.show_welcome(), |this| {
                this.child(self.render_status_bar(cx))
            })
    }
}

