//! The menus' text field (wash-fps `styles/LobbyPanel.css`'s
//! `.lobby-chat__input` and `.lobby__find`, which share it): a dark slab cut
//! off at its lower right, a violet border that lights on focus, cream text.
//! No SHAG source: the browser's is a plain `<input>` with that CSS.
//!
//! Engine code: the game names its font.

use egui::{Color32, FontId, Rect, Shape, Stroke, pos2};

use super::lobby_twin::{cream, hsla, violet};

/// A field in `rect`, editing `text`. `Enter` in it sets `submitted`.
pub struct TextField<'a> {
    pub id: egui::Id,
    pub rect: Rect,
    pub text: &'a mut String,
    pub placeholder: &'a str,
    pub font: FontId,
    pub enabled: bool,
    /// `maxLength`.
    pub max_chars: usize,
}

/// What a field did this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct FieldResponse {
    /// Enter was pressed in it.
    pub submitted: bool,
    pub focused: bool,
}

impl<'a> TextField<'a> {
    #[must_use]
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug, rect: Rect, text: &'a mut String, font: FontId) -> Self {
        Self {
            id: egui::Id::NULL.with(id),
            rect,
            text,
            placeholder: "",
            font,
            enabled: true,
            max_chars: 500,
        }
    }

    #[must_use]
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    #[must_use]
    pub fn max_chars(mut self, max: usize) -> Self {
        self.max_chars = max;
        self
    }

    /// Draws the field and edits its text.
    pub fn show(self, ui: &mut egui::Ui) -> FieldResponse {
        let r = self.rect;
        let fade = |c: Color32| if self.enabled { c } else { c.gamma_multiply(0.55) };
        // `clip-path: polygon(0 0, 100% 0, calc(100% - 10px) 100%, 0 100%)`.
        let shape = vec![
            r.left_top(),
            r.right_top(),
            pos2(r.right() - 10.0, r.bottom()),
            r.left_bottom(),
        ];
        let id = ui.scope_id().with(self.id);
        let focused_before = ui.ctx().memory(|m| m.has_focus(id));
        let painter = ui.painter().clone();
        painter.add(Shape::convex_polygon(
            shape.clone(),
            fade(hsla(250.0, 44.0, 5.0, 0.8)),
            Stroke::NONE,
        ));
        let border = if focused_before {
            violet()
        } else {
            hsla(276.0, 90.0, 64.0, 0.28)
        };
        painter.add(Shape::closed_line(shape, Stroke::new(1.5, fade(border))));
        let inner = Rect::from_min_max(pos2(r.left() + 16.0, r.top()), pos2(r.right() - 16.0, r.bottom()));
        let edit = egui::TextEdit::singleline(self.text)
            .id(id)
            .frame(egui::Frame::NONE)
            .font(self.font.clone())
            .text_color(fade(cream()))
            .char_limit(self.max_chars)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(egui::RichText::new(self.placeholder).color(hsla(45.0, 100.0, 95.0, 0.38)))
            .desired_width(inner.width());
        let response = ui.add_enabled_ui(self.enabled, |ui| ui.put(inner, edit)).inner;
        let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if submitted {
            // A chat line goes, and the field stays ready for the next one.
            response.request_focus();
        }
        FieldResponse {
            submitted,
            focused: response.has_focus() || submitted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_draws_its_slab_and_takes_no_input_unasked() {
        let ctx = egui::Context::default();
        let mut text = String::new();
        let mut answer = FieldResponse::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(300.0, 44.0));
            answer = TextField::new("chat", rect, &mut text, FontId::proportional(18.0))
                .placeholder("Say something…")
                .show(ui);
        });
        output.textures_delta.clear();
        assert!(!answer.submitted && !answer.focused);
        assert!(text.is_empty());
        assert!(output.shapes.len() >= 2);
    }
}
