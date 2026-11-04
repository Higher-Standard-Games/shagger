//! SHAG `src/engine/ui/components/ConnectingOverlay.ts` and
//! `styles/ConnectingOverlay.css`: the match handoff, the lobby's twin with
//! the match's title on its stage and the connection's status along the
//! bottom, its bar sweeping while the connector reports nothing.
//!
//! The stage here is the sheet's, shared with the map loading screen
//! (`ui::transfer_loading_view`): the kicker, the title, its lead, and the
//! status with its bar. The browser builds it from HTML; here it is painted
//! with egui at the sheet's sizes.

use egui::{Color32, FontId, Mesh, Painter, Rect, Shape, Stroke, TextureId, pos2, vec2};

use super::lobby_twin::{
    RAIL_SKEW, TwinRail, TwinStepState, TwinStyle, alarm, clamp_css, cream, cyan, elide, gold, horizontal_gradient,
    hsl, hsla, ink, paint_backdrop, paint_light, text, violet,
};

/// What the bar shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BarFill {
    /// The handoff knows no percentage: a sweep says it is working.
    Sweep,
    /// The loading screen's bar fills as the loader moves through its
    /// phases, 0..1.
    Fill(f32),
}

/// The stage's state (`data-state`), which colours the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageState {
    Preparing,
    Loading,
    /// The map is in; the game is still setting itself up: the bar pulses.
    SettingUp,
    Ready,
    Failed,
}

/// The stage: the kicker, the title, its lead, and the status with its bar.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayStage {
    pub kicker: String,
    /// The title's lines; the last is gold.
    pub title: Vec<String>,
    pub lead: String,
    /// "TRANSFER STATUS" on the handoff, "LOAD STATUS" on the map.
    pub status_label: String,
    pub status: String,
    pub state: StageState,
    pub bar: BarFill,
    /// The line under the bar.
    pub footer: String,
    /// The notice under the footer (`connecting-overlay__error`).
    pub error: Option<String>,
    /// The map's levelshot behind the stage, when there is one.
    pub shot: Option<(TextureId, egui::Vec2)>,
    /// When the title arrived (`co-title-arrive`), in the painter's seconds.
    pub title_at: Option<f64>,
    /// When the screen came up (`co-enter`); `None` when it took over from
    /// another without a fade (`--continued`).
    pub entered_at: Option<f64>,
    /// The bar's width as drawn, easing to [`BarFill::Fill`]'s
    /// (`transition: width 0.35s ease-out`).
    shown: f32,
    last_paint: Option<f64>,
}

impl OverlayStage {
    #[must_use]
    pub fn new(kicker: &str, title: Vec<String>, lead: &str, status_label: &str) -> Self {
        Self {
            kicker: kicker.into(),
            title,
            lead: lead.into(),
            status_label: status_label.into(),
            status: "PREPARING".into(),
            state: StageState::Preparing,
            bar: BarFill::Sweep,
            footer: String::new(),
            error: None,
            shot: None,
            title_at: None,
            entered_at: None,
            shown: 0.0,
            last_paint: None,
        }
    }

    /// A new title, arriving again.
    pub fn set_title(&mut self, lines: Vec<String>) {
        if lines != self.title {
            self.title = lines;
            self.title_at = None;
        }
    }
}

/// The match handoff: the rail with its three steps and the server, and the
/// stage saying the game is starting (`ConnectingOverlay.show`).
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectingOverlay {
    pub rail: TwinRail,
    pub stage: OverlayStage,
}

impl ConnectingOverlay {
    #[must_use]
    pub fn new(server_url: &str, as_spectator: bool) -> Self {
        let label = if as_spectator {
            "Joining live match"
        } else {
            "Match handoff"
        };
        let mut rail = TwinRail::new(
            label,
            &["Server", "Handshake", "Map load"],
            if as_spectator { "Live channel" } else { "Match handoff" },
        );
        rail.set_fact("server", "Server", server_url);
        rail.set_steps(&[TwinStepState::Done, TwinStepState::Active, TwinStepState::Pending]);
        rail.log("Preparing");
        let (kicker, title, lead) = if as_spectator {
            (
                "LIVE MATCH AVAILABLE",
                ["SPECTATING", "MATCH"],
                "Opening a read-only seat on the live match server.",
            )
        } else {
            (
                "MATCH SERVER ONLINE",
                ["GAME", "STARTING"],
                "Your match seat is reserved. Establishing the game connection now.",
            )
        };
        let mut stage = OverlayStage::new(kicker, title.map(String::from).to_vec(), lead, "TRANSFER STATUS");
        stage.footer = "KEEP THIS WINDOW OPEN".into();
        Self { rail, stage }
    }

    /// `setStatus`: the status, logged on the rail.
    pub fn set_status(&mut self, status: &str) {
        status.clone_into(&mut self.stage.status);
        self.rail.log(&title_case(status));
    }

    /// `showError`: the connection failed.
    pub fn show_error(&mut self, error: &str) {
        self.stage.state = StageState::Failed;
        self.stage.error = Some(error.into());
        self.rail
            .set_steps(&[TwinStepState::Done, TwinStepState::Error, TwinStepState::Pending]);
        self.rail.log(error);
    }
}

/// "CONNECTION FAILED" as the rail logs it: "Connection failed".
fn title_case(status: &str) -> String {
    let lower = status.to_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// Paints a twin screen over `screen`: the backdrop and its light, the rail,
/// then the stage. `time` is in seconds, for the animations.
pub fn paint_twin(
    painter: &Painter,
    screen: Rect,
    style: &TwinStyle,
    rail: &TwinRail,
    stage: &mut OverlayStage,
    time: f64,
) {
    let dt = stage.last_paint.map_or(0.0, |t| (time - t).max(0.0)) as f32;
    stage.last_paint = Some(time);
    let entered = *stage.entered_at.get_or_insert(time);
    let title_at = *stage.title_at.get_or_insert(time);
    paint_backdrop(painter, screen);
    paint_light(painter, screen, time);
    let rail_width = TwinRail::width(screen.width());
    let stage_rect = Rect::from_min_max(pos2(screen.min.x + rail_width, screen.min.y), screen.max);
    if let Some((texture, size)) = stage.shot {
        paint_shot(painter, stage_rect, texture, size);
    }
    rail.paint(painter, screen, style);
    paint_stage(painter, screen, stage_rect, style, stage, time, title_at, dt);
    // `co-enter`: the whole screen fades in over 0.32 s.
    let enter = ((time - entered) / 0.32).clamp(0.0, 1.0) as f32;
    if enter < 1.0 {
        painter.rect_filled(screen, 0.0, hsl(222.0, 56.0, 17.0).gamma_multiply(1.0 - enter));
    }
}

/// `connecting-overlay__shot`: the levelshot covering the stage at 30%,
/// fading in from the rail's side and out at the far edge.
fn paint_shot(painter: &Painter, stage: Rect, texture: TextureId, size: egui::Vec2) {
    // `background-size: cover`, centred.
    let scale = (stage.width() / size.x).max(stage.height() / size.y);
    let shown = vec2(stage.width() / (size.x * scale), stage.height() / (size.y * scale));
    let uv0 = pos2((1.0 - shown.x) / 2.0, (1.0 - shown.y) / 2.0);
    // `mask-image: linear-gradient(100deg, transparent 6%, #000 46%, #000 80%, transparent)`.
    let stops = [(0.0, 0.0), (0.06, 0.0), (0.46, 1.0), (0.80, 1.0), (1.0, 0.0)];
    let mut mesh = Mesh::with_texture(texture);
    for (i, (at, mask)) in stops.iter().enumerate() {
        let x = stage.min.x + at * stage.width();
        let u = uv0.x + at * shown.x;
        let tint = Color32::WHITE.gamma_multiply(0.3 * mask);
        mesh.vertices.push(egui::epaint::Vertex {
            pos: pos2(x, stage.min.y),
            uv: pos2(u, uv0.y),
            color: tint,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: pos2(x, stage.max.y),
            uv: pos2(u, uv0.y + shown.y),
            color: tint,
        });
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b + 1);
            mesh.add_triangle(b - 2, b + 1, b);
        }
    }
    painter.add(mesh);
}

#[allow(clippy::too_many_arguments)]
fn paint_stage(
    painter: &Painter,
    screen: Rect,
    stage_rect: Rect,
    style: &TwinStyle,
    stage: &mut OverlayStage,
    time: f64,
    title_at: f64,
    dt: f32,
) {
    let (vw, vh) = (screen.width() / 100.0, screen.height() / 100.0);
    let content = Rect::from_min_max(
        pos2(
            stage_rect.min.x + clamp_css(40.0, 5.0 * vw, 96.0) + RAIL_SKEW,
            stage_rect.min.y + clamp_css(28.0, 5.0 * vh, 64.0),
        ),
        pos2(
            stage_rect.max.x - clamp_css(30.0, 5.0 * vw, 96.0),
            stage_rect.max.y - clamp_css(40.0, 8.0 * vh, 96.0),
        ),
    );
    let display = || style.display.clone();
    let body = || style.body.clone();

    // ── The status and its bar, along the bottom ──
    let transfer_width = content.width().min(820.0);
    let status_size = clamp_css(12.0, vw, 16.0);
    let mut status_text = stage.status.clone();
    if !matches!(stage.state, StageState::Ready | StageState::Failed) {
        // `startDots`: one more dot each half second, back to none after three.
        let dots = ((time / 0.5) as usize) % 4;
        status_text.push_str(&".".repeat(dots));
    }
    let status = text(painter, &status_text, status_size, display(), ink(), 0.15 * status_size);
    // The dots' room is kept so the plate does not jump (`width: 1.8em`).
    let dot_room = if matches!(stage.state, StageState::Ready | StageState::Failed) {
        0.0
    } else {
        let full = text(painter, "...", status_size, display(), ink(), 0.15 * status_size)
            .size()
            .x;
        let now = text(
            painter,
            &".".repeat(((time / 0.5) as usize) % 4),
            status_size,
            display(),
            ink(),
            0.15 * status_size,
        )
        .size()
        .x;
        (full - now).max(0.0)
    };
    let plate_size = vec2(status.size().x + dot_room + 32.0, status.size().y + 12.0);
    let bar_height = clamp_css(12.0, 1.7 * vh, 18.0);
    let footer = text(
        painter,
        &elide(
            painter,
            &stage.footer.to_uppercase(),
            11.0,
            body(),
            transfer_width / 1.3,
        ),
        11.0,
        body(),
        hsla(45.0, 100.0, 95.0, 0.55),
        2.86,
    );
    let error = stage.error.as_ref().map(|e| {
        let size = clamp_css(10.0, 0.8 * vw, 12.5);
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = transfer_width - 32.0 - 5.0;
        job.append(
            e,
            0.0,
            egui::TextFormat {
                font_id: FontId::new(size, display()),
                color: hsl(330.0, 100.0, 92.0),
                extra_letter_spacing: 0.06 * size,
                line_height: Some(size * 1.4),
                ..Default::default()
            },
        );
        painter.fonts_mut(|f| f.layout_job(job))
    });
    let error_height = error.as_ref().map_or(0.0, |g| 18.0 + g.size().y + 20.0);
    let transfer_height = plate_size.y + 14.0 + bar_height + 16.0 + footer.size().y + error_height;
    let transfer = Rect::from_min_size(
        pos2(content.min.x, content.max.y - transfer_height),
        vec2(transfer_width, transfer_height),
    );
    let mut y = transfer.min.y;
    let label = text(
        painter,
        &stage.status_label,
        11.0,
        body(),
        hsla(45.0, 100.0, 95.0, 0.55),
        2.86,
    );
    painter.galley(
        pos2(transfer.min.x, y + (plate_size.y - label.size().y) / 2.0),
        label,
        cream(),
    );
    let plate = Rect::from_min_size(pos2(transfer.max.x - plate_size.x, y), plate_size);
    painter.add(Shape::convex_polygon(
        vec![
            pos2(plate.min.x + plate.width() * 0.05, plate.min.y),
            plate.right_top(),
            pos2(plate.max.x - plate.width() * 0.06, plate.max.y),
            plate.left_bottom(),
        ],
        gold(),
        Stroke::NONE,
    ));
    painter.galley(
        pos2(plate.max.x - 16.0 - dot_room - status.size().x, plate.min.y + 6.0),
        status,
        ink(),
    );
    y += plate_size.y + 14.0;
    let bar = Rect::from_min_size(pos2(transfer.min.x, y), vec2(transfer_width, bar_height));
    paint_bar(painter, bar, stage, time, dt);
    y += bar_height + 16.0;
    let footer_height = footer.size().y;
    painter.galley(pos2(transfer.min.x, y), footer, cream());
    y += footer_height;
    if let Some(error) = error {
        let notice = Rect::from_min_size(
            pos2(transfer.min.x, y + 18.0),
            vec2(transfer_width, error.size().y + 20.0),
        );
        painter.rect_filled(notice, 0.0, hsla(330.0, 88.0, 58.0, 0.14));
        painter.rect_filled(
            Rect::from_min_size(notice.min, vec2(5.0, notice.height())),
            0.0,
            alarm(),
        );
        painter.galley(pos2(notice.min.x + 5.0 + 16.0, notice.min.y + 10.0), error, cream());
    }

    // ── The hero: the kicker, the title and its lead, in the middle ──
    let hero_area = Rect::from_min_max(content.min, pos2(content.max.x, transfer.min.y));
    let hero_width = hero_area.width().min(980.0);
    let kicker_size = clamp_css(9.9, 0.85 * vw, 13.8);
    let kicker = text(
        painter,
        &stage.kicker,
        kicker_size,
        display(),
        ink(),
        0.24 * kicker_size,
    );
    let kicker_plate = vec2(kicker.size().x + 12.0 + 28.0, kicker.size().y + 12.0);
    let chars = stage.title.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(6) as f32;
    let title_size = clamp_css(51.2, (9.1 * vw).min(62.0 * vw / chars), 152.0);
    // `co-title-arrive`: in from 8vw to the right over 0.55 s, after 0.08 s.
    let t = (((time - title_at - 0.08) / 0.55).clamp(0.0, 1.0)) as f32;
    let eased = 1.0 - (1.0 - t).powi(4);
    let title_lines: Vec<_> = stage
        .title
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let color = if i + 1 == stage.title.len() && stage.title.len() > 1 {
                gold()
            } else {
                hsl(45.0, 100.0, 95.0)
            };
            let shadow = hsla(250.0, 50.0, 4.0, 0.55).gamma_multiply(eased);
            (
                text(
                    painter,
                    line,
                    title_size,
                    display(),
                    color.gamma_multiply(eased),
                    -0.06 * title_size,
                ),
                text(painter, line, title_size, display(), shadow, -0.06 * title_size),
            )
        })
        .collect();
    let line_height = title_size * 0.8;
    let lead_size = clamp_css(16.0, 1.45 * vw, 22.4);
    let lead = {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = hero_width.min(590.0);
        job.append(
            &stage.lead,
            0.0,
            egui::TextFormat {
                font_id: FontId::new(lead_size, body()),
                color: hsla(45.0, 100.0, 95.0, 0.65),
                line_height: Some(lead_size * 1.35),
                ..Default::default()
            },
        );
        painter.fonts_mut(|f| f.layout_job(job))
    };
    let lead_gap = clamp_css(22.0, 4.0 * vh, 40.0);
    let hero_height = kicker_plate.y + 18.0 + line_height * title_lines.len() as f32 + lead_gap + lead.size().y;
    let mut y = hero_area.center().y - hero_height / 2.0;
    let x = hero_area.min.x;
    let k = Rect::from_min_size(pos2(x, y), kicker_plate);
    painter.add(Shape::convex_polygon(
        vec![
            k.left_top(),
            k.right_top(),
            pos2(k.min.x + k.width() * 0.92, k.max.y),
            pos2(k.min.x + k.width() * 0.03, k.max.y),
        ],
        gold(),
        Stroke::NONE,
    ));
    painter.galley(pos2(k.min.x + 12.0, k.min.y + 6.0), kicker, ink());
    y += kicker_plate.y + 18.0;
    let shift = 8.0 * vw * (1.0 - eased);
    for (galley, shadow) in title_lines {
        // `line-height: 0.8`: the glyphs sit in a box shorter than the font's.
        let top = y - (galley.size().y - line_height) / 2.0;
        painter.galley(
            pos2(x + shift + 0.05 * title_size, top + 0.06 * title_size),
            shadow,
            cream(),
        );
        painter.galley(pos2(x + shift, top), galley, cream());
        y += line_height;
    }
    y += lead_gap;
    painter.galley(pos2(x, y), lead, cream());
}

/// `connecting-overlay__bar` and its fill, by state.
fn paint_bar(painter: &Painter, bar: Rect, stage: &mut OverlayStage, time: f64, dt: f32) {
    let skew = bar.width() * 0.01;
    let track = vec![
        bar.left_top(),
        bar.right_top(),
        pos2(bar.max.x - skew, bar.max.y),
        pos2(bar.min.x + skew, bar.max.y),
    ];
    painter.add(Shape::convex_polygon(
        track.clone(),
        hsla(238.0, 38.0, 12.0, 0.92),
        Stroke::NONE,
    ));
    let inner = bar.shrink(1.0);
    let painter = painter.with_clip_rect(inner);
    match (stage.bar, stage.state) {
        (_, StageState::Failed) => {
            painter.rect_filled(inner, 0.0, alarm().gamma_multiply(0.72));
        }
        (BarFill::Sweep, _) => {
            // `co-bar-sweep`: a third of the bar crossing it every 1.45 s.
            let t = (time / 1.45).rem_euclid(1.0) as f32;
            let width = inner.width() * 0.34;
            let x = inner.min.x - inner.width() * 0.35 + t * (inner.width() * 1.35);
            horizontal_gradient(
                &painter,
                Rect::from_min_size(pos2(x, inner.min.y), vec2(width, inner.height())),
                &[
                    (0.0, Color32::TRANSPARENT),
                    (0.24, cyan()),
                    (0.70, violet()),
                    (1.0, Color32::TRANSPARENT),
                ],
            );
        }
        (BarFill::Fill(share), state) => {
            // `transition: width 0.35s ease-out`.
            let k = if dt == 0.0 { 1.0 } else { (dt / 0.35 * 3.0).min(1.0) };
            stage.shown += (share - stage.shown) * k;
            let width = inner.width() * stage.shown.clamp(0.0, 1.0);
            if width > 0.5 {
                let fill = Rect::from_min_size(inner.min, vec2(width, inner.height()));
                let stops: Vec<(f32, Color32)> = if state == StageState::Ready {
                    vec![(0.0, cyan()), (1.0, gold())]
                } else {
                    vec![(0.0, cyan()), (0.7, violet()), (1.0, gold())]
                };
                // `co-step-pulse` while the game sets itself up.
                let pulse = if state == StageState::SettingUp {
                    0.75 + 0.25 * (time * std::f64::consts::TAU / 0.9).cos() as f32
                } else {
                    1.0
                };
                let stops: Vec<_> = stops.into_iter().map(|(a, c)| (a, c.gamma_multiply(pulse))).collect();
                horizontal_gradient(&painter, fill, &stops);
            }
        }
    }
    painter.add(Shape::closed_line(
        track,
        Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.32)),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handoff_starts_with_the_server_reached_and_the_handshake_under_way() {
        let overlay = ConnectingOverlay::new("wss://eu.washoi.net/game/1", false);
        assert_eq!(overlay.stage.title, ["GAME", "STARTING"]);
        assert_eq!(overlay.stage.kicker, "MATCH SERVER ONLINE");
        assert_eq!(
            overlay.rail.steps.iter().map(|(_, s)| *s).collect::<Vec<_>>(),
            [TwinStepState::Done, TwinStepState::Active, TwinStepState::Pending]
        );
        assert_eq!(overlay.rail.facts[0].2, "wss://eu.washoi.net/game/1");
        let watching = ConnectingOverlay::new("ws://x", true);
        assert_eq!(watching.stage.title, ["SPECTATING", "MATCH"]);
        assert_eq!(watching.rail.context, "Live channel");
    }

    #[test]
    fn a_status_is_logged_in_sentence_case() {
        let mut overlay = ConnectingOverlay::new("ws://x", false);
        overlay.set_status("CONNECTION FAILED");
        assert_eq!(overlay.rail.lines.last().map(String::as_str), Some("Connection failed"));
    }
}
