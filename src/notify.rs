//! OS desktop notification plus a short sound, via the platform's own tools
//! (`notify-send` / `paplay` on Linux, `osascript` / `afplay` on macOS).
//! Best effort: a missing tool is silently skipped.
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Mixkit "Long pop" (see docs/credits.md).
const DONE_SOUND: &[u8] = include_bytes!("../assets/sounds/agent-done.wav");

/// The chime written to the cache dir so a player can open it.
fn sound_file() -> Option<PathBuf> {
    let path = dirs::cache_dir()?.join("isengard").join("agent-done.wav");
    let current = std::fs::metadata(&path).is_ok_and(|m| m.len() == DONE_SOUND.len() as u64);
    if !current {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, DONE_SOUND).ok()?;
    }
    Some(path)
}

/// Show `title` / `body` and play the "complete" sound. Never blocks.
pub fn alert(title: &str, body: &str) {
    if cfg!(test) {
        return;
    }
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || platform(&title, &body));
}

fn run(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(target_os = "linux")]
fn platform(title: &str, body: &str) {
    // Icon name of the installed app (hicolor theme); the logo, not a stock icon.
    run("notify-send", &["-a", "Isengard", "-i", "app.isengard.edtir", title, body]);
    if let Some(file) = sound_file().as_deref().and_then(|p| p.to_str()) {
        let _ = run("paplay", &[file]) || run("pw-play", &[file]) || run("aplay", &["-q", file]);
    }
}

#[cfg(target_os = "macos")]
fn platform(title: &str, body: &str) {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        esc(body),
        esc(title)
    );
    run("osascript", &["-e", &script]);
    if let Some(file) = sound_file().as_deref().and_then(|p| p.to_str()) {
        run("afplay", &[file]);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform(_: &str, _: &str) {}
