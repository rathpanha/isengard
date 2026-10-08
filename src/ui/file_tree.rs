use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, h_flex,
    input::{Input, InputState},
    list::ListItem,
    tree::{TreeEvent, TreeItem, TreeState, tree},
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::app::{
    DeletePath, NewFileIn, NewFolderIn, RemoveWorkspaceFolder, RenamePath,
};
use crate::file_tree::{FsNode, GitIgnoreIndex, compute_guide_masks, tree_icon};

/// Separates a directory path from the suffix of its placeholder child's id.
const PLACEHOLDER_SUFFIX: &str = "\u{0}placeholder";
/// Horizontal space per tree depth level (indent guide column width).
const INDENT_COL: f32 = 16.;

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
}

impl FileTreePanel {
    pub fn new(cx: &mut App) -> Self {
        Self {
            roots: Vec::new(),
            state: cx.new(|cx| TreeState::new(cx)),
            gitignores: GitIgnoreIndex::default(),
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

    fn refresh_gitignores(&mut self) {
        let roots: Vec<PathBuf> = self.roots.iter().map(|r| r.path().to_path_buf()).collect();
        self.gitignores.rebuild(&roots);
    }

    fn sync(&self, cx: &mut App) {
        let items: Vec<TreeItem> = match self.roots.as_slice() {
            [root] => root
                .children()
                .unwrap_or_default()
                .iter()
                .map(to_tree_item)
                .collect(),
            roots => roots.iter().map(to_root_item).collect(),
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
    /// `on_open_file(path, permanent, …)` — `permanent` is true on double-click.
    /// `rename` — when set, that row shows an inline name `Input` instead of the label.
    pub fn render(
        &self,
        rename: Option<(&Path, &Entity<InputState>)>,
        on_open_file: impl Fn(PathBuf, bool, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let multi_root = self.roots.len() > 1;
        let on_open_file = Rc::new(on_open_file);
        let tree_state = self.state.clone();
        let rename_path = rename.map(|(p, _)| p.to_path_buf());
        let rename_input = rename.map(|(_, input)| input.clone());
        let guide_masks = Rc::new(compute_guide_masks(self.state.read(cx)));
        let ignored_paths = Rc::new({
            let state = self.state.read(cx);
            let mut set = HashSet::new();
            let mut ix = 0;
            while let Some(entry) = state.entry(ix) {
                let path = PathBuf::from(entry.item().id.as_ref());
                if !path.to_string_lossy().contains(PLACEHOLDER_SUFFIX)
                    && self.gitignores.is_ignored(&path)
                {
                    set.insert(path);
                }
                ix += 1;
            }
            set
        });

        tree(&self.state, move |ix, entry, _selected, _window, cx| {
                    let item = entry.item();
                    let is_placeholder = item.id.ends_with(PLACEHOLDER_SUFFIX);
                    let path = PathBuf::from(item.id.as_ref());
                    let is_missing_root = entry.depth() == 0 && entry.is_disabled();
                    let is_dir = entry.is_folder() || is_missing_root;
                    let git_ignored = !is_placeholder && ignored_paths.contains(&path);
                    let icon = (!is_placeholder).then(|| {
                        tree_icon(
                            &path,
                            is_dir,
                            entry.is_expanded(),
                            multi_root && entry.depth() == 0,
                            cx.theme().is_dark(),
                        )
                    });
                    let on_open_file = on_open_file.clone();
                    let tree_state = tree_state.clone();
                    let is_file = !entry.is_folder() && !is_placeholder;
                    let is_renaming = rename_path.as_ref().is_some_and(|p| p == &path);
                    let rename_input = rename_input.clone().filter(|_| is_renaming);
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
                        .when(!entry.is_disabled() && !is_renaming, |item| {
                            item.cursor_pointer()
                        })
                        .rounded(cx.theme().radius)
                        // Kit selects on mouse-down but does not focus the Tree;
                        // without this, Delete/F2 stay bound to the editor.
                        .when(!is_renaming && !entry.is_disabled(), |item| {
                            item.on_mouse_down(MouseButton::Left, {
                                let tree_state = tree_state.clone();
                                move |_, window, cx| {
                                    tree_state.update(cx, |state, cx| state.focus(window, cx));
                                }
                            })
                            .on_mouse_down(MouseButton::Right, move |_, window, cx| {
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
                                        .when(git_ignored && !is_renaming, |this| this.opacity(0.6))
                                        .children(icon)
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .overflow_hidden()
                                                .when(!is_renaming, |this| this.truncate())
                                                .when(is_placeholder, |this| {
                                                    this.text_color(muted)
                                                })
                                                .map(|this| match rename_input {
                                                    Some(input) => this.child(
                                                        Input::new(&input)
                                                            .id("tree-rename-input")
                                                            .xsmall()
                                                            .w_full(),
                                                    ),
                                                    None => this.child(item.label.clone()),
                                                }),
                                        ),
                                ),
                        )
                        .when(!is_renaming, |item| {
                            item.on_click(move |ev: &ClickEvent, window, cx| {
                                if is_file {
                                    on_open_file(path.clone(), ev.click_count() >= 2, window, cx);
                                }
                            })
                        })
                })
                .context_menu(move |_, entry, menu, _, _| {
                    let id = entry.item().id.as_ref();
                    if id.contains(PLACEHOLDER_SUFFIX) {
                        return menu;
                    }
                    let path = PathBuf::from(id);
                    let is_dir = entry.is_folder();
                    let is_workspace_root = multi_root && entry.depth() == 0;
                    let mut menu = menu;
                    if is_dir {
                        menu = menu
                            .menu("New File…", Box::new(NewFileIn(path.clone())))
                            .menu("New Folder…", Box::new(NewFolderIn(path.clone())))
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
                .overflow_hidden()
                .bg(theme.sidebar)
                .text_color(theme.sidebar_foreground)
                .text_sm()
                .into_any_element()
    }
}

fn to_tree_item(node: &FsNode) -> TreeItem {
    let id = node.path().to_string_lossy().into_owned();
    match node {
        FsNode::File { name, .. } => TreeItem::new(id, name.clone()),
        FsNode::Dir {
            name,
            children,
            expanded,
            ..
        } => {
            let children: Vec<TreeItem> = match children {
                Some(children) if !children.is_empty() => {
                    children.iter().map(to_tree_item).collect()
                }
                // A child is required for the tree to treat this as an expandable folder.
                Some(_) => vec![placeholder(&id, "(empty)")],
                None => vec![placeholder(&id, "Loading…")],
            };
            TreeItem::new(id, name.clone())
                .children(children)
                .expanded(*expanded)
        }
    }
}

/// A workspace root as a top-level item; a root that no longer exists on disk
/// is shown disabled with a "(missing)" label.
fn to_root_item(root: &FsNode) -> TreeItem {
    if root.path().is_dir() {
        to_tree_item(root)
    } else {
        TreeItem::new(
            root.path().to_string_lossy().into_owned(),
            format!("{} (missing)", root.name()),
        )
        .disabled(true)
    }
}

fn placeholder(parent_id: &str, label: &str) -> TreeItem {
    TreeItem::new(format!("{parent_id}{PLACEHOLDER_SUFFIX}"), label.to_owned()).disabled(true)
}
