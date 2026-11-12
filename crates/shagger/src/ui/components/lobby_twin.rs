//! SHAG `src/engine/ui/components/LobbyTwin.ts` and `styles/LobbyTwin.css`:
//! the lobby's rail and stage backdrop, for the screens before and between
//! it (the match handoff and the map loading screen).
//!
//! The lobby takes over from these screens, or they from it, so a change of
//! screen should only change what the rail and stage hold, never where they
//! are. Each screen builds the rail here and puts its own content on the
//! stage. The browser builds it from HTML and CSS; here it is painted with
//! egui at the CSS's sizes and colours (CSS pixels are egui points).
//!
//! Engine code, game-agnostic: the game names its brand and its fonts in
//! [`TwinStyle`].

use std::sync::Arc;

use super::raised_key::desaturate;

use egui::{Align2, Color32, FontFamily, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

/// The game's look for the twin: its logo or name at the head of the rail,
/// its fonts (`--w-font-display`, `--w-font-body`) and the engine line.
#[derive(Debug, Clone)]
pub struct TwinStyle {
    pub brand: String,
    /// `LobbyTwinBrand.logo`, decoded. Without one the name stands in, as
    /// the browser shows it until the logo loads.
    pub logo: Option<Arc<egui::ColorImage>>,
    pub display: FontFamily,
    pub body: FontFamily,
    /// `EngineVersion.getEngineName()`.
    pub engine: String,
    /// `EngineVersion.getVersionString()`.
    pub version: String,
}

impl Default for TwinStyle {
    fn default() -> Self {
        Self {
            brand: "SHAG".into(),
            logo: None,
            display: FontFamily::Proportional,
            body: FontFamily::Proportional,
            engine: "SHAG".into(),
            version: String::new(),
        }
    }
}

/// The logo's texture, uploaded once per image and kept in the context.
fn logo_texture(ctx: &egui::Context, image: &Arc<egui::ColorImage>) -> egui::TextureHandle {
    let id = egui::Id::unique(("twin-logo", Arc::as_ptr(image) as usize));
    if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    // Mipmapped: the rail draws it at a fraction of its size.
    let options = egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
    let texture = ctx.load_texture("twin-logo", (**image).clone(), options);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

/// A CSS `hsla()`, saturation and lightness in percent.
#[must_use]
pub fn hsla(h: f32, s: f32, l: f32, a: f32) -> Color32 {
    let s = s / 100.0;
    let l = l / 100.0;
    let k = |n: f32| (n + h / 30.0).rem_euclid(12.0);
    let chroma = s * l.min(1.0 - l);
    let f = |n: f32| l - chroma * (k(n) - 3.0).min(9.0 - k(n)).clamp(-1.0, 1.0);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(f(0.0)), byte(f(8.0)), byte(f(4.0)), byte(a))
}

/// A CSS `hsl()`.
#[must_use]
pub fn hsl(h: f32, s: f32, l: f32) -> Color32 {
    hsla(h, s, l, 1.0)
}

pub fn ink() -> Color32 {
    hsl(250.0, 45.0, 7.0)
}
pub fn cream() -> Color32 {
    hsl(45.0, 100.0, 95.0)
}
pub fn gold() -> Color32 {
    hsl(46.0, 100.0, 57.0)
}
pub fn violet() -> Color32 {
    hsl(276.0, 90.0, 64.0)
}
pub fn cyan() -> Color32 {
    hsl(182.0, 95.0, 52.0)
}
/// `--co-alarm`.
pub fn alarm() -> Color32 {
    hsl(330.0, 88.0, 58.0)
}

/// `--rail-skew`.
pub const RAIL_SKEW: f32 = 44.0;

/// CSS `clamp(min, preferred, max)`.
#[must_use]
pub fn clamp_css(min: f32, preferred: f32, max: f32) -> f32 {
    preferred.min(max).max(min)
}

/// A step's state: done (lit), active (latched in ink), pending (unlit) or error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwinStepState {
    Done,
    Active,
    Pending,
    Error,
}

impl TwinStepState {
    /// `STEP_HINTS`.
    #[must_use]
    pub fn hint(self) -> &'static str {
        match self {
            Self::Done => "Done",
            Self::Active => "Loading",
            Self::Pending => "",
            Self::Error => "Failed",
        }
    }
}

/// Lines kept in a rail's log; older ones scroll away.
pub const LOG_LINES: usize = 40;

/// The rail: the game's name, the screen's steps as the lobby's nav keys, a
/// panel with the screen's facts and log where the lobby lists rooms, and the
/// engine in the player card's place (`buildTwinRail`).
#[derive(Debug, Clone, PartialEq)]
pub struct TwinRail {
    pub label: String,
    /// The panel's heading: what this screen is (a match handoff, a local map).
    pub context: String,
    pub steps: Vec<(String, TwinStepState)>,
    /// The facts at the head of the panel, by key: (key, label, value).
    pub facts: Vec<(String, String, String)>,
    /// The panel's log, newest last.
    pub lines: Vec<String>,
}

impl TwinRail {
    #[must_use]
    pub fn new(label: &str, steps: &[&str], context: &str) -> Self {
        Self {
            label: label.into(),
            context: context.into(),
            steps: steps
                .iter()
                .map(|s| ((*s).to_owned(), TwinStepState::Pending))
                .collect(),
            facts: Vec::new(),
            lines: Vec::new(),
        }
    }

    /// Set each step's state at once, in order.
    pub fn set_steps(&mut self, states: &[TwinStepState]) {
        for (index, (_, state)) in self.steps.iter_mut().enumerate() {
            *state = states.get(index).copied().unwrap_or(TwinStepState::Pending);
        }
    }

    /// A fact at the head of the panel, added or updated by key.
    pub fn set_fact(&mut self, key: &str, label: &str, value: &str) {
        if let Some(fact) = self.facts.iter_mut().find(|(k, _, _)| k == key) {
            value.clone_into(&mut fact.2);
        } else {
            self.facts.push((key.into(), label.into(), value.into()));
        }
    }

    /// A line in the panel's log, newest last; a repeat of the last is dropped.
    pub fn log(&mut self, line: &str) {
        if line.is_empty() || self.lines.last().is_some_and(|l| l == line) {
            return;
        }
        self.lines.push(line.into());
        if self.lines.len() > LOG_LINES {
            let extra = self.lines.len() - LOG_LINES;
            self.lines.drain(..extra);
        }
    }

    /// The rail's width at a screen width: `clamp(300px, 26vw, 520px)`.
    #[must_use]
    pub fn width(screen_width: f32) -> f32 {
        clamp_css(300.0, screen_width * 0.26, 520.0)
    }

    /// Paints the rail down the left of `screen`.
    pub fn paint(&self, painter: &Painter, screen: Rect, style: &TwinStyle) {
        let w = Self::width(screen.width());
        let (x0, y0, h) = (screen.min.x, screen.min.y, screen.height());
        paint_rail(painter, x0, y0, y0 + h, w, RAIL_SKEW, 1.0);
        let inner_left = x0 + 22.0;
        let inner_right = x0 + w - RAIL_SKEW - 18.0;
        // The brand: `.twin__rail`'s padding, 14px when the window is short.
        let short = screen.width() > 700.0 && h <= 680.0;
        let top = y0 + if short { 14.0 } else { 22.0 };
        let max_h = short.then_some(h * 0.11);
        let mut y = paint_brand(painter, x0, w, top, max_h, screen.width(), style, 1.0);

        // The steps, as keys.
        let hints = screen.width() > 1500.0;
        for (label, state) in &self.steps {
            let key = Rect::from_min_max(pos2(inner_left, y), pos2(inner_right, y + 66.0));
            paint_key(painter, key, label, state.hint(), *state, hints, style, screen.width());
            y += 66.0 + 6.0 + 8.0;
        }

        // The panel: its heading, then the facts and the log.
        let card_height = 54.0;
        let bottom = y0 + h - 18.0 - card_height - 12.0;
        let sub = hsla(250.0, 45.0, 8.0, 0.62);
        y += 6.0;
        let context = text(
            painter,
            &self.context.to_uppercase(),
            11.0,
            style.body.clone(),
            sub,
            2.86,
        );
        painter.galley(pos2(inner_left + 2.0, y), context, sub);
        let version = text(painter, &style.version, 11.0, style.body.clone(), sub, 2.86);
        painter.galley(pos2(inner_right - 8.0 - version.size().x, y), version, sub);
        y += 11.0 + 12.0;
        if bottom - y > 60.0 {
            let panel = Rect::from_min_max(pos2(inner_left, y), pos2(inner_right, bottom));
            self.paint_panel(painter, panel, style);
        }

        // The engine, where the lobby has the player card.
        let card_top = y0 + h - 18.0 - card_height;
        let avatar = Rect::from_min_size(pos2(inner_left, card_top + 7.0), vec2(40.0, 40.0));
        painter.rect_filled(avatar, 6.0, hsla(250.0, 45.0, 8.0, 0.85));
        painter.text(
            avatar.center(),
            Align2::CENTER_CENTER,
            style.engine.chars().next().map(String::from).unwrap_or_default(),
            FontId::new(18.0, style.display.clone()),
            gold(),
        );
        painter.text(
            pos2(avatar.max.x + 12.0, avatar.center().y - 2.0),
            Align2::LEFT_BOTTOM,
            &style.engine,
            FontId::new(17.0, style.display.clone()),
            ink(),
        );
        painter.text(
            pos2(avatar.max.x + 12.0, avatar.center().y + 2.0),
            Align2::LEFT_TOP,
            &style.version,
            FontId::new(13.0, style.body.clone()),
            sub,
        );
    }

    fn paint_panel(&self, painter: &Painter, panel: Rect, style: &TwinStyle) {
        // Clipped at the top right corner.
        let cut = 22.0;
        let shape = [
            panel.min,
            pos2(panel.max.x - cut, panel.min.y),
            pos2(panel.max.x, panel.min.y + cut),
            panel.max,
            pos2(panel.min.x, panel.max.y),
        ];
        gradient_polygon(
            painter,
            &shape,
            &[
                (0.0, hsla(250.0, 40.0, 11.0, 0.86)),
                (1.0, hsla(250.0, 44.0, 7.0, 0.92)),
            ],
        );
        painter.add(Shape::closed_line(
            shape.to_vec(),
            Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.32)),
        ));
        let inner = panel.shrink2(vec2(18.0, 14.0));
        let mut y = inner.min.y;
        for (_, label, value) in &self.facts {
            let label = text(
                painter,
                &label.to_uppercase(),
                11.0,
                style.body.clone(),
                hsla(45.0, 100.0, 95.0, 0.5),
                2.2,
            );
            let lh = label.size();
            painter.galley(pos2(inner.min.x, y), label, cream());
            let value = elide(painter, value, 15.0, style.body.clone(), inner.width() - lh.x - 12.0);
            painter.text(
                pos2(inner.max.x, y + lh.y / 2.0),
                Align2::RIGHT_CENTER,
                value,
                FontId::new(15.0, style.body.clone()),
                gold(),
            );
            y += lh.y.max(15.0) + 8.0;
        }
        if !self.facts.is_empty() {
            y += 2.0;
            painter.hline(
                inner.min.x..=inner.max.x,
                y,
                Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.2)),
            );
            y += 10.0;
        }
        // The log, bottom up, as far as it fits.
        let line_height = 19.0;
        let top = y + 6.0;
        let mut at = inner.max.y - line_height;
        for line in self.lines.iter().rev() {
            if at < top {
                break;
            }
            let shown = elide(painter, line, 14.0, style.body.clone(), inner.width());
            painter.text(
                pos2(inner.min.x, at),
                Align2::LEFT_TOP,
                shown,
                FontId::new(14.0, style.body.clone()),
                hsla(45.0, 100.0, 95.0, 0.78),
            );
            at -= line_height;
        }
    }
}

/// The gold rail (`.lobby__rail`, `.twin__rail`): `width` wide at the top,
/// `skew` ([`RAIL_SKEW`]) narrower at the bottom, from `left`, with its glow at the
/// head and the violet edge along the slant. `alpha` fades it (a menu
/// sliding in).
pub fn paint_rail(painter: &Painter, left: f32, top: f32, bottom: f32, width: f32, skew: f32, alpha: f32) {
    let fade = |c: Color32| c.gamma_multiply(alpha);
    let (x0, y0, w, h) = (left, top, width, bottom - top);
    let rail = [
        pos2(x0, y0),
        pos2(x0 + w, y0),
        pos2(x0 + w - skew, y0 + h),
        pos2(x0, y0 + h),
    ];
    gradient_polygon(
        painter,
        &rail,
        &[(0.0, fade(hsl(46.0, 100.0, 57.0))), (1.0, fade(hsl(41.0, 96.0, 48.0)))],
    );
    // The highlight at the top left (`radial-gradient(120% 55% at 25% 0%)`).
    radial_glow(
        painter,
        pos2(x0 + w * 0.25, y0),
        vec2(w * 0.6, h * 0.275),
        fade(hsla(54.0, 100.0, 78.0, 0.55)),
        Some(&rail),
    );
    // The violet edge along the slant.
    let edge = [
        pos2(x0 + w - 8.0, y0),
        pos2(x0 + w, y0),
        pos2(x0 + w - skew, y0 + h),
        pos2(x0 + w - skew - 8.0, y0 + h),
    ];
    gradient_polygon(
        painter,
        &edge,
        &[(0.0, fade(violet())), (1.0, fade(hsl(262.0, 75.0, 42.0)))],
    );
}

/// The brand at the head of a rail `width` wide from `left` (`buildRailBrand`):
/// the logo up to 420px wide, at most `max_height` tall, or the name set in
/// type without one. Returns where the rail's next part starts.
#[allow(clippy::too_many_arguments)]
pub fn paint_brand(
    painter: &Painter,
    left: f32,
    width: f32,
    top: f32,
    max_height: Option<f32>,
    screen_width: f32,
    style: &TwinStyle,
    alpha: f32,
) -> f32 {
    let inner_left = left + 22.0;
    if let Some(texture) = style.logo.as_ref().map(|image| logo_texture(painter.ctx(), image)) {
        let brand_right = left + width - RAIL_SKEW - 16.0;
        let size = texture.size_vec2();
        let mut w = (brand_right - inner_left).min(420.0);
        if let Some(max) = max_height {
            w = w.min(max * size.x / size.y);
        }
        let logo = Rect::from_min_size(pos2(inner_left, top), vec2(w, w * size.y / size.x));
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        // The drop shadow, unblurred.
        painter.image(
            texture.id(),
            logo.translate(vec2(0.0, 6.0)),
            uv,
            hsla(28.0, 90.0, 14.0, 0.35).gamma_multiply(alpha),
        );
        painter.image(texture.id(), logo, uv, Color32::WHITE.gamma_multiply(alpha));
        logo.max.y + 16.0
    } else {
        let brand_size = clamp_css(24.0, screen_width * 0.026, 44.0);
        let brand = text(painter, &style.brand, brand_size, style.display.clone(), ink(), 0.0);
        let y = top + 14.0;
        painter.galley_with_override_text_color(
            pos2(inner_left + 3.0, y + 4.0),
            brand.clone(),
            hsla(30.0, 90.0, 32.0, 0.3).gamma_multiply(alpha),
        );
        painter.galley(pos2(inner_left, y), brand.clone(), ink().gamma_multiply(alpha));
        y + brand.size().y + 6.0 + 16.0
    }
}

/// A rail key (`twin-key`): the side below and to the right, the face on
/// top, pressed by its state.
#[allow(clippy::too_many_arguments)]
fn paint_key(
    painter: &Painter,
    key: Rect,
    label: &str,
    hint: &str,
    state: TwinStepState,
    show_hint: bool,
    style: &TwinStyle,
    screen_width: f32,
) {
    let (d, dx) = (6.0, 4.5);
    let active = state == TwinStepState::Active;
    let error = state == TwinStepState::Error;
    let press = if active || error { 0.62 } else { 0.0 };
    let cut = key.width() * 0.03;
    let parallelogram = |r: Rect| {
        [
            pos2(r.min.x + cut, r.min.y),
            pos2(r.max.x, r.min.y),
            pos2(r.max.x - cut, r.max.y),
            pos2(r.min.x, r.max.y),
        ]
    };
    let (face, side, label_color, sub) = if error {
        // A step that failed: in the alarm colour, so the rail says where it stopped.
        (
            vec![
                (0.0, hsl(338.0, 88.0, 56.0)),
                (0.6, hsl(330.0, 82.0, 44.0)),
                (1.0, hsl(324.0, 80.0, 34.0)),
            ],
            vec![(0.0, hsl(322.0, 80.0, 24.0)), (1.0, hsl(318.0, 80.0, 14.0))],
            hsl(45.0, 100.0, 96.0),
            hsla(45.0, 100.0, 96.0, 0.75),
        )
    } else if active {
        (
            vec![
                (0.0, hsl(250.0, 28.0, 21.0)),
                (0.6, hsl(250.0, 40.0, 10.0)),
                (1.0, hsl(252.0, 44.0, 6.0)),
            ],
            vec![(0.0, hsl(252.0, 44.0, 5.0)), (1.0, hsl(252.0, 44.0, 3.0))],
            hsl(46.0, 100.0, 60.0),
            hsla(46.0, 100.0, 60.0, 0.66),
        )
    } else {
        (
            vec![
                (0.0, hsl(51.0, 100.0, 73.0)),
                (0.55, hsl(46.0, 100.0, 58.0)),
                (1.0, hsl(42.0, 100.0, 50.0)),
            ],
            vec![(0.0, hsl(36.0, 92.0, 40.0)), (1.0, hsl(28.0, 88.0, 25.0))],
            hsl(250.0, 45.0, 8.0),
            hsla(250.0, 45.0, 8.0, 0.62),
        )
    };
    // Unlit: `saturate(0.45) brightness(0.94)`, the label at half.
    let pending = state == TwinStepState::Pending;
    let unlit = |c: Color32| if pending { desaturate(c, 0.45, 0.94) } else { c };
    let face: Vec<_> = face.into_iter().map(|(at, c)| (at, unlit(c))).collect();
    let side: Vec<_> = side.into_iter().map(|(at, c)| (at, unlit(c))).collect();
    let fade = |c: Color32| if pending { c.gamma_multiply(0.5) } else { c };
    // The drop shadow.
    let lift = 1.0 - press;
    painter.add(Shape::convex_polygon(
        parallelogram(key.translate(vec2(dx * lift + 2.0, d * lift + 5.0))).to_vec(),
        hsla(28.0, 90.0, 14.0, 0.2),
        Stroke::NONE,
    ));
    gradient_polygon(painter, &parallelogram(key.translate(vec2(dx, d))), &side);
    let face_rect = key.translate(vec2(dx * press, d * press));
    gradient_polygon(painter, &parallelogram(face_rect), &face);
    painter.rect_filled(
        Rect::from_min_max(pos2(face_rect.min.x, face_rect.max.y - 3.0), face_rect.max),
        0.0,
        hsla(250.0, 60.0, 5.0, 0.14),
    );
    let size = clamp_css(15.0, screen_width * 0.011, 21.0);
    let galley = text(
        painter,
        &label.to_uppercase(),
        size,
        style.display.clone(),
        fade(label_color),
        0.07 * size,
    );
    painter.galley(
        pos2(face_rect.min.x + 28.0, face_rect.center().y - galley.size().y / 2.0),
        galley,
        fade(label_color),
    );
    if show_hint && !hint.is_empty() {
        let hint = text(painter, &hint.to_uppercase(), 13.0, style.body.clone(), fade(sub), 1.56);
        painter.galley(
            pos2(
                face_rect.max.x - 24.0 - hint.size().x,
                face_rect.center().y - hint.size().y / 2.0,
            ),
            hint,
            fade(sub),
        );
    }
}

/// The stage's backdrop: the lobby's blue, `linear-gradient(165deg, ...)`.
pub fn paint_backdrop(painter: &Painter, screen: Rect) {
    gradient_polygon(
        painter,
        &[
            screen.left_top(),
            screen.right_top(),
            screen.right_bottom(),
            screen.left_bottom(),
        ],
        &[
            (0.0, hsl(222.0, 56.0, 17.0)),
            (0.55, hsl(217.0, 58.0, 24.0)),
            (1.0, hsl(224.0, 52.0, 16.0)),
        ],
    );
}

/// The stage's drifting light (`buildTwinLight`): three blurred fields
/// wandering slowly, `time` seconds into their drift.
pub fn paint_light(painter: &Painter, screen: Rect, time: f64) {
    paint_light_in(painter, screen, time, cyan());
}

/// [`paint_light`] with the first field in `accent`, as the lobby's stage
/// takes the picked mode's colour (`.lobby__orb--accent`).
pub fn paint_light_in(painter: &Painter, screen: Rect, time: f64, accent: Color32) {
    // (size, opacity, colour, period, from (vw, vh), to (vw, vh)), as `twin__orb--*`.
    let orbs = [
        (640.0, 0.24, accent, 38.0, (0.40, -0.10), (0.70, 0.40)),
        (760.0, 0.22, hsl(276.0, 90.0, 56.0), 47.0, (0.75, 0.50), (0.35, 0.05)),
        (560.0, 0.20, hsl(232.0, 85.0, 52.0), 31.0, (0.50, 0.70), (0.80, 0.10)),
    ];
    for (size, opacity, color, period, from, to) in orbs {
        // `ease-in-out infinite alternate`.
        let phase = (time / period).rem_euclid(2.0);
        let t = if phase > 1.0 { 2.0 - phase } else { phase } as f32;
        let eased = t * t * (3.0 - 2.0 * t);
        let at = |a: f32, b: f32| a + (b - a) * eased;
        let corner = pos2(
            screen.min.x + at(from.0, to.0) * screen.width(),
            screen.min.y + at(from.1, to.1) * screen.height(),
        );
        let radius: f32 = size / 2.0;
        // `blur(90px)` spreads the disc's edge out by about that much.
        radial_glow(
            painter,
            corner + vec2(radius, radius),
            vec2(radius + 90.0, radius + 90.0),
            color.gamma_multiply(opacity),
            None,
        );
    }
}

/// A soft elliptical glow, `color` at the centre to nothing at `radius`,
/// kept inside the convex `clip` when given.
fn radial_glow(painter: &Painter, centre: Pos2, radius: egui::Vec2, color: Color32, clip: Option<&[Pos2]>) {
    const SEGMENTS: usize = 48;
    const RINGS: usize = 8;
    let mut mesh = Mesh::default();
    let inside = |p: Pos2| clip.is_none_or(|poly| contains(poly, p));
    mesh.colored_vertex(centre, color);
    for ring in 1..=RINGS {
        let r = ring as f32 / RINGS as f32;
        // Smooth falloff, about a Gaussian blur's.
        let fall = (1.0 - r).powi(2);
        for s in 0..SEGMENTS {
            let a = s as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let p = centre + vec2(a.cos() * radius.x * r, a.sin() * radius.y * r);
            let c = if inside(p) {
                color.gamma_multiply(fall)
            } else {
                Color32::TRANSPARENT
            };
            mesh.colored_vertex(p, c);
        }
    }
    let seg = SEGMENTS as u32;
    for s in 0..seg {
        mesh.add_triangle(0, 1 + s, 1 + (s + 1) % seg);
    }
    for ring in 0..(RINGS as u32 - 1) {
        let a = 1 + ring * seg;
        let b = a + seg;
        for s in 0..seg {
            let n = (s + 1) % seg;
            mesh.add_triangle(a + s, b + s, b + n);
            mesh.add_triangle(a + s, b + n, a + n);
        }
    }
    painter.add(mesh);
}

fn contains(poly: &[Pos2], p: Pos2) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// A convex polygon filled top to bottom through `stops` (at 0..1 of its height).
pub fn gradient_polygon(painter: &Painter, points: &[Pos2], stops: &[(f32, Color32)]) {
    let (y0, y1) = points
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
    let color_at = |y: f32| {
        let t = ((y - y0) / (y1 - y0).max(1.0)).clamp(0.0, 1.0);
        let i = stops.iter().rposition(|(at, _)| *at <= t).unwrap_or(0);
        let (a, ca) = stops[i];
        let Some(&(b, cb)) = stops.get(i + 1) else { return ca };
        ca.lerp_to_gamma(cb, (t - a) / (b - a))
    };
    let mut mesh = Mesh::default();
    let mut cuts: Vec<f32> = stops.iter().map(|(at, _)| y0 + at * (y1 - y0)).collect();
    if cuts.len() < 2 {
        cuts = vec![y0, y1];
    }
    for pair in cuts.windows(2) {
        let clipped = clip_band(points, |p| p.y, pair[0], pair[1]);
        if clipped.len() < 3 {
            continue;
        }
        let base = mesh.vertices.len() as u32;
        for p in &clipped {
            mesh.colored_vertex(*p, color_at(p.y));
        }
        for i in 1..clipped.len() as u32 - 1 {
            mesh.add_triangle(base, base + i, base + i + 1);
        }
    }
    painter.add(mesh);
}

/// A convex polygon filled left to right through `stops` (at 0..1 of its
/// width): CSS's `linear-gradient(90deg, ...)` under a `clip-path`.
pub fn gradient_polygon_x(painter: &Painter, points: &[Pos2], stops: &[(f32, Color32)]) {
    let (x0, x1) = points
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
    let color_at = |x: f32| {
        let t = ((x - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0);
        let i = stops.iter().rposition(|(at, _)| *at <= t).unwrap_or(0);
        let (a, ca) = stops[i];
        let Some(&(b, cb)) = stops.get(i + 1) else { return ca };
        ca.lerp_to_gamma(cb, (t - a) / (b - a))
    };
    let mut mesh = Mesh::default();
    let mut cuts: Vec<f32> = stops.iter().map(|(at, _)| x0 + at * (x1 - x0)).collect();
    if cuts.first().is_none_or(|&c| c > x0) {
        cuts.insert(0, x0);
    }
    if cuts.last().is_none_or(|&c| c < x1) {
        cuts.push(x1);
    }
    for pair in cuts.windows(2) {
        let clipped = clip_band(points, |p| p.x, pair[0], pair[1]);
        if clipped.len() < 3 {
            continue;
        }
        let base = mesh.vertices.len() as u32;
        for p in &clipped {
            mesh.colored_vertex(*p, color_at(p.x));
        }
        for i in 1..clipped.len() as u32 - 1 {
            mesh.add_triangle(base, base + i, base + i + 1);
        }
    }
    painter.add(mesh);
}

/// A rectangle filled left to right through `stops` (at 0..1 of its width).
pub fn horizontal_gradient(painter: &Painter, rect: Rect, stops: &[(f32, Color32)]) {
    let mut mesh = Mesh::default();
    for (i, (at, color)) in stops.iter().enumerate() {
        let x = rect.min.x + at * rect.width();
        mesh.colored_vertex(pos2(x, rect.min.y), *color);
        mesh.colored_vertex(pos2(x, rect.max.y), *color);
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b + 1);
            mesh.add_triangle(b - 2, b + 1, b);
        }
    }
    painter.add(mesh);
}

/// The part of a convex polygon where `axis` lies between two values.
fn clip_band(points: &[Pos2], axis: impl Fn(Pos2) -> f32, low: f32, high: f32) -> Vec<Pos2> {
    let clip = |poly: &[Pos2], keep: &dyn Fn(Pos2) -> bool, edge: f32| {
        let mut out = Vec::new();
        for (i, &p) in poly.iter().enumerate() {
            let q = poly[(i + 1) % poly.len()];
            if keep(p) {
                out.push(p);
            }
            if keep(p) != keep(q) {
                let t = (edge - axis(p)) / (axis(q) - axis(p));
                out.push(p + (q - p) * t);
            }
        }
        out
    };
    let above = clip(points, &|p| axis(p) >= low, low);
    clip(&above, &|p| axis(p) <= high, high)
}

/// One run of text in a font, spaced as CSS `letter-spacing`.
#[must_use]
pub fn text(
    painter: &Painter,
    s: &str,
    size: f32,
    family: FontFamily,
    color: Color32,
    spacing: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        s,
        0.0,
        egui::TextFormat {
            font_id: FontId::new(size, family),
            color,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    painter.fonts_mut(|f| f.layout_job(job))
}

/// `s`, cut with an ellipsis to fit `width` (`text-overflow: ellipsis`).
#[must_use]
pub fn elide(painter: &Painter, s: &str, size: f32, family: FontFamily, width: f32) -> String {
    let fits = |t: &str| {
        painter
            .fonts_mut(|f| f.layout_no_wrap(t.to_owned(), FontId::new(size, family.clone()), Color32::WHITE))
            .size()
            .x
            <= width
    };
    if fits(s) {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&candidate) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_colours_convert_as_the_browser_does() {
        assert_eq!(hsl(0.0, 100.0, 50.0), Color32::from_rgb(255, 0, 0));
        assert_eq!(hsl(120.0, 100.0, 25.0), Color32::from_rgb(0, 128, 0));
        // `--gold`: hsl(46 100% 57%) is rgb(255, 204, 36).
        assert_eq!(gold(), Color32::from_rgb(255, 204, 36));
    }

    #[test]
    fn the_rail_keeps_its_last_forty_lines_without_repeats() {
        let mut rail = TwinRail::new("Map load", &["Server", "Handshake", "Map load"], "Match handoff");
        for i in 0..50 {
            rail.log(&format!("line {i}"));
            rail.log(&format!("line {i}"));
        }
        rail.log("");
        assert_eq!(rail.lines.len(), LOG_LINES);
        assert_eq!(rail.lines.first().map(String::as_str), Some("line 10"));
        assert_eq!(rail.lines.last().map(String::as_str), Some("line 49"));
    }

    #[test]
    fn facts_update_in_place_and_steps_default_to_pending() {
        let mut rail = TwinRail::new("Map load", &["Server", "Handshake", "Map load"], "Match handoff");
        rail.set_fact("map", "Map", "a");
        rail.set_fact("server", "Server", "ws://x");
        rail.set_fact("map", "Map", "b");
        assert_eq!(rail.facts.len(), 2);
        assert_eq!(rail.facts[0].2, "b");
        rail.set_steps(&[TwinStepState::Done]);
        assert_eq!(rail.steps[0].1, TwinStepState::Done);
        assert_eq!(rail.steps[2].1, TwinStepState::Pending);
    }

    #[test]
    fn the_rail_is_a_quarter_of_the_screen_within_its_limits() {
        assert!((TwinRail::width(1280.0) - 332.8).abs() < 1e-3);
        assert_eq!(TwinRail::width(800.0), 300.0);
        assert_eq!(TwinRail::width(3840.0), 520.0);
    }
}
