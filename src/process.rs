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
