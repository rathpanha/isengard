use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme as _, h_flex,
    list::ListItem,
    tree::{TreeEvent, TreeItem, TreeState, tree},
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::app::RemoveWorkspaceFolder;
use crate::file_tree::{FsNode, tree_icon};

/// Separates a directory path from the suffix of its placeholder child's id.
const PLACEHOLDER_SUFFIX: &str = "\u{0}placeholder";

/// The workspace's lazily-loaded folder tree, rendered with GPUI Kit's `Tree`.
///
/// `FsNode`s are the source of truth; they are converted to `TreeItem`s whenever
/// the roots change or a directory is loaded for the first time. With one root
/// its children are the top level (a plain folder); with several, each root is
/// a top-level item (a multi-root workspace).
pub struct FileTreePanel {
    roots: Vec<FsNode>,
    state: Entity<TreeState>,
}

impl FileTreePanel {
    pub fn new(cx: &mut App) -> Self {
        Self {
            roots: Vec::new(),
            state: cx.new(|cx| TreeState::new(cx)),
        }
    }

    pub fn state(&self) -> &Entity<TreeState> {
        &self.state
    }

    pub fn is_open(&self) -> bool {
        !self.roots.is_empty()
    }

    /// Replaces the roots, keeping the loaded/expanded state of roots that remain.
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

    /// Renders the header (`title`, upper-cased) and the tree. `on_open_file` is
    /// called when a file is clicked. In a multi-root workspace, right-clicking a
    /// root offers "Remove Folder from Workspace".
    pub fn render(
        &self,
        title: &str,
        on_open_file: impl Fn(PathBuf, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let name = title.to_uppercase();
        let multi_root = self.roots.len() > 1;
        let on_open_file = Rc::new(on_open_file);

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(name),
            )
            .child(
                tree(&self.state, move |ix, entry, _selected, _window, cx| {
                    let item = entry.item();
                    let is_placeholder = item.id.ends_with(PLACEHOLDER_SUFFIX);
                    let path = PathBuf::from(item.id.as_ref());
                    let is_missing_root = entry.depth() == 0 && entry.is_disabled();
                    let is_dir = entry.is_folder() || is_missing_root;
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
                    let is_file = !entry.is_folder() && !is_placeholder;

                    ListItem::new(ix)
                        .w_full()
                        .when(!entry.is_disabled(), |item| item.cursor_pointer())
                        .rounded(cx.theme().radius)
                        .py_0p5()
                        .px_2()
                        .pl(px(16.) * entry.depth() + px(8.))
                        .child(
                            h_flex()
                                .gap_2()
                                .children(icon)
                                .child(item.label.clone())
                                .when(is_placeholder, |this| {
                                    this.text_color(cx.theme().muted_foreground)
                                }),
                        )
                        .on_click(move |_, window, cx| {
                            if is_file {
                                on_open_file(path.clone(), window, cx);
                            }
                        })
                })
                .when(multi_root, |tree| {
                    tree.context_menu(|_, entry, menu, _, _| {
                        if entry.depth() > 0 {
                            return menu;
                        }
                        let root = PathBuf::from(entry.item().id.as_ref());
                        menu.menu(
                            "Remove Folder from Workspace",
                            Box::new(RemoveWorkspaceFolder(root)),
                        )
                    })
                })
                .flex_1()
                .text_sm()
                .px_1(),
            )
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
