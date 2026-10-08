# Current status

Phases 1–4 of the original plan are done and the UI has been ported from egui
to GPUI Kit. Phase 5 (Claude AI panel) has not started.

Working features (verified by screenshots on macOS: welcome screen, file tree,
tabs, highlighted editor, status bar):

- **Welcome:** Start is two columns (workspace | folder/file) with a vertical
  rule; New File (`⌘N` / `Ctrl+N`) opens an untitled buffer (save dialog only
  on Save / dirty close). Recent workspaces then recent folders (max 8 each,
  × to remove; empty lists hidden). Details in [design.md](design.md).
- **Workspaces:** multi-root folders in one window; `.isengard-workspace` JSON
  (relative folder paths); Add Folder / Open / Save As / Close; root context
  menu remove; switching closes tabs (unsaved + untitled-workspace prompts).
  Session restore (tabs with cursor/scroll + expanded tree dirs) per
  folder/workspace in `config.json` `sessions` — see design for restore rules.
- **File tree:** Kit `Tree` while a workspace is open; lazy load; dirs first,
  case-insensitive; hides `.git` / `.DS_Store`; full Material Icon Theme;
  gitignored rows shown muted; indent guides (design owns the pixel rules).
  Context menu: files Rename / Delete; folders New File / New Folder /
  Rename / Delete (multi-root roots also Remove Folder from Workspace).
  Right-click empty space in the tree panel → New File / New Folder at
  the workspace root (sole root when single-root; first root when multi).
  Mini toolbar above the tree: New File / New Folder (selection's dir,
  else root) so create still works when the tree has no blank space.
  Rename / New File / New Folder are inline in the tree (Enter/blur
  commit, Escape cancel). Clicking a tree row focuses the Tree (preview
  open does not steal focus); `F2` rename; `Delete` / `Backspace` → OS
  Trash. Double-click focuses the editor.
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
  and height persist per workspace session. Mouse drag selects (double =
  word, triple = line); copy/paste via OS terminal chords (`⌘C`/`⌘V` or
  `Ctrl+Shift+C`/`V`, chord wins over Toggle Preview when focused); URL hover
  continuous underline + pointer; Ctrl/Cmd+click opens.
  Scrollback UI still minimal.
- **Sidebar:** hide/show without closing the workspace (View > Toggle
  Sidebar / `⌘B` / `Ctrl+B` / status-bar icon); width + visibility persist
  per workspace session. Explorer ↔ Search via top sidebar strip (`⌘⇧F` /
  Find in Files): project find + replace across roots.
- **Project search:** parallel `ignore` walk + `regex` / `aho-corasick`
  prefilter; cancels in-flight scans on new query; skips `node_modules`/
  `target`/media/binaries and files >1 MiB; open buffers preferred over disk;
  Replace in File / Replace All (confirm); click match to open (result row +
  editor highlight). Results list paints at most 500 hits.
- **Save / close:** Save, Save All, close-tab and quit confirmations when
  unsaved (New File is buffer-only until Save; save dialog uses active tree
  dir / root). Linux in-window close × confirms unsaved changes.
- **Chrome:** native macOS menu bar; in-window `AppMenuBar` on Windows/Linux
  (Isengard / File / Edit / View). Status bar while a workspace is open
  (panel toggles; path / Ln·Col when a file tab is active). Notifications
  bottom-right. Logo on welcome + platform app icon (see architecture
  Development setup / icon matrix).
- **Theme:** JetBrains Mono Nerd Font; square corners; **dark mode only**;
  editor font-size zoom.

Not yet verified interactively (no clicking during the port): tree
expand/collapse, clicking files, tab switching/closing, dialogs, keyboard
shortcuts. Check these first if something is off.

## Known issues / TODO

- Terminal scrollback UI still minimal (selection/copy/paste/URL-click work).
  Workspace switch clears all terminal tabs (re-prompts for a root if the
  panel was open).
- Interactive flows not yet verified by a human (see above).
- No UI integration tests yet; GPUI Kit supports them via the `test-support`
  feature and `#[gpui_kit::test]` — worth adding for tab close, dialogs, tree.
- Modified flag is set on any edit and not cleared by undoing back to the saved
  text.
- No Save As; no file watching; no "Open Recent" in the native menu
  (welcome screen only). Tree New File creates on disk after the inline
  name (distinct from menu New File / `⌘N`, which stays untitled until
  Save). Tree Delete always goes to Trash. Gitignore rules refresh on
  folder open / expand, not on every save of `.gitignore`. `.env` /
  `.gitignore` / lockfiles / Dockerfile use Bash/JSON/TOML aliases (no
  dedicated Dockerfile grammar compatible with GPUI's tree-sitter); GraphQL
  highlights are patched in `editor::highlights` because GPUI Kit ships an
  empty query.
- Workspaces: not yet verified by clicking — Add Folder, Save Workspace As,
  root context menu, switch dialogs. Quitting with an untitled multi-root
  workspace does not offer to save it (only switching does). Workspace
  `settings` are stored but not applied. Session restore does not restore
  unsaved buffer text or auto-open last workspace on launch.
- Save All stops at the first failing file.
- Tree item ids are absolute paths; placeholder ids append `\0placeholder`.
- App icons for `cargo run`: macOS Dock + Windows `.exe` embed + Linux X11
  window icon are implemented; Wayland still needs
  `scripts/install-dev-icon.sh` once. Windows embed and Wayland panel matching
  are not verified on every machine/DE.
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
