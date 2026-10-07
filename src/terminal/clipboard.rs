//! Clipboard helpers for OSC 52 / paste. Uses GPUI's platform clipboard when
//! a callback supplies an App handle; this module stays free of arboard.

/// Text copy/paste used by optional terminal callbacks.
/// Prefer wiring `TerminalView::with_clipboard_store_callback` to
/// `cx.write_to_clipboard` from the app instead of this type.
#[derive(Default)]
pub struct Clipboard;

impl Clipboard {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub fn copy(&mut self, _text: &str) -> anyhow::Result<()> {
        // Handled via TerminalView clipboard callbacks in the host app.
        Ok(())
    }

    pub fn paste(&mut self) -> anyhow::Result<String> {
        Ok(String::new())
    }
}
