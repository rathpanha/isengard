//! Small app-level compositions of GPUI Kit components that encode our design
//! rules (see DESIGN.md). Use these instead of re-styling the same pattern.

use gpui_kit::component::dialog::Dialog;
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
