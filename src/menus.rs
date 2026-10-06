use gpui_kit::component::{GlobalState, input, menu::AppMenuBar};
use gpui_kit::{App, Entity, Menu, MenuItem};

use crate::app::{
    About, AddFolderToWorkspace, CloseTab, CloseWorkspace, DecreaseFontSize, IncreaseFontSize,
    OpenFile, OpenFolder, OpenWorkspace, Quit, ResetFontSize, Save, SaveAll, SaveWorkspaceAs,
    TogglePreview,
};

/// Installs the native menu bar (macOS) and returns the in-window menu bar
/// rendered in the title bar on Windows and Linux.
pub fn init(cx: &mut App) -> Entity<AppMenuBar> {
    cx.set_menus(build_menus());
    let menus = build_menus().into_iter().map(|menu| menu.owned()).collect();
    GlobalState::global_mut(cx).set_app_menus(menus);

    let app_menu_bar = AppMenuBar::new(cx);
    app_menu_bar.update(cx, |bar, cx| bar.reload(cx));
    app_menu_bar
}

fn build_menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Isengard".into(),
            items: vec![
                MenuItem::action("About Isengard", About),
                MenuItem::separator(),
                MenuItem::action("Quit Isengard", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Open Workspace…", OpenWorkspace),
                MenuItem::action("Open Folder…", OpenFolder),
                MenuItem::action("Open File…", OpenFile),
                MenuItem::separator(),
                MenuItem::action("Add Folder to Workspace…", AddFolderToWorkspace),
                MenuItem::action("Save Workspace As…", SaveWorkspaceAs),
                MenuItem::separator(),
                MenuItem::action("Save", Save),
                MenuItem::action("Save All", SaveAll),
                MenuItem::separator(),
                MenuItem::action("Close Tab", CloseTab),
                MenuItem::action("Close Workspace", CloseWorkspace),
            ],
            disabled: false,
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::action("Undo", input::Undo),
                MenuItem::action("Redo", input::Redo),
                MenuItem::separator(),
                MenuItem::action("Cut", input::Cut),
                MenuItem::action("Copy", input::Copy),
                MenuItem::action("Paste", input::Paste),
                MenuItem::separator(),
                MenuItem::action("Find", input::Search),
                MenuItem::action("Select All", input::SelectAll),
            ],
            disabled: false,
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Toggle Preview", TogglePreview),
                MenuItem::separator(),
                MenuItem::action("Increase Font Size", IncreaseFontSize),
                MenuItem::action("Decrease Font Size", DecreaseFontSize),
                MenuItem::action("Reset Font Size", ResetFontSize),
                // Trailing separator so macOS-injected "Enter Full Screen" sits alone.
                MenuItem::separator(),
            ],
            disabled: false,
        },
    ]
}
