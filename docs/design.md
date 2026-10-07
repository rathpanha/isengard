# Design rules

These rules keep the UI consistent. They are requirements, not suggestions:
every new or changed UI must follow them, and any new pattern the user approves
must be added here in the same change. When a rule and existing code disagree,
fix the code (or ask the user before changing the rule).

History goes in [changelog.md](changelog.md) only — do not add a Changelog
section here.

## 1. Foundation

- **Components:** build UI from GPUI Kit components only. Don't hand-build a
  widget the library already has. When a pattern repeats, wrap it once in
  `src/ui/components.rs` and reuse it (see §4).
- **Shape — block style:** no rounded corners anywhere in the app. The theme
  sets `radius = 0` and `radius_lg = 0` (`src/theme.rs`); never pass a custom
  radius. (The macOS window frame and Dock mask are drawn by the OS and are
  outside our control.)
- **Font:** JetBrains Mono Nerd Font for everything — UI and editor
  (`theme::FONT_FAMILY`). UI base size 14px (the rem base), editor size from
  settings.
- **Color:** only semantic theme colors from `cx.theme()` — `foreground`,
  `muted_foreground`, `background`, `border`, `danger`, `sidebar`, … No raw
  hex/rgb in UI code. Opacity variations of a theme color are fine
  (e.g. `danger.opacity(0.15)`). Exceptions: the file tree uses Material Icon
  Theme SVGs with their upstream fill colours (see §5); the terminal ANSI
  16-colour table may keep documented defaults in `terminal::colors` (bg/fg/
  cursor mapped from `cx.theme()` where possible).
- **Dark only:** the app is dark mode permanently — no light theme, no toggle.
  Editor syntax colours come from `assets/themes/ayu-darker-highlight.json`
  (GPL-3.0); editor/gutter background is forced to `theme.sidebar` (same as the
  file tree).

## 2. Spacing

- Use GPUI's rem-based helpers (`gap_2`, `px_3`, `mr_2`, …), not `px(..)`,
  except for documented physical sizes.
- **Edge symmetry:** an element at the trailing edge of a container sits the
  same distance from that edge as the content at the leading edge. Example: the
  tab's close × is 12px from the tab's right border, matching the label's 12px
  left padding.
- Related controls in a row: `gap_3`; tightly grouped items (e.g. key caps):
  `gap_1`; sections on a page: `gap_8`.

## 3. Interaction states

- **Destructive actions** (close, remove, delete, discard):
  - Icon-only close/remove buttons use Kit's default ghost `Button` (no custom
    red hover). Keep `.cursor_pointer()`, `.xsmall()`, and a tooltip.
  - Destructive commands with data loss ask first, using
    `IsengardApp::open_choice_dialog`: buttons in the order Cancel · secondary
    ("Don't Save") · primary (the safe default, e.g. "Save", "Save All").
- **Modal dialogs** sit in the middle of the window — wrap every
  `open_dialog` with `components::center_dialog`. Context menus stay
  click-anchored.
- **Pointer cursor** on everything clickable (buttons, tabs, tree/list rows,
  links); disabled items and plain text keep the default arrow, text areas the
  I-beam. GPUI Kit only does this itself for `link()`/`text()` buttons, so add
  `.cursor_pointer()` to every other clickable component (for list/tree rows:
  `.when(!disabled, |item| item.cursor_pointer())`). Shared components in
  `src/ui/components.rs` already include it.
- **Tooltips** on every icon-only button, naming the action ("Close",
  "Remove from recent").

## 4. Shared components (`src/ui/components.rs`)

| Component | Use for |
| --- | --- |
| `center_dialog(dialog, window)` | Wrap every `open_dialog` builder so the modal sits in the middle of the window (GPUI Kit defaults to top ~10%). Not for context menus. |

Add a row here whenever a new shared component is created.

## 5. Patterns

- **Dirty tabs:** unsaved buffer shows a 6px square in `theme.blue` before
  the close × (block style — no round “dot”).
- **Markdown / SVG preview:** `.md` and `.svg` tabs show an Edit / Preview
  `ButtonGroup` under the tab bar. Markdown preview uses
  `TextView::markdown`; SVG preview renders the buffer via
  `Image::from_bytes(Svg)` (live, including unsaved edits). Toggle also via
  View > Toggle Preview or `⌘⇧V` / `Ctrl+Shift+V`. Distinct from **preview
  tabs** (italic, temporary).
- **Image preview:** raster images (png/jpg/gif/webp/…) open as a centred
  `img(path)` pane (no text editor). Status bar shows "Image". SVG is not in
  this list — it uses Edit/Preview above.
- **Preview tabs:** single-click a file in the tree → italic preview tab
  (only one; the next preview replaces it unless dirty, then it pins).
  Double-click the file or the tab, or edit the buffer → permanent. File >
  Open File / CLI opens permanent.
- **Tab context menu:** right-click a tab → Close, Close Others (disabled when
  only one tab), Close All. Dirty tabs get one Save All / Don't Save dialog
  for Close Others / Close All. Implemented as Kit `Tab`s in an `h_flex` (not
  `TabBar`) because `TabBar::children` requires bare `Tab` and cannot host
  `ContextMenu`.
- **File tree icons:** full Material Icon Theme under
  `assets/icons/material/` (MIT; see its `LICENSE`) with associations from
  `assets/icons/material-icons.json`, via `file_tree::tree_icon`. Render with
  `img()` so upstream fills survive — GPUI Kit `Icon`/`svg()` alpha-masks and
  re-tints. Do not replace Lucide for other UI chrome.
- **File tree indent guides:** vertical 1px lines in each ancestor column
  (`sidebar_border`) through every descendant row (including the last child),
  so a folder's rail runs the full height of its contents. Multi-root: col 0
  stops before the next workspace root (no line between roots). Each deeper
  level adds one `INDENT_COL`. Guide segments use a 1px vertical overlap;
  ListItem `py_0`/`px_0` so hover is full-bleed to the panel edge; row content
  keeps `px_3`. A narrow panel truncates labels instead of compressing the tree.
- **Gitignored entries:** still listed; icon + label at 60% opacity (not
  hidden). Matched via `ignore` + each root's `.gitignore` tree.
- **Title bar:** macOS shows the centred window title; Windows/Linux show the
  in-window `AppMenuBar`. No theme toggle (dark only).
- **Keyboard shortcut hints:** one `Kbd` key cap per key with `gap_1`
  (`⌘` `O`, `Ctrl` `O`) — see `welcome::shortcut_keys`. Never a single cap with
  the keys run together.
- **Empty states:** don't show chrome that has nothing to say. The file tree
  only while a folder is open and the sidebar is not minimized; the welcome
  screen replaces the workspace when nothing is open; the terminal panel is
  omitted entirely when minimized (no empty strip). Status bar only while a
  folder/workspace is open (panel toggles; path / Ln·Col when a file tab is
  active) — not on welcome.
- **Bottom terminal panel:** Kit `v_resizable("editor-term-split")` under the
  **editor column only** (file tree stays full height in `main-split`); when
  the sidebar is hidden, under the full editor body. Requires an open
  folder/workspace (not available on welcome). Open/closed + height remembered
  in `config.terminal_visible` / `terminal_height` (default closed, 200px).
  Header: tab strip (title = folder name) + `Plus` (new) + `Minus` (minimize).
  Toggle (`Ctrl+\`` / status-bar) shows/hides without spawning; if no sessions
  yet, creates the first. **New Terminal** (`Ctrl+Shift+\`` / `Plus` / View menu)
  always spawns a tab. Multi-root: root picker only when creating a session
  (↑↓ / Enter / Cancel), not when un-minimizing. Tab cwds, visibility, and
  panel height are stored per workspace in `sessions` (shells respawn).
- **Sidebar toggle:** file tree can be hidden while the workspace stays open.
  Width + visibility remembered per workspace in `sessions` (default 260px /
  visible). Toggle via status-bar icon, View > Toggle Sidebar, or `⌘B` /
  `Ctrl+B`.
- **Notifications:** bottom-right (`theme.notification.placement =
  BottomRight` in `theme::apply`). Don't set placement per toast.
- **Links vs buttons:** navigation-like actions on the welcome screen
  (Open Folder, recent folders) are `Button::link()`; commands in dialogs are
  regular buttons with one `primary`.
- **Secondary text:** paths and hints use `muted_foreground` and a smaller
  size (`text_sm`/`text_xs`).
- **Workspaces:**
  - Naming: a single folder shows its name; a workspace file or several folders
    show `<name> (Workspace)` (`Untitled (Workspace)` before it is saved). Used
    for the window / title-bar title only (no duplicate label above the tree).
  - Session restore: reopening a folder or workspace (welcome recent, Open, or
    CLI) restores its last open tabs (cursor + scroll), and expanded tree
    directories from `config.json` `sessions`. Multi-root roots stay collapsed
    unless they were expanded when saved. Unsaved edits are not restored;
    cold start still shows the welcome screen.
  - Multi-root tree: each root is a top-level item; a root missing on disk is
    shown disabled as `<name> (missing)` with a folder icon.
  - Actions on a tree item live in its right-click menu (`Tree::context_menu`),
    e.g. "Remove Folder from Workspace" on roots.
- **Welcome "Start" actions:** two columns separated by a vertical
  `Separator` — left: Open Workspace…, New Workspace…; right: Open Folder…
  (`⌘O` / `Ctrl+O`), Open File…, New File… (`⌘N` / `Ctrl+N`). File menu keeps
  the same order stacked (workspace group, separator, folder/file group).
  Distinct Kit icons: `LayoutDashboard`, `FolderClosed`, `FolderOpen`, `File`,
  `FileText`.
- **Workspaces before folders** wherever both appear (Start actions, recent
  lists, menus).
- **Recent lists:** one titled section per kind ("Recent workspaces", then
  "Recent folders"); a section is hidden when empty, and when all are empty
  a single "Recent" section says so. Rows: link-style name, muted parent path,
  ghost close × to remove.
- **Logo:** use `branding::LOGO_MARK` (block "I" + amber cursor) on the dark
  UI; never recolor or round it. Brand colors (logo only, not UI): tile
  `#0b0b0c`, mark `#ececec`, cursor `#f5a524`.
- **Slogan:** welcome subtitle and About / bundle blurb —
  "The already configured editor" (no settings-panel cosplay). About may add
  one short supporting line; don't invent a second tagline.
