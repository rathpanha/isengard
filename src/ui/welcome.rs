use std::path::{Path, PathBuf};

use egui::{Align, Layout, Rect, RichText, Ui, UiBuilder, Vec2};

pub enum WelcomeAction {
    OpenFolder,
    OpenFile,
    OpenRecent(PathBuf),
    RemoveRecent(PathBuf),
}

const COLUMN_WIDTH: f32 = 480.0;

/// Start page shown while no folder is open: start actions plus recent folders.
pub fn show(ui: &mut Ui, recent: &[PathBuf], open_folder_shortcut: &str) -> Option<WelcomeAction> {
    let mut action = None;

    let full = ui.max_rect();
    let width = full.width().min(COLUMN_WIDTH);
    let top = full.top() + full.height() * 0.15;
    let column = Rect::from_min_size(
        egui::pos2(full.center().x - width / 2.0, top),
        Vec2::new(width, full.bottom() - top),
    );

    ui.allocate_new_ui(UiBuilder::new().max_rect(column), |ui| {
        ui.label(RichText::new("Isengard").size(34.0).strong());
        ui.label(RichText::new("Code editor").size(15.0).weak());
        ui.add_space(32.0);

        section_title(ui, "Start");
        ui.horizontal(|ui| {
            if ui.link("Open Folder…").clicked() {
                action = Some(WelcomeAction::OpenFolder);
            }
            ui.weak(open_folder_shortcut);
        });
        if ui.link("Open File…").clicked() {
            action = Some(WelcomeAction::OpenFile);
        }
        ui.add_space(28.0);

        section_title(ui, "Recent");
        if recent.is_empty() {
            ui.weak("No recent folders");
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            for folder in recent {
                ui.horizontal(|ui| {
                    let name = folder
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| folder.display().to_string());
                    if ui.link(name).on_hover_text(folder.display().to_string()).clicked() {
                        action = Some(WelcomeAction::OpenRecent(folder.clone()));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button(RichText::new("×").weak())
                            .on_hover_text("Remove from recent")
                            .clicked()
                        {
                            action = Some(WelcomeAction::RemoveRecent(folder.clone()));
                        }
                        let parent = folder.parent().map(abbreviate_home).unwrap_or_default();
                        ui.add(egui::Label::new(RichText::new(parent).weak()).truncate());
                    });
                });
            }
        });
    });

    action
}

fn section_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(16.0).strong());
    ui.add_space(4.0);
}

/// Shows paths under the home directory as `~/...`.
fn abbreviate_home(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if let Ok(rest) = path.strip_prefix(&home) {
            return Path::new("~").join(rest).display().to_string();
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviates_home_directory() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(abbreviate_home(&home.join("code/app")), "~/code/app");
        assert_eq!(abbreviate_home(Path::new("/tmp/x")), "/tmp/x");
    }
}
