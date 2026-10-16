# Vendored crates

## `gpui-component`

Patched copy of crates.io `gpui-component` 0.7.1.

**Why:** completion documentation popover used a fixed `.w(max_width)` and
measured free space with editor-relative `pos.x`, so with a sidebar the docs
pane was laid out past the window edge and clipped.

**Change:** `src/input/popovers/completion_menu.rs` — content-sized list+docs
(sidebar-aware max width; do not stretch to full remaining width), stack when
side-by-side would squeeze either pane, scroll docs at max 240px height.

Wired via `[patch.crates-io]` in the repo `Cargo.toml`. Drop the patch (and
this folder) once upstream includes the same fix.
