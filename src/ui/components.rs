//! Small app-level compositions of GPUI Kit components that encode our design
//! rules (see DESIGN.md). Use these instead of re-styling the same pattern.

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
};
use gpui_kit::*;

use crate::app::IsengardApp;

/// A destructive icon button (close a tab, remove an item): a muted icon that
/// turns `danger` red with a faint red background tint while hovered.
///
/// `Button`'s hover style can only change its background, and `Icon` resolves
/// its color at render time, so the red icon is driven by hover state kept on
/// [`IsengardApp`] (keyed by `id`) and updated from a wrapper's `on_hover`.
/// Pass `hovered` from [`IsengardApp::is_destructive_hovered`] (the view can't be
/// read through `cx` while it is rendering).
/// Style the returned wrapper (e.g. margins) to position the button.
pub fn destructive_icon_button(
    id: SharedString,
    icon: IconName,
    tooltip: &'static str,
    hovered: bool,
    on_click: impl Fn(&mut IsengardApp, &mut Window, &mut Context<IsengardApp>) + 'static,
    cx: &mut Context<IsengardApp>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (danger, muted) = (theme.danger, theme.muted_foreground);
    let variant = ButtonCustomVariant::new(cx)
        .foreground(muted)
        .hover(danger.opacity(0.15))
        .active(danger.opacity(0.25));

    let hover_id = id.clone();
    div()
        .id(SharedString::from(format!("{id}-hover")))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            this.set_destructive_hovered(hover_id.clone(), *hovered);
            cx.notify();
        }))
        .child(
            Button::new(id)
                .custom(variant)
                .cursor_pointer()
                .xsmall()
                .icon(Icon::new(icon).text_color(if hovered { danger } else { muted }))
                .tooltip(tooltip)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    on_click(this, window, cx);
                })),
        )
}
