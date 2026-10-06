use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

use crate::config::AppConfig;

/// Family name of the bundled JetBrains Mono Nerd Font (typographic family, name ID 16).
pub const FONT_FAMILY: &str = "JetBrainsMono Nerd Font";

/// Base UI font size; the theme uses it as the rem size for the whole UI scale.
const UI_FONT_SIZE: f32 = 14.0;

/// Registers the bundled fonts with GPUI's text system.
pub fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/JetBrainsMonoNerdFont-Medium.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/JetBrainsMonoNerdFont-SemiBold.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/JetBrainsMonoNerdFont-Bold.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/JetBrainsMonoNerdFont-Italic.ttf"
        )),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        log::error!("failed to load bundled fonts: {err:#}");
    }
}

/// Applies light/dark mode, the Nerd Font everywhere, the editor font size, and
/// square corners (radius 0 squares every component, including pills).
pub fn apply(config: &AppConfig, window: Option<&mut Window>, cx: &mut App) {
    let mode = if config.dark_mode {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    // `change` reloads the mode's theme config, so the overrides below must come after it.
    Theme::change(mode, window, cx);
    Theme::update(cx, |theme| {
        theme.font_family = FONT_FAMILY.into();
        theme.mono_font_family = FONT_FAMILY.into();
        theme.font_size = px(UI_FONT_SIZE);
        theme.mono_font_size = px(config.font_size);
        theme.radius = px(0.);
        theme.radius_lg = px(0.);
    });
}
