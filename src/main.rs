mod app;
mod config;
mod editor;
mod file_tree;
mod ui;

fn main() -> eframe::Result<()> {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Isengard")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Isengard",
        options,
        Box::new(|cc| Ok(Box::new(app::IsengardApp::new(cc)))),
    )
}
