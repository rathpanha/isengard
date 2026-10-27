//! OS desktop notification plus a short sound, via the platform's own tools
//! (`notify-send` / `canberra-gtk-play` / `paplay` on Linux, `osascript` on
//! macOS). Best effort: a missing tool is silently skipped.
use std::process::{Command, Stdio};

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
    run("notify-send", &["-a", "Isengard", "-i", "dialog-information", title, body]);
    if !run("canberra-gtk-play", &["-i", "complete"]) {
        run("paplay", &["/usr/share/sounds/freedesktop/stereo/complete.oga"]);
    }
}

#[cfg(target_os = "macos")]
fn platform(title: &str, body: &str) {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"{}\" sound name \"Glass\"",
        esc(body),
        esc(title)
    );
    run("osascript", &["-e", &script]);
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform(_: &str, _: &str) {}
