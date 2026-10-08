//! Small app-level compositions of GPUI Kit components that encode our design
//! rules (see docs/design.md). Use these instead of re-styling the same pattern.

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _,
    dialog::Dialog,
    h_flex,
    notification::Notification,
};
use gpui_kit::*;

/// Typical dialog height used to estimate vertical centre. GPUI Kit only
/// exposes `Dialog::margin_top` (top edge), not true centre after layout.
const DIALOG_CENTER_HEIGHT: f32 = 180.;

/// Vertically centres a modal dialog. GPUI Kit defaults to `viewport/10` from
/// the top; call this on every `open_dialog` builder. Context menus stay
/// click-anchored — do not use this for those.
pub fn center_dialog(dialog: Dialog, window: &Window) -> Dialog {
    let h = window.viewport_size().height;
    // ponytail: Dialog only has margin_top; upgrade if Kit adds true centre.
    dialog.margin_top(((h - px(DIALOG_CENTER_HEIGHT)) / 2.).max(px(24.)))
}

/// One-line success toast with icon + text vertically centred.
///
/// Kit's typed `Notification::success` absolutely places the icon at
/// `top: 18px` (title-row layout), which misaligns short message-only toasts.
/// We skip `with_type` and lay out the row ourselves.
pub fn notify_success(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    notify_with_icon(message, IconName::CircleCheck, |cx| cx.theme().success, window, cx);
}

/// One-line error toast with icon + text vertically centred. See [`notify_success`].
pub fn notify_error(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    notify_with_icon(message, IconName::CircleX, |cx| cx.theme().danger, window, cx);
}

fn notify_with_icon(
    message: impl Into<SharedString>,
    icon: IconName,
    color: fn(&App) -> Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let message: SharedString = message.into();
    window.push_notification(
        Notification::new().content(move |_, _, cx| {
            h_flex()
                .items_center()
                .gap_2()
                .child(Icon::new(icon.clone()).text_color(color(cx)).small())
                .child(div().text_sm().child(message.clone()))
                .into_any_element()
        }),
        cx,
    );
}
