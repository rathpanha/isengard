use gpui_kit::component::{GlobalState, input, menu::AppMenuBar};
use gpui_kit::{App, Entity, Menu, MenuItem};

use crate::app::{
    About, AddFolderToWorkspace, CloseTab, CloseWorkspace, DecreaseFontSize, FindInFiles,
    IncreaseFontSize, NewFile, NewTerminal, NewWorkspace, OpenFile, OpenFolder, OpenWorkspace, Quit,
    ResetFontSize, Save, SaveAll, SaveWorkspaceAs, ShowFileTree, ShowGit, TogglePreview,
    ToggleSidebar, ToggleTerminal,
};

/// What the window currently has open — drives which menu items exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuContext {
    /// At least one workspace root folder.
    pub has_folder: bool,
    /// At least one editor tab (file or untitled).
    pub has_tabs: bool,
    /// "Close Folder" or "Close Workspace".
    pub close_label: &'static str,
}

impl MenuContext {
    pub const WELCOME: Self = Self {
        has_folder: false,
        has_tabs: false,
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
    let mut menus = vec![
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
            items: file_items(ctx),
            disabled: false,
        },
    ];
    let edit = edit_items(ctx);
    if !edit.is_empty() {
        menus.push(Menu {
            name: "Edit".into(),
            items: edit,
            disabled: false,
        });
    }
    let view = view_items(ctx);
    if !view.is_empty() {
        menus.push(Menu {
            name: "View".into(),
            items: view,
            disabled: false,
        });
    }
    menus
}

fn file_items(ctx: MenuContext) -> Vec<MenuItem> {
    // Welcome Start order: workspace group, then folder/file group.
    let mut sections: Vec<Vec<MenuItem>> = vec![
        vec![
            MenuItem::action("Open Workspace…", OpenWorkspace),
            MenuItem::action("New Workspace…", NewWorkspace),
        ],
        vec![
            MenuItem::action("Open Folder…", OpenFolder),
            MenuItem::action("Open File…", OpenFile),
            MenuItem::action("New File", NewFile),
        ],
    ];

    if ctx.has_folder {
        sections.push(vec![
            MenuItem::action("Add Folder to Workspace…", AddFolderToWorkspace),
            MenuItem::action("Save Workspace As…", SaveWorkspaceAs),
        ]);
    }

    if ctx.has_tabs {
        sections.push(vec![
            MenuItem::action("Save", Save),
            MenuItem::action("Save All", SaveAll),
        ]);
    }

    let mut close = Vec::new();
    if ctx.has_tabs {
        close.push(MenuItem::action("Close Tab", CloseTab));
    }
    if ctx.has_folder {
        close.push(MenuItem::action(ctx.close_label, CloseWorkspace));
    }
    if !close.is_empty() {
        sections.push(close);
    }

    join_sections(sections)
}

fn edit_items(ctx: MenuContext) -> Vec<MenuItem> {
    let mut sections: Vec<Vec<MenuItem>> = Vec::new();

    if ctx.has_tabs {
        sections.push(vec![
            MenuItem::action("Undo", input::Undo),
            MenuItem::action("Redo", input::Redo),
            MenuItem::separator(),
            MenuItem::action("Cut", input::Cut),
            MenuItem::action("Copy", input::Copy),
            MenuItem::action("Paste", input::Paste),
            MenuItem::separator(),
            MenuItem::action("Find", input::Search),
            MenuItem::action("Select All", input::SelectAll),
        ]);
    }

    if ctx.has_folder {
        sections.push(vec![MenuItem::action("Find in Files", FindInFiles)]);
    }

    // Empty Edit on pure welcome is fine — File still has Open / New.
    join_sections(sections)
}

fn view_items(ctx: MenuContext) -> Vec<MenuItem> {
    let mut sections: Vec<Vec<MenuItem>> = Vec::new();

    if ctx.has_tabs {
        sections.push(vec![
            MenuItem::action("Toggle Preview", TogglePreview),
            MenuItem::action("Increase Font Size", IncreaseFontSize),
            MenuItem::action("Decrease Font Size", DecreaseFontSize),
            MenuItem::action("Reset Font Size", ResetFontSize),
        ]);
    }

    if ctx.has_folder {
        sections.push(vec![
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Explorer", ShowFileTree),
            MenuItem::action("Search", FindInFiles),
            MenuItem::action("Source Control", ShowGit),
            MenuItem::action("Toggle Terminal", ToggleTerminal),
            MenuItem::action("New Terminal", NewTerminal),
        ]);
    }

    let mut items = join_sections(sections);
    if !items.is_empty() {
        // Trailing separator so macOS-injected "Enter Full Screen" sits alone.
        items.push(MenuItem::separator());
    }
    items
}

fn join_sections(sections: Vec<Vec<MenuItem>>) -> Vec<MenuItem> {
    let mut out = Vec::new();
    for section in sections.into_iter().filter(|s| !s.is_empty()) {
        if !out.is_empty() {
            out.push(MenuItem::separator());
        }
        out.extend(section);
    }
    out
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
    fn welcome_omits_workspace_and_panel_actions() {
        let file_items = file_items(MenuContext::WELCOME);
        let file = labels(&file_items);
        assert!(!file.iter().any(|l| l.contains("Add Folder")));
        assert!(!file.iter().any(|l| l.contains("Save Workspace")));
        assert!(!file.iter().any(|l| *l == "Close Folder" || *l == "Close Workspace"));
        assert!(file.contains(&"Open Folder…"));

        let edit_items = edit_items(MenuContext::WELCOME);
        assert!(labels(&edit_items).is_empty());

        let view_items = view_items(MenuContext::WELCOME);
        assert!(labels(&view_items).is_empty());
    }

    #[test]
    fn folder_shows_add_folder_and_close_folder() {
        let ctx = MenuContext {
            has_folder: true,
            has_tabs: true,
            close_label: "Close Folder",
        };
        let file_items = file_items(ctx);
        let file = labels(&file_items);
        assert!(file.iter().any(|l| l.contains("Add Folder")));
        assert!(file.contains(&"Close Folder"));
        assert!(file.contains(&"Save"));

        let edit_items = edit_items(ctx);
        let edit = labels(&edit_items);
        assert!(edit.contains(&"Find in Files"));
        assert!(edit.contains(&"Find"));

        let view_items = view_items(ctx);
        let view = labels(&view_items);
        assert!(view.contains(&"Toggle Terminal"));
        assert!(view.contains(&"Source Control"));
    }

    #[test]
    fn untitled_on_welcome_gets_save_not_add_folder() {
        let ctx = MenuContext {
            has_folder: false,
            has_tabs: true,
            close_label: "Close Folder",
        };
        let file_items = file_items(ctx);
        let file = labels(&file_items);
        assert!(file.contains(&"Save"));
        assert!(file.contains(&"Close Tab"));
        assert!(!file.iter().any(|l| l.contains("Add Folder")));
        assert!(!file.contains(&"Close Folder"));
    }
}
