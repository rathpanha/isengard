# Isengard — Agent Context

Isengard is a cross-platform desktop code editor written in Rust on
**GPUI Kit** (`gpui-kit` crate — formerly "gpui-component", by Longbridge,
built on Zed's GPUI). This file is the hand-off document for AI agents: read it
fully before making changes.

> **Rule for every agent:** any change to code, behavior, dependencies, or plans
> must also update this file in the same commit — at minimum the
> [Current status](#current-status), [Known issues](#known-issues--todo), and
> [Changelog](#changelog) sections, plus any section the change makes stale.
> Keep it accurate; a stale hand-off doc is worse than none.

> **UI rule (user decision):** the app uses GPUI Kit components only — do not
> hand-build widgets that the library already provides. Look for a component
> first (catalog below), and only compose/style when none fits.

## Quick start

Rust is installed via Homebrew's keg-only `rustup`; `cargo` is not on the
default PATH:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"

cargo run                       # dev build, opens the welcome screen
cargo run -- /path/to/folder    # open a folder at launch
cargo run -- /path/to/file.js   # open a file (and its parent folder)
cargo test                      # unit tests (all in #[cfg(test)] modules)
cargo clippy --all-targets      # must be warning-free
cargo build --release
```

The first build compiles GPUI and takes several minutes; later builds are fast.
The `block v0.1.6` future-incompat notice comes from a dependency — ignore it.

Settings live in `~/Library/Application Support/isengard/config.json` on
macOS (`dirs::config_dir()/isengard/config.json` elsewhere). Delete it to reset.

## Current status

Phases 1–4 of the original plan are done and the UI has been ported from egui
to GPUI Kit. Phase 5 (Claude AI panel) has not started.

Working features (verified by screenshots on macOS: welcome screen, file tree,
tabs, highlighted editor, status bar):
- Welcome screen on every launch (unless a path is passed on the CLI): Open
  Folder, Open File, recent folders (max 8, click to open, × to remove).
- File tree (GPUI Kit `Tree`) only while a folder is open; lazy directory
  loading; dirs first, case-insensitive sort; hides `.git` and `.DS_Store`.
- Tabs (`TabBar`/`Tab` with a close button), one `EditorState` per tab so each
  keeps its own undo history and cursor. The close button is a muted × that turns
  destructive on hover (theme `danger` icon + faint danger tint), and sits 12px
  from the tab's right border — the same as the label's left padding.
- Code editor (GPUI Kit `Editor`): tree-sitter highlighting for JavaScript,
  TypeScript, TSX, HTML, CSS, JSON; line numbers, indent guides, search.
- Save (`•` marks modified tabs), Save All, close-tab confirmation (Save /
  Don't Save / Cancel), quit confirmation when anything is unsaved (window
  close button, Cmd/Ctrl+Q, menu).
- Native macOS menu bar; in-window `AppMenuBar` in the title bar on
  Windows/Linux. Menus: Isengard (About, Quit), File, Edit, View.
- Status bar: relative path, Ln/Col, language — rendered only while a file
  tab is active (hidden on the welcome screen and with a folder but no file).
  Notifications for saves/errors.
- JetBrains Mono Nerd Font everywhere (UI + editor); square corners
  (`theme.radius = 0`); dark/light theme toggle; editor font size zoom.

Not yet verified interactively (no clicking was possible during the port):
tree expand/collapse, clicking files, tab switching/closing, dialogs,
keyboard shortcuts, theme toggle. Check these first if something is off.

## Architecture

```
src/
├── main.rs              app bootstrap: gpui_kit::init, fonts, theme, key bindings,
│                        menus, window (QuitMode::LastWindowClosed)
├── app.rs               IsengardApp view: actions, tabs, dialogs, layout, status bar
├── theme.rs             bundled font loading + Theme overrides (font, radius 0, sizes)
├── menus.rs             native menus + AppMenuBar (Windows/Linux)
├── config.rs            AppConfig (serde JSON file) + recent-folder helpers
├── editor/
│   ├── document.rs      read_text (UTF-8 only) / write_text / file_name
│   ├── language.rs      Language enum: from_path, highlighter_name, display_name
│   └── tabs.rs          TabList<T>: pure tab ordering/activation logic (unit-tested)
├── file_tree/node.rs    FsNode (File/Dir), lazy load_one_level, set_expanded, find_mut
└── ui/
    ├── file_tree.rs     FileTreePanel: FsNode -> TreeItem sync, TreeEvent handling, render
    └── welcome.rs       welcome screen (start actions + recent folders)
```

### How it fits together
- `gpui_kit::open_window` wraps `IsengardApp` in a `Root` (needed for dialogs,
  notifications, tooltips, menus). Never add a second Root.
- **Actions**: declared with `actions!(isengard, [...])` in `app.rs`, bound in
  `app::init` (`secondary-*` = Cmd on macOS, Ctrl elsewhere), handled by
  `.on_action(cx.listener(..))` on the root div, which `track_focus`es
  `IsengardApp::focus_handle`. Actions only reach the handlers if focus is
  inside the view — `focus_active_editor` refocuses the editor or the root.
  A global `Quit` fallback in `app::init` quits without checking (used when a
  dialog holds focus).
- **Tabs**: `TabList<EditorTab>`; each `EditorTab` owns an `Entity<EditorState>`
  plus two subscriptions: `InputEvent::Change` → mark modified, and `observe` →
  re-render (keeps Ln/Col current).
- **File tree**: `FsNode` is the source of truth. Unloaded/empty directories get
  a disabled placeholder child (`"Loading…"` / `"(empty)"`) because GPUI Kit's
  `TreeItem::is_folder()` is just "has children". On `TreeEvent::Expanded` the
  node loads its children and the `TreeItem`s are rebuilt via `set_items`
  (selection is restored by id). Tree item ids are absolute paths.
- **Dialogs**: `window.open_dialog` with a `DialogFooter` of `Button`s; buttons
  capture a `WeakEntity<IsengardApp>` and call back into it.
- **Hover-colored icons**: `Button`'s hover style only changes its background,
  and `Icon` resolves its color at render time, so `group_hover`/`.hover` can't
  recolor an icon. Track hover state in the view instead (see
  `IsengardApp::hovered_close`, set from a wrapper div's `on_hover`).
- **Theme**: `theme::apply` calls `Theme::change(mode)` first (it reloads the
  theme config) and then overrides fonts, sizes and `radius`/`radius_lg = 0`.
  Re-run it after any theme/font change. `theme.font_size` (14px) is the rem
  base for the whole UI; `mono_font_size` is the editor size from config.
- **Fonts**: five JetBrains Mono Nerd Font weights are embedded with
  `include_bytes!` (~13 MB) and registered via `cx.text_system().add_fonts`.
  Family name: `"JetBrainsMono Nerd Font"`. Licenses: `assets/fonts/OFL.txt`,
  `assets/fonts/LICENSE-NerdFonts.txt`.

### Conventions
- Follow GPUI Kit's own agent guides (cloned docs: `skills/gpui-kit` in
  github.com/longbridge/gpui-component; online: https://gpui-kit.com/llms.txt,
  per-component `https://gpui-kit.com/component/<name>.md`). Key rules: never
  invent APIs — grep the crate source in `~/.cargo/registry/src/*/gpui-component-0.7.1`
  / `gpui-base-0.7.1` / `gpui-pre-0.3.8`; colors from `cx.theme()`, no raw
  hex/rgb; prefer rem-based helpers (`p_2()`, `gap_3()`) over `px(..)`.
- Repeated elements need domain-derived `ElementId`s (we use paths), not indexes.
- Side effects live on `IsengardApp`; UI modules take `Context<IsengardApp>` and
  call its `pub(crate)`/`pub` methods via `cx.listener`.
- Functions that build elements while `cx` is borrowed return `AnyElement`
  (edition 2024 `impl Trait` captures all lifetimes, which otherwise blocks
  later `cx` use).
- Errors: `anyhow` + `.with_context` in non-UI code; shown via
  `notify_error` (a `Notification::error`). Never `unwrap` on I/O.
- **Tests:** `use gpui_kit::*` brings in gpui's `test` macro, which shadows
  `#[test]`. In test modules of files that glob-import `gpui_kit`, import the
  needed items explicitly instead of `use super::*`.
- Commits go on `master` (user's choice). Only commit when the user asks.

### Version pinning
`gpui-kit = "0.7.1"` pins an exact GPUI snapshot (`gpui-pre =0.3.8`). GPUI's API
changes between snapshots; upgrade `gpui-kit` as a whole and re-check every
API used here. Tree-sitter languages are opt-in cargo features of `gpui-kit`
(`tree-sitter-<lang>`); JSON comes with the base `tree-sitter` feature. To add
a language: enable the feature, extend `Language` in `editor/language.rs`.

## Known issues / TODO
- Interactive flows not yet verified by a human (see Current status).
- No UI integration tests yet; GPUI Kit supports them via the `test-support`
  feature and `#[gpui_kit::test]` — worth adding for tab close, dialogs, tree.
- Modified flag is set on any edit and not cleared by undoing back to the saved
  text.
- No New File / Save As / rename / delete; no file watching; `.gitignore` is not
  respected in the tree; no "Open Recent" in the native menu (welcome screen
  only).
- Save All stops at the first failing file.
- Tree item ids are absolute paths; placeholder ids append `\0placeholder`.
- No app icon yet.

## Roadmap — Phase 5: Claude AI panel (not started)
- GPUI Kit has chat components (`Message`, `Bubble`, `MessageScroller`,
  `TextView::markdown`) — use them for the panel.
- `src/ai/`: Anthropic Messages API client with SSE streaming; run network work
  with `cx.background_spawn` / `cx.spawn` (GPUI's executor) instead of a
  separate tokio runtime if the HTTP client allows; push chunks into the view
  and `cx.notify()`.
- Panel as another `resizable_panel` on the right of `main-split`.
- Config: API key from `ANTHROPIC_API_KEY` (see `.env.example`), first-launch
  dialog if missing. Model name in `AppConfig.model` (currently
  `"claude-sonnet-4-5"`; check the latest Claude model IDs before building).

## Changelog
Newest first. Add an entry for every change.

- **2026-10-06** — Tab close button: muted ×, red only on hover; 12px right spacing
  matching the label padding.
- **2026-10-06** — Status bar is hidden entirely unless a file tab is active.
- **2026-10-06** — Ported the whole UI from egui/eframe to GPUI Kit 0.7.1
  (Tree, TabBar, Editor, Dialog, StatusBar, TitleBar, native menus). Switched
  to JetBrains Mono Nerd Font for UI and editor. Settings moved from eframe
  storage to a JSON file. Removed our own tree-sitter highlighter (GPUI Kit's
  Editor highlights). New: Save All, Edit menu, notifications.
- **2026-10-06** — Square (block) corners on all widgets, windows, menus.
- **2026-10-06** — Welcome screen with recent folders; file tree hidden until a
  folder is open; File > Open Recent / Close Folder.
- **2026-10-06** — Fix panic when closing the last tab via its × button.
- **2026-10-06** — Initial implementation of phases 1–4 on egui: layout, lazy
  file tree, tabbed editor, tree-sitter highlighting, save/close confirmation,
  shortcuts, status bar, persisted settings, `isengard <path>` CLI argument.
