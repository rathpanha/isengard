use std::sync::{Arc, LazyLock};

use gpui_kit::{Image, ImageFormat};
use image::RgbaImage;

/// Freedesktop / Wayland app id — must match the `.desktop` file basename
/// (`dev.isengard.editor.desktop`) and `package.metadata.bundle.identifier`.
pub const APP_ID: &str = "dev.isengard.editor";

/// The logo mark (no background), for use inside the UI.
pub static LOGO_MARK: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        include_bytes!("../assets/logo/logo-mark.svg").to_vec(),
    ))
});

/// Shows the app icon in the macOS Dock. Bundled builds get it from the .app's
/// Info.plist; this covers `cargo run`, where the binary has no bundle.
#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::{AllocAnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("set_dock_icon must run on the main thread");
        return;
    };
    let data = NSData::with_bytes(include_bytes!("../assets/logo/icon-1024.png"));
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: called on the main thread with a valid, retained NSImage.
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_icon() {}

/// X11 `_NET_WM_ICON` via `WindowOptions::icon`. Wayland ignores this — panel
/// icons come from a matching `.desktop` + hicolor icon (see
/// `scripts/install-linux-dev-icon.sh`).
pub fn window_icon() -> Option<Arc<RgbaImage>> {
    match image::load_from_memory(include_bytes!("../assets/logo/icon-1024.png")) {
        Ok(img) => Some(Arc::new(img.into_rgba8())),
        Err(err) => {
            log::warn!("failed to decode window icon: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_png_decodes() {
        let icon = window_icon().expect("icon-1024.png should decode");
        assert_eq!(icon.width(), 1024);
        assert_eq!(icon.height(), 1024);
    }
}
