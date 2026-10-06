use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme as _, IconName, h_flex,
    list::ListItem,
    tree::{TreeEvent, TreeItem, TreeState, tree},
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::file_tree::FsNode;

/// Separates a directory path from the suffix of its placeholder child's id.
const PLACEHOLDER_SUFFIX: &str = "\u{0}placeholder";

/// The open folder's lazily-loaded tree, rendered with GPUI Kit's `Tree`.
///
/// `FsNode` is the source of truth; it is converted to `TreeItem`s whenever a
/// directory is loaded for the first time.
pub struct FileTreePanel {
    root: Option<FsNode>,
    state: Entity<TreeState>,
}

impl FileTreePanel {
    pub fn new(cx: &mut App) -> Self {
        Self {
            root: None,
            state: cx.new(|cx| TreeState::new(cx)),
        }
    }

    pub fn state(&self) -> &Entity<TreeState> {
        &self.state
    }

    pub fn is_open(&self) -> bool {
        self.root.is_some()
    }

    pub fn root_path(&self) -> Option<&Path> {
        self.root.as_ref().map(FsNode::path)
    }

    pub fn open_folder(&mut self, path: &Path, cx: &mut App) {
        let mut root = FsNode::from_path(path);
        root.set_expanded(true);
        self.root = Some(root);
        self.sync(cx);
    }

    pub fn close(&mut self, cx: &mut App) {
        self.root = None;
        self.state
            .update(cx, |state, cx| state.set_items(Vec::new(), cx));
    }

    /// Keeps `FsNode` in step with the tree's expand/collapse events.
    pub fn handle_event(&mut self, event: &TreeEvent, cx: &mut App) {
        let (id, expanded) = match event {
            TreeEvent::Expanded(id) => (id, true),
            TreeEvent::Collapsed(id) => (id, false),
        };
        let Some(node) = self
            .root
            .as_mut()
            .and_then(|r| r.find_mut(Path::new(id.as_ref())))
        else {
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
        let items: Vec<TreeItem> = self
            .root
            .as_ref()
            .and_then(FsNode::children)
            .unwrap_or_default()
            .iter()
            .map(to_tree_item)
            .collect();
        self.state.update(cx, |state, cx| {
            let selected = state.selected_item().map(|item| item.id.clone());
            state.set_items(items, cx);
            if let Some(id) = selected {
                let ix = state.index_of(&id);
                state.set_selected_index(ix, cx);
            }
        });
    }

    /// Renders the folder header and tree. `on_open_file` is called when a file is clicked.
    pub fn render(
        &self,
        on_open_file: impl Fn(PathBuf, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let name = self
            .root
            .as_ref()
            .map(|r| r.name().to_uppercase())
            .unwrap_or_default();
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
                    let icon = if is_placeholder {
                        None
                    } else if !entry.is_folder() {
                        Some(IconName::File)
                    } else if entry.is_expanded() {
                        Some(IconName::FolderOpen)
                    } else {
                        Some(IconName::Folder)
                    };
                    let on_open_file = on_open_file.clone();
                    let path = PathBuf::from(item.id.as_ref());
                    let is_file = !entry.is_folder() && !is_placeholder;

                    ListItem::new(ix)
                        .w_full()
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

fn placeholder(parent_id: &str, label: &str) -> TreeItem {
    TreeItem::new(format!("{parent_id}{PLACEHOLDER_SUFFIX}"), label.to_owned()).disabled(true)
}
