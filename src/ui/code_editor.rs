use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, FontId, Galley, Id, Sense, TextEdit, Ui, Vec2};

use crate::editor::buffer::TextBuffer;
use crate::editor::highlight::highlight;
use crate::editor::highlight_theme::color_for;
use crate::editor::language::Language;

const GUTTER_PADDING: f32 = 12.0;

/// Highlighted layout for the last-seen text. Rebuilt only when the text or style changes.
#[derive(Default)]
pub struct HighlightCache {
    key: Option<(String, Language, u32, bool)>,
    job: LayoutJob,
}

impl HighlightCache {
    fn job_for(&mut self, text: &str, lang: Language, font_size: f32, dark: bool, default_color: Color32) -> LayoutJob {
        let stale = match &self.key {
            Some((t, l, s, d)) => t != text || *l != lang || *s != font_size.to_bits() || *d != dark,
            None => true,
        };
        if stale {
            self.job = build_job(text, lang, font_size, dark, default_color);
            self.key = Some((text.to_owned(), lang, font_size.to_bits(), dark));
        }
        self.job.clone()
    }
}

fn build_job(text: &str, lang: Language, font_size: f32, dark: bool, default_color: Color32) -> LayoutJob {
    let font_id = FontId::monospace(font_size);
    let mut job = LayoutJob::default();
    for span in highlight(text, lang) {
        let color = span
            .highlight
            .and_then(|h| color_for(h, dark))
            .unwrap_or(default_color);
        job.append(&text[span.range], 0.0, TextFormat::simple(font_id.clone(), color));
    }
    job
}

pub struct CodeEditorOutput {
    pub changed: bool,
    /// 1-based (line, column) of the primary cursor, if the editor has one.
    pub cursor: Option<(usize, usize)>,
}

/// Renders `buffer` as an editable, syntax-highlighted code view with a line-number gutter.
pub fn show(
    ui: &mut Ui,
    id: Id,
    buffer: &mut TextBuffer,
    cache: &mut HighlightCache,
    font_size: f32,
) -> CodeEditorOutput {
    let dark = ui.visuals().dark_mode;
    let default_color = ui.visuals().text_color();
    let font_id = FontId::monospace(font_size);
    let language = buffer.language;

    let line_count = buffer.content.split('\n').count();
    let digits = line_count.to_string().len().max(3);
    let char_width = ui.fonts(|f| f.glyph_width(&font_id, '0'));
    let gutter_width = digits as f32 * char_width + 2.0 * GUTTER_PADDING;

    let mut layouter = |ui: &Ui, text: &str, _wrap_width: f32| -> Arc<Galley> {
        let mut job = cache.job_for(text, language, font_size, dark, default_color);
        job.wrap.max_width = f32::INFINITY; // code is never soft-wrapped
        ui.fonts(|f| f.layout_job(job))
    };

    let mut result = CodeEditorOutput {
        changed: false,
        cursor: None,
    };

    egui::ScrollArea::both()
        .id_salt(id.with("scroll"))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (gutter_rect, _) =
                    ui.allocate_exact_size(Vec2::new(gutter_width, 0.0), Sense::hover());

                let output = TextEdit::multiline(&mut buffer.content)
                    .id(id)
                    .font(font_id.clone())
                    .code_editor()
                    .frame(false)
                    .lock_focus(true)
                    .desired_width(f32::INFINITY)
                    .min_size(ui.available_size())
                    .layouter(&mut layouter)
                    .show(ui);

                result.changed = output.response.changed();
                let current_line = output.cursor_range.map(|range| {
                    let p = range.primary.pcursor;
                    result.cursor = Some((p.paragraph + 1, p.offset + 1));
                    p.paragraph
                });

                paint_gutter(ui, gutter_rect, &output.galley, output.galley_pos.y, current_line, &font_id);
            });
        });

    if result.changed {
        buffer.is_modified = true;
    }
    result
}

fn paint_gutter(
    ui: &Ui,
    gutter_rect: egui::Rect,
    galley: &Galley,
    galley_top: f32,
    current_line: Option<usize>,
    font_id: &FontId,
) {
    let clip = ui.clip_rect();
    let painter = ui.painter();
    let visuals = ui.visuals();
    let bottom = (galley_top + galley.size().y).max(clip.bottom());
    painter.rect_filled(
        egui::Rect::from_x_y_ranges(gutter_rect.x_range(), clip.top()..=bottom),
        0.0,
        visuals.faint_bg_color,
    );

    let x = gutter_rect.right() - GUTTER_PADDING;
    let mut line = 0;
    let mut row_starts_line = true;
    for row in &galley.rows {
        let y = galley_top + row.min_y();
        if row_starts_line && y + row.height() >= clip.top() && y <= clip.bottom() {
            let color = if Some(line) == current_line {
                visuals.strong_text_color()
            } else {
                visuals.weak_text_color()
            };
            painter.text(
                egui::pos2(x, y),
                Align2::RIGHT_TOP,
                (line + 1).to_string(),
                font_id.clone(),
                color,
            );
        }
        row_starts_line = row.ends_with_newline;
        if row.ends_with_newline {
            line += 1;
        }
    }
}
