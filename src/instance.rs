//! One window per folder/workspace across processes. Each running Isengard
//! registers the folders/workspace file it has open in
//! `<data dir>/isengard/instances/<pid>` (line 1: loopback port, then one key
//! per line) and listens on that port. A launch with a path first asks the
//! instance that already has it open to come to the front, and exits if one
//! answers; otherwise it starts normally.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::workspace::Workspace;

/// Sent to the running app when another launch asked for what it has open.
pub struct RaiseRequest {
    /// A file passed on the command line, to open once the window is up.
    pub file: Option<PathBuf>,
}

fn registry_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("isengard").join("instances"))
}

fn key_of(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// What a command-line path claims: the workspace file, the folder, or a
/// file's parent folder (plus the file to open).
fn launch_key(arg: &Path) -> Option<(String, Option<PathBuf>)> {
    if Workspace::is_workspace_file(arg) || arg.is_dir() {
        return Some((key_of(arg), None));
    }
    if arg.is_file() {
        let parent = arg.parent().filter(|p| !p.as_os_str().is_empty())?;
        return Some((key_of(parent), Some(fs::canonicalize(arg).ok()?)));
    }
    None
}

/// Asks a running instance that has `arg` open to raise its window. `true`
/// means one did, so this process should exit instead of opening a second
/// window. Stale registry entries (crashed instance) are removed.
pub fn forward_to_running(arg: &Path) -> bool {
    let (Some(dir), Some((key, file))) = (registry_dir(), launch_key(arg)) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return false;
    };
    let own = std::process::id().to_string();
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy() == own {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let mut lines = text.lines();
        let Some(port) = lines.next().and_then(|l| l.parse::<u16>().ok()) else {
            continue;
        };
        if !lines.any(|l| l == key) {
            continue;
        }
        match ask_raise(port, file.as_deref()) {
            true => return true,
            false => {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    false
}

fn ask_raise(port: u16, file: Option<&Path>) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    let line = file.map(|f| f.to_string_lossy().into_owned());
    if writeln!(stream, "raise\t{}", line.unwrap_or_default()).is_err() {
        return false;
    }
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply).is_ok() && &reply == b"ok"
}

/// This process's entry in the registry. Dropping removes it.
pub struct Registration {
    path: PathBuf,
    port: u16,
}

impl Registration {
    /// Starts listening; raise requests arrive on the returned receiver.
    pub fn start() -> Option<(Self, flume::Receiver<RaiseRequest>)> {
        let dir = registry_dir()?;
        fs::create_dir_all(&dir).ok()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
        let port = listener.local_addr().ok()?.port();
        let (tx, rx) = flume::unbounded();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).is_err() {
                    continue;
                }
                let Some(rest) = line.trim_end_matches(['\r', '\n']).strip_prefix("raise\t")
                else {
                    continue;
                };
                let file = (!rest.is_empty()).then(|| PathBuf::from(rest));
                if tx.send(RaiseRequest { file }).is_err() {
                    break;
                }
                let _ = (&stream).write_all(b"ok");
            }
        });
        let this = Self {
            path: dir.join(std::process::id().to_string()),
            port,
        };
        this.set_workspace(None);
        Some((this, rx))
    }

    /// Publishes what this window has open (nothing on the welcome screen).
    pub fn set_workspace(&self, workspace: Option<&Workspace>) {
        let mut text = format!("{}\n", self.port);
        if let Some(ws) = workspace.filter(|w| !w.is_empty()) {
            if let Some(file) = ws.file() {
                text.push_str(&key_of(file));
                text.push('\n');
            }
            if ws.file().is_none() && !ws.is_multi_root() {
                text.push_str(&key_of(&ws.folders()[0]));
                text.push('\n');
            }
        }
        let _ = fs::write(&self.path, text);
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raise_round_trip_and_stale_entry() {
        let (reg, rx) = Registration::start().expect("registry");
        let dir = std::env::temp_dir().join(format!("isengard-inst-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let ws = Workspace::from_folder(&dir);
        reg.set_workspace(Some(&ws));
        assert!(ask_raise(reg.port, None));
        assert!(rx.try_recv().is_ok());
        drop(reg);
        assert!(!ask_raise(1, None));
        let _ = fs::remove_dir_all(&dir);
    }
}
