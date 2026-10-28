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

/// A program to run in a terminal tab instead of the default shell (e.g. the
/// agent's `--cli auth login`).
#[derive(Debug, Clone)]
pub struct TerminalCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl PtySession {
    /// Opens a PTY and spawns `command` (or the user's default shell) in `cwd`.
    pub fn spawn(
        cwd: &Path,
        command: Option<&TerminalCommand>,
        cols: u16,
        rows: u16,
    ) -> Result<(Self, Box<dyn Write + Send>, Box<dyn Read + Send>)> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("open PTY")?;

        let mut cmd = match command {
            Some(command) => {
                let mut cmd = CommandBuilder::new(&command.program);
                cmd.args(&command.args);
                for (key, value) in &command.env {
                    cmd.env(key, value);
                }
                cmd
            }
            None => {
                let mut cmd = CommandBuilder::new(default_shell());
                // Like Terminal.app: a login shell reads ~/.zprofile (Homebrew
                // `shellenv`, …), which a Finder-launched app does not inherit.
                if cfg!(target_os = "macos") {
                    cmd.arg("-l");
                }
                cmd
            }
        };
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

    /// A program other than the shell itself is running in the foreground
    /// (e.g. `npm run dev`, `sleep`). Unix only; Windows reports `false`.
    pub fn has_foreground_process(&self) -> bool {
        #[cfg(unix)]
        {
            let leader = self.master.lock().process_group_leader();
            match (leader, self._child.process_id()) {
                (Some(leader), Some(shell)) => i64::from(leader) != i64::from(shell),
                _ => false,
            }
        }
        #[cfg(not(unix))]
        false
    }

    /// Stop what this terminal is running: SIGTERM to the foreground program
    /// (SIGKILL if it is still alive after ~1s), then hang up the shell.
    /// Used on quit so servers don't outlive the editor and hold their port.
    /// Unix only; elsewhere closing the PTY is all we do.
    pub fn terminate(&mut self) {
        #[cfg(unix)]
        {
            let shell = self._child.process_id().map(|p| p as libc::pid_t);
            let leader = self.master.lock().process_group_leader();
            if let (Some(group), Some(shell)) = (leader, shell)
                && group != shell
            {
                // SAFETY: plain signal syscalls on pids we own.
                unsafe { libc::killpg(group, libc::SIGTERM) };
                for _ in 0..10 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    // Signal 0: does the group still exist?
                    if unsafe { libc::killpg(group, 0) } != 0 {
                        break;
                    }
                }
                unsafe { libc::killpg(group, libc::SIGKILL) };
            }
            if let Some(shell) = shell {
                unsafe { libc::kill(shell, libc::SIGHUP) };
            }
        }
        let _ = self._child.kill();
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

    #[cfg(unix)]
    #[test]
    fn detects_a_foreground_process() {
        let shell = TerminalCommand {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-i".into()],
            env: Vec::new(),
        };
        let (session, mut writer, _reader) =
            PtySession::spawn(Path::new("/tmp"), Some(&shell), 80, 24).unwrap();
        assert!(!session.has_foreground_process(), "idle shell prompt");
        writer.write_all(b"sleep 30\n").unwrap();
        let mut busy = false;
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if session.has_foreground_process() {
                busy = true;
                break;
            }
        }
        assert!(busy, "sleep should be the foreground process");
        let group = session.master.lock().process_group_leader().unwrap();
        let mut session = session;
        session.terminate();
        let gone = (0..30).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            // SAFETY: signal 0 only probes for the process group.
            unsafe { libc::killpg(group, 0) != 0 }
        });
        assert!(gone, "terminate should kill the foreground process");
    }

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
