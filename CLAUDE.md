# Isengard — Agent Context

Isengard is a cross-platform desktop code editor written in Rust (egui/eframe +
tree-sitter). This file is the hand-off document for AI agents: read it fully
before making changes.

> **Rule for every agent:** any change to code, behavior, dependencies, or plans
> must also update this file in the same commit — at minimum the
> [Current status](#current-status), [Known issues](#known-issues--todo), and
> [Changelog](#changelog) sections, plus any section the change makes stale.
> Keep it accurate; a stale hand-off doc is worse than none.

## Quick start

Rust is installed via Homebrew's keg-only `rustup`; `cargo` is not on the
default PATH:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"

cargo run                       # dev build, opens the welcome screen
cargo run -- /path/to/folder    # open a folder at launch
cargo run -- /path/to/file.js   # open a file (and its parent folder)
cargo test                      # unit tests (all live in #[cfg(test)] modules)
cargo clippy --all-targets      # should be warning-free
cargo build --release
```

Before finishing any change: `cargo test` passes and `cargo clippy --all-targets`
has no warnings (the `block v0.1.6` future-incompat notice from a dependency is
expected and can be ignored).

## Current status

Phases 1–4 of the original plan are done, plus a welcome screen and block-style
UI. Phase 5 (Claude AI panel) has not started.

Working features:
- Welcome screen on every launch (unless a path is passed on the CLI): Open
  Folder, Open File, recent folders (max 8, click to open, × to remove).
- File tree (left panel) only shown while a folder is open; lazy directory
  loading; dirs first, case-insensitive sort; hides `.git` and `.DS_Store`.
- Tabbed editor with line-number gutter, no soft wrap, horizontal scroll.
- tree-sitter highlighting: JavaScript (incl. JSX), TypeScript, TSX, HTML (with
  injected `<script>`/`<style>`), CSS, JSON.
- Save (Cmd/Ctrl+S), `•` on modified tabs, confirmation on closing a modified
  tab and on quitting with unsaved changes (Save All & Quit / Quit Without
  Saving / Cancel).
- Shortcuts: Cmd/Ctrl+O open folder, Cmd/Ctrl+S save, Cmd/Ctrl+W close tab,
  Ctrl+Tab / Ctrl+Shift+Tab cycle tabs.
- Menus: File (Open Folder, Open File, Open Recent, Close Folder, Save, Save
  All, Close Tab, Quit), View (theme toggle, font size), Help (About).
- Status bar (hidden on welcome screen): relative path, Ln/Col, language,
  transient messages (4 s).
- Embedded JetBrains Mono for monospace; square corners everywhere.
- Persisted settings (eframe storage, key `isengard_config`): theme, font size,
  model name, recent folders. On macOS stored under
  `~/Library/Application Support/Isengard/`.

## Architecture

```
src/
├── main.rs               eframe::run_native, window size/title
├── app.rs                IsengardApp: eframe::App; menus, shortcuts, dialogs,
│                         status bar, panel layout, styling, fonts, persistence
├── config.rs             AppConfig (serde, persisted) + recent-folder helpers
├── file_tree/node.rs     FsNode enum (File/Dir), lazy load_one_level/ensure_loaded
├── editor/
│   ├── buffer.rs         TextBuffer { content: String, path, is_modified, language }
│   ├── language.rs       Language enum, from_path (extension), from_name (injections)
│   ├── highlight.rs      tree-sitter configs (Lazy static) + highlight() -> Vec<HighlightSpan>
│   └── highlight_theme.rs capture index -> Color32 (dark + light palettes)
└── ui/
    ├── welcome.rs        start page; returns WelcomeAction for app.rs to handle
    ├── file_tree.rs      FileTreePanel: open/close folder, show() -> clicked file path
    ├── editor_workspace.rs tabs, active tab, close confirmation, save/save_all
    └── code_editor.rs    TextEdit + layouter + gutter painting; HighlightCache
```

### Frame flow (`IsengardApp::update`)
1. `apply_style` — only when theme/font size changed (cached in `applied_style`).
2. `handle_close_request` — sends `ViewportCommand::CancelClose` and opens the
   quit dialog if there are unsaved tabs.
3. `handle_shortcuts` — consumes key events **before** widgets run, so the
   focused `TextEdit` never sees them. Skipped while a dialog is open.
4. Menu bar, status bar (not on welcome), left panel (only if folder open),
   central panel (welcome screen if no folder and no tabs, else workspace).
5. Dialogs: tab-close confirmation, quit confirmation, About window. While a
   dialog is open, panels are wrapped in `add_enabled_ui(false, ..)`.

### Conventions
- UI components are plain structs with a `show(&mut self, ui) -> Option<Action>`
  style API; they return events and `app.rs` performs side effects (dialogs,
  opening files, status messages). Follow this rather than passing `&mut App`.
- Errors surface to the user via `IsengardApp::set_status`; use `anyhow` with
  `.with_context` in non-UI code. Never `unwrap` on I/O.
- Never block in `update()`. Future async work (AI panel) goes on a background
  thread with channels + `ctx.request_repaint()`.
- Visual style: block/square — `square_corners()` in `app.rs` zeros all
  rounding. New painted shapes must use rounding `0.0`. Avoid emoji/unusual
  glyphs in UI text (default fonts render many as tofu, e.g. `▸`).
- Tests live next to the code in `#[cfg(test)] mod tests`, using `tempfile`
  for filesystem cases.
- Commits go on `master` (user's choice; repo's nominal main branch is `main`).
  Only commit when the user asks.

### Syntax highlighting details
- `HIGHLIGHT_NAMES` in `highlight.rs` is the recognized capture list;
  tree-sitter matches by prefix, so `function.method` → `function`. Add a name
  there **and** a color in `highlight_theme.rs` (a test enforces every name has
  a color except `variable` and `embedded`).
- Configs are built once in `CONFIGS: Lazy<HashMap<Language, HighlightConfiguration>>`;
  `Highlighter` is `thread_local!`. Injections resolve via `Language::from_name`.
- TypeScript/TSX highlight query = TS query + JS query (TSX also adds the JSX
  query). JS config includes the JSX query.
- JSON uses a custom `JSON_HIGHLIGHTS_QUERY`: the upstream query tags keys as
  both `@string.special.key` and `@string`, and `@string` wins.
- `highlight()` returns contiguous, non-overlapping spans covering the whole
  source (innermost capture wins); tests assert this.
- `code_editor::HighlightCache` re-highlights the whole file only when the text,
  language, font size, or theme changes.

### egui gotchas (egui/eframe 0.29 — APIs differ in newer versions)
- `consume_key(CTRL, Tab)` also matches Ctrl+Shift+Tab (`matches_logically`
  ignores extra Shift), so consume the Shift variant first.
- The tab bar can close the last tab mid-frame; `EditorWorkspace::show` re-checks
  `tabs.get_mut(active)` after drawing it. Never index `tabs[active]` blindly.
- `TextEdit` ids are `Id::new(("code_editor", path))`; focus is requested via
  `focus_editor` flag after activating a tab.
- Gutter line numbers are painted from `galley.rows` (only visible rows),
  aligned with `galley_pos`; works because soft wrap is disabled.
- Uses `UiBuilder` (`allocate_new_ui`) and `id_salt` — 0.29 APIs.
- The macOS window's own rounded corners come from the OS, not egui.

## Known issues / TODO
- Inside JS template strings, some characters render in the default color
  rather than string green (nested `@embedded`/`@punctuation.special` captures).
- Whole-file re-highlight on every edit; fine for normal files, slow for very
  large ones. Consider incremental parsing (`tree_sitter::Parser` + `InputEdit`)
  or `ropey` (dropped from deps for now).
- No New File / Save As / rename / delete; no find/replace; no file watching
  (external changes are not detected); `.gitignore` is not respected in the tree.
- Tab-close confirmation offers Save / Don't Save / Cancel; Save All errors stop
  at the first failing file.
- No app icon yet (`assets/icon.png` was in the original plan).
- Recent-folder list with entries has not been visually verified (logic is
  unit-tested).

## Roadmap — Phase 5: Claude AI panel (not started)
Original plan, still the intended design:
- Deps to add: `tokio`, `reqwest` (streaming), `serde_json`, `crossbeam-channel`.
- `src/ai/`: `types.rs` (ChatMessage, Role, AiRequest, AiEvent), `client.rs`
  (Anthropic Messages API POST + SSE stream parser), `worker.rs` (background OS
  thread owning a tokio runtime; channels to/from the UI; holds an
  `egui::Context` clone and calls `request_repaint()` per chunk).
- `src/ui/ai_panel.rs`: chat history, input, Send/Stop, inject current file /
  selection as context. Add as `SidePanel::right` in `app.rs`.
- Config: API key from `ANTHROPIC_API_KEY` (see `.env.example`), first-launch
  modal if missing. Model name lives in `AppConfig.model` (currently
  `"claude-sonnet-4-5"`; check the latest Claude model IDs before building).

## Changelog
Newest first. Add an entry for every change.

- **2026-10-06** — Square (block) corners on all widgets, windows, menus.
- **2026-10-06** — Welcome screen with recent folders; file tree hidden until a
  folder is open; File > Open Recent / Close Folder; no auto-reopen of last
  folder; status bar hidden on welcome screen; tree root shown as a heading.
- **2026-10-06** — Fix panic when closing the last tab via its × button.
- **2026-10-06** — Initial implementation of phases 1–4: layout, lazy file
  tree, tabbed editor with gutter, tree-sitter highlighting, save/close
  confirmation, shortcuts, status bar, JetBrains Mono, persisted settings,
  `isengard <path>` CLI argument.
