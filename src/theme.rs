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

fn load_highlight(name: &str, json: &str) -> Arc<HighlightTheme> {
    let style: HighlightThemeStyle =
        serde_json::from_str(json).unwrap_or_else(|err| panic!("{name} highlight theme: {err}"));
    Arc::new(HighlightTheme {
        name: name.into(),
        appearance: ThemeMode::Dark,
        style,
    })
}

/// Ayu Darker token colours for tree-sitter scopes (syntax only; GPL-3.0 asset).
fn ayu_darker_highlight() -> Arc<HighlightTheme> {
    static THEME: LazyLock<Arc<HighlightTheme>> = LazyLock::new(|| {
        load_highlight(
            "Ayu Darker",
            include_str!("../assets/themes/ayu-darker-highlight.json"),
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

/// Applies dark mode, the Nerd Font everywhere, the editor font size, and
/// square corners (radius 0 squares every component, including pills).
/// Editor/syntax colours use Ayu Darker. Editor + gutter backgrounds match
/// the file-tree sidebar (`theme.sidebar`) so the pane isn't a lighter
/// `input` grey.
pub fn apply(config: &AppConfig, window: Option<&mut Window>, cx: &mut App) {
    // `change` reloads the mode's theme config, so the overrides below must come after it.
    Theme::change(ThemeMode::Dark, window, cx);
    Theme::update(cx, |theme| {
        theme.font_family = FONT_FAMILY.into();
        theme.mono_font_family = FONT_FAMILY.into();
        theme.font_size = px(UI_FONT_SIZE);
        theme.mono_font_size = px(config.font_size);
        theme.radius = px(0.);
        theme.radius_lg = px(0.);
        theme.notification.placement = Anchor::BottomRight;
        let mut highlight = (*ayu_darker_highlight()).clone();
        highlight.style.editor_background = Some(theme.sidebar);
        highlight.style.editor_gutter_background = Some(theme.sidebar);
        theme.highlight_theme = Arc::new(highlight);
    });
}

#[cfg(test)]
mod tests {
    // Don't `use super::*` / `gpui_kit::*` — gpui's `test` macro shadows `#[test]`.

    #[test]
    fn ayu_darker_highlight_parses() {
        let theme = super::ayu_darker_highlight();
        assert_eq!(theme.name, "Ayu Darker");
        assert!(theme.style.syntax.keyword.is_some());
        assert!(theme.style.syntax.string.is_some());
        assert!(theme.style.syntax.comment.is_some());
    }
}
