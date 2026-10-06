use std::path::{Path, PathBuf};

use egui::{Id, RichText, Ui};

use super::code_editor::{self, HighlightCache};
use crate::editor::buffer::TextBuffer;

pub struct EditorTab {
    pub buffer: TextBuffer,
    cache: HighlightCache,
    pub cursor: Option<(usize, usize)>,
}

impl EditorTab {
    fn editor_id(&self) -> Id {
        Id::new(("code_editor", &self.buffer.path))
    }

    fn label(&self) -> String {
        if self.buffer.is_modified {
            format!("{} •", self.buffer.file_name())
        } else {
            self.buffer.file_name()
        }
    }
}

#[derive(Default)]
pub struct EditorWorkspace {
    tabs: Vec<EditorTab>,
    active: usize,
    /// Tab awaiting confirmation because it has unsaved changes.
    pending_close: Option<usize>,
    focus_editor: bool,
}

impl EditorWorkspace {
    /// Opens `path` in a new tab, or activates its existing tab.
    pub fn open_file(&mut self, path: &Path) -> anyhow::Result<()> {
        if let Some(idx) = self.tabs.iter().position(|t| t.buffer.path == path) {
            self.activate(idx);
            return Ok(());
        }
        let buffer = TextBuffer::open(path)?;
        self.tabs.push(EditorTab {
            buffer,
            cache: HighlightCache::default(),
            cursor: None,
        });
        self.activate(self.tabs.len() - 1);
        Ok(())
    }

    fn activate(&mut self, idx: usize) {
        self.active = idx;
        self.focus_editor = true;
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_tab(&self) -> Option<&EditorTab> {
        self.tabs.get(self.active)
    }

    pub fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.buffer.is_modified)
    }

    pub fn is_dialog_open(&self) -> bool {
        self.pending_close.is_some()
    }

    /// Saves the active tab. Returns the saved path, or `None` if no tab is open.
    pub fn save_active(&mut self) -> anyhow::Result<Option<PathBuf>> {
        match self.tabs.get_mut(self.active) {
            Some(tab) => {
                tab.buffer.save()?;
                Ok(Some(tab.buffer.path.clone()))
            }
            None => Ok(None),
        }
    }

    pub fn save_all(&mut self) -> anyhow::Result<()> {
        for tab in self.tabs.iter_mut().filter(|t| t.buffer.is_modified) {
            tab.buffer.save()?;
        }
        Ok(())
    }

    /// Closes tab `idx`, asking for confirmation first if it has unsaved changes.
    pub fn request_close(&mut self, idx: usize) {
        match self.tabs.get(idx) {
            Some(tab) if tab.buffer.is_modified => self.pending_close = Some(idx),
            Some(_) => self.close(idx),
            None => {}
        }
    }

    pub fn request_close_active(&mut self) {
        self.request_close(self.active);
    }

    fn close(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        if idx < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.focus_editor = true;
    }

    pub fn cycle_tab(&mut self, forward: bool) {
        let n = self.tabs.len();
        if n > 1 {
            let next = if forward {
                (self.active + 1) % n
            } else {
                (self.active + n - 1) % n
            };
            self.activate(next);
        }
    }

    /// Renders the tab bar and the active editor.
    pub fn show(&mut self, ui: &mut Ui, font_size: f32) {
        if !self.tabs.is_empty() {
            self.show_tab_bar(ui);
            ui.separator();
        }

        // The tab bar may have just closed the last tab, so re-check before indexing.
        let Some(tab) = self.tabs.get_mut(self.active) else {
            ui.centered_and_justified(|ui| {
                ui.weak("Select a file to start editing");
            });
            return;
        };
        let id = tab.editor_id();
        if std::mem::take(&mut self.focus_editor) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let output = code_editor::show(ui, id, &mut tab.buffer, &mut tab.cache, font_size);
        if output.cursor.is_some() {
            tab.cursor = output.cursor;
        }
    }

    fn show_tab_bar(&mut self, ui: &mut Ui) {
        let mut activate = None;
        let mut close = None;
        egui::ScrollArea::horizontal()
            .id_salt("tab_bar")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (idx, tab) in self.tabs.iter().enumerate() {
                        let is_active = idx == self.active;
                        let label = ui
                            .selectable_label(is_active, tab.label())
                            .on_hover_text(tab.buffer.path.display().to_string());
                        if label.clicked() {
                            activate = Some(idx);
                        }
                        if label.middle_clicked() {
                            close = Some(idx);
                        }
                        if ui.small_button(RichText::new("×").weak()).on_hover_text("Close").clicked() {
                            close = Some(idx);
                        }
                        ui.add_space(6.0);
                    }
                });
            });
        if let Some(idx) = activate {
            self.activate(idx);
        }
        if let Some(idx) = close {
            self.request_close(idx);
        }
    }

    /// Shows the "unsaved changes" confirmation for a pending tab close.
    pub fn show_close_dialog(&mut self, ctx: &egui::Context) -> Option<String> {
        let idx = self.pending_close?;
        let Some(tab) = self.tabs.get(idx) else {
            self.pending_close = None;
            return None;
        };
        let name = tab.buffer.file_name();

        let mut choice = None;
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("Save changes to {name} before closing?"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(CloseChoice::Save);
                    }
                    if ui.button("Don't Save").clicked() {
                        choice = Some(CloseChoice::Discard);
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        choice = Some(CloseChoice::Cancel);
                    }
                });
            });

        let mut error = None;
        match choice? {
            CloseChoice::Save => match self.tabs[idx].buffer.save() {
                Ok(()) => self.close(idx),
                Err(err) => error = Some(format!("{err:#}")),
            },
            CloseChoice::Discard => self.close(idx),
            CloseChoice::Cancel => self.focus_editor = true,
        }
        self.pending_close = None;
        error
    }
}

enum CloseChoice {
    Save,
    Discard,
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_with(files: &[&str]) -> (tempfile::TempDir, EditorWorkspace) {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = EditorWorkspace::default();
        for f in files {
            let p = tmp.path().join(f);
            std::fs::write(&p, "x").unwrap();
            ws.open_file(&p).unwrap();
        }
        (tmp, ws)
    }

    #[test]
    fn reopening_activates_existing_tab() {
        let (tmp, mut ws) = workspace_with(&["a.js", "b.js"]);
        ws.open_file(&tmp.path().join("a.js")).unwrap();
        assert_eq!(ws.tabs.len(), 2);
        assert_eq!(ws.active, 0);
    }

    #[test]
    fn cycle_and_close() {
        let (_tmp, mut ws) = workspace_with(&["a.js", "b.js", "c.js"]);
        assert_eq!(ws.active, 2);
        ws.cycle_tab(true);
        assert_eq!(ws.active, 0);
        ws.cycle_tab(false);
        assert_eq!(ws.active, 2);

        ws.request_close(0);
        assert_eq!(ws.tabs.len(), 2);
        assert_eq!(ws.active_tab().unwrap().buffer.file_name(), "c.js");
        ws.request_close_active();
        assert_eq!(ws.active_tab().unwrap().buffer.file_name(), "b.js");
    }

    #[test]
    fn modified_tab_requires_confirmation() {
        let (_tmp, mut ws) = workspace_with(&["a.js"]);
        ws.tabs[0].buffer.is_modified = true;
        ws.request_close_active();
        assert_eq!(ws.tabs.len(), 1);
        assert!(ws.is_dialog_open());
    }
}
