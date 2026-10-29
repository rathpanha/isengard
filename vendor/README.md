# Vendored crates

## `gpui-component`

Patched copy of crates.io `gpui-component` 0.7.1 (wired by `[patch.crates-io]`
in the repo `Cargo.toml`). Drop the patch (and this folder) once upstream
includes the same fixes.

### Completion docs popover

**Why:** documentation pane used a fixed `.w(max_width)` and measured free
space with editor-relative `pos.x`, so with a sidebar the docs walked past the
window edge and clipped.

**Change:** `src/input/popovers/completion_menu.rs` — content-sized list+docs
(sidebar-aware max width; do not stretch to full remaining width), stack when
side-by-side would squeeze either pane, scroll docs at max 240px height.

### Native menus (macOS)

**Change:** `src/native_menu/mod.rs` — `NativeMenu::show` uses the drawn
`fallback::show` popup on macOS instead of the system NSMenu, so the editor
right-click menu matches the Kit terminal / tab menus. Windows is unchanged.

### Dialogs

**Why:** Isengard wants instant modals (no enter motion), no floating panel
shadow, and a dim overlay that does not fill transparent window-shadow padding
on Linux.

**Change:** `src/dialog/dialog.rs` — `ANIMATION_DURATION = 0`; no panel drop
shadow; host box sized/positioned to the CSD content frame.

### Editor / Input context menu

**Change:** `Input` / `Editor::context_menu` receives the default Cut/Copy/…
menu so callers can append (editor adds Add Selection to Agent) without
rebuilding it.
