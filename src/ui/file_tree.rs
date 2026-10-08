use std::cell::Cell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    list::ListItem,
    menu::ContextMenuExt as _,
    tree::{TreeEvent, TreeItem, TreeState, tree},
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::app::{
    DeletePath, NewFileIn, NewFolderIn, RemoveWorkspaceFolder, RenamePath,
};
use crate::file_tree::{FsNode, GitIgnoreIndex, compute_guide_masks, tree_icon};

/// Separates a directory path from the suffix of its placeholder child's id.
const PLACEHOLDER_SUFFIX: &str = "\u{0}placeholder";
/// Temporary New File / New Folder row while the name is being typed.
const CREATING_SUFFIX: &str = "\u{0}creating";
/// Horizontal space per tree depth level (indent guide column width).
const INDENT_COL: f32 = 16.;

/// Inline edit target shown in the tree (rename existing, or create under a dir).
#[derive(Clone, Debug)]
pub enum TreeEditTarget {
    Rename(PathBuf),
    NewFile { parent: PathBuf },
    NewFolder { parent: PathBuf },
}

impl TreeEditTarget {
    pub fn row_id(&self) -> SharedString {
        match self {
            Self::Rename(path) => SharedString::from(path.to_string_lossy().into_owned()),
            Self::NewFile { parent } | Self::NewFolder { parent } => {
                SharedString::from(format!("{}{CREATING_SUFFIX}", parent.to_string_lossy()))
            }
        }
    }

    pub fn is_folder_icon(&self) -> bool {
        matches!(self, Self::NewFolder { .. })
    }
}

/// The workspace's lazily-loaded folder tree, rendered with GPUI Kit's `Tree`.
///
/// `FsNode`s are the source of truth; they are converted to `TreeItem`s whenever
/// the roots change or a directory is loaded for the first time. With one root
/// its children are the top level (a plain folder); with several, each root is
/// a top-level item (a multi-root workspace).
pub struct FileTreePanel {
    roots: Vec<FsNode>,
    state: Entity<TreeState>,
    gitignores: GitIgnoreIndex,
    /// Parent dir + is_folder while an inline New File/Folder row is open.
    creating: Option<(PathBuf, bool)>,
}

impl FileTreePanel {
    pub fn new(cx: &mut App) -> Self {
        Self {
            roots: Vec::new(),
            state: cx.new(|cx| TreeState::new(cx)),
            gitignores: GitIgnoreIndex::default(),
            creating: None,
        }
    }

    pub fn state(&self) -> &Entity<TreeState> {
        &self.state
    }

    pub fn is_open(&self) -> bool {
        !self.roots.is_empty()
    }

    /// Replaces the roots, keeping the loaded/expanded state of roots that remain.
    /// New roots start expanded (single-root needs that so children show; multi-root
    /// first open expands all). Session restore then calls [`Self::expand_paths`].
    pub fn set_folders(&mut self, folders: &[PathBuf], cx: &mut App) {
        let mut previous = std::mem::take(&mut self.roots);
        self.roots = folders
            .iter()
            .map(
                |folder| match previous.iter().position(|node| node.path() == folder) {
                    Some(ix) => previous.swap_remove(ix),
                    None => {
                        let mut root = FsNode::from_path(folder);
                        root.set_expanded(true);
                        root
                    }
                },
            )
            .collect();
        self.refresh_gitignores();
        self.sync(cx);
    }

    /// Paths of directories that are currently expanded (nested under collapsed
    /// ancestors are omitted — they are not visible).
    pub fn expanded_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for root in &self.roots {
            root.collect_expanded(&mut out);
        }
        out
    }

    /// Applies saved expand state: multi-root roots not listed stay collapsed;
    /// each path (and its ancestors) is expanded and loaded, then the tree syncs.
    pub fn expand_paths(&mut self, paths: &[PathBuf], cx: &mut App) {
        let want: HashSet<PathBuf> = paths.iter().cloned().collect();
        if self.roots.len() > 1 {
            for root in &mut self.roots {
                let expand = want.contains(root.path());
                root.set_expanded(expand);
            }
        }
        for path in paths {
            for root in &mut self.roots {
                if path.starts_with(root.path()) {
                    root.expand_toward(path);
                    break;
                }
            }
        }
        self.refresh_gitignores();
        self.sync(cx);
    }

    /// Keeps `FsNode`s in step with the tree's expand/collapse events.
    pub fn handle_event(&mut self, event: &TreeEvent, cx: &mut App) {
        let (id, expanded) = match event {
            TreeEvent::Expanded(id) => (id, true),
            TreeEvent::Collapsed(id) => (id, false),
        };
        let path = Path::new(id.as_ref());
        let Some(node) = self.roots.iter_mut().find_map(|root| root.find_mut(path)) else {
            return;
        };
        if node.set_expanded(expanded) {
            // Nested `.gitignore` may have appeared under the newly loaded dir.
            self.refresh_gitignores();
            self.sync(cx);
        }
    }

    /// Highlights `path` in the tree if it is currently visible.
    pub fn select_path(&self, path: &Path, cx: &mut App) {
        let id = SharedString::from(path.to_string_lossy().into_owned());
        self.state.update(cx, |state, cx| {
            let ix = state.index_of(&id);
            state.set_selected_index(ix, cx);
        });
    }

    /// Currently selected tree path, if any (placeholder rows resolve to their dir).
    pub fn selected_path(&self, cx: &App) -> Option<PathBuf> {
        let id = self.state.read(cx).selected_item()?.id.clone();
        let raw = id.as_ref();
        let path = raw
            .strip_suffix(PLACEHOLDER_SUFFIX)
            .unwrap_or(raw);
        Some(PathBuf::from(path))
    }


    /// Expands `dir`, reloads its children from disk, and syncs the Kit tree.
    pub fn refresh_dir(&mut self, dir: &Path, cx: &mut App) {
        for root in &mut self.roots {
            if dir == root.path() || dir.starts_with(root.path()) {
                root.expand_toward(dir);
                if let Some(node) = root.find_mut(dir) {
                    node.reload_children();
                }
                break;
            }
        }
        self.refresh_gitignores();
        self.sync(cx);
    }

    /// Shows or clears the temporary New File / New Folder row under `parent`.
    pub fn set_creating(&mut self, creating: Option<(PathBuf, bool)>, cx: &mut App) {
        self.creating = creating;
        if let Some((dir, _)) = &self.creating {
            for root in &mut self.roots {
                if dir == root.path() || dir.starts_with(root.path()) {
                    root.expand_toward(dir);
                    break;
                }
            }
        }
        self.refresh_gitignores();
        self.sync(cx);
        if let Some((dir, _)) = &self.creating {
            let id = SharedString::from(format!("{}{CREATING_SUFFIX}", dir.to_string_lossy()));
            self.state.update(cx, |state, cx| {
                let ix = state.index_of(&id);
                state.set_selected_index(ix, cx);
            });
        }
    }

    fn refresh_gitignores(&mut self) {
        let roots: Vec<PathBuf> = self.roots.iter().map(|r| r.path().to_path_buf()).collect();
        self.gitignores.rebuild(&roots);
    }

    fn sync(&self, cx: &mut App) {
        let creating = self
            .creating
            .as_ref()
            .map(|(p, is_folder)| (p.as_path(), *is_folder));
        let items: Vec<TreeItem> = match self.roots.as_slice() {
            [root] => {
                let mut items: Vec<TreeItem> = root
                    .children()
                    .unwrap_or_default()
                    .iter()
                    .map(|n| to_tree_item(n, creating))
                    .collect();
                if creating.is_some_and(|(p, _)| p == root.path()) {
                    inject_creating_row(&mut items, root.path(), creating.unwrap().1);
                }
                items
            }
            roots => roots
                .iter()
                .map(|root| to_root_item(root, creating))
                .collect(),
        };
        self.state.update(cx, |state, cx| {
            let selected = state.selected_item().map(|item| item.id.clone());
            state.set_items(items, cx);
            if let Some(id) = selected {
                let ix = state.index_of(&id);
                state.set_selected_index(ix, cx);
            }
        });
    }

    /// Renders the file tree.
    /// `title` — folder or workspace display name in the toolbar (left).
    /// `on_open_file(path, permanent, …)` — `permanent` is true on double-click.
    /// `edit` — when set, that row shows an inline name `Input` instead of the label.
    pub fn render(
        &self,
        title: SharedString,
        edit: Option<(&TreeEditTarget, &Entity<InputState>)>,
        on_open_file: impl Fn(PathBuf, bool, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let multi_root = self.roots.len() > 1;
        // Single-root hides the root row, so blank-space create targets that folder.
        // Multi-root: each root is clickable; blank space still creates in the first root.
        let blank_create_root = self.roots.first().map(|r| r.path().to_path_buf());
        // Row + panel ContextMenus both hover (Normal hitboxes); row right-click
        // sets this so the blank-space menu builder returns empty.
        let blank_menu_blocked = Rc::new(Cell::new(false));
        let on_open_file = Rc::new(on_open_file);
        let tree_state = self.state.clone();
        let edit_id = edit.map(|(target, _)| target.row_id());
        let edit_folder_icon = edit.map(|(target, _)| target.is_folder_icon()).unwrap_or(false);
        let edit_input = edit.map(|(_, input)| input.clone());
        let guide_masks = Rc::new(compute_guide_masks(self.state.read(cx)));
        let ignored_paths = Rc::new({
            let state = self.state.read(cx);
            let mut set = HashSet::new();
            let mut ix = 0;
            while let Some(entry) = state.entry(ix) {
                let path = PathBuf::from(entry.item().id.as_ref());
                if !path.to_string_lossy().contains(PLACEHOLDER_SUFFIX)
                    && !path.to_string_lossy().contains(CREATING_SUFFIX)
                    && self.gitignores.is_ignored(&path)
                {
                    set.insert(path);
                }
                ix += 1;
            }
            set
        });

        let blank_menu_blocked_row = blank_menu_blocked.clone();
        let tree_el = tree(&self.state, move |ix, entry, _selected, _window, cx| {
                    let item = entry.item();
                    let is_placeholder = item.id.ends_with(PLACEHOLDER_SUFFIX);
                    let is_creating = item.id.ends_with(CREATING_SUFFIX);
                    let path = PathBuf::from(item.id.as_ref());
                    let is_missing_root = entry.depth() == 0 && entry.is_disabled();
                    let is_editing = edit_id.as_ref().is_some_and(|id| id == &item.id);
                    let is_dir =
                        entry.is_folder() || is_missing_root || (is_creating && edit_folder_icon);
                    let git_ignored = !is_placeholder
                        && !is_creating
                        && ignored_paths.contains(&path);
                    let icon = (!is_placeholder).then(|| {
                        let icon_path = if is_creating {
                            PathBuf::from(if edit_folder_icon {
                                "New Folder"
                            } else {
                                "untitled.txt"
                            })
                        } else {
                            path.clone()
                        };
                        tree_icon(
                            &icon_path,
                            is_dir,
                            entry.is_expanded(),
                            multi_root && entry.depth() == 0,
                            cx.theme().is_dark(),
                        )
                    });
                    let on_open_file = on_open_file.clone();
                    let tree_state = tree_state.clone();
                    let is_file = !entry.is_folder() && !is_placeholder && !is_creating;
                    let row_input = edit_input.clone().filter(|_| is_editing);
                    let guides = guide_masks.get(ix).cloned().unwrap_or_default();
                    let guide_color = cx.theme().sidebar_border;
                    let muted = cx.theme().muted_foreground;

                    ListItem::new(ix)
                        .w_full()
                        .min_w_0()
                        // Kit's ListItem defaults to py_1 / px_3 — py gaps break
                        // guides; px would inset the hover. Zero both; inset the
                        // row content instead so hover is full-bleed.
                        .py_0()
                        .px_0()
                        .when(!entry.is_disabled() && !is_editing, |item| {
                            item.cursor_pointer()
                        })
                        .rounded(cx.theme().radius)
                        // Kit selects on mouse-down but does not focus the Tree;
                        // without this, Delete/F2 stay bound to the editor.
                        .when(!is_editing && !entry.is_disabled(), |item| {
                            let blank_menu_blocked = blank_menu_blocked_row.clone();
                            item.on_mouse_down(MouseButton::Left, {
                                let tree_state = tree_state.clone();
                                move |_, window, cx| {
                                    tree_state.update(cx, |state, cx| state.focus(window, cx));
                                }
                            })
                            .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                                blank_menu_blocked.set(true);
                                tree_state.update(cx, |state, cx| state.focus(window, cx));
                            })
                        })
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .items_stretch()
                                .px_3()
                                // Do not overflow_hidden here — guide lines extend
                                // 1px past the row to meet the next segment.
                                .children(guides.into_iter().map(|continues| {
                                    // Absolute line with 1px overlap so segments
                                    // still meet if ListItem/list leaves a hairline gap.
                                    div()
                                        .relative()
                                        .w(px(INDENT_COL))
                                        .flex_shrink_0()
                                        .children(continues.then(|| {
                                            div()
                                                .absolute()
                                                .top(px(-1.))
                                                .bottom(px(-1.))
                                                .left(px((INDENT_COL - 1.) / 2.))
                                                .w(px(1.))
                                                .bg(guide_color)
                                        }))
                                }))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .min_w_0()
                                        .flex_1()
                                        .overflow_hidden()
                                        .py_0p5()
                                        .when(git_ignored && !is_editing, |this| this.opacity(0.6))
                                        .children(icon)
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .overflow_hidden()
                                                .when(!is_editing, |this| this.truncate())
                                                .when(is_placeholder, |this| {
                                                    this.text_color(muted)
                                                })
                                                .map(|this| match row_input {
                                                    Some(input) => this.child(
                                                        Input::new(&input)
                                                            .id("tree-name-input")
                                                            .xsmall()
                                                            .w_full(),
                                                    ),
                                                    None => this.child(item.label.clone()),
                                                }),
                                        ),
                                ),
                        )
                        .when(!is_editing, |item| {
                            item.on_click(move |ev: &ClickEvent, window, cx| {
                                if is_file {
                                    on_open_file(path.clone(), ev.click_count() >= 2, window, cx);
                                }
                            })
                        })
                })
                .context_menu(move |_, entry, menu, _, _| {
                    let id = entry.item().id.as_ref();
                    if id.contains(PLACEHOLDER_SUFFIX) || id.contains(CREATING_SUFFIX) {
                        return menu;
                    }
                    let path = PathBuf::from(id);
                    let is_dir = entry.is_folder();
                    let is_workspace_root = multi_root && entry.depth() == 0;
                    let mut menu = menu;
                    if is_dir {
                        menu = menu
                            .menu("New File", Box::new(NewFileIn(path.clone())))
                            .menu("New Folder", Box::new(NewFolderIn(path.clone())))
                            .separator();
                    }
                    menu = menu
                        .menu("Rename", Box::new(RenamePath(path.clone())))
                        .menu("Delete", Box::new(DeletePath(path.clone())));
                    if is_workspace_root {
                        menu = menu.separator().menu(
                            "Remove Folder from Workspace",
                            Box::new(RemoveWorkspaceFolder(path)),
                        );
                    }
                    menu
                })
                .size_full()
                .min_w_0()
                .text_color(theme.sidebar_foreground)
                .text_sm();

        // Toolbar: folder/workspace name | New File / New Folder. Parent for
        // create resolved on click: selection's dir, parent of selected file,
        // else first workspace root.
        let toolbar_roots: Vec<PathBuf> =
            self.roots.iter().map(|r| r.path().to_path_buf()).collect();
        let tree_state_toolbar = self.state.clone();
        let toolbar = h_flex()
            .id("file-tree-toolbar")
            .w_full()
            .h(px(crate::theme::PANEL_HEADER_ROW_H))
            .items_center()
            .justify_between()
            .gap_2()
            .px_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .child({
                        let tree_state = tree_state_toolbar.clone();
                        let roots = toolbar_roots.clone();
                        Button::new("tree-new-file")
                            .ghost()
                            .cursor_pointer()
                            .small()
                            .icon(LucideIcon::FilePlus)
                            .tooltip("New File")
                            .on_click(
                                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                    let Some(parent) =
                                        toolbar_create_parent(&tree_state, &roots, &*cx)
                                    else {
                                        return;
                                    };
                                    tree_state.update(cx, |state, cx| state.focus(window, cx));
                                    window.dispatch_action(Box::new(NewFileIn(parent)), cx);
                                },
                            )
                    })
                    .child({
                        let tree_state = tree_state_toolbar;
                        let roots = toolbar_roots;
                        Button::new("tree-new-folder")
                            .ghost()
                            .cursor_pointer()
                            .small()
                            .icon(LucideIcon::FolderPlus)
                            .tooltip("New Folder")
                            .on_click(
                                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                    let Some(parent) =
                                        toolbar_create_parent(&tree_state, &roots, &*cx)
                                    else {
                                        return;
                                    };
                                    tree_state.update(cx, |state, cx| state.focus(window, cx));
                                    window.dispatch_action(Box::new(NewFolderIn(parent)), cx);
                                },
                            )
                    }),
            );

        // Fill the panel so right-click on empty space (below rows) can create
        // at the workspace root — single-root never shows a root row to target.
        let tree_state_blank = self.state.clone();
        let tree_panel = div()
            .id("file-tree-panel")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .w_full()
            .overflow_hidden()
            .context_menu(move |menu, window, cx| {
                if blank_menu_blocked.replace(false) {
                    return menu;
                }
                let Some(root) = blank_create_root.clone() else {
                    return menu;
                };
                tree_state_blank.update(cx, |state, cx| state.focus(window, cx));
                menu.menu("New File", Box::new(NewFileIn(root.clone())))
                    .menu("New Folder", Box::new(NewFolderIn(root)))
            })
            .child(tree_el);

        v_flex()
            .id("file-tree")
            .size_full()
            .min_w_0()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(toolbar)
            .child(tree_panel)
            .into_any_element()
    }
}

/// Parent dir for toolbar create: selected folder, parent of selected file,
/// else the first workspace root.
fn toolbar_create_parent(
    tree_state: &Entity<TreeState>,
    roots: &[PathBuf],
    cx: &App,
) -> Option<PathBuf> {
    if let Some(item) = tree_state.read(cx).selected_item() {
        let raw = item.id.as_ref();
        if !raw.contains(CREATING_SUFFIX) {
            let path = PathBuf::from(
                raw.strip_suffix(PLACEHOLDER_SUFFIX).unwrap_or(raw),
            );
            if path.is_dir() {
                return Some(path);
            }
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                return Some(parent.to_path_buf());
            }
        }
    }
    roots.first().cloned()
}

fn to_tree_item(node: &FsNode, creating: Option<(&Path, bool)>) -> TreeItem {
    let id = node.path().to_string_lossy().into_owned();
    match node {
        FsNode::File { name, .. } => TreeItem::new(id, name.clone()),
        FsNode::Dir {
            name,
            children,
            expanded,
            path,
            ..
        } => {
            let mut kids: Vec<TreeItem> = match children {
                Some(children) if !children.is_empty() => {
                    children.iter().map(|n| to_tree_item(n, creating)).collect()
                }
                // A child is required for the tree to treat this as an expandable folder.
                Some(_) => vec![placeholder(&id, "(empty)")],
                None => vec![placeholder(&id, "Loading…")],
            };
            if creating.is_some_and(|(p, _)| p == path.as_path()) {
                inject_creating_row(&mut kids, path, creating.unwrap().1);
            }
            TreeItem::new(id, name.clone())
                .children(kids)
                .expanded(*expanded)
        }
    }
}

/// A workspace root as a top-level item; a root that no longer exists on disk
/// is shown disabled with a "(missing)" label.
fn to_root_item(root: &FsNode, creating: Option<(&Path, bool)>) -> TreeItem {
    if root.path().is_dir() {
        to_tree_item(root, creating)
    } else {
        TreeItem::new(
            root.path().to_string_lossy().into_owned(),
            format!("{} (missing)", root.name()),
        )
        .disabled(true)
    }
}

fn inject_creating_row(children: &mut Vec<TreeItem>, parent: &Path, is_folder: bool) {
    children.retain(|c| !c.id.ends_with(PLACEHOLDER_SUFFIX));
    let id = format!("{}{CREATING_SUFFIX}", parent.to_string_lossy());
    let label = if is_folder { "New Folder" } else { "untitled.txt" };
    children.insert(0, TreeItem::new(id, label.to_owned()));
}

fn placeholder(parent_id: &str, label: &str) -> TreeItem {
    TreeItem::new(format!("{parent_id}{PLACEHOLDER_SUFFIX}"), label.to_owned()).disabled(true)
}
