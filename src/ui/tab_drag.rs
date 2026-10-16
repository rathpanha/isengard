//! Within-strip tab drag payloads (editor / terminal — not interchangeable).

use std::path::PathBuf;

use gpui_kit::*;

/// Dragging an editor tab; identify by path (stable across reorder).
#[derive(Clone, Debug)]
pub struct EditorTabDrag {
    pub path: PathBuf,
    pub label: SharedString,
}

/// Dragging a terminal tab; identify by session id.
#[derive(Clone, Debug)]
pub struct TerminalTabDrag {
    pub id: usize,
    pub title: SharedString,
}

/// Lightweight drag preview (label only — Kit dock style, no fancy ghost).
pub struct TabDragPreview {
    label: SharedString,
}

impl TabDragPreview {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl Render for TabDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::ActiveTheme as _;
        div()
            .px_3()
            .py_1()
            .bg(cx.theme().tab_active)
            .border_1()
            .border_color(cx.theme().border)
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(self.label.clone())
    }
}
