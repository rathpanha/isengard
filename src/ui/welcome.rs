use std::path::Path;

use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    kbd::Kbd,
    separator::Separator,
    v_flex,
};
use gpui_kit::*;

use crate::app::{IsengardApp, NewFile, NewWorkspace, OpenFile, OpenFolder, OpenWorkspace};

#[derive(Clone, Copy)]
enum RecentKind {
    Folder,
    Workspace,
}

/// Start page shown while nothing is open: start actions plus the recent
/// workspaces and recent folders lists (each hidden when empty).
pub fn render(app: &IsengardApp, cx: &mut Context<IsengardApp>) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let mut recents: Vec<AnyElement> = Vec::new();
    // Workspaces before folders, matching the Start actions above.
    for (title, kind, paths) in [
        (
            "Recent workspaces",
            RecentKind::Workspace,
            app.recent_workspaces(),
        ),
        ("Recent folders", RecentKind::Folder, app.recent_folders()),
    ] {
        if paths.is_empty() {
            continue;
        }
        let rows: Vec<AnyElement> = paths
            .iter()
            .map(|path| render_recent(kind, path, muted, cx))
            .collect();
        recents.push(
            v_flex()
                .gap_2()
                .child(section_title(title))
                .child(v_flex().gap_1().children(rows))
                .into_any_element(),
        );
    }
    if recents.is_empty() {
        recents.push(
            v_flex()
                .gap_2()
                .child(section_title("Recent"))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("No recent folders or workspaces"),
                )
                .into_any_element(),
        );
    }

    div()
        .size_full()
        .flex()
        .justify_center()
        .child(
            v_flex()
                .w(rems(40.))
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
                        .child(div().text_color(muted).child("The already configured editor")),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap_3()
                        .items_start()
                        .child(section_title("Start"))
                        // Workspace column | folder & file column.
                        .child(
                            h_flex()
                                .w_full()
                                .items_stretch()
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .gap_2()
                                        .items_start()
                                        .pr_6()
                                        .child(
                                            Button::new("welcome-open-workspace")
                                                .link()
                                                .icon(IconName::LayoutDashboard)
                                                .label("Open Workspace…")
                                                .on_click(|_, window, cx| {
                                                    window.dispatch_action(
                                                        Box::new(OpenWorkspace),
                                                        cx,
                                                    )
                                                }),
                                        )
                                        .child(
                                            Button::new("welcome-new-workspace")
                                                .link()
                                                .icon(IconName::FolderClosed)
                                                .label("New Workspace…")
                                                .on_click(|_, window, cx| {
                                                    window.dispatch_action(
                                                        Box::new(NewWorkspace),
                                                        cx,
                                                    )
                                                }),
                                        ),
                                )
                                .child(Separator::vertical())
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .gap_2()
                                        .items_start()
                                        .pl_6()
                                        .child(
                                            h_flex()
                                                .gap_3()
                                                .child(
                                                    Button::new("welcome-open-folder")
                                                        .link()
                                                        .icon(IconName::FolderOpen)
                                                        .label("Open Folder…")
                                                        .on_click(|_, window, cx| {
                                                            window.dispatch_action(
                                                                Box::new(OpenFolder),
                                                                cx,
                                                            )
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
                                                    window.dispatch_action(
                                                        Box::new(OpenFile),
                                                        cx,
                                                    )
                                                }),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_3()
                                                .child(
                                                    Button::new("welcome-new-file")
                                                        .link()
                                                        .icon(IconName::FileText)
                                                        .label("New File")
                                                        .on_click(|_, window, cx| {
                                                            window.dispatch_action(
                                                                Box::new(NewFile),
                                                                cx,
                                                            )
                                                        }),
                                                )
                                                .child(shortcut_keys("secondary-n")),
                                        ),
                                ),
                        ),
                )
                .children(recents),
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
    kind: RecentKind,
    path: &Path,
    muted: Hsla,
    cx: &mut Context<IsengardApp>,
) -> AnyElement {
    let id = path.to_string_lossy().into_owned();
    let (prefix, name) = match kind {
        RecentKind::Folder => ("recent-folder", path.file_name()),
        RecentKind::Workspace => ("recent-workspace", path.file_stem()),
    };
    let name = name
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| id.clone());
    let parent = path.parent().map(abbreviate_home).unwrap_or_default();

    h_flex()
        .gap_3()
        .child(
            Button::new(SharedString::from(format!("{prefix}-open-{id}")))
                .link()
                .label(name)
                .tooltip(id.clone())
                .on_click(cx.listener({
                    let path = path.to_path_buf();
                    move |this, _, window, cx| match kind {
                        RecentKind::Folder => this.open_folder(&path, window, cx),
                        RecentKind::Workspace => this.open_workspace_file(&path, window, cx),
                    }
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
            let path = path.to_path_buf();
            let button_id = SharedString::from(format!("{prefix}-remove-{id}"));
            Button::new(button_id)
                .ghost()
                .cursor_pointer()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("Remove from recent")
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    match kind {
                        RecentKind::Folder => this.remove_recent_folder(&path, cx),
                        RecentKind::Workspace => this.remove_recent_workspace(&path, cx),
                    }
                }))
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
