use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

use gpui_kit::component::highlighter::{HighlightTheme, HighlightThemeStyle};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

use crate::config::AppConfig;

/// Family name of the bundled JetBrains Mono Nerd Font (typographic family, name ID 16).
pub const FONT_FAMILY: &str = "JetBrainsMono Nerd Font";

/// Base UI font size; the theme uses it as the rem size for the whole UI scale.
const UI_FONT_SIZE: f32 = 14.0;

fn load_highlight(name: &str, mode: ThemeMode, json: &str) -> Arc<HighlightTheme> {
    let style: HighlightThemeStyle =
        serde_json::from_str(json).unwrap_or_else(|err| panic!("{name} highlight theme: {err}"));
    Arc::new(HighlightTheme {
        name: name.into(),
        appearance: mode,
        style,
    })
}

/// Classic GitHub Dark token colours for tree-sitter scopes.
fn github_dark_highlight() -> Arc<HighlightTheme> {
    static THEME: LazyLock<Arc<HighlightTheme>> = LazyLock::new(|| {
        load_highlight(
            "GitHub Dark",
            ThemeMode::Dark,
            include_str!("../assets/themes/github-dark-highlight.json"),
        )
    });
    THEME.clone()
}

/// Classic GitHub Light token colours for tree-sitter scopes.
fn github_light_highlight() -> Arc<HighlightTheme> {
    static THEME: LazyLock<Arc<HighlightTheme>> = LazyLock::new(|| {
        load_highlight(
            "GitHub Light",
            ThemeMode::Light,
            include_str!("../assets/themes/github-light-highlight.json"),
        )
    });
    THEME.clone()
}

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
/// Editor/syntax colours use GitHub Light or GitHub Dark (tree-sitter scopes
/// unchanged). Only token colours are overridden — editor background/gutter
/// stay on the GPUI Kit theme.
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
        theme.highlight_theme = if config.dark_mode {
            github_dark_highlight()
        } else {
            github_light_highlight()
        };
    });
}

#[cfg(test)]
mod tests {
    // Don't `use super::*` / `gpui_kit::*` — gpui's `test` macro shadows `#[test]`.

    #[test]
    fn github_highlight_themes_parse() {
        let dark = super::github_dark_highlight();
        let light = super::github_light_highlight();
        assert_eq!(dark.name, "GitHub Dark");
        assert_eq!(light.name, "GitHub Light");
        assert!(dark.style.syntax.keyword.is_some());
        assert!(light.style.syntax.keyword.is_some());
        assert!(dark.style.syntax.string.is_some());
        assert!(light.style.syntax.string.is_some());
    }
}
