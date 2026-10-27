use gpui_kit::component::{GlobalState, menu::AppMenuBar};
use gpui_kit::{App, Entity, Menu, MenuItem};

use crate::app::{About, AddFolderToWorkspace, CheckForUpdates, CloseWorkspace, SaveWorkspaceAs};

/// What the window currently has open — drives which menu items exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuContext {
    /// At least one workspace root folder.
    pub has_folder: bool,
    /// "Close Folder" or "Close Workspace".
    pub close_label: &'static str,
}

impl MenuContext {
    pub const WELCOME: Self = Self {
        has_folder: false,
        close_label: "Close Folder",
    };
}

/// Installs the native menu bar (macOS) and returns the in-window menu bar
/// rendered in the title bar on Windows and Linux.
pub fn init(cx: &mut App) -> Entity<AppMenuBar> {
    apply(cx, MenuContext::WELCOME);
    let app_menu_bar = AppMenuBar::new(cx);
    app_menu_bar.update(cx, |bar, cx| bar.reload(cx));
    app_menu_bar
}

/// Rebuild native + in-window menus for the current editor state.
pub fn update(cx: &mut App, bar: &Entity<AppMenuBar>, ctx: MenuContext) {
    apply(cx, ctx);
    bar.update(cx, |bar, cx| bar.reload(cx));
}

fn apply(cx: &mut App, ctx: MenuContext) {
    cx.set_menus(build_menus(ctx));
    let menus = build_menus(ctx)
        .into_iter()
        .map(|menu| menu.owned())
        .collect();
    GlobalState::global_mut(cx).set_app_menus(menus);
}

fn build_menus(ctx: MenuContext) -> Vec<Menu> {
    let mut menus = vec![Menu {
        name: "Isengard".into(),
        items: vec![
            MenuItem::action("About Isengard", About),
            MenuItem::action("Check for Updates…", CheckForUpdates),
        ],
        disabled: false,
    }];
    let file = file_items(ctx);
    if !file.is_empty() {
        menus.push(Menu {
            name: "File".into(),
            items: file,
            disabled: false,
        });
    }
    menus
}

fn file_items(ctx: MenuContext) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if ctx.has_folder {
        items.push(MenuItem::action("Add Folder to Workspace…", AddFolderToWorkspace));
        items.push(MenuItem::action("Save Workspace As…", SaveWorkspaceAs));
        items.push(MenuItem::separator());
        items.push(MenuItem::action(ctx.close_label, CloseWorkspace));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[MenuItem]) -> Vec<&str> {
        items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.as_ref()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn welcome_has_no_file_menu_items() {
        assert!(file_items(MenuContext::WELCOME).is_empty());
    }

    #[test]
    fn folder_shows_only_workspace_items() {
        let ctx = MenuContext {
            has_folder: true,
            close_label: "Close Folder",
        };
        let items = file_items(ctx);
        assert_eq!(
            labels(&items),
            ["Add Folder to Workspace…", "Save Workspace As…", "Close Folder"]
        );
    }
}
