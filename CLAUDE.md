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

> **Design rule (user decision):** all UI must follow [DESIGN.md](DESIGN.md)
> (block style, font, colors, spacing, shared components). Read it before any
> UI change and add new approved patterns to it in the same commit.

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
  Workspace (listed first), Open Folder, Open File, then "Recent workspaces"
  and "Recent folders"
  (max 8 each, click to open, × to remove; empty lists are hidden).
  The Open Folder shortcut hint is one `Kbd` per key (`⌘` `O` / `Ctrl` `O`),
  built by `welcome::shortcut_keys`; a single `Kbd` runs the keys together.
- VS Code–style workspaces: several root folders in one window
  (File > Add Folder to Workspace…), saved to / opened from
  `.isengard-workspace` JSON files (File > Save Workspace As… / Open
  Workspace…, or `isengard x.isengard-workspace`). Folder paths are stored
  relative to the file. Switching folder/workspace closes all tabs, asking
  first about unsaved files and about saving an untitled multi-root workspace.
  Right-click a root to remove it. A saved workspace file is re-written when
  folders are added/removed.
- File tree (GPUI Kit `Tree`) only while a workspace is open; lazy directory
  loading; dirs first, case-insensitive sort; hides `.git` and `.DS_Store`.
  Icons are the full Material Icon Theme set (associations from
  `material-icons.json`); other UI chrome still uses Lucide via GPUI Kit.
- Tabs (`TabBar`/`Tab` with a close button), one `EditorState` per tab so each
  keeps its own undo history and cursor. Tree single-click opens a **preview**
  tab (italic; replaced by the next preview open); double-click the file or the
  tab (or edit) pins it permanent. Tab close and welcome "remove recent" use
  Kit ghost `Button`s (default hover). Markdown files get an Edit / Preview
  toolbar (`TextView::markdown` / SVG `img` from buffer); View > Toggle Preview
  or `⌘⇧V` / `Ctrl+Shift+V`. Raster images (png/jpg/gif/webp/…) open as an
  `img(path)` preview tab; SVG is editable text with the same Edit/Preview
  toggle as Markdown.
- Code editor (GPUI Kit `Editor`): tree-sitter highlighting for every language
  GPUI Kit vendors via `tree-sitter-languages` (astro, bash, c/c++/c#, cmake,
  css, diff, ejs, elixir, erb, go, graphql, html, java, javascript, json,
  kotlin, lua, make, markdown, php, proto, python, ruby, rust, scala, sql,
  svelte, swift, toml, tsx, typescript, yaml, zig); line numbers, indent
  guides, search. Unknown extensions stay Plain Text.
- Save (`•` marks modified tabs), Save All, close-tab confirmation (Save /
  Don't Save / Cancel), quit confirmation when anything is unsaved (window
  close button, Cmd/Ctrl+Q, menu).
- Native macOS menu bar; in-window `AppMenuBar` in the title bar on
  Windows/Linux. Menus: Isengard (About, Quit), File, Edit, View.
- Status bar: relative path, Ln/Col, language — rendered only while a file
  tab is active (hidden on the welcome screen and with a folder but no file).
  Notifications for saves/errors (bottom-right; Kit default is top-right).
- Logo (block "I" with battlements + amber cursor) on the welcome screen and as
  the app icon: Dock icon at runtime on macOS, embedded .exe icon on Windows,
  `cargo bundle` metadata for a macOS .app / Linux .deb.
- JetBrains Mono Nerd Font everywhere (UI + editor); square corners
  (`theme.radius = 0`); **dark mode only** (no light theme / toggle); editor
  font size zoom. Editor/syntax colours are Ayu Darker
  (`assets/themes/ayu-darker-highlight.json`, GPL-3.0 from
  k4yt3x/zed-theme-ayu-darker); tree-sitter grammars unchanged.

Not yet verified interactively (no clicking was possible during the port):
tree expand/collapse, clicking files, tab switching/closing, dialogs,
keyboard shortcuts. Check these first if something is off.

## Architecture

```
src/
├── main.rs              app bootstrap: gpui_kit::init, fonts, theme, key bindings,
│                        menus, window (QuitMode::LastWindowClosed)
├── app.rs               IsengardApp view: actions, tabs, dialogs, layout, status bar
├── theme.rs             bundled font loading + Theme overrides (font, radius 0, sizes)
├── branding.rs          logo image for the UI + macOS Dock icon (objc2 AppKit)
├── menus.rs             native menus + AppMenuBar (Windows/Linux)
├── config.rs            AppConfig (serde JSON) + recents + workspace sessions
├── workspace.rs         Workspace (root folders + optional file): naming,
│                        root_for/relative_label, load/save .isengard-workspace
├── editor/
│   ├── document.rs      read_text / write_text / file_name / is_image
│   ├── language.rs      Language enum: from_path, highlighter_name, display_name
│   ├── highlights.rs    patches empty GPUI Kit highlight queries (GraphQL)
│   └── tabs.rs          TabList<T>: pure tab ordering/activation logic (unit-tested)
├── file_tree/
│   ├── node.rs          FsNode (File/Dir), lazy load_one_level, set_expanded, find_mut
│   ├── ignore.rs        GitIgnoreIndex — dim gitignored paths (show, don't hide)
│   ├── indent_guides.rs vertical tree lines from flat entry depths
│   └── icons.rs         Material Icon Theme (full set) → tree_icon(path, …)
└── ui/
    ├── components.rs    shared compositions encoding DESIGN.md rules
    │                    (center_dialog)
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
- **Workspace**: `IsengardApp::workspace` is the source of truth for open
  folders; after changing it call `workspace_changed` (re-syncs the tree via
  `FileTreePanel::set_folders` and the titles) and `persist_workspace` (writes
  the file if there is one). Switching goes through `request_switch(Switch)` →
  optional "save untitled workspace" dialog → `confirm_unsaved_then` →
  `apply_switch` (persists session, closes tabs, records recents, restores
  the new workspace's session). Confirmations use
  `open_choice_dialog` (Cancel / secondary / primary).
  `RemoveWorkspaceFolder(PathBuf)` is a data-carrying action
  (`#[derive(Action)] #[action(namespace = isengard, no_json)]`) dispatched from
  the tree's context menu.
- **File tree**: `FsNode` is the source of truth. Unloaded/empty directories get
  a disabled placeholder child (`"Loading…"` / `"(empty)"`) because GPUI Kit's
  `TreeItem::is_folder()` is just "has children". On `TreeEvent::Expanded` the
  node loads its children and the `TreeItem`s are rebuilt via `set_items`
  (selection is restored by id). Tree item ids are absolute paths. Row icons
  come from `file_tree::tree_icon` (full Material Icon Theme via `img()` +
  `material-icons.json`), not Lucide.
- **Dialogs**: `window.open_dialog` with a `DialogFooter` of `Button`s; buttons
  capture a `WeakEntity<IsengardApp>` and call back into it. Always wrap with
  `ui::components::center_dialog` (Kit defaults to top ~10%; we centre). Close
  × buttons (tabs, recent rows, dialogs, toasts) use Kit's default ghost style
  — no custom red hover.
- **Theme**: `theme::apply` always uses `ThemeMode::Dark` (it reloads the
  theme config) and then overrides fonts, sizes, `radius`/`radius_lg = 0`, and
  `highlight_theme` (Ayu Darker tokens + editor/gutter bg = `theme.sidebar` so
  it matches the file tree), and `notification.placement = BottomRight`. Re-run
  it after any font-size change.
  `theme.font_size` (14px) is the rem base for the whole UI;
  `mono_font_size` is the editor size from config.
- **Logo / icons** (`assets/logo/`): `logo.svg` is the master (1024 square,
  near-black tile). `logo-mark.svg` is the shapes only (welcome screen,
  `branding::LOGO_MARK`). `icon-macos.svg` puts the tile on Apple's 824px grid
  with a transparent margin and square corners (block style). Exports:
  `Isengard.icns`, `icon-1024.png` (Dock icon, cargo-bundle), `logo-512.png`,
  `Isengard.ico` (Windows, via `build.rs` + `assets/windows/isengard.rc` +
  `embed-resource`). Colors: tile `#0b0b0c`, mark `#ececec`, cursor `#f5a524`.
  The group is optically centred (halfway between bbox centre and area
  centroid). To re-export after editing an SVG: render PNGs with a transparent
  background using `resvg` (e.g. a tiny `resvg = "=0.45.1"` CLI; `qlmanage`
  fills transparency with white, don't use it), build the iconset sizes
  16–512 @1x/@2x, `iconutil -c icns`, and pack PNGs 16–256 into the .ico.
- **Fonts**: five JetBrains Mono Nerd Font weights are embedded with
  `include_bytes!` (~13 MB) and registered via `cx.text_system().add_fonts`.
  Family name: `"JetBrainsMono Nerd Font"`. Licenses: project is GPL-3.0-only
  (`LICENSE`); fonts `assets/fonts/OFL.txt` + `LICENSE-NerdFonts.txt`;
  highlight theme under `assets/themes/` (also GPL-3.0; see NOTICE.txt).
  Material icons: MIT under `assets/icons/material/LICENSE`.

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
API used here. Tree-sitter languages come from the `tree-sitter-languages`
feature (JSON is included via base `tree-sitter`). Detection lives in
`Language` in `editor/language.rs`. Direct dep `cc = "~1.2.1"` keeps
`tree-sitter-sequel` (SQL) resolvable against `embed-resource`.

## Known issues / TODO
- Interactive flows not yet verified by a human (see Current status).
- No UI integration tests yet; GPUI Kit supports them via the `test-support`
  feature and `#[gpui_kit::test]` — worth adding for tab close, dialogs, tree.
- Modified flag is set on any edit and not cleared by undoing back to the saved
  text.
- No New File / Save As / rename / delete; no file watching; no "Open Recent"
  in the native menu (welcome screen only). Gitignored files are shown muted
  in the tree (not hidden); ignore rules refresh on folder open / expand, not
  on every save of `.gitignore`. `.env` / `.gitignore` / lockfiles / Dockerfile
  use Bash/JSON/TOML aliases (no dedicated Dockerfile grammar compatible with
  GPUI's tree-sitter); GraphQL highlights are patched in `editor::highlights`
  because GPUI Kit ships an empty query.
- Workspaces: not yet verified by clicking — Add Folder, Save Workspace As,
  root context menu, switch dialogs. Quitting with an untitled multi-root
  workspace does not offer to save it (only switching does). Workspace
  `settings` are stored but not applied. Session restore reopens tabs (with
  cursor + scroll) and expanded tree folders per workspace/folder (config
  `sessions`); does not restore unsaved buffer text or auto-open last
  workspace on launch.
- Save All stops at the first failing file.
- Tree item ids are absolute paths; placeholder ids append `\0placeholder`.
- Windows icon embedding (`build.rs`) and the Linux X11 window icon are
  untested; X11 would need `WindowOptions::icon` set (not done).
- `package.metadata.bundle.identifier` (`dev.isengard.editor`) is a placeholder.
- Syntax highlight theme (`assets/themes/`, Ayu Darker) is GPL-3.0; the project
  as a whole is also **GPL-3.0-only** (`LICENSE`, `Cargo.toml`). See
  `CONTRIBUTING.md` — forks are fine, PRs are not accepted.

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

- **2026-10-07** — Slogan "The already configured editor" on welcome, About,
  and cargo-bundle short_description.
- **2026-10-07** — Session restore also keeps per-tab cursor (line/col) and
  editor scroll offset.
- **2026-10-07** — Session restore: multi-root expand respects collapsed roots
  (`expand_paths` applies the saved set; nested expands under a collapsed
  parent are not persisted).
- **2026-10-07** — Session restore: open tabs + expanded tree dirs persisted
  per folder/workspace in `config.json` (`sessions`); restored when reopening
  from welcome / Open / CLI. No dirty-buffer or cold-start auto-open.
- **2026-10-07** — File tree hover full-bleed (no tree `px`; inset on row content).
- **2026-10-07** — Dropped the file-tree header label (name lives in the title bar).
- **2026-10-07** — Indent guides: paint every ancestor column through all
  descendants (incl. last child) + 1px overlap; multi-root skips only the
  cross-root rail.
- **2026-10-07** — Dropped unused `document::is_svg` (SVG via `Language::Svg`).
- **2026-10-07** — File tree: depth-0 rows align with the workspace title
  (`px_3`); each nest is one `INDENT_COL` only (no extra `INDENT_BASE`).
  Multi-root still suppresses only the cross-root rail.
- **2026-10-07** — SVG Edit/Preview like Markdown (buffer → `Image::from_bytes`);
  View menu "Toggle Preview" covers both. Raster images remain image-only.
- **2026-10-07** — Image preview tabs (`img(path)` for png/jpg/gif/webp/…).
- **2026-10-07** — Notifications bottom-right (`theme.notification.placement`).
- **2026-10-07** — Markdown Edit/Preview toggle via Kit `TextView::markdown`
  (toolbar + View menu + `⌘⇧V` / `Ctrl+Shift+V`). Not the same as preview tabs.
- **2026-10-07** — Unsaved tab indicator: blue square (`theme.blue`; was yellow).
- **2026-10-07** — Close/remove × use Kit ghost default (removed red-hover
  `destructive_icon_button` / dialog override).
- **2026-10-07** — Modal dialogs centred in the window (`center_dialog`; Kit
  default was top ~10%). Context menus unchanged.
- **2026-10-07** — Unsaved tabs: yellow square indicator (`theme.yellow`).
- **2026-10-07** — Preview tabs (VS Code-style): tree single-click = preview,
  double-click file/tab or edit = pin.
- **2026-10-07** — Gitignored tree rows: 60% opacity.
- **2026-10-07** — File tree: indent columns don't shrink on narrow panels
  (labels truncate instead).
- **2026-10-07** — File tree indent guides + gitignore dimming (show muted,
  don't hide); `ignore` crate + `file_tree::{indent_guides,ignore}`.
- **2026-10-07** — Editor/gutter background = sidebar (matches file tree);
  Dockerfile/Containerfile detected (Bash grammar alias; status shows
  "Dockerfile"); `.dockerignore` → Bash.
- **2026-10-07** — Language aliases: `.env*` / ignore files → Bash; lockfiles →
  JSON/TOML/YAML; GraphQL highlight query patch (`assets/highlights/graphql.scm`)
  so `.gql`/`.graphql` are not plain white (kit ships empty highlights).
- **2026-10-07** — Project licensed GPL-3.0-only (`LICENSE`, `Cargo.toml`);
  `CONTRIBUTING.md` states forks OK, PRs not accepted.
- **2026-10-07** — Syntax tokens: Ayu Darker from
  [k4yt3x/zed-theme-ayu-darker](https://github.com/k4yt3x/zed-theme-ayu-darker)
  (GPL-3.0; `assets/themes/LICENSE`). Replaces MIT Ayu Dark / Gruvbox.
- **2026-10-07** — Syntax tokens: Ayu Dark (MIT from Zed's theme). Gruvbox
  removed. (Superseded same day by Ayu Darker.)
- **2026-10-07** — Dark mode only: removed light theme, View/title-bar toggle,
  and `AppConfig.dark_mode`. Syntax tokens were briefly Gruvbox Dark.
- **2026-10-06** — Title-bar sun/moon icon toggles light/dark (same as View
  menu); macOS title stays centred.
- **2026-10-06** — Editor/syntax colours: GitHub Dark + GitHub Light highlight
  themes (token colours only; editor chrome unchanged).
- **2026-10-06** — Enabled all GPUI Kit tree-sitter languages
  (`tree-sitter-languages`) and expanded `Language` detection accordingly.
- **2026-10-06** — File tree: full Material Icon Theme pack (1251 SVGs +
  `material-icons.json` associations) instead of a curated subset.
- **2026-10-06** — File tree Material icons rendered with `img()` so colours
  survive (GPUI `Icon`/`svg()` alpha-masks and tints).
- **2026-10-06** — File tree icons: curated Material Icon Theme subset
  (`assets/icons/material/`, MIT) via `file_tree::tree_icon`; Lucide kept
  for the rest of the UI.
- **2026-10-06** — View menu: trailing separator so macOS "Enter Full Screen"
  sits in its own section below font-size items.
- **2026-10-06** — Removed Next/Previous Tab from the View menu (the
  Ctrl+Tab / Ctrl+Shift+Tab shortcuts still work).
- **2026-10-06** — Pointer cursor on all clickable elements (dialog buttons,
  tabs, tree rows, destructive buttons); GPUI Kit only sets it for link/text
  buttons (DESIGN.md §3).
- **2026-10-06** — Welcome screen: workspaces before folders ("Open Workspace…"
  first in Start; "Recent workspaces" above "Recent folders").
- **2026-10-06** — VS Code–style workspaces: multi-root folders, `.isengard-workspace`
  files (relative paths), Add Folder / Open / Save As / Close Workspace,
  root context menu, separate recent folders/workspaces lists, shared
  `open_choice_dialog`.
- **2026-10-06** — Added DESIGN.md (design rules). Extracted
  `destructive_icon_button`; the recent-folder remove × now matches the tab
  close × (red only on hover).
- **2026-10-06** — Welcome screen shortcut hint shows separate key caps (`⌘` `O`).
- **2026-10-06** — App logo (concept B, optically centred) on the welcome
  screen and as the app icon (macOS Dock at runtime, Windows .exe icon,
  cargo-bundle metadata).
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
