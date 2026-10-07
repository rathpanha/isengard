//! PTY lifecycle: open, spawn default shell, resize, tear down.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// Owns a live PTY session (master + child process).
pub struct PtySession {
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    _child: Box<dyn Child + Send + Sync>,
}

impl PtySession {
    /// Opens a PTY and spawns the user's default shell with `cwd`.
    pub fn spawn(cwd: &Path, cols: u16, rows: u16) -> Result<(Self, Box<dyn Write + Send>, Box<dyn Read + Send>)> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("open PTY")?;

        let mut cmd = CommandBuilder::new(default_shell());
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("spawn shell in PTY")?;
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .context("PTY writer")?;
        let reader = pair
            .master
            .try_clone_reader()
            .context("PTY reader")?;

        let master = Arc::new(Mutex::new(pair.master));
        Ok((
            Self {
                master,
                _child: child,
            },
            writer,
            reader,
        ))
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.lock().resize(PtySize {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    pub fn master_handle(&self) -> Arc<Mutex<Box<dyn MasterPty + Send>>> {
        self.master.clone()
    }
}

/// `$SHELL` on Unix; `ComSpec` then `powershell` on Windows.
pub fn default_shell() -> String {
    #[cfg(windows)]
    {
        std::env::var("ComSpec")
            .or_else(|_| std::env::var("PSModulePath").map(|_| "powershell.exe".into()))
            .unwrap_or_else(|_| "powershell.exe".into())
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    }
}

/// Preferred cwd for a new session: first workspace folder, else home.
pub fn default_cwd(workspace_folders: &[PathBuf]) -> PathBuf {
    workspace_folders
        .first()
        .cloned()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cwd_prefers_first_workspace_folder() {
        let folders = vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")];
        assert_eq!(default_cwd(&folders), PathBuf::from("/tmp/a"));
    }

    #[test]
    fn default_cwd_falls_back_without_folders() {
        let cwd = default_cwd(&[]);
        assert!(!cwd.as_os_str().is_empty());
    }

    #[test]
    fn default_shell_is_nonempty() {
        assert!(!default_shell().is_empty());
    }
}
