//! App-local language-server install / uninstall.
//!
//! Downloads land under `dirs::data_dir()/isengard/lsp/` (bin + pkg). Resolve
//! prefers that tree, then falls back to the system `PATH`. Install never runs
//! global package-manager commands (`npm -g`, `pip`, `brew`, …).

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use tar::Archive;

/// How to fetch a language server into the app data dir.
#[derive(Debug, Clone, Copy)]
pub enum FetchKind {
    /// GitHub / release asset for this host.
    Binary {
        select: fn() -> Option<BinaryAsset>,
    },
    /// `GOBIN=<lsp/bin> go install <module>@<version>` (app-local only).
    GoInstall {
        module: &'static str,
        version: &'static str,
    },
    /// `npm install --prefix <lsp/pkg/…>` (needs system `npm`; no `-g`).
    Npm {
        packages: &'static [NpmSpec],
    },
}

#[derive(Debug, Clone, Copy)]
pub struct NpmSpec {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub enum AssetFormat {
    /// `.gz` wrapping a single binary (no tar).
    GzipFile,
    /// Uncompressed executable bytes.
    RawBinary,
    TarGz,
    TarXz,
    Zip,
}

/// One host-specific download.
#[derive(Debug, Clone, Copy)]
pub struct BinaryAsset {
    pub url: &'static str,
    pub format: AssetFormat,
    /// Path of the executable inside the archive (empty for single-file formats).
    pub member: &'static str,
    /// Keep the full extract under `pkg/<id>/` and wrap `bin/` (needed when the
    /// binary loads sibling files, e.g. lua-language-server).
    pub keep_tree: bool,
}

/// One-click install recipe shown in dialogs / status-bar catalog.
#[derive(Debug, Clone, Copy)]
pub struct InstallRecipe {
    /// Short name shown in dialogs.
    pub name: &'static str,
    /// One-line explanation.
    pub summary: &'static str,
    /// Subdirectory under `lsp/pkg/` (and uninstall key).
    pub pkg_id: &'static str,
    /// Names written into `lsp/bin/` (may be several for one npm package).
    pub bin_names: &'static [&'static str],
    pub fetch: FetchKind,
}

impl InstallRecipe {
    /// Dialog line describing what Install will do.
    pub fn install_hint(&self) -> String {
        match self.fetch {
            FetchKind::Binary { .. } => {
                format!("Download {0} into Isengard’s data folder (not your system PATH).", self.name)
            }
            FetchKind::GoInstall { module, version } => {
                format!(
                    "Run `go install {module}@{version}` with GOBIN set to Isengard’s lsp/bin."
                )
            }
            FetchKind::Npm { packages } => {
                let list: Vec<_> = packages
                    .iter()
                    .map(|p| format!("{}@{}", p.name, p.version))
                    .collect();
                format!(
                    "Run `npm install --prefix …/isengard/lsp/pkg/{}` ({}) — not global.",
                    self.pkg_id,
                    list.join(", ")
                )
            }
        }
    }
}

/// Root: `…/isengard/lsp`.
pub fn lsp_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("isengard")
        .join("lsp")
}

pub fn bin_dir(root: &Path) -> PathBuf {
    root.join("bin")
}

pub fn pkg_dir(root: &Path, pkg_id: &str) -> PathBuf {
    root.join("pkg").join(pkg_id)
}

fn command_basename(command: &str) -> &str {
    Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command)
}

fn bin_candidate(root: &Path, basename: &str) -> PathBuf {
    let dir = bin_dir(root);
    #[cfg(windows)]
    {
        let exe = dir.join(format!("{basename}.exe"));
        if exe.is_file() {
            return exe;
        }
        let cmd = dir.join(format!("{basename}.cmd"));
        if cmd.is_file() {
            return cmd;
        }
    }
    dir.join(basename)
}

/// `true` when Isengard has an app-local binary/wrapper for this command.
pub fn is_app_local(command: &str) -> bool {
    is_app_local_in(command, &lsp_root())
}

pub fn is_app_local_in(command: &str, root: &Path) -> bool {
    let path = Path::new(command);
    if path.is_absolute() || command.contains('/') || command.contains('\\') {
        return false;
    }
    bin_candidate(root, command_basename(command)).is_file()
}

/// Resolve executable: absolute path as-is → app-local bin → system `PATH`.
pub fn resolve_command(command: &str) -> Option<PathBuf> {
    resolve_command_in(command, &lsp_root())
}

pub fn resolve_command_in(command: &str, root: &Path) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() || command.contains('/') || command.contains('\\') {
        return path.is_file().then(|| path.to_path_buf());
    }
    let local = bin_candidate(root, command_basename(command));
    if local.is_file() {
        return Some(local);
    }
    find_on_path(command)
}

fn find_on_path(command: &str) -> Option<PathBuf> {
    let path_os = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_os) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{command}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// Known install recipe for this server binary (basename of `command`).
pub fn recipe_for_command(command: &str) -> Option<InstallRecipe> {
    let name = command_basename(command);

    Some(match name {
        "rust-analyzer" => InstallRecipe {
            name: "rust-analyzer",
            summary: "Rust IntelliSense (completion, hover, go-to-definition).",
            pkg_id: "rust-analyzer",
            bin_names: &["rust-analyzer"],
            fetch: FetchKind::Binary {
                select: rust_analyzer_asset,
            },
        },
        "gopls" => InstallRecipe {
            name: "gopls",
            summary: "Go language server.",
            pkg_id: "gopls",
            bin_names: &["gopls"],
            fetch: FetchKind::GoInstall {
                module: "golang.org/x/tools/gopls",
                version: "v0.20.0",
            },
        },
        "typescript-language-server" => InstallRecipe {
            name: "typescript-language-server",
            summary: "JavaScript / TypeScript language server.",
            pkg_id: "typescript-language-server",
            bin_names: &["typescript-language-server"],
            fetch: FetchKind::Npm {
                packages: &[
                    NpmSpec {
                        name: "typescript",
                        version: "5.7.3",
                    },
                    NpmSpec {
                        name: "typescript-language-server",
                        version: "4.3.3",
                    },
                ],
            },
        },
        "pyright-langserver" | "pyright" => InstallRecipe {
            name: "pyright",
            summary: "Python language server (Pyright).",
            pkg_id: "pyright",
            bin_names: &["pyright-langserver", "pyright"],
            fetch: FetchKind::Npm {
                packages: &[NpmSpec {
                    name: "pyright",
                    version: "1.1.391",
                }],
            },
        },
        "vscode-json-language-server"
        | "vscode-css-language-server"
        | "vscode-html-language-server" => InstallRecipe {
            name: "vscode-langservers-extracted",
            summary: "JSON / CSS / HTML language servers.",
            pkg_id: "vscode-langservers-extracted",
            bin_names: &[
                "vscode-json-language-server",
                "vscode-css-language-server",
                "vscode-html-language-server",
            ],
            fetch: FetchKind::Npm {
                packages: &[NpmSpec {
                    name: "vscode-langservers-extracted",
                    version: "4.10.0",
                }],
            },
        },
        "yaml-language-server" => InstallRecipe {
            name: "yaml-language-server",
            summary: "YAML language server.",
            pkg_id: "yaml-language-server",
            bin_names: &["yaml-language-server"],
            fetch: FetchKind::Npm {
                packages: &[NpmSpec {
                    name: "yaml-language-server",
                    version: "1.15.0",
                }],
            },
        },
        "bash-language-server" => InstallRecipe {
            name: "bash-language-server",
            summary: "Bash language server.",
            pkg_id: "bash-language-server",
            bin_names: &["bash-language-server"],
            fetch: FetchKind::Npm {
                packages: &[NpmSpec {
                    name: "bash-language-server",
                    version: "5.4.0",
                }],
            },
        },
        "dockerfile-language-server-nodejs" | "docker-langserver" => InstallRecipe {
            name: "dockerfile-language-server",
            summary: "Dockerfile language server.",
            pkg_id: "dockerfile-language-server",
            bin_names: &["docker-langserver"],
            fetch: FetchKind::Npm {
                packages: &[NpmSpec {
                    name: "dockerfile-language-server-nodejs",
                    version: "0.13.0",
                }],
            },
        },
        "svelteserver" => InstallRecipe {
            name: "svelte-language-server",
            summary: "Svelte language server.",
            pkg_id: "svelte-language-server",
            bin_names: &["svelteserver"],
            fetch: FetchKind::Npm {
                packages: &[NpmSpec {
                    name: "svelte-language-server",
                    version: "0.17.10",
                }],
            },
        },
        "astro-ls" => InstallRecipe {
            name: "astro-ls",
            summary: "Astro language server.",
            pkg_id: "astro-language-server",
            bin_names: &["astro-ls"],
            fetch: FetchKind::Npm {
                packages: &[NpmSpec {
                    name: "@astrojs/language-server",
                    version: "2.15.4",
                }],
            },
        },
        "taplo" => InstallRecipe {
            name: "taplo",
            summary: "TOML language server.",
            pkg_id: "taplo",
            bin_names: &["taplo"],
            fetch: FetchKind::Binary {
                select: taplo_asset,
            },
        },
        "marksman" => InstallRecipe {
            name: "marksman",
            summary: "Markdown language server.",
            pkg_id: "marksman",
            bin_names: &["marksman"],
            fetch: FetchKind::Binary {
                select: marksman_asset,
            },
        },
        "zls" => InstallRecipe {
            name: "zls",
            summary: "Zig language server.",
            pkg_id: "zls",
            bin_names: &["zls"],
            fetch: FetchKind::Binary {
                select: zls_asset,
            },
        },
        "lua-language-server" => InstallRecipe {
            name: "lua-language-server",
            summary: "Lua language server.",
            pkg_id: "lua-language-server",
            bin_names: &["lua-language-server"],
            fetch: FetchKind::Binary {
                select: lua_ls_asset,
            },
        },
        _ => return None,
    })
}

fn host_os_arch() -> Option<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => return None,
    };
    Some((os, arch))
}

fn rust_analyzer_asset() -> Option<BinaryAsset> {
    // Pinned release: https://github.com/rust-lang/rust-analyzer/releases/tag/2025-12-22
    const TAG: &str = "2025-12-22";
    let (os, arch) = host_os_arch()?;
    let (file, format) = match (os, arch) {
        ("darwin", "aarch64") => (
            "rust-analyzer-aarch64-apple-darwin.gz",
            AssetFormat::GzipFile,
        ),
        ("darwin", "x86_64") => (
            "rust-analyzer-x86_64-apple-darwin.gz",
            AssetFormat::GzipFile,
        ),
        ("linux", "aarch64") => (
            "rust-analyzer-aarch64-unknown-linux-gnu.gz",
            AssetFormat::GzipFile,
        ),
        ("linux", "x86_64") => (
            "rust-analyzer-x86_64-unknown-linux-gnu.gz",
            AssetFormat::GzipFile,
        ),
        ("windows", "aarch64") => (
            "rust-analyzer-aarch64-pc-windows-msvc.zip",
            AssetFormat::Zip,
        ),
        ("windows", "x86_64") => (
            "rust-analyzer-x86_64-pc-windows-msvc.zip",
            AssetFormat::Zip,
        ),
        _ => return None,
    };
    let member = if matches!(format, AssetFormat::Zip) {
        "rust-analyzer.exe"
    } else {
        ""
    };
    Some(BinaryAsset {
        url: leak_url(format!(
            "https://github.com/rust-lang/rust-analyzer/releases/download/{TAG}/{file}"
        )),
        format,
        member,
        keep_tree: false,
    })
}

fn taplo_asset() -> Option<BinaryAsset> {
    const TAG: &str = "0.10.0";
    let (os, arch) = host_os_arch()?;
    let file = match (os, arch) {
        ("darwin", "aarch64") => "taplo-darwin-aarch64.gz",
        ("darwin", "x86_64") => "taplo-darwin-x86_64.gz",
        ("linux", "aarch64") => "taplo-linux-aarch64.gz",
        ("linux", "x86_64") => "taplo-linux-x86_64.gz",
        ("windows", "aarch64") => "taplo-windows-aarch64.zip",
        ("windows", "x86_64") => "taplo-windows-x86_64.zip",
        _ => return None,
    };
    let format = if file.ends_with(".zip") {
        AssetFormat::Zip
    } else {
        AssetFormat::GzipFile
    };
    let member = if matches!(format, AssetFormat::Zip) {
        "taplo.exe"
    } else {
        ""
    };
    Some(BinaryAsset {
        url: leak_url(format!(
            "https://github.com/tamasfe/taplo/releases/download/{TAG}/{file}"
        )),
        format,
        member,
        keep_tree: false,
    })
}

fn marksman_asset() -> Option<BinaryAsset> {
    const TAG: &str = "2026-02-08";
    let (os, arch) = host_os_arch()?;
    let file = match (os, arch) {
        ("darwin", _) => "marksman-macos",
        ("linux", "aarch64") => "marksman-linux-arm64",
        ("linux", "x86_64") => "marksman-linux-x64",
        ("windows", _) => "marksman.exe",
        _ => return None,
    };
    Some(BinaryAsset {
        url: leak_url(format!(
            "https://github.com/artempyanykh/marksman/releases/download/{TAG}/{file}"
        )),
        format: AssetFormat::RawBinary,
        member: "",
        keep_tree: false,
    })
}

fn zls_asset() -> Option<BinaryAsset> {
    const TAG: &str = "0.16.0";
    let (os, arch) = host_os_arch()?;
    let (file, format, member) = match (os, arch) {
        ("darwin", "aarch64") => (
            "zls-aarch64-macos.tar.xz",
            AssetFormat::TarXz,
            "zls",
        ),
        ("darwin", "x86_64") => (
            "zls-x86_64-macos.tar.xz",
            AssetFormat::TarXz,
            "zls",
        ),
        ("linux", "aarch64") => (
            "zls-aarch64-linux.tar.xz",
            AssetFormat::TarXz,
            "zls",
        ),
        ("linux", "x86_64") => (
            "zls-x86_64-linux.tar.xz",
            AssetFormat::TarXz,
            "zls",
        ),
        ("windows", "aarch64") => ("zls-aarch64-windows.zip", AssetFormat::Zip, "zls.exe"),
        ("windows", "x86_64") => ("zls-x86_64-windows.zip", AssetFormat::Zip, "zls.exe"),
        _ => return None,
    };
    Some(BinaryAsset {
        url: leak_url(format!(
            "https://github.com/zigtools/zls/releases/download/{TAG}/{file}"
        )),
        format,
        member,
        keep_tree: false,
    })
}

fn lua_ls_asset() -> Option<BinaryAsset> {
    const VER: &str = "3.19.2";
    let (os, arch) = host_os_arch()?;
    let (file, format, member) = match (os, arch) {
        ("darwin", "aarch64") => (
            "lua-language-server-3.19.2-darwin-arm64.tar.gz",
            AssetFormat::TarGz,
            "bin/lua-language-server",
        ),
        ("darwin", "x86_64") => (
            "lua-language-server-3.19.2-darwin-x64.tar.gz",
            AssetFormat::TarGz,
            "bin/lua-language-server",
        ),
        ("linux", "aarch64") => (
            "lua-language-server-3.19.2-linux-arm64.tar.gz",
            AssetFormat::TarGz,
            "bin/lua-language-server",
        ),
        ("linux", "x86_64") => (
            "lua-language-server-3.19.2-linux-x64.tar.gz",
            AssetFormat::TarGz,
            "bin/lua-language-server",
        ),
        ("windows", "x86_64") => (
            "lua-language-server-3.19.2-win32-x64.zip",
            AssetFormat::Zip,
            "bin/lua-language-server.exe",
        ),
        _ => return None,
    };
    Some(BinaryAsset {
        url: leak_url(format!(
            "https://github.com/LuaLS/lua-language-server/releases/download/{VER}/{file}"
        )),
        format,
        member,
        keep_tree: true,
    })
}

/// Leak a URL string for `'static` recipe tables (install is rare; OK).
fn leak_url(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Whether the language-server binary looks usable (app-local or PATH).
pub fn is_server_ready(command: &str) -> bool {
    let Some(path) = resolve_command(command) else {
        return false;
    };
    let name = command_basename(command);
    // rustup ships a PATH shim that exists before `rustup component add`.
    // App-local downloads are real binaries — skip the shim check for those.
    if name == "rust-analyzer" && !is_app_local(command) {
        return Command::new(&path)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
    true
}

/// Download / install [`InstallRecipe`] into the app data dir.
pub fn run_install(recipe: &InstallRecipe) -> Result<(), String> {
    run_install_in(recipe, &lsp_root())
}

pub fn run_install_in(recipe: &InstallRecipe, root: &Path) -> Result<(), String> {
    fs::create_dir_all(bin_dir(root)).map_err(|e| e.to_string())?;
    match recipe.fetch {
        FetchKind::Binary { select } => {
            let asset = select().ok_or_else(|| {
                format!(
                    "No download asset for {} on this OS/arch",
                    recipe.name
                )
            })?;
            install_binary(recipe, &asset, root)
        }
        FetchKind::GoInstall { module, version } => install_go(module, version, root),
        FetchKind::Npm { packages } => install_npm(recipe, packages, root),
    }
}

fn install_binary(
    recipe: &InstallRecipe,
    asset: &BinaryAsset,
    root: &Path,
) -> Result<(), String> {
    let bytes = download(asset.url)?;
    let staging = root.join("tmp").join(recipe.pkg_id);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    match asset.format {
        AssetFormat::GzipFile => {
            let dest = if asset.keep_tree {
                staging.join(recipe.bin_names[0])
            } else {
                bin_path(root, recipe.bin_names[0])
            };
            write_gzip_file(&bytes, &dest)?;
            set_executable(&dest)?;
            if asset.keep_tree {
                write_wrapper(root, recipe.bin_names[0], &dest)?;
            }
        }
        AssetFormat::RawBinary => {
            let dest = bin_path(root, recipe.bin_names[0]);
            fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
            set_executable(&dest)?;
        }
        AssetFormat::TarGz => {
            let dec = GzDecoder::new(bytes.as_slice());
            Archive::new(dec)
                .unpack(&staging)
                .map_err(|e| format!("tar.gz extract: {e}"))?;
            finish_extracted(recipe, asset, root, &staging)?;
        }
        AssetFormat::TarXz => {
            let dec = xz2::read::XzDecoder::new(bytes.as_slice());
            Archive::new(dec)
                .unpack(&staging)
                .map_err(|e| format!("tar.xz extract: {e}"))?;
            finish_extracted(recipe, asset, root, &staging)?;
        }
        AssetFormat::Zip => {
            extract_zip(&bytes, &staging)?;
            finish_extracted(recipe, asset, root, &staging)?;
        }
    }

    let _ = fs::remove_dir_all(root.join("tmp").join(recipe.pkg_id));
    Ok(())
}

fn finish_extracted(
    recipe: &InstallRecipe,
    asset: &BinaryAsset,
    root: &Path,
    staging: &Path,
) -> Result<(), String> {
    let member = if asset.member.is_empty() {
        recipe.bin_names[0]
    } else {
        asset.member
    };
    let found = find_member(staging, member)
        .ok_or_else(|| format!("archive missing `{member}`"))?;

    if asset.keep_tree {
        let pkg = pkg_dir(root, recipe.pkg_id);
        let _ = fs::remove_dir_all(&pkg);
        fs::create_dir_all(pkg.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
        // Move staging → pkg (contents may already be at staging root).
        rename_or_copy_dir(staging, &pkg)?;
        let target = pkg.join(member);
        if !target.is_file() {
            // Some archives nest an extra top folder.
            let target = find_member(&pkg, Path::new(member).file_name().and_then(|s| s.to_str()).unwrap_or(member))
                .ok_or_else(|| format!("extracted tree missing `{member}`"))?;
            set_executable(&target)?;
            write_wrapper(root, recipe.bin_names[0], &target)?;
        } else {
            set_executable(&target)?;
            write_wrapper(root, recipe.bin_names[0], &target)?;
        }
    } else {
        let dest = bin_path(root, recipe.bin_names[0]);
        fs::copy(&found, &dest).map_err(|e| e.to_string())?;
        set_executable(&dest)?;
        let _ = fs::remove_dir_all(staging);
    }
    Ok(())
}

fn find_member(root: &Path, member: &str) -> Option<PathBuf> {
    let direct = root.join(member);
    if direct.is_file() {
        return Some(direct);
    }
    // Search one level of nesting (zls / lua archives vary).
    let name = Path::new(member).file_name()?.to_str()?;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = fs::read_dir(&dir).ok()?;
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(p);
            }
        }
    }
    None
}

fn install_go(module: &str, version: &str, root: &Path) -> Result<(), String> {
    let bin = bin_dir(root);
    fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let status = Command::new("go")
        .args(["install", &format!("{module}@{version}")])
        .env("GOBIN", &bin)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Could not run `go`: {e}"))?;
    if status.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&status.stderr);
    let out = String::from_utf8_lossy(&status.stdout);
    let detail = if err.trim().is_empty() {
        out.trim()
    } else {
        err.trim()
    };
    Err(if detail.is_empty() {
        format!("`go install` failed ({})", status.status)
    } else {
        detail.chars().take(400).collect()
    })
}

fn install_npm(
    recipe: &InstallRecipe,
    packages: &[NpmSpec],
    root: &Path,
) -> Result<(), String> {
    let pkg = pkg_dir(root, recipe.pkg_id);
    let _ = fs::remove_dir_all(&pkg);
    fs::create_dir_all(&pkg).map_err(|e| e.to_string())?;

    let mut args = vec!["install".to_string(), "--prefix".to_string()];
    args.push(pkg.to_string_lossy().into_owned());
    for p in packages {
        args.push(format!("{}@{}", p.name, p.version));
    }

    let output = Command::new("npm")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Could not run `npm` (required for this server): {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        let detail = if err.trim().is_empty() {
            out.trim()
        } else {
            err.trim()
        };
        return Err(if detail.is_empty() {
            format!("`npm install` failed ({})", output.status)
        } else {
            detail.chars().take(400).collect()
        });
    }

    for bin_name in recipe.bin_names {
        let nm_bin = pkg.join("node_modules").join(".bin").join(bin_name);
        #[cfg(windows)]
        let nm_bin = {
            let cmd = pkg
                .join("node_modules")
                .join(".bin")
                .join(format!("{bin_name}.cmd"));
            if cmd.is_file() {
                cmd
            } else {
                nm_bin
            }
        };
        if !nm_bin.is_file() {
            return Err(format!(
                "npm install succeeded but `{}` is missing under node_modules/.bin",
                bin_name
            ));
        }
        write_wrapper(root, bin_name, &nm_bin)?;
    }
    Ok(())
}

fn bin_path(root: &Path, name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        if !name.ends_with(".exe") && !name.ends_with(".cmd") {
            return bin_dir(root).join(format!("{name}.exe"));
        }
    }
    bin_dir(root).join(name)
}

fn write_wrapper(root: &Path, bin_name: &str, target: &Path) -> Result<(), String> {
    let target = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    fs::create_dir_all(bin_dir(root)).map_err(|e| e.to_string())?;

    #[cfg(windows)]
    {
        let dest = bin_dir(root).join(format!("{bin_name}.cmd"));
        let body = format!("@echo off\r\n\"{}\" %*\r\n", target.display());
        fs::write(&dest, body).map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let dest = bin_dir(root).join(bin_name);
        let body = format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", target.display());
        fs::write(&dest, body).map_err(|e| e.to_string())?;
        set_executable(&dest)?;
        Ok(())
    }
}

fn write_gzip_file(bytes: &[u8], dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut decoder = GzDecoder::new(bytes);
    let mut out = File::create(dest).map_err(|e| e.to_string())?;
    io::copy(&mut decoder, &mut out).map_err(|e| format!("gunzip: {e}"))?;
    Ok(())
}

fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let cursor = io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("zip: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        let Some(path) = file.enclosed_name().map(|p| dest.join(p)) else {
            continue;
        };
        if file.is_dir() {
            fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = File::create(&path).map_err(|e| e.to_string())?;
        io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

fn rename_or_copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_dir_all(from, to)?;
            let _ = fs::remove_dir_all(from);
            Ok(())
        }
    }
}

fn copy_dir_all(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir_all(&src, &dst)?;
        } else {
            fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(perms.mode() | 0o755);
        fs::set_permissions(path, perms).map_err(|e| e.to_string())?;
    }
    let _ = path;
    Ok(())
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("download HTTP {status}"));
    }
    let mut body = Vec::new();
    response
        .into_body()
        .as_reader()
        .read_to_end(&mut body)
        .map_err(|e| format!("download read: {e}"))?;
    if body.is_empty() {
        return Err("download returned empty body".into());
    }
    Ok(body)
}

/// Delete app-local files for this recipe (never touches system packages).
pub fn run_uninstall(recipe: &InstallRecipe) -> Result<(), String> {
    run_uninstall_in(recipe, &lsp_root())
}

pub fn run_uninstall_in(recipe: &InstallRecipe, root: &Path) -> Result<(), String> {
    for name in recipe.bin_names {
        let p = bin_dir(root).join(name);
        let _ = fs::remove_file(&p);
        #[cfg(windows)]
        {
            let _ = fs::remove_file(bin_dir(root).join(format!("{name}.exe")));
            let _ = fs::remove_file(bin_dir(root).join(format!("{name}.cmd")));
        }
    }
    let pkg = pkg_dir(root, recipe.pkg_id);
    if pkg.exists() {
        fs::remove_dir_all(&pkg).map_err(|e| format!("remove pkg: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_analyzer_has_binary_recipe() {
        let r = recipe_for_command("rust-analyzer").unwrap();
        assert!(matches!(r.fetch, FetchKind::Binary { .. }));
        assert_eq!(r.bin_names, &["rust-analyzer"]);
    }

    #[test]
    fn unknown_command_has_no_recipe() {
        assert!(recipe_for_command("totally-unknown-lsp-xyz").is_none());
    }

    #[test]
    fn basename_from_path() {
        let r = recipe_for_command("/usr/local/bin/gopls").unwrap();
        assert_eq!(r.name, "gopls");
        assert!(matches!(r.fetch, FetchKind::GoInstall { .. }));
    }

    #[test]
    fn pylsp_has_no_download_recipe() {
        assert!(recipe_for_command("pylsp").is_none());
    }

    #[test]
    fn resolve_prefers_app_local_over_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lsp");
        let bin = bin_dir(&root);
        fs::create_dir_all(&bin).unwrap();
        let local = bin.join("fake-lsp-tool");
        fs::write(&local, "#!/bin/sh\n").unwrap();
        set_executable(&local).unwrap();

        let resolved = resolve_command_in("fake-lsp-tool", &root).unwrap();
        assert_eq!(resolved, local);
        assert!(is_app_local_in("fake-lsp-tool", &root));
    }

    #[test]
    fn uninstall_removes_bin_and_pkg() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lsp");
        let bin = bin_dir(&root);
        let pkg = pkg_dir(&root, "demo");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&pkg).unwrap();
        fs::write(bin.join("demo-bin"), "x").unwrap();
        fs::write(pkg.join("marker"), "y").unwrap();

        let recipe = InstallRecipe {
            name: "demo",
            summary: "test",
            pkg_id: "demo",
            bin_names: &["demo-bin"],
            fetch: FetchKind::GoInstall {
                module: "unused",
                version: "v0",
            },
        };
        run_uninstall_in(&recipe, &root).unwrap();
        assert!(!bin.join("demo-bin").exists());
        assert!(!pkg.exists());
    }
}
