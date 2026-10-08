# Changelog

Newest first. Append **one** bullet per change here only — never duplicate in
status, architecture, or design. See [README.md](README.md).

- **2026-10-08** — Git Push toast uses git’s own success text (e.g.
  `Everything up-to-date` from stderr), not a synthetic "Pushed".
- **2026-10-08** — Git busy: disable sibling actions; spinner only on the
  button that started Pull/Push/Commit/discard.
- **2026-10-08** — Git label-only busy buttons (Pull/Push/Suggest/Commit)
  set a `Loader` icon so Kit `.loading()` actually paints a spinner.
- **2026-10-08** — In-flight Button `.loading()` rule in design.md; git +
  search replace use spinners; git checkboxes get `cursor_pointer`.
- **2026-10-08** — Git commit selection defaults to none selected.
- **2026-10-08** — Git commit selection: per-file + select-all checkboxes;
  Suggest/Commit use selection only (`git add -- paths`).
- **2026-10-08** — Git Pull/Push labels show behind/ahead counts vs
  upstream (`Pull (N)` / `Push (N)`).
- **2026-10-08** — Git Commit disabled while the message is empty/whitespace.
- **2026-10-08** — Git repo sections: bottom border only between repos
  (not under a single or last section).
- **2026-10-08** — Git panel Suggest fills commit box from change paths
  (Add/Update/Remove + optional docs:/test:; no AI, does not commit).
- **2026-10-08** — Git change rows: Open File icon (permanent tab, like
  the tree) next to discard.
- **2026-10-08** — Git diff overview ruler: absolute overlay (flex sibling
  blew out pane height); ticks still via document fractions.
- **2026-10-08** — Git side-by-side diff: overview-ruler ticks on each
  pane (Kit scrollbar has no marker API).
- **2026-10-08** — Git side-by-side diff: line highlights (delete/insert
  fills) via `similar`.
- **2026-10-08** — Git change preview: equal side-by-side HEAD | Working
  Tree panes, both read-only (replaces unified diff tab).
- **2026-10-08** — Git sections: flush layout, bottom border only (no cards).
- **2026-10-08** — Git changes: Material icons, status letter on the right,
  discard file / discard all (confirm); Refresh ghost icon-only.
- **2026-10-08** — Git panel UX: strip order Explorer|Git|Search; commit
  textarea; outlined buttons; bordered repo sections; collapsible changes;
  click opens read-only diff preview.
- **2026-10-08** — Minimal Source Control sidebar (stacked multi-root repos;
  status / commit-all / pull ff-only / push via CLI `git`).
- **2026-10-08** — Remember main window size/position/maximized in
  `config.json` across launches.
- **2026-10-08** — Project search speed: parallel walk, cancel on new query,
  skip build/media dirs & >1 MiB files, aho-corasick prefilter, paint ≤500 hits.
- **2026-10-08** — Project search: highlight match in result rows and in the
  editor on preview click (`RangeDecoration` fill + selection).
- **2026-10-08** — Project search file headers use Material `tree_icon`
  (same as tree/tabs).
- **2026-10-08** — Explorer / Search toggle back at the top of the sidebar
  (status bar only shows/hides the panel).
- **2026-10-08** — Sidebar icon buttons use `.small()` (was `.xsmall`);
  search find/replace rows use `gap_3`; status line only when non-empty.
- **2026-10-08** — Embed Lucide `FolderTree` / `Search` (+ search option icons).
- **2026-10-08** — Project Find & Replace in the left sidebar (Explorer ↔
  Search); `⌘⇧F` / Edit > Find in Files; replace-all confirm; `.gitignore`
  + open-buffer aware (`src/search`).
- **2026-10-08** — Toast icon/text vertically centred via custom notification
  content (`notify_success` / `notify_error`); tab/recent × stay `.ghost()`.
- **2026-10-08** — Kit semantic variants: discard dialog buttons
  `.danger().outline()`; success/error toasts typed (design §3).
- **2026-10-08** — Embed Lucide `FilePlus` / `FolderPlus` via `AppAssets`
  (default Kit icons + those two); tree toolbar uses them.
- **2026-10-08** — File tree toolbar icons: `FilePlus` / `FolderPlus`.
- **2026-10-08** — File tree mini toolbar: New File / New Folder (for when
  the tree has no blank space to right-click).
- **2026-10-08** — Right-click empty space in the file tree → New File /
  New Folder at the workspace root.
- **2026-10-08** — New File / New Folder are inline in the tree (same as
  rename); no name modal.
- **2026-10-08** — Removed Delete Permanent; tree Delete is Trash-only.
- **2026-10-08** — Tree click keeps Tree focus (preview open no longer steals
  it); Delete / F2 work after selecting a file.
- **2026-10-08** — File tree Delete → OS Trash.
- **2026-10-08** — File tree rename is inline in the row (no modal); New
  File/Folder still use a name dialog.
- **2026-10-08** — File tree: context menu New File/Folder (dirs), Rename /
  Delete; `F2` / `Delete`/`Backspace` on tree selection.
- **2026-10-08** — Terminal selection: `write_to_primary` only on
  linux/freebsd so macOS/`cargo run` compiles again.
- **2026-10-07** — New File in workspace stays in-memory until Save; Linux
  title-bar close runs unsaved-changes quit dialog.
- **2026-10-07** — Save / Save All no longer show a toast on success.
- **2026-10-07** — New File: welcome opens untitled buffer (save on demand);
  with a folder/workspace, auto-creates under active tree dir / root.
- **2026-10-07** — Terminal copy shows a "Copied" toast; URL hover underline
  sits farther under the glyphs (`font_size` + 5px).
- **2026-10-07** — Terminal paste: `TogglePreview` no longer steals
  `Ctrl+Shift+V` / `⌘⇧V` while Terminal focused; URL hover underline is one
  continuous stroke with a gap below the glyphs.
- **2026-10-07** — Terminal copy/paste use OS chords (Linux/Win
  Ctrl+Shift+C/V; macOS ⌘C/V); URL hover underline + pointer cursor.
- **2026-10-07** — Terminal: mouse selection (word/line clicks), copy/paste,
  Ctrl/Cmd+click to open URLs.
- **2026-10-07** — Notifications clear the status bar (`margins.bottom` =
  status-bar height + 16px).
- **2026-10-07** — Reverted no-animation experiment; Kit dialog/toast motion
  is default again (`reduce_motion` left off).
- **2026-10-07** — Cross-platform Development setup in architecture (macOS /
  Linux / Windows) + app-icon matrix; `scripts/install-dev-icon.sh` /
  `.ps1` entrypoints (Windows `.exe` icon already via `build.rs`).
- **2026-10-07** — Linux app icon for `cargo run`: `WindowOptions::{app_id,icon}`
  (X11) + `scripts/install-linux-dev-icon.sh` for Wayland panel matching.
- **2026-10-07** — Editor tab file icons: `tree_icon` + name as `Tab`
  children (`gap_2`, match file tree); avoid `Tab::prefix` (extra gap).
- **2026-10-07** — Sidebar width/visibility and terminal height persist per
  workspace/folder session (not globally).
- **2026-10-07** — Terminal tabs (cwds + visibility) persist in the workspace
  session and restore when reopening the editor.
- **2026-10-07** — Multi-tab terminals; root picker only when creating a
  session (New Terminal / first open), not when un-minimizing.
- **2026-10-07** — Multi-root terminal folder dialog: ↑↓ / Enter / Escape.
- **2026-10-07** — Terminal only while a folder/workspace is open (not on
  welcome).
- **2026-10-07** — Multi-root: opening the terminal asks which folder to use
  as the shell cwd.
- **2026-10-07** — Terminal open/closed state persisted in config
  (`terminal_visible`).
- **2026-10-07** — Panel chrome: status-bar Show/Minimize Terminal + Toggle
  Sidebar; terminal/sidebar sizes persisted in config across hide/show.
- **2026-10-07** — Terminal panel sits under the editor column only (file tree
  keeps full height).
- **2026-10-07** — Bottom integrated terminal: `v_resizable` panel, default
  shell via `portable-pty` + adapted `alacritty_terminal` view; View >
  Toggle Terminal / `Ctrl+\``.
- **2026-10-07** — Welcome Start: two columns (workspace | folder/file) with
  a vertical separator; unique icons per action.
- **2026-10-07** — Welcome Start: side-by-side rows + unique icons
  (LayoutDashboard / FolderClosed / FolderOpen / File / FileText).
- **2026-10-07** — Welcome + File menu Start grouping: Open/New Workspace,
  separator, then Open Folder / Open File / New File (`⌘N` / `Ctrl+N`).
- **2026-10-07** — Welcome + File menu: New Workspace… (pick folders, save
  `.isengard-workspace`) and New File… (`⌘N` / `Ctrl+N`; creates empty file).
- **2026-10-07** — Added [credits.md](credits.md): recognition for GPUI Kit,
  Zed/GPUI, Lucide, Tree-sitter, Ayu Darker / Ayu, fonts, Material Icon Theme,
  and direct crates.
- **2026-10-07** — Docs restructure: living hand-off under `docs/` (status,
  architecture, design) + single `changelog.md`; root `AGENTS.md` /
  `CLAUDE.md` are stubs. Removed duplicate DESIGN/CLAUDE changelogs.
- **2026-10-07** — Tab right-click: Close / Close Others / Close All (Kit
  `Tab`s in `h_flex` + `ContextMenu`; `TabBar` cannot host menus).
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
  buttons (docs/design.md §3).
- **2026-10-06** — Welcome screen: workspaces before folders ("Open Workspace…"
  first in Start; "Recent workspaces" above "Recent folders").
- **2026-10-06** — VS Code–style workspaces: multi-root folders, `.isengard-workspace`
  files (relative paths), Add Folder / Open / Save As / Close Workspace,
  root context menu, separate recent folders/workspaces lists, shared
  `open_choice_dialog`.
- **2026-10-06** — Added design rules doc. Extracted `destructive_icon_button`;
  the recent-folder remove × now matches the tab close × (red only on hover).
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
