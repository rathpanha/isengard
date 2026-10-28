//! Window content entity (`IsengardApp`) and its action handlers / layout.
//!
//! One GPUI entity still owns the workspace state; the former monolith `app.rs` is
//! split into child modules that `impl IsengardApp` (private fields stay visible to
//! descendants). Add methods next to their concern — not back into this file.
//!
//! | Module | Responsibility |
//! | --- | --- |
//! | `actions` | Action types, key bindings, menu → app bridge |
//! | `agent` | Right Agent panel: toggle, file/login requests from the agent |
//! | `tab` | `EditorTab` / `TabBody` / diff kind |
//! | `workspace` | Open/switch/persist workspace, recents, titles |
//! | `session` | Session + window geometry restore/persist |
//! | `files` | Open/save/close tabs, untitled, font/preview |
//! | `tree` | Inline create/rename/delete in the file tree |
//! | `lsp` | LSP attach, install/remove UI, event pump |
//! | `search` | Project search/replace + selection soft-highlight |
//! | `git` | Source Control diff/conflict tabs |
//! | `terminal` / `terminal_cwd` | Bottom PTY panel + cwd picker |
//! | `chrome` | Sidebar toggle, About, config, quit |
//! | `update` | Update check, download dialogs, restart / install command |
//! | `render` | Title/sidebar/editor/status-bar paint + `Render` |
//! | `welcome` | Welcome screen (start actions + recents) |

mod actions;
mod agent;
mod chrome;
mod files;
mod git;
mod lsp;
mod render;
mod search;
mod session;
mod tab;
mod terminal;
mod terminal_cwd;
mod tree;
#[cfg(test)]
mod ui_tests;
mod update;
mod watch;
mod welcome;
mod workspace;

// `use super::*` in children only sees `pub` items — re-export deps + sibling types.
pub(super) use std::cell::Cell;
pub(super) use std::collections::HashMap;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::rc::Rc;
pub(super) use std::sync::atomic::{AtomicBool, Ordering};
pub(super) use std::sync::Arc;
pub(super) use std::time::Duration;

pub(super) use gpui_kit::assets::IconName as LucideIcon;
pub(super) use gpui_kit::component::{
    ActiveTheme as _, IconName, Selectable as _, Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonGroup, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
    input::{
        Editor, EditorState, Input, InputEvent, InputState, Position, RangeDecoration,
        RangeDecorationCollection, RangeDecorationStyle, Rope, RopeExt as _, TabSize,
    },
    list::ListItem,
    menu::{AppMenuBar, ContextMenuExt as _},
    popover::{Popover, PopoverState},
    resizable::{ResizableState, h_resizable, resizable_panel, v_resizable},
    scroll::ScrollableElement as _,
    status_bar::StatusBar,
    tab::Tab,
    text::TextView,
    tree::TreeEvent,
    v_flex,
};
pub(super) use gpui_kit::{prelude::FluentBuilder as _, *};
pub(super) use serde::Deserialize;

pub(super) use crate::agent::{AgentPanel, AgentPanelEvent};
pub(super) use crate::config::{
    AppConfig, DEFAULT_AGENT_WIDTH, DEFAULT_EDITOR_FONT_SIZE, DEFAULT_SIDEBAR_WIDTH,
    DEFAULT_TERMINAL_HEIGHT, FileViewState, MAX_SESSION_UNSAVED_BYTES, OpenFileEntry,
    SidebarPanel, WorkspaceSession,
};
pub(super) use crate::editor::document;
pub(super) use crate::editor::language::Language;
pub(super) use crate::editor::selection_matches;
pub(super) use crate::editor::tabs::{self as tab_order, TabList};
pub(super) use crate::file_tree::tree_icon;
pub(super) use crate::git::{
    ConflictHunk, ConflictSide, GitPanel, GitPanelEvent, HunkSide, OverviewMark, RepoStatus,
    apply_hunk_choices, change_at_path, conflict_file_sides, conflict_hunks, file_sides,
    line_highlight_ranges, load_workspace_status, overview_marks,
};
pub(super) use crate::lsp::{
    AttachOutcome, CatalogServer, LspClient, LspEvent, LspHost, MissingServerOffer,
    ServerDisplayStatus, install_tab_accepts_completion, is_app_local, recipe_for_command,
    run_install, run_uninstall,
};
pub(super) use crate::menus;
pub(super) use crate::search::{
    SearchMatch, SearchPanel, SearchPanelEvent, SearchQuery, apply_replace, run_search,
};
pub(super) use crate::terminal::TerminalPanel;
pub(super) use crate::theme;
pub(super) use crate::ui::components::{center_dialog, notify_error, notify_success, notify_warning};
pub(super) use crate::ui::file_tree::{
    DeletePath, FileTreePanel, MovePath, NewFileIn, NewFolderIn, RemoveWorkspaceFolder,
    RenamePath, ReorderRoot, RevealPath, ToggleDir, TreeEditTarget,
};
pub(super) use crate::ui::tab_drag::{EditorTabDrag, TabDragPreview};
pub(super) use crate::workspace::{WORKSPACE_EXTENSION, Workspace};

use tab::EditorTab;

/// Callback after a single tab save (path may change when materializing untitled).
pub(super) type AfterSave = Rc<dyn Fn(&mut IsengardApp, PathBuf, &mut Window, &mut Context<IsengardApp>)>;
/// Write failed after the user picked a path for an untitled tab (Save All queue).
pub(super) type AfterSaveFail =
    Rc<dyn Fn(&mut IsengardApp, anyhow::Error, &mut Window, &mut Context<IsengardApp>)>;
/// Callback after Save All / a save queue finishes.
pub(super) type AfterSaveAll = Rc<dyn Fn(&mut IsengardApp, &mut Window, &mut Context<IsengardApp>)>;

// Action types live in `actions` but must stay at `crate::app::*` for menus/UI.
pub use actions::init;
pub use actions::{
    About, AddFolderToWorkspace, AddSelectionToAgent, CheckForUpdates, CloseAllTabs, CloseOtherTabs, CloseTab,
    CloseTabAt, CloseWorkspace, DecreaseFontSize, DeleteSelectedTreeItem, FindInFiles,
    IncreaseFontSize, NewAgentChat, NewFile, NewTerminal, NewWorkspace, OpenFile,
    FormatDocument, OpenFolder, OpenWorkspace, Quit, RenameSelectedTreeItem, RenameSymbol,
    ResetFontSize, Save, SaveAll, SaveAs,
    SaveWorkspaceAs, NextTab, PrevTab, ShowFileTree, ShowGit, TerminalTabsChanged, ToggleAgentPanel,
    TogglePreview, ToggleSidebar, ToggleTerminal,
};

#[derive(Clone, Copy)]
pub(super) struct WindowGeometrySnapshot {
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    maximized: bool,
    fullscreen: bool,
}


pub(super) fn window_geometry_snapshot(window: &Window) -> WindowGeometrySnapshot {
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

/// Active inline name edit in the file tree (Enter/Blur commits, Escape cancels).
pub(super) struct TreeInlineEdit {
    target: crate::ui::file_tree::TreeEditTarget,
    input: Entity<InputState>,
    _events: Subscription,
    _keys: Subscription,
}

/// What to switch the window to once the user has confirmed leaving the
/// current workspace (VS Code closes all editors when switching).
#[derive(Clone)]
pub(super) enum Switch {
    Folder(PathBuf),
    Workspace(Workspace),
    Close,
}

/// After a successful "Save Workspace As…" during leave/quit.
#[derive(Clone)]
pub(super) enum AfterWorkspaceSave {
    /// Menu Save Workspace As — stay put.
    None,
    /// Switching / closing: continue with unsaved-file confirm then switch.
    Switch(Switch),
    /// Quit: continue with unsaved-file confirm then quit.
    ContinueQuit,
}

/// Which view fills the left sidebar (same panel width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SidebarView {
    Files,
    Search,
    Git,
}

impl From<SidebarView> for SidebarPanel {
    fn from(view: SidebarView) -> Self {
        match view {
            SidebarView::Files => Self::Files,
            SidebarView::Search => Self::Search,
            SidebarView::Git => Self::Git,
        }
    }
}

impl From<SidebarPanel> for SidebarView {
    fn from(panel: SidebarPanel) -> Self {
        match panel {
            SidebarPanel::Files => Self::Files,
            SidebarPanel::Search => Self::Search,
            SidebarPanel::Git => Self::Git,
        }
    }
}

pub struct IsengardApp {
    config: AppConfig,
    focus_handle: FocusHandle,
    /// The open folders; the file tree mirrors `workspace.folders()`.
    workspace: Workspace,
    file_tree: FileTreePanel,
    search: Entity<SearchPanel>,
    git: Entity<GitPanel>,
    sidebar_view: SidebarView,
    /// Bumps on each query change so stale background searches are ignored.
    search_generation: u64,
    /// Flipped to abort an in-flight `run_search` when a newer query arrives.
    search_cancel: Arc<AtomicBool>,
    /// Fill decorations for project-search hits in the active preview editor.
    /// Dropping the handle does not clear them — we `dispose` on replace.
    search_hit_decorations: Option<RangeDecorationCollection>,
    /// Soft fills for other occurrences of the current selection (per editor).
    selection_match_decorations: HashMap<EntityId, RangeDecorationCollection>,
    /// Coalesce / debounce selection-match refreshes (observe + drag fire often).
    selection_match_gen: u64,
    /// Last selection range we scheduled matches for (skip scroll/blink no-ops).
    selection_match_last_range: HashMap<EntityId, std::ops::Range<usize>>,
    /// Last cursor shown in the status bar (skip app re-render on scroll/blink).
    status_bar_cursor: Option<(u32, u32)>,
    /// Controlled LSP catalog popover — install/remove must close it or focus
    /// dies on the ephemeral trigger and File > Close Folder stops working.
    lsp_catalog_open: bool,
    tabs: TabList<EditorTab>,
    app_menu_bar: Entity<AppMenuBar>,
    /// Bottom integrated terminal (lazy; created on first show).
    terminal: Entity<TerminalPanel>,
    /// Per-workspace layout (restored from [`WorkspaceSession`]).
    sidebar_width: f32,
    sidebar_visible: bool,
    terminal_height: f32,
    /// Agent panel of the open folder/workspace (connects lazily on first show).
    agent: Option<Entity<AgentPanel>>,
    /// Every folder/workspace's panel by session key, so a background agent
    /// can finish its turn and a returning workspace finds its transcript.
    agents: HashMap<String, (Entity<AgentPanel>, Subscription)>,
    agent_visible: bool,
    agent_width: f32,
    /// Set once the user chose to quit despite unsaved changes.
    allow_quit: bool,
    /// An update check or download is running (one at a time).
    update_busy: bool,
    /// After the next real quit, start this (freshly updated) binary.
    relaunch_exe: Option<PathBuf>,
    /// Suppresses session writes while batch-restoring tabs.
    restoring_session: bool,
    /// Debounce token for persisting window bounds while resizing.
    window_geometry_generation: u64,
    /// Debounce token for persisting session (dirty buffers, search, git UI).
    session_persist_generation: u64,
    /// Inline rename / new-file / new-folder field over a tree row (not a modal).
    tree_edit: Option<TreeInlineEdit>,
    /// Language servers for the current workspace (rust-analyzer MVP).
    lsp: LspHost,
    /// OS watcher over the workspace roots (`app/watch.rs`).
    fs_watch: Option<crate::watch::FsWatcher>,
    /// Bumps on every restart so a replaced watcher's pump stops applying.
    fs_watch_gen: u64,
    /// Open dirty files whose disk text diverged (warned once each).
    disk_conflicts: std::collections::HashSet<PathBuf>,
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
                        this.schedule_persist_session(cx);
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

        let git = cx.new(|cx| GitPanel::new(window, cx));
        let git_subscription =
            cx.subscribe_in(&git, window, |this, _, event: &GitPanelEvent, window, cx| {
                match event {
                    GitPanelEvent::OpenDiff {
                        repo,
                        path,
                        rel_path,
                        status,
                        old_rel_path,
                    } => {
                        this.open_git_diff(
                            repo.clone(),
                            path.clone(),
                            rel_path.clone(),
                            *status,
                            old_rel_path.clone(),
                            true,
                            window,
                            cx,
                        );
                    }
                    GitPanelEvent::OpenConflict {
                        repo,
                        path,
                        rel_path,
                    } => {
                        this.open_git_conflict(
                            repo.clone(),
                            path.clone(),
                            rel_path.clone(),
                            true,
                            window,
                            cx,
                        );
                    }
                    GitPanelEvent::OpenFile { path } => {
                        this.open_file(path, true, window, cx);
                    }
                    GitPanelEvent::ReloadFile { path } => {
                        this.reload_open_file_from_disk(path, window, cx);
                    }
                    GitPanelEvent::CloseConflictPreview { path } => {
                        this.close_conflict_preview(path, cx);
                    }
                    GitPanelEvent::ConfirmDiscardFile {
                        repo,
                        rel_path,
                        status,
                        old_rel_path,
                    } => {
                        this.confirm_git_discard_file(
                            repo.clone(),
                            rel_path.clone(),
                            *status,
                            old_rel_path.clone(),
                            window,
                            cx,
                        );
                    }
                    GitPanelEvent::ConfirmDiscardAll { repo, count } => {
                        this.confirm_git_discard_all(repo.clone(), *count, window, cx);
                    }
                    GitPanelEvent::SuggestCommitAi { root, paths } => {
                        this.suggest_git_commit_ai(root.clone(), paths.clone(), window, cx);
                    }
                    GitPanelEvent::ToastError(msg) => {
                        notify_error(msg.clone(), window, cx);
                    }
                    GitPanelEvent::ToastSuccess(msg) => {
                        notify_success(msg.clone(), window, cx);
                    }
                    GitPanelEvent::DiscardedPaths { paths } => {
                        if let Some(agent) = this.agent.clone() {
                            agent.update(cx, |agent, cx| {
                                agent.note_discarded_paths(paths, cx);
                            });
                        }
                    }
                    GitPanelEvent::DiscardedRepo { root } => {
                        if let Some(agent) = this.agent.clone() {
                            agent.update(cx, |agent, cx| {
                                agent.note_discarded_repo(root, cx);
                            });
                        }
                    }
                    GitPanelEvent::StatusUpdated => {
                        let dirty = this.git.read(cx).dirty_paths();
                        this.file_tree.set_git_dirty(dirty, cx);
                        cx.notify();
                    }
                    GitPanelEvent::UiChanged => {
                        this.schedule_persist_session(cx);
                    }
                }
            });

        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| {
                // A plain close is not a pending "restart to update".
                this.relaunch_exe = None;
                this.persist_window_geometry(window);
                this.persist_session(cx);
                this.confirm_quit(window, cx)
            })
            .unwrap_or(true)
        });

        let bounds_sub = cx.observe_window_bounds(window, |this, window, cx| {
            this.schedule_persist_window_geometry(window, cx);
        });
        // Watcher can miss events while the window is inactive / after sleep.
        let activation_sub = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.reconcile_open_tabs_with_disk(window, cx);
            }
        });

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        cx.set_global(actions::ActiveIsengard(cx.weak_entity()));

        let terminal = cx.new(|_| TerminalPanel::new(&[]));


        let mut app = Self {
            config,
            focus_handle,
            workspace: Workspace::default(),
            file_tree,
            search,
            git,
            // ponytail: QA/demo only (`ISENGARD_OPEN_GIT=1 cargo run -- folder`).
            sidebar_view: if std::env::var_os("ISENGARD_OPEN_GIT").is_some() {
                SidebarView::Git
            } else {
                SidebarView::Files
            },
            search_generation: 0,
            search_cancel: Arc::new(AtomicBool::new(false)),
            search_hit_decorations: None,
            selection_match_decorations: HashMap::new(),
            selection_match_gen: 0,
            selection_match_last_range: HashMap::new(),
            status_bar_cursor: None,
            lsp_catalog_open: false,
            tabs: TabList::default(),
            app_menu_bar,
            terminal,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_visible: true,
            terminal_height: DEFAULT_TERMINAL_HEIGHT,
            agent: None,
            agents: HashMap::new(),
            agent_visible: false,
            agent_width: DEFAULT_AGENT_WIDTH,
            allow_quit: false,
            update_busy: false,
            relaunch_exe: None,
            restoring_session: false,
            window_geometry_generation: 0,
            session_persist_generation: 0,
            tree_edit: None,
            lsp: LspHost::default(),
            fs_watch: None,
            fs_watch_gen: 0,
            disk_conflicts: std::collections::HashSet::new(),
            _subscriptions: vec![
                tree_subscription,
                search_subscription,
                git_subscription,
                bounds_sub,
                activation_sub,
            ],
        };

        app.lsp.set_settings(app.config.lsp.clone());
        let app_weak = cx.weak_entity();
        app.lsp.set_workspace_edit_handler(Rc::new(move |edit, window, cx| {
            let _ = app_weak.update(cx, |this, cx| {
                this.apply_workspace_edit(edit, window, cx);
            });
        }));
        app.start_lsp_event_pump(cx);

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

        app.refresh_menus(cx);
        app.check_for_updates_on_start(window, cx);
        app
    }


}
