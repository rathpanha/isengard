# Credits & recognition

Isengard stands on other people's work. This page names the projects and
people we rely on. Full legal text lives next to each asset or in the
dependency's crate; this is recognition, not a substitute for those licenses.

Isengard itself is **GPL-3.0-only** — see [LICENSE](../LICENSE) and
[CONTRIBUTING.md](../CONTRIBUTING.md).

## UI framework & components

| Project | Who | License | How we use it |
| --- | --- | --- | --- |
| [GPUI Kit](https://gpui-kit.com) (`gpui-kit` / `gpui-component`) | [Longbridge](https://github.com/longbridge/gpui-kit) | Apache-2.0 | Desktop UI components (Tree, Editor, Dialog, menus, …) |
| [GPUI](https://gpui.rs) (`gpui-pre`) | [Zed Industries](https://zed.dev) (Nathan Sobo and contributors) | Apache-2.0 | GPU UI runtime under GPUI Kit |

Lucide icons ship with GPUI Kit's default assets ([Lucide](https://lucide.dev))
and power most of our chrome (buttons, menus, etc.).

## Syntax highlighting

| Project | Who | License | How we use it |
| --- | --- | --- | --- |
| Tree-sitter grammars (via GPUI Kit `tree-sitter-languages`) | Tree-sitter community / grammar authors | various (per grammar) | Language parsing and highlight scopes in the editor |
| [Ayu Darker](https://github.com/k4yt3x/zed-theme-ayu-darker) (Zed theme) | [k4yt3x](https://github.com/k4yt3x) | GPL-3.0 | Editor token colours (`assets/themes/`) |
| [Ayu](https://github.com/dempfi/ayu) | [Ike Ku](https://github.com/dempfi) | (original palette) | Palette Ayu Darker builds on |
| GraphQL highlight query | Adapted from [nvim-treesitter](https://github.com/nvim-treesitter/nvim-treesitter) / [Helix](https://github.com/helix-editor/helix) queries for [joowani/tree-sitter-graphql](https://github.com/joowani/tree-sitter-graphql) | as upstream | `assets/highlights/graphql.scm` (Kit ships an empty GraphQL query) |

Theme notice: [`assets/themes/NOTICE.txt`](../assets/themes/NOTICE.txt).

## Fonts

| Project | Who | License | How we use it |
| --- | --- | --- | --- |
| [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) | JetBrains / The JetBrains Mono Project Authors | SIL OFL 1.1 | Base UI + editor typeface |
| [Nerd Fonts](https://github.com/ryanoasis/nerd-fonts) | [Ryan L McIntyre](https://github.com/ryanoasis) and contributors | OFL (fonts) / MIT (patcher) | Glyphs patched into our JetBrains Mono Nerd Font files |

License files: [`assets/fonts/OFL.txt`](../assets/fonts/OFL.txt),
[`assets/fonts/LICENSE-NerdFonts.txt`](../assets/fonts/LICENSE-NerdFonts.txt).

## Icons (file tree)

| Project | Who | License | How we use it |
| --- | --- | --- | --- |
| [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) | [Material Extensions](https://github.com/material-extensions) | MIT | File/folder SVGs + `material-icons.json` associations |

License: [`assets/icons/material/LICENSE`](../assets/icons/material/LICENSE).

## Rust crates (direct)

These are the crates we declare in `Cargo.toml` (transitive crates are too
many to list; `cargo tree` is the full graph).

| Crate | Role |
| --- | --- |
| `gpui-kit` | UI (see above) |
| `anyhow` | Error context |
| `serde` / `serde_json` | Config and workspace JSON |
| `dirs` | Config directory path |
| `log` / `env_logger` | Logging |
| `rust-embed` | Bundled assets |
| `ignore` | `.gitignore` matching for muted tree rows |
| `alacritty_terminal` | VTE / terminal grid state for the integrated panel |
| `portable-pty` | PTY + default shell spawn (wezterm) |
| `parking_lot` / `flume` / `ropey` | Sync + channels for the adapted terminal view |
| `cc` | Build pin for tree-sitter/SQL + Windows resource embed |
| `embed-resource` | Windows `.exe` icon (build) |
| `objc2` / `objc2-app-kit` / `objc2-foundation` | macOS Dock icon at runtime |
| `image` | Decode PNG for `WindowOptions::icon` (X11) |
| `tempfile` | Tests only |

Packaging metadata targets [cargo-bundle](https://github.com/burtonageo/cargo-bundle)
for macOS `.app` / Linux `.deb` builds (dev tool, not a runtime dependency).

## Inspiration (not copied)

Workspace UX (multi-root folders, preview tabs, session restore) and the
bottom integrated terminal follow patterns familiar from **Visual Studio Code**
and other editors. That is behavioural inspiration, not copied source.

The terminal view under `src/terminal/` is adapted from
[gpui-terminal](https://github.com/zortax/gpui-terminal) (MIT/Apache-2.0) for
our GPUI Kit / `gpui-pre` 0.3.8 pin — the crates.io crate targets a different
GPUI and is not a direct dependency.

## Keeping this page honest

When you add a bundled asset, vendored query, or notable direct dependency,
add a row here in the same change and keep the license file next to the asset.
See [README.md](README.md) (agent protocol).
