use std::path::Path;

use gpui_kit::component::{
    ActiveTheme as _, IconName,
    button::{Button, ButtonVariants as _},
    h_flex,
    kbd::Kbd,
    v_flex,
};
use gpui_kit::*;

use crate::app::{IsengardApp, OpenFile, OpenFolder};
use crate::ui::components::destructive_icon_button;

/// Start page shown while no folder or file is open: start actions plus recent folders.
pub fn render(app: &IsengardApp, cx: &mut Context<IsengardApp>) -> AnyElement {
    let recent = app.recent_folders();
    let theme = cx.theme();
    let muted = theme.muted_foreground;

    let recent_list = if recent.is_empty() {
        div()
            .text_sm()
            .text_color(muted)
            .child("No recent folders")
            .into_any_element()
    } else {
        let rows: Vec<AnyElement> = recent
            .iter()
            .map(|folder| render_recent(app, folder, muted, cx))
            .collect();
        v_flex().gap_1().children(rows).into_any_element()
    };

    div()
        .size_full()
        .flex()
        .justify_center()
        .child(
            v_flex()
                .w(rems(30.))
                .mt(relative(0.15))
                .gap_8()
                .child(
                    v_flex()
                        .child(
                            img(crate::branding::LOGO_MARK.clone())
                                .w(rems(4.))
                                .h(rems(4.7))
                                .mb_4(),
                        )
                        .child(
                            div()
                                .text_3xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Isengard"),
                        )
                        .child(div().text_color(muted).child("Code editor")),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .items_start()
                        .child(section_title("Start"))
                        .child(
                            h_flex()
                                .gap_3()
                                .child(
                                    Button::new("welcome-open-folder")
                                        .link()
                                        .icon(IconName::FolderOpen)
                                        .label("Open Folder…")
                                        .on_click(|_, window, cx| {
                                            window.dispatch_action(Box::new(OpenFolder), cx)
                                        }),
                                )
                                .child(shortcut_keys("secondary-o")),
                        )
                        .child(
                            Button::new("welcome-open-file")
                                .link()
                                .icon(IconName::File)
                                .label("Open File…")
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(OpenFile), cx)
                                }),
                        ),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(section_title("Recent"))
                        .child(recent_list),
                ),
        )
        .into_any_element()
}

/// Renders a shortcut as one key cap per key (e.g. `⌘` `O`) so the keys don't run together.
fn shortcut_keys(binding: &str) -> impl IntoElement {
    let stroke = Keystroke::parse(binding).expect("valid keystroke");
    let m = stroke.modifiers;
    // Same order as the platform's own shortcut display: ⌃⌥⇧⌘ / Ctrl+Alt+Shift+Win.
    // Each modifier is passed as a bare key name, which `Kbd` formats per platform.
    let modifiers = [
        (m.control, "ctrl"),
        (m.alt, "alt"),
        (m.shift, "shift"),
        (m.platform, "cmd"),
    ];
    let keys = modifiers
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name.to_owned())
        .chain([stroke.key.clone()]);
    h_flex().gap_1().children(keys.map(|key| {
        // Built directly: `Keystroke::parse` would read "cmd" as a modifier.
        Kbd::new(Keystroke {
            modifiers: Modifiers::default(),
            key,
            key_char: None,
        })
    }))
}

fn section_title(title: &'static str) -> impl IntoElement {
    div()
        .text_lg()
        .font_weight(FontWeight::SEMIBOLD)
        .child(title)
}

fn render_recent(
    app: &IsengardApp,
    folder: &Path,
    muted: Hsla,
    cx: &mut Context<IsengardApp>,
) -> AnyElement {
    let id = folder.to_string_lossy().into_owned();
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| id.clone());
    let parent = folder.parent().map(abbreviate_home).unwrap_or_default();

    h_flex()
        .gap_3()
        .child(
            Button::new(SharedString::from(format!("recent-open-{id}")))
                .link()
                .label(name)
                .tooltip(id.clone())
                .on_click(cx.listener({
                    let folder = folder.to_path_buf();
                    move |this, _, window, cx| this.open_folder(&folder, window, cx)
                })),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .text_color(muted)
                .child(parent),
        )
        .child({
            let folder = folder.to_path_buf();
            let button_id = SharedString::from(format!("recent-remove-{id}"));
            let hovered = app.is_destructive_hovered(&button_id);
            destructive_icon_button(
                button_id,
                IconName::Close,
                "Remove from recent",
                hovered,
                move |this, _, cx| this.remove_recent_folder(&folder, cx),
                cx,
            )
        })
        .into_any_element()
}

/// Shows paths under the home directory as `~/...`.
fn abbreviate_home(path: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return Path::new("~").join(rest).display().to_string();
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that would bring in gpui's `test` macro, which shadows the built-in one.
    use super::abbreviate_home;
    use std::path::Path;

    #[test]
    fn abbreviates_home_directory() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(abbreviate_home(&home.join("code/app")), "~/code/app");
        assert_eq!(abbreviate_home(Path::new("/tmp/x")), "/tmp/x");
    }
}
