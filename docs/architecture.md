# Architecture

## Development setup

Stable Rust via [rustup](https://rustup.rs/). First `cargo run` compiles GPUI
and takes several minutes; later builds are fast. The `block v0.1.6`
future-incompat notice comes from a dependency — ignore it.

```sh
cargo run                       # welcome screen
cargo run -- /path/to/folder    # open a folder
cargo run -- /path/to/file.js   # open a file (+ parent folder)
cargo test
cargo clippy --all-targets      # must be warning-free
cargo build --release
```

Settings: `dirs::config_dir()/isengard/config.json` (macOS:
`~/Library/Application Support/isengard/config.json`). Delete to reset.

### macOS

1. Xcode Command Line Tools (`xcode-select --install`).
2. Rust via Homebrew's keg-only `rustup` (or rustup directly). If `cargo` is
   missing after Homebrew install:

   ```sh
   export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
   ```

3. App icon: automatic at runtime (`branding::set_dock_icon`) for `cargo run`.
   Bundled `.app` builds use `Isengard.icns` via cargo-bundle.

### Linux (Fedora example)

1. Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`, then
   `source "$HOME/.cargo/env"` (or open a new shell).
2. Link/build deps (adjust names on Debian/Ubuntu — usually `*-dev` instead of
   `*-devel`):

   ```sh
   sudo dnf install -y gcc clang cmake pkgconf-pkg-config \
     fontconfig-devel freetype-devel \
     libxkbcommon-devel libxkbcommon-x11-devel \
     libxcb-devel libX11-devel libXcursor-devel \
     wayland-devel alsa-lib-devel openssl-devel
   ```

3. App icon:
   - **X11:** automatic via `WindowOptions::icon` (`icon-1024.png`).
   - **Wayland:** one-time user `.desktop` + hicolor icon so the compositor can
     match `app_id` (`dev.isengard.editor`):

     ```sh
     ./scripts/install-dev-icon.sh   # or install-linux-dev-icon.sh
     ```

     Re-run if you move the repo (Exec path is absolute to `target/debug/isengard`).

### Windows

1. [rustup](https://rustup.rs/) with the **MSVC** toolchain
   (`x86_64-pc-windows-msvc`), not GNU.
2. [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   with the **Desktop development with C++** workload (MSVC + Windows SDK).
   Needed for linking GPUI and for `embed-resource` / `rc.exe` (`.exe` icon).
3. In a **Developer PowerShell** or a shell where `cargo` and the MSVC tools are
   on `PATH`:

   ```powershell
   cargo run
   ```

4. App icon: automatic — `build.rs` embeds `assets/logo/Isengard.ico` into every
   Windows binary (`cargo run` / `cargo build`). No install script. Optional
   reminder: `.\scripts\install-dev-icon.ps1`.

### App icon matrix (`cargo run`)

| Platform | Mechanism |
| --- | --- |
| macOS | `branding::set_dock_icon` (runtime Dock image) |
| Windows | `.exe` resource via `build.rs` + `assets/windows/isengard.rc` |
| Linux X11 | `WindowOptions::icon` |
| Linux Wayland | `WindowOptions::app_id` + `scripts/install-dev-icon.sh` |

`./scripts/install-dev-icon.sh` detects the OS and either installs (Linux) or
prints that nothing is needed (macOS / Git Bash on Windows).

## Source tree

```
src/
├── main.rs              app bootstrap: gpui_kit::init, fonts, theme, key bindings,
│                        menus, window (QuitMode::LastWindowClosed)
├── app.rs               IsengardApp view: actions, tabs, dialogs, layout, status bar
├── theme.rs             bundled font loading + Theme overrides (font, radius 0, sizes)
├── branding.rs          logo mark, APP_ID, Dock / window icon helpers
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
├── terminal/            bottom integrated PTY panel (adapted from gpui-terminal)
│   ├── panel.rs         TerminalPanel: Kit chrome, show/hide, restart on switch
│   ├── pty.rs           portable-pty session + default shell / cwd
│   ├── view.rs          TerminalView: grid paint + keyboard → PTY
│   ├── state.rs         alacritty_terminal Term wrapper
│   ├── render.rs        cell → GPUI paint (JetBrains Mono)
│   ├── input.rs         keystroke → bytes
│   ├── colors.rs        ANSI / theme palette
│   └── …                mouse, box_drawing, clipboard, event (v1 partial use)
└── ui/
    ├── components.rs    shared compositions encoding docs/design.md rules
    │                    (center_dialog)
    ├── file_tree.rs     FileTreePanel: FsNode -> TreeItem sync, TreeEvent handling, render
    └── welcome.rs       welcome screen (start actions + recent folders)
```

## How it fits together

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
- **Terminal:** `IsengardApp::terminal` is an `Entity<TerminalPanel>` (PTY
  session lazy on first show). When the tree is open, `v_resizable("editor-term-split")`
  is the right child of `main-split` (under editor only; tree full height).
  Without a tree, the same vertical split wraps the body. Panel omitted when
  minimized; `AppConfig.terminal_visible` + `terminal_height` restore open
  state and height (`resizable_panel().size`, saved on toggle / resize end).
  `apply_switch` restarts the shell in the first folder cwd (else home).
  Requires at least one workspace folder (hidden on welcome / Close
  Workspace). `ToggleTerminal` shows/hides existing tabs; `NewTerminal`
  always `add_session`. Multi-root create uses `pick_terminal_cwd` before
  `add_terminal_in`. Layout sizes live on `WorkspaceSession`
  (`sidebar_width` / `sidebar_visible` / `terminal_height`); resizable
  element ids include the session key so Kit state does not leak across
  folders. `ToggleSidebar` / `⌘B`/`Ctrl+B`.
- **Dialogs**: `window.open_dialog` with a `DialogFooter` of `Button`s; buttons
  capture a `WeakEntity<IsengardApp>` and call back into it. Always wrap with
  `ui::components::center_dialog` (Kit defaults to top ~10%; we centre). Close
  × buttons (tabs, recent rows, dialogs, toasts) use Kit's default ghost style
  — no custom red hover.
- **Theme**: `theme::apply` always uses `ThemeMode::Dark` (it reloads the
  theme config) and then overrides fonts, sizes, `radius`/`radius_lg = 0`, and
  `highlight_theme` (Ayu Darker tokens + editor/gutter bg = `theme.sidebar` so
  it matches the file tree), `notification.placement = BottomRight`, and
  `notification.margins.bottom` above the status bar. Re-run it after any
  font-size change.
  `theme.font_size` (14px) is the rem base for the whole UI;
  `mono_font_size` is the editor size from config.
- **Logo / icons** (`assets/logo/`): `logo.svg` is the master (1024 square,
  near-black tile). `logo-mark.svg` is the shapes only (welcome screen,
  `branding::LOGO_MARK`). `icon-macos.svg` puts the tile on Apple's 824px grid
  with a transparent margin and square corners (block style). Exports:
  `Isengard.icns`, `icon-1024.png` (macOS Dock, X11 window icon, cargo-bundle),
  `logo-512.png` (Linux hicolor via `scripts/install-dev-icon.sh`),
  `Isengard.ico` (Windows `.exe` via `build.rs` + `assets/windows/isengard.rc` +
  `embed-resource`). App id `branding::APP_ID` (`dev.isengard.editor`) is set
  on the window for Wayland `.desktop` matching. See **App icon matrix** above.
  Colors: tile `#0b0b0c`, mark `#ececec`, cursor `#f5a524`. The group is
  optically centred (halfway between bbox centre and area centroid). To
  re-export after editing an SVG: render PNGs with a transparent background
  using `resvg` (e.g. a tiny `resvg = "=0.45.1"` CLI; `qlmanage` fills
  transparency with white, don't use it), build the iconset sizes 16–512
  @1x/@2x, `iconutil -c icns`, and pack PNGs 16–256 into the .ico.
- **Fonts**: five JetBrains Mono Nerd Font weights are embedded with
  `include_bytes!` (~13 MB) and registered via `cx.text_system().add_fonts`.
  Family name: `"JetBrainsMono Nerd Font"`. Attribution and license paths:
  [credits.md](credits.md).

## Conventions

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
- Doc updates follow [README.md](README.md) (living topic file + one changelog
  bullet).

## Version pinning

`gpui-kit = "0.7.1"` pins an exact GPUI snapshot (`gpui-pre =0.3.8`). GPUI's API
changes between snapshots; upgrade `gpui-kit` as a whole and re-check every
API used here. Tree-sitter languages come from the `tree-sitter-languages`
feature (JSON is included via base `tree-sitter`). Detection lives in
`Language` in `editor/language.rs`. Direct dep `cc = "~1.2.1"` keeps
`tree-sitter-sequel` (SQL) resolvable against `embed-resource`. Terminal deps:
`alacritty_terminal = "0.25.1"`, `portable-pty = "0.9"` (plus `parking_lot`,
`flume`, `ropey` for the adapted view). Do not depend on crates.io
`gpui-terminal` — it targets a different GPUI; our panel is a local adaptation.
