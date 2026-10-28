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
    // Bundled app: a system notification from Isengard itself (app icon,
    // listed under Isengard in System Settings > Notifications).
    // `cargo run` has no bundle, so it falls back to osascript.
    if !native::show(title, body) {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            esc(body),
            esc(title)
        );
        run("osascript", &["-e", &script]);
    }
    if let Some(file) = sound_file().as_deref().and_then(|p| p.to_str()) {
        run("afplay", &[file]);
    }
}

#[cfg(target_os = "macos")]
mod native {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNUserNotificationCenter,
    };

    /// Hand the notification to the system. `false` when this process is not an
    /// app bundle (UserNotifications needs one). The first call makes macOS ask
    /// the user to allow notifications; the chime plays either way, and a denial
    /// is respected (nothing is shown).
    pub fn show(title: &str, body: &str) -> bool {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return false;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&format!("isengard-{id}")),
            &content,
            None,
        );
        let add_to = center.clone();
        let on_authorized = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            if granted.as_bool() {
                add_to.addNotificationRequest_withCompletionHandler(&request, None);
            } else if error.is_null() {
                log::info!("notifications are not allowed for Isengard");
            } else {
                // SAFETY: non-null NSError handed to the completion handler.
                log::warn!("notification authorization failed: {:?}", unsafe { &*error });
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &on_authorized,
        );
        true
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform(_: &str, _: &str) {}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn native_notifications_need_an_app_bundle() {
        // UserNotifications raises an exception without a bundle; the test
        // binary (like `cargo run`) has none and must take the osascript path.
        assert!(!super::native::show("title", "body"));
    }
}
