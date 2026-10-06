mod app;
mod branding;
mod config;
mod editor;
mod file_tree;
mod menus;
mod theme;
mod ui;
mod workspace;

use std::path::PathBuf;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

use crate::app::IsengardApp;
use crate::config::AppConfig;

fn main() {
    env_logger::init();
    // `isengard <folder>` opens a folder, `isengard <x.isengard-workspace>` a workspace,
    // and `isengard <file>` opens its parent folder and the file.
    let initial_path = std::env::args_os().nth(1).map(PathBuf::from);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            gpui_kit::init(cx); // must come before any component is used
            branding::set_dock_icon();
            theme::load_fonts(cx);
            let config = AppConfig::load();
            theme::apply(&config, None, cx);
            app::init(cx);
            let app_menu_bar = menus::init(cx);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.), px(800.)),
                    cx,
                ))),
                window_min_size: Some(size(px(640.), px(400.))),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title("Isengard");
                cx.new(|cx| IsengardApp::new(config, initial_path, app_menu_bar, window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
