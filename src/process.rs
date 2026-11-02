//! Subprocess spawning that never flashes a console window on Windows.
use std::ffi::OsStr;
use std::process::Command;

/// `Command::new` that never opens a console window on Windows. Release builds
/// are GUI-subsystem (no console of their own), so each console child (`git`,
/// `bun`, `cmd.exe` wrappers…) would otherwise get a window of its own.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `git` command; for network subcommands (`push`, `pull`, …) it carries the
/// user's shell `SSH_AUTH_SOCK`. A GUI launched from the desktop doesn't see
/// exports from `~/.bashrc`, so SSH pushes would use the session default agent
/// (wrong keys) instead of the one the terminal uses.
pub fn git_command(args: &[&str]) -> Command {
    let mut cmd = command("git");
    #[cfg(unix)]
    if matches!(args.first(), Some(&("push" | "pull" | "fetch" | "clone")))
        && let Some(sock) = shell_ssh_auth_sock()
    {
        cmd.env("SSH_AUTH_SOCK", sock);
    }
    #[cfg(not(unix))]
    let _ = args;
    cmd
}

/// Start the (slow) shell lookup in the background so the first network git
/// call doesn't pay for it.
pub fn prewarm_git_env() {
    #[cfg(unix)]
    std::thread::spawn(|| {
        shell_ssh_auth_sock();
    });
}

#[cfg(unix)]
fn shell_ssh_auth_sock() -> Option<&'static str> {
    use std::sync::OnceLock;
    static SOCK: OnceLock<Option<String>> = OnceLock::new();
    SOCK.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let out = Command::new(shell)
            .args(["-ic", "printf '<<%s>>' \"$SSH_AUTH_SOCK\""])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let sock = text.rsplit_once("<<")?.1.split_once(">>")?.0.trim();
        (!sock.is_empty()).then(|| sock.to_owned())
    })
    .as_deref()
}
