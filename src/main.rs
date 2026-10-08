mod app;
mod branding;
mod config;
mod editor;
mod file_tree;
mod menus;
mod search;
#[allow(dead_code, clippy::type_complexity, clippy::manual_range_contains)]
mod terminal;
mod theme;
mod ui;
mod workspace;

use std::borrow::Cow;
use std::path::PathBuf;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

use crate::app::IsengardApp;
use crate::config::AppConfig;

// Default `Assets` is only ~101 component icons; extras live in the full
// Lucide catalog and must be opted in (not `AllAssets` — ~1 MiB).
gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        FilePlus,
        FolderPlus,
        FolderTree,
        Search,
        CaseSensitive,
        WholeWord,
        Regex,
        Replace,
        ReplaceAll,
    ]
);

struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

fn main() {
    env_logger::init();
    // `isengard <folder>` opens a folder, `isengard <x.isengard-workspace>` a workspace,
    // and `isengard <file>` opens its parent folder and the file.
    let initial_path = std::env::args_os().nth(1).map(PathBuf::from);

    gpui_kit::application()
        .with_assets(AppAssets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            gpui_kit::init(cx); // must come before any component is used
            editor::highlights::init();
            branding::set_dock_icon();
            cx.set_app_identity(branding::APP_ID, "Isengard");
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
                // app_id: Wayland `.desktop` match. icon: X11 `_NET_WM_ICON`
                // (ignored elsewhere; Windows uses .exe embed, macOS Dock API).
                app_id: Some(branding::APP_ID.into()),
                icon: branding::window_icon(),
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
