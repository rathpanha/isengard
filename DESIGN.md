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
- **Light and dark:** every screen must work in both themes; because colors are
  semantic this is automatic — check both when adding something visual. Editor
  syntax colours come from `assets/themes/github-*-highlight.json` (not raw
  hex in UI chrome).

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

Add a row here whenever a new shared component is created.

## 5. Patterns

- **File tree icons:** full Material Icon Theme under
  `assets/icons/material/` (MIT; see its `LICENSE`) with associations from
  `assets/icons/material-icons.json`, via `file_tree::tree_icon`. Render with
  `img()` so upstream fills survive — GPUI Kit `Icon`/`svg()` alpha-masks and
  re-tints. Do not replace Lucide for other UI chrome.
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
- **Logo:** use `branding::LOGO_MARK` (block "I" + amber cursor) on light/dark
  surfaces; never recolor or round it. Brand colors (logo only, not UI): tile
  `#0b0b0c`, mark `#ececec`, cursor `#f5a524`.

## Changelog

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
