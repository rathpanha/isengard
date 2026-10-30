//! App-local install of built-in agents.
//!
//! - **Claude:** `bun add --cwd …/isengard/agents/claude` — never global.
//! - **Cursor:** download a pinned Cursor Agent CLI tarball into
//!   `…/isengard/agents/cursor/` (never PATH / Cursor.app / `bun add`).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;

/// npm package that speaks ACP on top of the Claude Agent SDK.
pub const CLAUDE_PACKAGE: &str = "@agentclientprotocol/claude-agent-acp";
/// Pinned on purpose — upgrade together with the `agent-client-protocol` crate.
pub const CLAUDE_VERSION: &str = "0.94.0";
/// Executable name in `node_modules/.bin` (and on `PATH` for global installs).
pub const CLAUDE_BIN: &str = "claude-agent-acp";

/// Cursor lab release id (matches `curl https://cursor.com/install`).
pub const CURSOR_VERSION: &str = "2026.10.22-27dcf5f";
/// Launcher script inside the extracted package (`agent` is the PATH symlink name).
pub const CURSOR_BIN: &str = "cursor-agent";

/// Root: `…/isengard/agents`.
pub fn agents_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("isengard")
        .join("agents")
}

fn claude_dir(root: &Path) -> PathBuf {
    root.join("claude")
}

fn cursor_dir(root: &Path) -> PathBuf {
    root.join("cursor")
}

/// `node_modules/.bin/claude-agent-acp` under `root` (may not exist).
pub fn claude_bin_in(root: &Path) -> PathBuf {
    let bin = claude_dir(root).join("node_modules").join(".bin");
    if cfg!(windows) {
        bin.join(format!("{CLAUDE_BIN}.cmd"))
    } else {
        bin.join(CLAUDE_BIN)
    }
}

/// `cursor-agent` launcher under `root` (may not exist).
pub fn cursor_bin_in(root: &Path) -> PathBuf {
    let bin = cursor_dir(root).join(CURSOR_BIN);
    if cfg!(windows) {
        let exe = cursor_dir(root).join(format!("{CURSOR_BIN}.exe"));
        if exe.is_file() {
            return exe;
        }
    }
    bin
}

/// Installs the pinned Claude adapter app-locally (blocking — run off the UI thread).
pub fn install_claude() -> Result<(), String> {
    install_claude_in(&agents_root())
}

pub fn install_claude_in(root: &Path) -> Result<(), String> {
    crate::lsp::bun_install(
        &claude_dir(root),
        &[format!("{CLAUDE_PACKAGE}@{CLAUDE_VERSION}")],
    )?;
    let bin = claude_bin_in(root);
    if bin.is_file() {
        Ok(())
    } else {
        Err(format!(
            "bun add succeeded but `{CLAUDE_BIN}` is missing under node_modules/.bin"
        ))
    }
}

/// Installs the pinned Cursor Agent CLI app-locally (blocking — off the UI thread).
///
/// ~600 MB download; streams to disk. Does not touch `~/.local` or Cursor.app.
pub fn install_cursor() -> Result<(), String> {
    install_cursor_in(&agents_root())
}

pub fn install_cursor_in(root: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = root;
        return Err(
            "Cursor Agent app-local install is not wired for Windows yet (Phase 0 is Unix tar.gz)"
                .into(),
        );
    }
    #[cfg(not(windows))]
    {
        install_cursor_unix(root)
    }
}

#[cfg(not(windows))]
fn install_cursor_unix(root: &Path) -> Result<(), String> {
    let url = cursor_download_url()?;
    let dest = cursor_dir(root);
    let staging = root.join(format!(".cursor-staging-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| format!("create staging: {e}"))?;

    let result = (|| {
        download_and_extract_tarball(&url, &staging)?;
        let bin = staging.join(CURSOR_BIN);
        if !bin.is_file() {
            return Err(format!(
                "download extracted but `{CURSOR_BIN}` is missing (url {url})"
            ));
        }
        set_executable(&bin)?;
        // Swap into place so a failed download never leaves a half tree as "installed".
        let backup = root.join(format!(".cursor-prev-{}", std::process::id()));
        let _ = fs::remove_dir_all(&backup);
        if dest.exists() {
            fs::rename(&dest, &backup).map_err(|e| format!("park old cursor install: {e}"))?;
        }
        match fs::rename(&staging, &dest) {
            Ok(()) => {
                let _ = fs::remove_dir_all(&backup);
                Ok(())
            }
            Err(e) => {
                let _ = fs::rename(&backup, &dest);
                Err(format!("promote cursor install: {e}"))
            }
        }
    })();

    let _ = fs::remove_dir_all(&staging);
    result
}

fn cursor_download_url() -> Result<String, String> {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err("unsupported OS for Cursor Agent download".into());
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => return Err(format!("unsupported arch for Cursor Agent: {other}")),
    };
    Ok(format!(
        "https://downloads.cursor.com/lab/{CURSOR_VERSION}/{os}/{arch}/agent-cli-package.tar.gz"
    ))
}

fn download_and_extract_tarball(url: &str, dest: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("Cursor Agent download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Cursor Agent download HTTP {}",
            response.status()
        ));
    }
    // Stream gzip+tar — the package is ~600 MB; do not buffer the whole body.
    let reader = response.into_body().into_reader();
    let decoder = GzDecoder::new(reader);
    let mut archive = Archive::new(decoder);
    archive
        .entries()
        .map_err(|e| format!("Cursor Agent tar: {e}"))?
        .try_for_each(|entry| {
            let mut entry = entry.map_err(|e| format!("Cursor Agent tar entry: {e}"))?;
            let rel = strip_first_component(
                entry
                    .path()
                    .map_err(|e| format!("Cursor Agent tar path: {e}"))?
                    .as_ref(),
            );
            if rel.as_os_str().is_empty() {
                return Ok(());
            }
            if rel
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
            {
                return Err(format!(
                    "Cursor Agent tar refused unsafe path {}",
                    rel.display()
                ));
            }
            let out = dest.join(&rel);
            if entry.header().entry_type().is_dir() {
                fs::create_dir_all(&out).map_err(|e| format!("mkdir {}: {e}", out.display()))?;
                return Ok(());
            }
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            let mut file =
                File::create(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
            io::copy(&mut entry, &mut file)
                .map_err(|e| format!("write {}: {e}", out.display()))?;
            file.flush()
                .map_err(|e| format!("flush {}: {e}", out.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(mode) = entry.header().mode() {
                    let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode));
                }
            }
            Ok(())
        })?;
    Ok(())
}

fn strip_first_component(path: &Path) -> PathBuf {
    let mut comps = path.components();
    let _ = comps.next();
    comps.as_path().to_path_buf()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_bin_lives_under_node_modules_bin() {
        let bin = claude_bin_in(Path::new("/data/agents"));
        assert!(bin.starts_with("/data/agents/claude/node_modules/.bin"));
        assert!(bin.to_string_lossy().contains(CLAUDE_BIN));
    }

    #[test]
    fn cursor_bin_lives_under_agents_cursor() {
        let bin = cursor_bin_in(Path::new("/data/agents"));
        assert_eq!(bin, PathBuf::from("/data/agents/cursor/cursor-agent"));
    }

    #[test]
    fn cursor_download_url_is_pinned_lab_tarball() {
        let url = cursor_download_url().expect("unix host");
        assert!(url.contains(CURSOR_VERSION), "{url}");
        assert!(url.ends_with("/agent-cli-package.tar.gz"), "{url}");
        assert!(
            url.contains("/darwin/") || url.contains("/linux/"),
            "{url}"
        );
    }

    #[test]
    fn strip_first_component_drops_dist_package() {
        assert_eq!(
            strip_first_component(Path::new("dist-package/cursor-agent")),
            PathBuf::from("cursor-agent")
        );
        assert_eq!(
            strip_first_component(Path::new("dist-package")),
            PathBuf::new()
        );
    }

    /// Network + ~600 MB: `cargo test cursor_cli_installs_app_locally -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn cursor_cli_installs_app_locally() {
        let tmp = tempfile::tempdir().unwrap();
        install_cursor_in(tmp.path()).unwrap();
        let bin = cursor_bin_in(tmp.path());
        assert!(bin.is_file(), "missing {bin:?}");
        assert!(bin.starts_with(tmp.path()));
        let out = std::process::Command::new(&bin)
            .arg("--version")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        let version = String::from_utf8_lossy(&out.stdout);
        assert!(
            version.contains(CURSOR_VERSION) || version.contains("2026."),
            "unexpected version {version:?}"
        );
        fs::remove_dir_all(cursor_dir(tmp.path())).unwrap();
        assert!(!bin.exists());
    }
}
