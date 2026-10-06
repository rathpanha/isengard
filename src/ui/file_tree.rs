use std::path::{Path, PathBuf};

use egui::{CollapsingHeader, RichText, Ui};

use crate::file_tree::FsNode;

#[derive(Default)]
pub struct FileTreePanel {
    root: Option<FsNode>,
    selected: Option<PathBuf>,
}

impl FileTreePanel {
    pub fn open_folder(&mut self, path: &Path) {
        let mut root = FsNode::from_path(path);
        root.toggle_expand();
        self.root = Some(root);
        self.selected = None;
    }

    pub fn close(&mut self) {
        self.root = None;
        self.selected = None;
    }

    pub fn is_open(&self) -> bool {
        self.root.is_some()
    }

    pub fn root_path(&self) -> Option<&Path> {
        self.root.as_ref().map(FsNode::path)
    }

    pub fn set_selected(&mut self, path: Option<PathBuf>) {
        self.selected = path;
    }

    /// Renders the tree. Returns the path of a file the user clicked this frame.
    pub fn show(&mut self, ui: &mut Ui) -> Option<PathBuf> {
        let root = self.root.as_mut()?;

        ui.add_space(6.0);
        ui.label(RichText::new(root.name().to_uppercase()).small().strong())
            .on_hover_text(root.path().display().to_string());
        ui.add_space(4.0);

        let mut clicked = None;
        let selected = self.selected.as_deref();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                root.ensure_loaded();
                for child in root.children_mut().into_iter().flatten() {
                    show_node(ui, child, selected, &mut clicked);
                }
            });
        if let Some(path) = &clicked {
            self.selected = Some(path.clone());
        }
        clicked
    }
}

fn show_node(ui: &mut Ui, node: &mut FsNode, selected: Option<&Path>, clicked: &mut Option<PathBuf>) {
    match node {
        FsNode::File { path, name } => {
            let is_selected = selected == Some(path.as_path());
            if ui.selectable_label(is_selected, name.as_str()).clicked() {
                *clicked = Some(path.clone());
            }
        }
        FsNode::Dir { .. } => {
            let header = CollapsingHeader::new(RichText::new(node.name()).strong())
                .id_salt(node.path())
                .open(Some(node.is_expanded()))
                .show(ui, |ui| {
                    if let Some(children) = node.children_mut() {
                        if children.is_empty() {
                            ui.weak("(empty)");
                        }
                        for child in children {
                            show_node(ui, child, selected, clicked);
                        }
                    }
                });
            if header.header_response.clicked() {
                node.toggle_expand();
            }
        }
    }
}
