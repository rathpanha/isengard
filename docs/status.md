# Current status

Phases 1–4 of the original plan are done and the UI has been ported from egui
to GPUI Kit. Phase 5 (Claude AI panel) has not started.

Working features (verified by screenshots on macOS: welcome screen, file tree,
tabs, highlighted editor, status bar):

- **Welcome:** Start is two columns (workspace | folder/file) with a vertical
  rule; New File uses `⌘N` / `Ctrl+N`. Recent workspaces then recent folders
  (max 8 each, × to remove; empty lists hidden). Details in
  [design.md](design.md).
- **Workspaces:** multi-root folders in one window; `.isengard-workspace` JSON
  (relative folder paths); Add Folder / Open / Save As / Close; root context
  menu remove; switching closes tabs (unsaved + untitled-workspace prompts).
  Session restore (tabs with cursor/scroll + expanded tree dirs) per
  folder/workspace in `config.json` `sessions` — see design for restore rules.
- **File tree:** Kit `Tree` while a workspace is open; lazy load; dirs first,
  case-insensitive; hides `.git` / `.DS_Store`; full Material Icon Theme;
  gitignored rows shown muted; indent guides (design owns the pixel rules).
- **Tabs:** Kit `Tab`s in `h_flex` (not `TabBar` — needed for per-tab
  `ContextMenu`); Material file icon + name as tab children; Close / Close
  Others / Close All; preview tabs; dirty mark; Markdown/SVG Edit–Preview;
  raster image preview tabs. Details in design.
- **Editor:** Kit `Editor` + tree-sitter for every language Kit vendors via
  `tree-sitter-languages`; line numbers, indent guides, search; unknown
  extensions stay Plain Text. Syntax: Ayu Darker; editor/gutter bg = sidebar.
- **Terminal:** multi-tab bottom panel while a folder/workspace is open (not
  on welcome). Toggle show/hide (`Ctrl+\``); New Terminal (`Ctrl+Shift+\`` /
  header `+`) always adds a tab. Multi-root asks for root only when creating
  a session, not when restoring a minimized panel. Tab cwds, visibility,
  and height persist per workspace session. Mouse selection / scrollback
  still minimal.
- **Sidebar:** hide/show file tree without closing the workspace (View >
  Toggle Sidebar / `⌘B` / `Ctrl+B` / status-bar icon); width + visibility
  persist per workspace session.
- **Save / close:** Save, Save All, close-tab and quit confirmations when
  unsaved.
- **Chrome:** native macOS menu bar; in-window `AppMenuBar` on Windows/Linux
  (Isengard / File / Edit / View). Status bar while a workspace is open
  (panel toggles; path / Ln·Col when a file tab is active). Notifications
  bottom-right. Logo on welcome + Dock / X11 window icon / .exe /
  cargo-bundle; Wayland panel icon via `scripts/install-linux-dev-icon.sh`.
- **Theme:** JetBrains Mono Nerd Font; square corners; **dark mode only**;
  editor font-size zoom.

Not yet verified interactively (no clicking during the port): tree
expand/collapse, clicking files, tab switching/closing, dialogs, keyboard
shortcuts. Check these first if something is off.

## Known issues / TODO

- Terminal mouse selection / scrollback UI still minimal. Workspace switch
  clears all terminal tabs (re-prompts for a root if the panel was open).
- Interactive flows not yet verified by a human (see above).
- No UI integration tests yet; GPUI Kit supports them via the `test-support`
  feature and `#[gpui_kit::test]` — worth adding for tab close, dialogs, tree.
- Modified flag is set on any edit and not cleared by undoing back to the saved
  text.
- No Save As / rename / delete; no file watching; no "Open Recent"
  in the native menu (welcome screen only). Gitignore rules refresh on folder
  open / expand, not on every save of `.gitignore`. `.env` / `.gitignore` /
  lockfiles / Dockerfile use Bash/JSON/TOML aliases (no dedicated Dockerfile
  grammar compatible with GPUI's tree-sitter); GraphQL highlights are patched
  in `editor::highlights` because GPUI Kit ships an empty query.
- Workspaces: not yet verified by clicking — Add Folder, Save Workspace As,
  root context menu, switch dialogs. Quitting with an untitled multi-root
  workspace does not offer to save it (only switching does). Workspace
  `settings` are stored but not applied. Session restore does not restore
  unsaved buffer text or auto-open last workspace on launch.
- Save All stops at the first failing file.
- Tree item ids are absolute paths; placeholder ids append `\0placeholder`.
- Windows icon embedding (`build.rs`) is untested on a real Windows box.
  Linux: X11 uses `WindowOptions::icon`; Wayland needs the one-time
  `scripts/install-linux-dev-icon.sh` (not verified on every DE).
- `package.metadata.bundle.identifier` (`dev.isengard.editor`) is a placeholder.
- Syntax highlight theme (`assets/themes/`, Ayu Darker) is GPL-3.0; the project
  as a whole is also **GPL-3.0-only** (`LICENSE`, `Cargo.toml`). See
  [CONTRIBUTING.md](../CONTRIBUTING.md) — forks are fine, PRs are not accepted.

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
