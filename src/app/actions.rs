//! Actions, key bindings, and the weak handle menus use to reach [`super::IsengardApp`].
use super::*;

actions!(
    isengard,
    [
        NewFile,
        NewWorkspace,
        OpenFolder,
        OpenFile,
        OpenWorkspace,
        AddFolderToWorkspace,
        SaveWorkspaceAs,
        CloseWorkspace,
        Save,
        SaveAs,
        SaveAll,
        CloseTab,
        CloseAllTabs,
        NextTab,
        PrevTab,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        TogglePreview,
        ToggleSidebar,
        /// Show the project Search sidebar (`⌘⇧F` / `Ctrl+Shift+F`).
        FindInFiles,
        /// Switch the sidebar to the file tree.
        ShowFileTree,
        /// Switch the sidebar to Source Control.
        ShowGit,
        ToggleTerminal,
        NewTerminal,
        /// Show/hide the right-hand Agent panel (`⌘⇧B` / `Ctrl+Shift+B`).
        ToggleAgentPanel,
        /// New agent chat when the Agent panel has focus (`⌘N` / `Ctrl+N`).
        NewAgentChat,
        /// Add the editor selection (or current file) to the Agent prompt
        /// (`⌘L` / `Ctrl+L`).
        AddSelectionToAgent,
        /// Format the active document via LSP (`⇧⌥F` / `Ctrl+Shift+I`).
        FormatDocument,
        /// Rename the symbol under the cursor (F2 while editor Input focused).
        RenameSymbol,
        /// Fired by the terminal panel after tabs change (close / need persist).
        TerminalTabsChanged,
        /// Rename the tree selection (F2 while Tree focused).
        RenameSelectedTreeItem,
        /// Move the tree selection to Trash (Delete/Backspace while Tree focused).
        DeleteSelectedTreeItem,
        About,
        /// Open `config.json` in a tab (create defaults if missing).
        OpenConfigFile,
        Quit,
    ]
);

/// Closes the tab for this path (tab context menu; may not be the active tab).
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct CloseTabAt(pub PathBuf);

/// Closes every tab except the one for this path.
#[derive(Action, Clone, PartialEq, Deserialize)]
#[action(namespace = isengard, no_json)]
pub struct CloseOtherTabs(pub PathBuf);

/// Weak handle to the window content — Kit wraps it in `Root`, so menu
/// fallbacks must not `downcast` the window to [`IsengardApp`].
pub(super) struct ActiveIsengard(pub(super) WeakEntity<IsengardApp>);

impl Global for ActiveIsengard {}

/// Run `f` on the live [`IsengardApp`], preferring the active window.
pub(super) fn with_active_app(
    cx: &mut App,
    f: impl FnOnce(&mut IsengardApp, &mut Window, &mut Context<IsengardApp>),
) {
    let Some(app_entity) = cx
        .try_global::<ActiveIsengard>()
        .and_then(|g| g.0.upgrade())
    else {
        return;
    };
    let Some(window) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    else {
        return;
    };
    let _ = window.update(cx, |_root, window, cx| {
        app_entity.update(cx, |app, cx| {
            // Menu dispatch restores ephemeral sidebar-chrome focus; park on
            // the app handle so Close/etc. always hit a live dispatch path.
            app.focus_handle.focus(window, cx);
            f(app, window, cx);
        });
    });
}

/// Registers global key bindings. `secondary` is Cmd on macOS and Ctrl elsewhere.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        // Agent composer steals ⌘N / Ctrl+N for New Chat (see `Agent` key context).
        KeyBinding::new("secondary-n", NewFile, Some("!Agent")),
        KeyBinding::new("secondary-n", NewAgentChat, Some("Agent")),
        KeyBinding::new("secondary-o", OpenFolder, None),
        KeyBinding::new("secondary-s", Save, None),
        KeyBinding::new("secondary-shift-s", SaveAs, None),
        KeyBinding::new("secondary-alt-s", SaveAll, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("secondary-=", IncreaseFontSize, None),
        KeyBinding::new("secondary--", DecreaseFontSize, None),
        KeyBinding::new("secondary-0", ResetFontSize, None),
        // Yield to terminal paste (`Ctrl+Shift+V` / `⌘⇧V` elsewhere) when focused.
        KeyBinding::new("secondary-shift-v", TogglePreview, Some("!Terminal")),
        KeyBinding::new("secondary-b", ToggleSidebar, None),
        KeyBinding::new("secondary-shift-f", FindInFiles, None),
        KeyBinding::new("ctrl-`", ToggleTerminal, None),
        KeyBinding::new("ctrl-shift-`", NewTerminal, None),
        // Agent panel: `Ctrl+Shift+B` / `⌘⇧B` (Ctrl+Alt+B is unreliable on Linux).
        KeyBinding::new("secondary-shift-b", ToggleAgentPanel, None),
        // Editor / terminal → Agent: selection (editor: or whole file) as a chip.
        KeyBinding::new("secondary-l", AddSelectionToAgent, None),
        // Format Document — VS Code-ish Ctrl+Shift+I / Cmd+Shift+I.
        KeyBinding::new("secondary-shift-i", FormatDocument, None),
        // Symbol rename in the editor; Tree keeps F2 for path rename.
        KeyBinding::new("f2", RenameSymbol, Some("Input")),
        KeyBinding::new("f2", RenameSelectedTreeItem, Some("Tree")),
        KeyBinding::new("delete", DeleteSelectedTreeItem, Some("Tree")),
        KeyBinding::new("backspace", DeleteSelectedTreeItem, Some("Tree")),
        KeyBinding::new("secondary-q", Quit, None),
    ]);
    // App-level fallbacks: title-bar menus restore whatever had focus (tree /
    // git chrome / nothing). That path often misses the Isengard root's
    // `.on_action` handlers — same reason Quit already had a global listener.
    cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
    cx.on_action(|_: &CloseWorkspace, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_close_workspace(&CloseWorkspace, window, cx);
        });
    });
    cx.on_action(|_: &NewFile, cx| {
        with_active_app(cx, |app, window, cx| app.on_new_file(&NewFile, window, cx));
    });
    cx.on_action(|_: &NewWorkspace, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_new_workspace(&NewWorkspace, window, cx);
        });
    });
    cx.on_action(|_: &OpenFolder, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_open_folder(&OpenFolder, window, cx);
        });
    });
    cx.on_action(|_: &OpenFile, cx| {
        with_active_app(cx, |app, window, cx| app.on_open_file(&OpenFile, window, cx));
    });
    cx.on_action(|_: &OpenWorkspace, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_open_workspace(&OpenWorkspace, window, cx);
        });
    });
    cx.on_action(|_: &AddFolderToWorkspace, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_add_folder_to_workspace(&AddFolderToWorkspace, window, cx);
        });
    });
    cx.on_action(|_: &SaveWorkspaceAs, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_save_workspace_as(&SaveWorkspaceAs, window, cx);
        });
    });
    cx.on_action(|_: &Save, cx| {
        with_active_app(cx, |app, window, cx| app.on_save(&Save, window, cx));
    });
    cx.on_action(|_: &SaveAs, cx| {
        with_active_app(cx, |app, window, cx| app.on_save_as(&SaveAs, window, cx));
    });
    cx.on_action(|_: &SaveAll, cx| {
        with_active_app(cx, |app, window, cx| app.on_save_all(&SaveAll, window, cx));
    });
    cx.on_action(|_: &CloseTab, cx| {
        with_active_app(cx, |app, window, cx| app.on_close_tab(&CloseTab, window, cx));
    });
    cx.on_action(|_: &FindInFiles, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_find_in_files(&FindInFiles, window, cx);
        });
    });
    cx.on_action(|_: &ShowFileTree, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_show_file_tree(&ShowFileTree, window, cx);
        });
    });
    cx.on_action(|_: &ShowGit, cx| {
        with_active_app(cx, |app, window, cx| app.on_show_git(&ShowGit, window, cx));
    });
    cx.on_action(|_: &ToggleSidebar, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_toggle_sidebar(&ToggleSidebar, window, cx);
        });
    });
    cx.on_action(|_: &ToggleTerminal, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_toggle_terminal(&ToggleTerminal, window, cx);
        });
    });
    cx.on_action(|_: &NewTerminal, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_new_terminal(&NewTerminal, window, cx);
        });
    });
    cx.on_action(|_: &ToggleAgentPanel, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_toggle_agent_panel(&ToggleAgentPanel, window, cx);
        });
    });
    cx.on_action(|_: &NewAgentChat, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_new_agent_chat(&NewAgentChat, window, cx);
        });
    });
    cx.on_action(|_: &AddSelectionToAgent, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_add_selection_to_agent(&AddSelectionToAgent, window, cx);
        });
    });
    cx.on_action(|_: &FormatDocument, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_format_document(&FormatDocument, window, cx);
        });
    });
    cx.on_action(|_: &RenameSymbol, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_rename_symbol(&RenameSymbol, window, cx);
        });
    });
    cx.on_action(|_: &TogglePreview, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_toggle_preview(&TogglePreview, window, cx);
        });
    });
    cx.on_action(|_: &IncreaseFontSize, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_increase_font(&IncreaseFontSize, window, cx);
        });
    });
    cx.on_action(|_: &DecreaseFontSize, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_decrease_font(&DecreaseFontSize, window, cx);
        });
    });
    cx.on_action(|_: &ResetFontSize, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_reset_font(&ResetFontSize, window, cx);
        });
    });
    cx.on_action(|_: &About, cx| {
        with_active_app(cx, |app, window, cx| app.on_about(&About, window, cx));
    });
    cx.on_action(|_: &OpenConfigFile, cx| {
        with_active_app(cx, |app, window, cx| {
            app.on_open_config_file(&OpenConfigFile, window, cx);
        });
    });
}

