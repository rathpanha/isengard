use std::sync::{Arc, LazyLock};

use gpui_kit::{Image, ImageFormat};

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
