//! Make a Finder / Dock launch see the same `PATH` as a terminal.
//!
//! macOS starts GUI apps with a bare `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`),
//! so tools installed through Homebrew, nvm, cargo, … are invisible: the agent
//! adapter dies with `env: node: No such file or directory` and the terminal
//! prompt cannot find `oh-my-posh`. `cargo run` works only because it inherits
//! the shell's environment.
//!
//! Starting a login shell costs about a second, so the result is cached in the
//! cache dir: a launch applies the cached `PATH` instantly and refreshes the
//! cache from a background thread for the next launch. Only the very first
//! launch (no cache yet) waits for the shell. Skipped when started from a
//! terminal (already right).
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::time::Duration;

const START: &str = "__ISENGARD_PATH_START__";
const END: &str = "__ISENGARD_PATH_END__";

#[cfg(target_os = "macos")]
fn cache_file() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("isengard").join("login-path"))
}

#[cfg(target_os = "macos")]
fn read_cache(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let path = text.trim();
    (!path.is_empty()).then(|| path.to_string())
}

#[cfg(target_os = "macos")]
fn write_cache(file: &Path, path: &str) {
    if read_cache(file).as_deref() == Some(path) {
        return;
    }
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, path);
}

/// Adopt the login shell's `PATH` when launched outside a terminal.
#[cfg(target_os = "macos")]
pub fn import_login_path() {
    // SAFETY: plain libc call on fd 0.
    if unsafe { libc::isatty(0) } == 1 {
        return;
    }
    let cache = cache_file();
    let shell_path = match cache.as_deref().and_then(read_cache) {
        Some(cached) => Some(cached),
        None => {
            let fresh = login_shell_path(Duration::from_secs(5));
            if let (Some(file), Some(path)) = (cache.as_deref(), fresh.as_deref()) {
                write_cache(file, path);
            }
            fresh
        }
    };
    match shell_path {
        Some(shell_path) => {
            let current = std::env::var("PATH").unwrap_or_default();
            let merged = merge_path(&shell_path, &current);
            // SAFETY: called first thing in `main`; the refresh thread below
            // starts only after this and never touches the environment.
            unsafe { std::env::set_var("PATH", merged) };
        }
        None => log::warn!("could not read PATH from the login shell; keeping the default"),
    }
    // Pick up tools installed since the cache was written, for the next launch.
    if let Some(file) = cache {
        std::thread::spawn(move || {
            if let Some(path) = login_shell_path(Duration::from_secs(10)) {
                write_cache(&file, &path);
            }
        });
    }
}

#[cfg(not(target_os = "macos"))]
pub fn import_login_path() {}

/// `PATH` as seen by `$SHELL -ilc`, or `None` if it fails or times out.
#[cfg(target_os = "macos")]
fn login_shell_path(timeout: Duration) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut child = Command::new(shell)
        // -i: nvm / Homebrew are often configured in ~/.zshrc, not ~/.zprofile.
        .args(["-ilc", &format!("printf '%s%s%s' {START} \"$PATH\" {END}")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = tx.send(out);
    });
    match rx.recv_timeout(timeout) {
        Ok(out) => {
            let _ = child.wait();
            extract_path(&out)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

/// The text between our markers (rc files may print banners around it).
fn extract_path(output: &str) -> Option<String> {
    let start = output.find(START)? + START.len();
    let end = output[start..].find(END)? + start;
    let path = output[start..end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

/// `shell` entries first, then any `current` entries the shell did not have.
fn merge_path(shell: &str, current: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    shell
        .split(':')
        .chain(current.split(':'))
        .filter(|entry| !entry.is_empty() && seen.insert(*entry))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_path_between_markers() {
        let out = format!("welcome banner\n{START}/opt/homebrew/bin:/usr/bin{END}\nbye");
        assert_eq!(extract_path(&out).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(extract_path("no markers"), None);
        assert_eq!(extract_path(&format!("{START}{END}")), None);
    }

    /// Runs the real login shell: proves the markers survive rc-file noise.
    #[cfg(target_os = "macos")]
    #[test]
    fn reads_path_from_real_login_shell() {
        let path = login_shell_path(Duration::from_secs(10)).expect("login shell PATH");
        assert!(path.split(':').any(|p| p == "/usr/bin"), "{path}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cache_round_trips_and_skips_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join("login-path");
        assert_eq!(read_cache(&file), None, "missing file");
        write_cache(&file, "/opt/homebrew/bin:/usr/bin");
        assert_eq!(read_cache(&file).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
        std::fs::write(&file, "  \n").unwrap();
        assert_eq!(read_cache(&file), None, "blank cache is ignored");
    }

    #[test]
    fn merges_without_duplicates() {
        assert_eq!(
            merge_path("/opt/homebrew/bin:/usr/bin", "/usr/bin:/bin:/usr/sbin"),
            "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin"
        );
        assert_eq!(merge_path("", "/usr/bin"), "/usr/bin");
    }
}
