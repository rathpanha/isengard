# Isengard

**The already configured editor.**

Cross-platform desktop code editor built in Rust on [GPUI Kit](https://gpui-kit.com).
**v0.1.9 ships Linux only** (`.deb`, `.rpm`, and a binary tarball). macOS and
Windows builds are not released yet.

## Install (Linux)

Grab the asset for your CPU from the
[GitHub Releases](https://github.com/rathpanha/isengard/releases) page.

| Distro family | Package |
| --- | --- |
| Debian / Ubuntu / Mint / … | `isengard_*_amd64.deb` (or `_arm64`) |
| Fedora / RHEL / openSUSE / … | `isengard-*.x86_64.rpm` (or `*.aarch64.rpm`) |
| Any (Arch, Void, manual) | `isengard-*-linux-*.tar.gz` |

```sh
# Debian/Ubuntu
sudo apt install ./isengard_*_amd64.deb

# Fedora
sudo dnf install ./isengard-*.x86_64.rpm

# Tarball
tar -xzf isengard-*-linux-*.tar.gz
sudo install -m755 isengard-*/bin/isengard /usr/local/bin/isengard
# …copy .desktop + icon from share/ (see README.txt inside the archive)
```

App id / Wayland window class: `app.isengard.edtir` (icons and `.desktop` are
included in the packages).

Verify downloads with `SHA256SUMS` on the release.

### After install

- **Language servers** are not inside the package. Use the status-bar LSP menu
  → **Install** (downloads into `~/.local/share/isengard/lsp/` — never into
  your system toolchain).
- **Claude Agent** is optional: Install / Login from the Agent panel. No API
  key is stored in Isengard; you sign in with the agent’s own flow.

## Build from source

See [docs/architecture.md](docs/architecture.md) (Rust via rustup, Linux
system packages, `cargo run` / `cargo build --release`).

Package on Linux after a release build:

```sh
cargo build --release
./scripts/package-linux.sh   # needs `nfpm` on PATH → writes dist/linux/
```

## License

**GPL-3.0-only** — [LICENSE](LICENSE). Third-party credits:
[docs/credits.md](docs/credits.md).

Forks and private copies are fine. **Pull requests are not accepted** —
[CONTRIBUTING.md](CONTRIBUTING.md).

## Docs

| Doc | What |
| --- | --- |
| [docs/status.md](docs/status.md) | What works / deferred |
| [docs/architecture.md](docs/architecture.md) | Layout, bootstrap, conventions |
| [docs/design.md](docs/design.md) | UI rules |
| [docs/changelog.md](docs/changelog.md) | History |
