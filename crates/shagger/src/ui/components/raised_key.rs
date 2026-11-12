//! SHAG `src/engine/ui/components/RaisedKey.ts` and `styles/RaisedKey.css`:
//! the menus' 3D key, a face standing on its side.
//!
//! The browser builds it from two clipped layers and CSS custom properties
//! (`--key-face`, `--key-side`, `--key-d`, `--key-press`, ...); here a
//! [`KeyLook`] holds the same values and [`paint_key`] paints them with
//! egui. [`RaisedKey`] adds the states the CSS takes from the element:
//! hover and focus share the violet lift, a press sinks the face, a latched
//! key (`aria-current="page"`) is pressed in and inked, and a disabled one
//! is unlit. What the face holds is the caller's: [`KeyText`] lays out the
//! usual label and hint.
//!
//! Engine code: the game names its fonts in [`KeyText`].

use egui::{Color32, FontFamily, Painter, Pos2, Rect, Vec2, pos2, vec2};

use super::lobby_twin::{gradient_polygon, hsl, hsla, text};

/// A gradient's stops, top to bottom (0..1 of the height).
pub type Stops = Vec<(f32, Color32)>;

/// A key's custom properties: `--key-face`, `--key-side`, `--key-text`,
/// `--key-sub`, `--key-shadow`, `--key-d` and `--key-cut`.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyLook {
    pub face: Stops,
    /// `--key-side`, top and bottom.
    pub side: [Color32; 2],
    pub text: Color32,
    pub sub: Color32,
    pub shadow: Color32,
    /// `--key-d`: how far the face stands off its side.
    pub depth: f32,
    /// `--key-cut`, of the width: the slant off the lower left and upper right.
    pub cut: f32,
    /// The lit top edge (`__face::before`); the gold rail goes without it.
    pub bevel: bool,
}

/// A stage's accent (`--accent`, `--accent-hi`, `--accent-lo`,
/// `--accent-side-hi`, `--accent-side-lo`), for the keys that take its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accent {
    pub face: Color32,
    pub hi: Color32,
    pub lo: Color32,
    pub side_hi: Color32,
    pub side_lo: Color32,
}

/// `--ink` lettering.
fn ink(a: f32) -> Color32 {
    hsla(250.0, 45.0, 8.0, a)
}

impl KeyLook {
    /// `.raised-key`: gold.
    #[must_use]
    pub fn gold() -> Self {
        Self {
            face: vec![
                (0.0, hsl(51.0, 100.0, 73.0)),
                (0.55, hsl(46.0, 100.0, 58.0)),
                (1.0, hsl(42.0, 100.0, 50.0)),
            ],
            side: [hsl(36.0, 92.0, 40.0), hsl(28.0, 88.0, 25.0)],
            text: ink(1.0),
            sub: ink(0.62),
            shadow: hsla(28.0, 90.0, 14.0, 0.42),
            depth: 6.0,
            cut: 0.03,
            bevel: true,
        }
    }

    /// `.raised-key--cream`.
    #[must_use]
    pub fn cream() -> Self {
        Self {
            face: vec![
                (0.0, hsl(45.0, 100.0, 97.0)),
                (0.6, hsl(42.0, 62.0, 90.0)),
                (1.0, hsl(40.0, 42.0, 82.0)),
            ],
            side: [hsl(36.0, 26.0, 60.0), hsl(30.0, 22.0, 42.0)],
            ..Self::gold()
        }
    }

    /// `.raised-key--ink`: ink at rest, gold lettering.
    #[must_use]
    pub fn ink() -> Self {
        Self {
            face: ink_face(),
            side: [hsl(252.0, 44.0, 5.0), hsl(252.0, 44.0, 3.0)],
            text: hsl(46.0, 100.0, 60.0),
            sub: hsla(46.0, 100.0, 60.0, 0.62),
            shadow: hsla(28.0, 90.0, 14.0, 0.5),
            ..Self::gold()
        }
    }

    /// `.raised-key--cta`: the arcade key, in the stage's accent.
    #[must_use]
    pub fn cta(accent: &Accent) -> Self {
        Self {
            face: vec![(0.0, accent.hi), (0.58, accent.face), (1.0, accent.lo)],
            side: [accent.side_hi, accent.side_lo],
            shadow: hsla(250.0, 70.0, 3.0, 0.62),
            depth: 9.6,
            cut: 0.05,
            ..Self::gold()
        }
    }

    /// `.raised-key--sm`: shallower.
    #[must_use]
    pub fn sm(self) -> Self {
        Self { depth: 3.6, ..self }
    }

    /// The same key at another depth.
    #[must_use]
    pub fn depth(self, depth: f32) -> Self {
        Self { depth, ..self }
    }

    /// Without the lit top edge.
    #[must_use]
    pub fn flat(self) -> Self {
        Self { bevel: false, ..self }
    }

    /// `:hover, :focus-visible`: the violet lift, which is also the focus ring.
    #[must_use]
    pub fn hovered(&self) -> Self {
        Self {
            face: vec![
                (0.0, hsl(278.0, 92.0, 75.0)),
                (0.55, hsl(273.0, 80.0, 61.0)),
                (1.0, hsl(268.0, 72.0, 50.0)),
            ],
            side: [hsl(266.0, 72.0, 34.0), hsl(258.0, 72.0, 19.0)],
            text: hsl(45.0, 100.0, 96.0),
            sub: hsla(45.0, 100.0, 96.0, 0.72),
            shadow: hsla(268.0, 80.0, 14.0, 0.5),
            ..self.clone()
        }
    }

    /// `:active`, over the hover look.
    #[must_use]
    pub fn pressed(&self) -> Self {
        Self {
            face: vec![(0.0, hsl(272.0, 76.0, 58.0)), (1.0, hsl(266.0, 70.0, 45.0))],
            ..self.hovered()
        }
    }

    /// `[aria-current="page"]`: a latched key, inked.
    #[must_use]
    pub fn current(&self) -> Self {
        Self {
            face: ink_face(),
            side: [hsl(252.0, 44.0, 5.0), hsl(252.0, 44.0, 3.0)],
            text: hsl(46.0, 100.0, 60.0),
            sub: hsla(46.0, 100.0, 60.0, 0.66),
            ..self.clone()
        }
    }

    /// `:disabled`: `saturate(0.45) brightness(0.94)` on the face, the
    /// lettering at half.
    #[must_use]
    pub fn unlit(&self) -> Self {
        let dim = |c: Color32| desaturate(c, 0.45, 0.94);
        Self {
            face: self.face.iter().map(|&(at, c)| (at, dim(c))).collect(),
            text: self.text.gamma_multiply(0.5),
            sub: self.sub.gamma_multiply(0.5),
            ..self.clone()
        }
    }
}

fn ink_face() -> Stops {
    vec![
        (0.0, hsl(250.0, 28.0, 21.0)),
        (0.6, hsl(250.0, 40.0, 10.0)),
        (1.0, hsl(252.0, 44.0, 6.0)),
    ]
}

/// CSS `saturate()` then `brightness()`.
#[must_use]
pub fn desaturate(c: Color32, saturation: f32, brightness: f32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied().map(|v| f32::from(v) / 255.0);
    let gray = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let mix = |v: f32| ((gray + (v - gray) * saturation) * brightness).clamp(0.0, 1.0);
    let byte = |v: f32| (v * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(mix(r)), byte(mix(g)), byte(mix(b)), byte(a))
}

/// The key's outline at `rect`, moved by `offset`: cut off the lower left
/// and upper right (`clip-path`).
fn outline(rect: Rect, cut: f32, offset: Vec2) -> [Pos2; 4] {
    [
        rect.left_top() + vec2(cut, 0.0) + offset,
        rect.right_top() + offset,
        rect.right_bottom() + vec2(-cut, 0.0) + offset,
        rect.left_bottom() + offset,
    ]
}

/// Paints a key whose face rests at `rect`, `press` (`--key-press`) down:
/// negative lifts the face off its side, 1 sinks it flat. Returns the face
/// as drawn. `alpha` fades the whole key (a menu sliding in).
pub fn paint_key(painter: &Painter, rect: Rect, look: &KeyLook, press: f32, alpha: f32) -> Rect {
    let fade = |c: Color32| c.gamma_multiply(alpha);
    let d = look.depth;
    let dx = d * 0.75;
    let cut = rect.width() * look.cut;
    // The cast shadow: `drop-shadow(... 9px)`, softened by three widening copies.
    let shadow_at = vec2(dx * (1.0 - press) + 2.0, d * (1.0 - press) + 5.0);
    for (grow, share) in [(4.0, 0.25), (2.0, 0.35), (0.0, 0.45)] {
        let [a, b, c, e] = outline(rect, cut, shadow_at);
        let g = vec2(grow, grow);
        let corners = [a - g, b + vec2(grow, -grow), c + g, e + vec2(-grow, grow)];
        let color = fade(look.shadow.gamma_multiply(share));
        gradient_polygon(painter, &corners, &[(0.0, color), (1.0, color)]);
    }
    // The side it stands on.
    gradient_polygon(
        painter,
        &outline(rect, cut, vec2(dx, d)),
        &[(0.0, fade(look.side[0])), (1.0, fade(look.side[1]))],
    );
    // The face, its lit top edge and its shaded foot.
    let lift = vec2(dx * press, d * press);
    let face = outline(rect, cut, lift);
    let stops: Stops = look.face.iter().map(|&(at, c)| (at, fade(c))).collect();
    gradient_polygon(painter, &face, &stops);
    let h = rect.height();
    let band = |from: f32, to: f32| {
        let at = |t: f32| face[0].lerp(face[3], t);
        let at_right = |t: f32| face[1].lerp(face[2], t);
        [at(from / h), at_right(from / h), at_right(to / h), at(to / h)]
    };
    if look.bevel {
        let white = fade(Color32::from_white_alpha(140));
        gradient_polygon(painter, &band(0.0, 2.0), &[(0.0, white), (1.0, white)]);
    }
    let foot = fade(hsla(250.0, 60.0, 5.0, 0.14));
    gradient_polygon(painter, &band(h - 3.0, h), &[(0.0, foot), (1.0, foot)]);
    rect.translate(lift)
}

/// A key on screen, with its states.
#[derive(Debug, Clone)]
pub struct RaisedKey {
    /// Unique among the keys of one `Ui`.
    pub id: egui::Id,
    /// The face at rest.
    pub rect: Rect,
    pub look: KeyLook,
    /// `--key-press` at rest (a picked mode key stands lifted, -0.5).
    pub rest_press: f32,
    /// The hover look and its press, when the variant has its own
    /// (`.lobby__rail-nav` keeps gold); `None` is the violet lift at -0.35.
    pub hover: Option<(KeyLook, f32)>,
    /// The face while held, when the variant has its own.
    pub active_face: Option<Stops>,
    /// `[aria-current="page"]`.
    pub current: bool,
    pub disabled: bool,
    /// Lit by a controller (`UINavigation`'s focus), as hover.
    pub focused: bool,
    /// Fades the whole key.
    pub alpha: f32,
}

/// What a key did, and where its face is now, for its contents.
#[derive(Debug, Clone)]
pub struct KeyFace {
    pub clicked: bool,
    pub hovered: bool,
    /// The face as drawn, pressed or lifted.
    pub rect: Rect,
    /// `--key-text` and `--key-sub` in its state.
    pub text: Color32,
    pub sub: Color32,
}

impl RaisedKey {
    #[must_use]
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug, rect: Rect, look: KeyLook) -> Self {
        Self {
            id: egui::Id::NULL.with(id),
            rect,
            look,
            rest_press: 0.0,
            hover: None,
            active_face: None,
            current: false,
            disabled: false,
            focused: false,
            alpha: 1.0,
        }
    }

    #[must_use]
    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }

    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    #[must_use]
    pub fn hover(mut self, look: KeyLook, press: f32) -> Self {
        self.hover = Some((look, press));
        self
    }

    #[must_use]
    pub fn rest_press(mut self, press: f32) -> Self {
        self.rest_press = press;
        self
    }

    #[must_use]
    pub fn active_face(mut self, face: Stops) -> Self {
        self.active_face = Some(face);
        self
    }

    #[must_use]
    pub fn alpha(mut self, alpha: f32) -> Self {
        self.alpha = alpha;
        self
    }

    /// The look and `--key-press` for a state, in the CSS's order (later
    /// rules win): rest, hover, active, current; disabled only unlights.
    #[must_use]
    pub fn look_for(&self, hovered: bool, held: bool) -> (KeyLook, f32) {
        if self.disabled {
            let (look, press) = if self.current {
                (self.look.current(), 0.62)
            } else {
                (self.look.clone(), self.rest_press)
            };
            return (look.unlit(), press);
        }
        if self.current {
            return (self.look.current(), 0.62);
        }
        let (hover_look, hover_press) = self.hover.clone().unwrap_or_else(|| (self.look.hovered(), -0.35));
        if held {
            // A variant's own hover outranks `:active` in the CSS, so it
            // keeps its face unless it names one for the press.
            let mut look = if self.hover.is_some() {
                hover_look
            } else {
                self.look.pressed()
            };
            if let Some(face) = &self.active_face {
                look.face.clone_from(face);
            }
            return (look, 1.0);
        }
        if hovered {
            return (hover_look, hover_press);
        }
        (self.look.clone(), self.rest_press)
    }

    /// Draws the key; its contents are the caller's, on the face returned.
    pub fn show(self, ui: &mut egui::Ui) -> KeyFace {
        let sense = if self.disabled {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        let response = ui.interact(self.rect, ui.scope_id().with(self.id), sense);
        let hovered = !self.disabled && (response.hovered() || self.focused);
        let held = !self.disabled && response.is_pointer_button_down_on();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        } else if self.disabled && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
        }
        let (look, press) = self.look_for(hovered, held);
        let rect = paint_key(ui.painter(), self.rect, &look, press, self.alpha);
        KeyFace {
            clicked: !self.disabled && response.clicked(),
            hovered,
            rect,
            text: look.text.gamma_multiply(self.alpha),
            sub: look.sub.gamma_multiply(self.alpha),
        }
    }
}

/// How a face sets its label and hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyTextLayout {
    /// Label left, hint right (`justify-content: space-between`).
    Row,
    /// The hint under the label, as a narrow rail sets them.
    Stacked,
    /// The label alone, centred (`.raised-key--cta`, `--util`).
    Centered,
}

/// `raised-key__label` and `__hint`: uppercase, the label in the display
/// face and the hint in the body face.
#[derive(Debug, Clone)]
pub struct KeyText<'a> {
    pub label: &'a str,
    pub hint: &'a str,
    pub label_size: f32,
    /// `letter-spacing` of the label, in em.
    pub label_spacing: f32,
    pub hint_size: f32,
    pub display: FontFamily,
    pub body: FontFamily,
    /// The face's padding, left and right.
    pub pad: (f32, f32),
    pub layout: KeyTextLayout,
}

impl<'a> KeyText<'a> {
    /// The base key's lettering: 20px label, 13px hint, 26/22 padding.
    #[must_use]
    pub fn new(label: &'a str, hint: &'a str, display: FontFamily, body: FontFamily) -> Self {
        Self {
            label,
            hint,
            label_size: 20.0,
            label_spacing: 0.07,
            hint_size: 13.0,
            display,
            body,
            pad: (26.0, 22.0),
            layout: KeyTextLayout::Row,
        }
    }

    #[must_use]
    pub fn size(mut self, label: f32, hint: f32) -> Self {
        self.label_size = label;
        self.hint_size = hint;
        self
    }

    #[must_use]
    pub fn spacing(mut self, em: f32) -> Self {
        self.label_spacing = em;
        self
    }

    #[must_use]
    pub fn pad(mut self, left: f32, right: f32) -> Self {
        self.pad = (left, right);
        self
    }

    #[must_use]
    pub fn layout(mut self, layout: KeyTextLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Sets the lettering on `face`, the label trimmed with an ellipsis
    /// before it would run into the hint.
    pub fn paint(&self, painter: &Painter, face: &KeyFace) {
        let inner = Rect::from_min_max(
            face.rect.min + vec2(self.pad.0, 0.0),
            face.rect.max - vec2(self.pad.1, 0.0),
        );
        let hint = (!self.hint.is_empty()).then(|| {
            text(
                painter,
                &self.hint.to_uppercase(),
                self.hint_size,
                self.body.clone(),
                face.sub,
                self.hint_size * 0.12,
            )
        });
        let room = match (&hint, self.layout) {
            (Some(h), KeyTextLayout::Row) => inner.width() - h.size().x - 14.0,
            _ => inner.width(),
        };
        let label = super::lobby_twin::elide(
            painter,
            &self.label.to_uppercase(),
            self.label_size,
            self.display.clone(),
            room.max(0.0),
        );
        let label = text(
            painter,
            &label,
            self.label_size,
            self.display.clone(),
            face.text,
            self.label_size * self.label_spacing,
        );
        match (self.layout, hint) {
            (KeyTextLayout::Centered, _) => {
                painter.galley(inner.center() - label.size() / 2.0, label, face.text);
            }
            (_, None) => {
                let y = inner.center().y - label.size().y / 2.0;
                painter.galley(pos2(inner.left(), y), label, face.text);
            }
            (KeyTextLayout::Stacked, Some(hint)) => {
                let gap = 5.0;
                let total = label.size().y + gap + hint.size().y;
                let top = inner.center().y - total / 2.0;
                let hint_y = top + total - hint.size().y;
                painter.galley(pos2(inner.left(), top), label, face.text);
                painter.galley(pos2(inner.left(), hint_y), hint, face.sub);
            }
            (KeyTextLayout::Row, Some(hint)) => {
                let label_y = inner.center().y - label.size().y / 2.0;
                let hint_at = pos2(inner.right() - hint.size().x, inner.center().y - hint.size().y / 2.0);
                painter.galley(pos2(inner.left(), label_y), label, face.text);
                painter.galley(hint_at, hint, face.sub);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> RaisedKey {
        RaisedKey::new("k", Rect::from_min_size(Pos2::ZERO, vec2(200.0, 60.0)), KeyLook::gold())
    }

    #[test]
    fn states_follow_the_css_order() {
        let k = key();
        assert_eq!(k.look_for(false, false), (KeyLook::gold(), 0.0));
        let (hover, press) = k.look_for(true, false);
        assert_eq!(press, -0.35);
        assert_eq!(hover, KeyLook::gold().hovered());
        let (held, press) = k.look_for(true, true);
        assert_eq!(press, 1.0);
        assert_eq!(held.face, KeyLook::gold().pressed().face);
        // A latched key stays latched under the pointer.
        let latched = key().current(true);
        assert_eq!(latched.look_for(true, true), (KeyLook::gold().current(), 0.62));
    }

    #[test]
    fn a_variant_keeps_its_own_hover() {
        let k = key().hover(KeyLook::gold(), -0.2);
        assert_eq!(k.look_for(true, false), (KeyLook::gold(), -0.2));
        let (held, _) = k.look_for(true, true);
        assert_eq!(held.text, KeyLook::gold().text);
    }

    #[test]
    fn a_disabled_key_is_unlit_and_does_not_lift() {
        let k = key().disabled(true);
        let (look, press) = k.look_for(true, true);
        assert_eq!(press, 0.0);
        assert_eq!(look, KeyLook::gold().unlit());
        assert_ne!(look.face, KeyLook::gold().face);
    }

    #[test]
    fn a_key_draws_and_clicks_nothing_unasked() {
        let ctx = egui::Context::default();
        let mut clicked = true;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let face = key().show(ui);
            KeyText::new("Play", "4 modes", FontFamily::Proportional, FontFamily::Proportional)
                .paint(ui.painter(), &face);
            clicked = face.clicked;
        });
        output.textures_delta.clear();
        assert!(!clicked);
        assert!(!output.shapes.is_empty());
    }
}
