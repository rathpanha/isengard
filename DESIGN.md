# Isengard — Design Rules

These rules keep the UI consistent. They are requirements, not suggestions:
every new or changed UI must follow them, and any new pattern the user approves
must be added here in the same change. When a rule and existing code disagree,
fix the code (or ask the user before changing the rule).

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
  (e.g. `danger.opacity(0.15)`). Exception: the file tree uses Material Icon
  Theme SVGs with their upstream fill colours (see §5).
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
  - Icon buttons are **muted** at rest (`muted_foreground` icon, no background).
  - On **hover** the icon turns `danger` red and the button gets a faint red
    tint (`danger.opacity(0.15)`); pressed: `danger.opacity(0.25)`.
  - Never show red at rest — red means "you're about to destroy something".
  - Always use `components::destructive_icon_button`; never restyle a `Button`
    by hand for this.
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
| `destructive_icon_button(id, icon, tooltip, hovered, on_click, cx)` | Close/remove/delete icon buttons. Pass `hovered` from `IsengardApp::is_destructive_hovered(&id)`. Position it by styling the returned wrapper (e.g. `.mr_2()`). |
| `center_dialog(dialog, window)` | Wrap every `open_dialog` builder so the modal sits in the middle of the window (GPUI Kit defaults to top ~10%). Not for context menus. |

Add a row here whenever a new shared component is created.

## 5. Patterns

- **Dirty tabs:** unsaved buffer shows a 6px square in `theme.yellow` before
  the close × (block style — no round “dot”).
- **Preview tabs:** single-click a file in the tree → italic preview tab
  (only one; the next preview replaces it unless dirty, then it pins).
  Double-click the file or the tab, or edit the buffer → permanent. File >
  Open File / CLI opens permanent.
- **File tree icons:** full Material Icon Theme under
  `assets/icons/material/` (MIT; see its `LICENSE`) with associations from
  `assets/icons/material-icons.json`, via `file_tree::tree_icon`. Render with
  `img()` so upstream fills survive — GPUI Kit `Icon`/`svg()` alpha-masks and
  re-tints. Do not replace Lucide for other UI chrome.
- **File tree indent guides:** vertical 1px lines in each depth column
  (`sidebar_border`) when a sibling exists below that level (VS Code–style).
  Indent columns are fixed-width (`flex_shrink_0`); a narrow panel truncates
  labels instead of compressing the tree.
- **Gitignored entries:** still listed; icon + label at 60% opacity (not
  hidden). Matched via `ignore` + each root's `.gitignore` tree.
- **Title bar:** macOS shows the centred window title; Windows/Linux show the
  in-window `AppMenuBar`. No theme toggle (dark only).
- **Keyboard shortcut hints:** one `Kbd` key cap per key with `gap_1`
  (`⌘` `O`, `Ctrl` `O`) — see `welcome::shortcut_keys`. Never a single cap with
  the keys run together.
- **Empty states:** don't show chrome that has nothing to say. The status bar
  exists only while a file tab is active; the file tree only while a folder is
  open; the welcome screen replaces the workspace when nothing is open.
- **Links vs buttons:** navigation-like actions on the welcome screen
  (Open Folder, recent folders) are `Button::link()`; commands in dialogs are
  regular buttons with one `primary`.
- **Secondary text:** paths and hints use `muted_foreground` and a smaller
  size (`text_sm`/`text_xs`).
- **Workspaces:**
  - Naming: a single folder shows its name; a workspace file or several folders
    show `<name> (Workspace)` (`Untitled (Workspace)` before it is saved). The
    same text is used for the window title and, upper-cased, the tree header.
  - Multi-root tree: each root is a top-level item; a root missing on disk is
    shown disabled as `<name> (missing)` with a folder icon.
  - Actions on a tree item live in its right-click menu (`Tree::context_menu`),
    e.g. "Remove Folder from Workspace" on roots.
- **Welcome "Start" actions,** top to bottom: Open Workspace…, Open Folder…
  (with its shortcut), Open File….
- **Workspaces before folders** wherever both appear (Start actions, recent
  lists, menus).
- **Recent lists:** one titled section per kind ("Recent workspaces", then
  "Recent folders"); a section is hidden when empty, and when all are empty
  a single "Recent" section says so. Rows: link-style name, muted parent path,
  `destructive_icon_button` to remove.
- **Logo:** use `branding::LOGO_MARK` (block "I" + amber cursor) on the dark
  UI; never recolor or round it. Brand colors (logo only, not UI): tile
  `#0b0b0c`, mark `#ececec`, cursor `#f5a524`.

## Changelog

- **2026-10-07** — Unsaved tab indicator: yellow square (`theme.yellow`).
- **2026-10-07** — Preview tabs: italic until double-click / edit.
- **2026-10-07** — Gitignored tree rows: 60% opacity.
- **2026-10-07** — Editor background matches file-tree sidebar; Dockerfile
  language alias (Bash highlighter).
- **2026-10-07** — Project GPL-3.0-only; CONTRIBUTING: no PRs.
- **2026-10-07** — Syntax tokens: Ayu Darker from
  k4yt3x/zed-theme-ayu-darker (GPL-3.0); `assets/themes/LICENSE` is GPL.
- **2026-10-07** — Syntax tokens: Ayu Dark (MIT, from Zed). Gruvbox removed.
  (Superseded same day by Ayu Darker.)
- **2026-10-07** — Dark mode only; Gruvbox Dark syntax tokens (GitHub themes
  and the light/dark toggle removed).
- **2026-10-06** — Title-bar sun/moon icon toggles light/dark theme.
- **2026-10-06** — Editor syntax: GitHub Dark / Light token colours only
  (no editor background override).
- **2026-10-06** — File tree uses the full Material Icon Theme pack +
  association JSON (not a curated subset).
- **2026-10-06** — File tree Material icons via `img()` (keep colours); GPUI
  `Icon`/`svg()` would alpha-mask them.
- **2026-10-06** — File tree uses a Material Icon Theme subset (type-specific
  file/folder icons); other UI stays on Lucide.
- **2026-10-06** — Pointer cursor on everything clickable.
- **2026-10-06** — Workspaces before folders: Open Workspace first in Start,
  Recent workspaces above Recent folders.
- **2026-10-06** — Workspace naming, multi-root tree, context menus, recent
  lists, and the shared choice dialog.
- **2026-10-06** — Created: foundation, spacing, destructive-action rule,
  shared `destructive_icon_button`, shortcut/empty-state/logo patterns.
