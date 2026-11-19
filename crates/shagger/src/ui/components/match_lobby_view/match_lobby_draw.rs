//! `MatchLobbyView` drawn: the top bar, the status slab, the roster, the
//! votes and the actions (`MatchLobbyView.ts`'s render and
//! `styles/MatchLobbyView.css`). Split out of `match_lobby_view.rs`; the
//! same module tree.

use super::*;

impl MatchLobbyView {
    /// Draws the room in `rect`; what the panel must do, when the player asks.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        client: &LobbyClient,
        style: &RoomStyle<'_>,
    ) -> Option<RoomAction> {
        self.tick();
        let painter = ui.painter_at(rect.expand2(vec2(40.0, 24.0)));
        let time = ui.input(|i| i.time);
        let s = Sizes::new(style);
        let mut action = None;

        let top_h = self.top_bar(ui, &painter, rect, style);
        let status_top = rect.top() + top_h + s.gap;
        let status_h = self.status_slab(&painter, rect, status_top, style, time);
        let actions_h = s.ready_h + 10.2 + 4.0;
        let actions_top = rect.bottom() - actions_h;
        let body = Rect::from_min_max(
            pos2(rect.left(), status_top + status_h + s.gap),
            pos2(rect.right(), actions_top - s.gap),
        );
        self.body(ui, body, client, style, &s, time);
        if let Some(a) = self.actions(ui, &painter, rect, client, style, &s, time) {
            action = Some(a);
        }
        // Keep the radar turning.
        ui.ctx().request_repaint();
        action
    }

    /// `match-lobby__top`: the mode chip, the room's id, and the invite.
    pub(super) fn top_bar(&mut self, ui: &mut egui::Ui, painter: &Painter, rect: Rect, style: &RoomStyle<'_>) -> f32 {
        let a = style.accent;
        let chip_size = clamp_css(18.0, style.window_width * 0.016, 26.0);
        let label = text(
            painter,
            &self.gametype_label,
            chip_size,
            style.display.clone(),
            ink(1.0),
            chip_size * 0.06,
        );
        let chip_w = label.size().x + 48.0;
        let chip_h = label.size().y.max(chip_size) + 20.0;
        let h = chip_h.max(46.0);
        let chip = Rect::from_min_size(pos2(rect.left(), rect.top() + (h - chip_h) / 2.0), vec2(chip_w, chip_h));
        let slant = chip_w * 0.06;
        gradient_polygon(
            painter,
            &[
                chip.left_top() + vec2(slant, 0.0),
                chip.right_top(),
                chip.right_bottom() - vec2(slant, 0.0),
                chip.left_bottom(),
            ],
            &[(0.0, a.hi), (0.6, a.face), (1.0, a.lo)],
        );
        painter.galley(
            pos2(chip.left() + 22.0, chip.center().y - label.size().y / 2.0),
            label,
            ink(1.0),
        );
        // `match-lobby__invite`, on the right: label, the link, Copy.
        let href = invite_href(style.site, &self.share_url, &self.lobby_id);
        let shown = href
            .split_once("://")
            .map_or(href.as_str(), |(_, rest)| rest)
            .to_owned();
        let copy_label = if self.copied.is_some() { "Copied" } else { "Copy" };
        let copy_text = text(
            painter,
            &copy_label.to_uppercase(),
            15.0,
            style.display.clone(),
            ink(1.0),
            1.0,
        );
        let copy_w = copy_text.size().x + 40.0;
        let copy_h = 41.0;
        let copy = Rect::from_min_size(
            pos2(rect.right() - copy_w - 4.0, rect.top() + (h - copy_h) / 2.0 - 2.0),
            vec2(copy_w, copy_h),
        );
        let face = RaisedKey::new(("room-copy", &self.lobby_id), copy, KeyLook::cream().sm()).show(ui);
        KeyText::new(copy_label, "", style.display.clone(), style.body.clone())
            .size(15.0, 12.0)
            .spacing(0.07)
            .pad(20.0, 20.0)
            .layout(KeyTextLayout::Centered)
            .paint(painter, &face);
        if face.clicked {
            ui.ctx().copy_text(href.clone());
            self.copied = Some(Instant::now());
        }
        let meta_right = chip.right() + 18.0 + 200.0;
        let mut invite_left = copy.left();
        let url_max = (style.window_width * 0.32).clamp(80.0, 46.0 * 15.0 * 0.62);
        let url_room = (copy.left() - 12.0 - meta_right - 80.0).clamp(0.0, url_max);
        if url_room > 60.0 {
            let elided = elide(painter, &shown, 15.0, style.display.clone(), url_room - 36.0);
            let url = text(painter, &elided, 15.0, style.display.clone(), cream(), 15.0 * 0.04);
            let url_w = url.size().x + 36.0;
            let field = Rect::from_min_size(
                pos2(copy.left() - 12.0 - url_w, rect.top() + (h - copy_h) / 2.0),
                vec2(url_w, copy_h),
            );
            let slab = [
                field.left_top(),
                field.right_top(),
                field.right_bottom() - vec2(10.0, 0.0),
                field.left_bottom(),
            ];
            painter.add(Shape::convex_polygon(
                slab.to_vec(),
                hsla(250.0, 44.0, 5.0, 0.8),
                Stroke::NONE,
            ));
            painter.add(Shape::closed_line(
                slab.to_vec(),
                Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.3)),
            ));
            painter.galley(
                pos2(field.left() + 18.0, field.center().y - url.size().y / 2.0),
                url,
                cream(),
            );
            // At the size of the room's id, in the mode chip's type.
            let label = text(
                painter,
                "INVITE",
                chip_size,
                style.display.clone(),
                hsla(45.0, 100.0, 95.0, 0.5),
                chip_size * 0.06,
            );
            let lx = field.left() - 12.0 - label.size().x;
            invite_left = field.left();
            if lx > meta_right {
                invite_left = lx;
                painter.galley(pos2(lx, field.center().y - label.size().y / 2.0), label, cream());
            }
        }
        // `match-lobby__meta`: "Lobby" and the room's id, both in the mode
        // chip's type beside it.
        let faint = hsla(45.0, 100.0, 95.0, 0.5);
        let meta_x = chip.right() + 18.0;
        let lobby = text(
            painter,
            "LOBBY",
            chip_size,
            style.display.clone(),
            faint,
            chip_size * 0.06,
        );
        let id = text(
            painter,
            &self.lobby_id.to_uppercase(),
            chip_size,
            style.display.clone(),
            cream(),
            chip_size * 0.06,
        );
        let my = chip.center().y - lobby.size().y / 2.0;
        let lw = lobby.size().x;
        painter.galley(pos2(meta_x, my), lobby, faint);
        if meta_x + lw + chip_size * 0.5 + id.size().x < invite_left - 24.0 {
            painter.galley(pos2(meta_x + lw + chip_size * 0.5, my), id, cream());
        }
        h
    }

    /// `match-lobby__status`: the radar, what the room waits on, the count and the fill.
    pub(super) fn status_slab(&self, painter: &Painter, rect: Rect, top: f32, style: &RoomStyle<'_>, time: f64) -> f32 {
        let a = style.accent;
        let phase = self.status_phase();
        let (head, sub) = self.status_text();
        let error = self.room_error.is_some();
        let (pad_t, pad_b, pad_l, pad_r) = if style.short {
            (12.0, 18.0, 20.0, 30.0)
        } else {
            (20.0, 26.0, 24.0, 34.0)
        };
        let radar = if style.short { 78.0 } else { 104.0 };
        let gap = if style.short { 20.0 } else { 28.0 };
        let head_size = if style.short {
            clamp_css(22.0, style.window_width * 0.021, 38.0)
        } else {
            clamp_css(26.0, style.window_width * 0.026, 46.0)
        };
        let sub_size = if style.short { 17.0 } else { 19.0 };
        let count_size = if style.short { 48.0 } else { 60.0 };
        let of_size = if style.short { 24.0 } else { 28.0 };

        // The tally on the right decides how wide the words may run.
        let count = text(
            painter,
            &self.players.len().to_string(),
            count_size,
            style.display.clone(),
            cream(),
            0.0,
        );
        let of = text(
            painter,
            &format!("/{}", self.max_players),
            of_size,
            style.display.clone(),
            hsla(45.0, 100.0, 95.0, 0.42),
            0.0,
        );
        let clock = text(
            painter,
            &self.clock().to_uppercase(),
            13.0,
            style.body.clone(),
            hsla(45.0, 100.0, 95.0, 0.5),
            13.0 * 0.2,
        );
        // `match-lobby__ai-invite`, under the clock.
        let ai_invite = self.ai_invite_text().map(|t| {
            text(
                painter,
                &t.to_uppercase(),
                11.0,
                style.body.clone(),
                cream().gamma_multiply(0.75),
                11.0 * 0.16,
            )
        });
        let tally_w = (count.size().x + of.size().x)
            .max(clock.size().x)
            .max(ai_invite.as_ref().map_or(0.0, |g| g.size().x));
        let words_left = rect.left() + 10.0 + pad_l + radar + gap;
        let words_w = (rect.right() - pad_r - tally_w - gap - words_left).max(80.0);
        let head_color = if error {
            hsl(340.0, 100.0, 72.0)
        } else if phase == StatusPhase::Ready {
            a.hi
        } else {
            cream()
        };
        let sub_color = if error {
            hsl(340.0, 100.0, 72.0)
        } else {
            hsla(45.0, 100.0, 95.0, 0.78)
        };
        let head_shown = elide(painter, &head.to_uppercase(), head_size, style.display.clone(), words_w);
        let head = text(
            painter,
            &head_shown,
            head_size,
            style.display.clone(),
            head_color,
            head_size * 0.02,
        );
        let mut job =
            egui::text::LayoutJob::simple(sub, egui::FontId::new(sub_size, style.body.clone()), sub_color, words_w);
        job.wrap.max_rows = 2;
        let sub = painter.layout_job(job);
        let words_h = head.size().y + 10.0 + sub.size().y;
        let h = pad_t + words_h.max(radar) + pad_b;
        let slab = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), h));

        // The slab, cut off its top right, on its accent edge.
        let cut = 26.0;
        let shape = [
            slab.left_top(),
            pos2(slab.right() - cut, slab.top()),
            pos2(slab.right(), slab.top() + cut),
            slab.right_bottom(),
            slab.left_bottom(),
        ];
        let tinted = hsl(250.0, 40.0, 10.0).lerp_to_gamma(a.face, 0.16);
        gradient_polygon_x(painter, &shape, &[(0.0, tinted), (0.55, hsla(250.0, 44.0, 7.0, 0.92))]);
        painter.rect_filled(Rect::from_min_size(slab.min, vec2(10.0, h)), 0.0, a.face);
        // A light band sweeping across while the room waits.
        let period = match phase {
            StatusPhase::Searching => 3.2,
            StatusPhase::Ready => 1.6,
            StatusPhase::Starting => 0.9,
        };
        let t = ease_in_out(((time / period).fract()) as f32);
        let band_x = slab.left() + slab.width() * (t * 2.0 - 1.0);
        let band = Rect::from_min_size(pos2(band_x, slab.top()), slab.size());
        let clipped = painter.with_clip_rect(slab.intersect(painter.clip_rect()));
        horizontal_band(
            &clipped,
            band,
            &[
                (0.3, Color32::TRANSPARENT),
                (0.5, a.face.gamma_multiply(0.14)),
                (0.7, Color32::TRANSPARENT),
            ],
        );

        // The radar.
        let centre = pos2(slab.left() + 10.0 + pad_l + radar / 2.0, slab.top() + h / 2.0 - 3.0);
        self.paint_radar(painter, centre, radar, phase, &a, style, time);

        // The words.
        let wy = slab.top() + (h - pad_b + pad_t) / 2.0 - words_h / 2.0;
        let hy = wy;
        let hh = head.size().y;
        painter.galley_with_override_text_color(pos2(words_left + 4.0, hy + 5.0), head.clone(), hsl(250.0, 50.0, 3.0));
        painter.galley(pos2(words_left, hy), head, head_color);
        painter.galley(pos2(words_left, hy + hh + 10.0), sub, sub_color);

        // The tally.
        let right = slab.right() - pad_r;
        let invite_h = ai_invite.as_ref().map_or(0.0, |g| g.size().y + 6.0);
        let total_h = count.size().y * 0.9 + 8.0 + clock.size().y + invite_h;
        let ty = slab.top() + (h - pad_b + pad_t) / 2.0 - total_h / 2.0;
        let ow = of.size().x;
        let cw = count.size().x;
        let ch = count.size().y;
        painter.galley(pos2(right - ow, ty + ch - of.size().y - ch * 0.08), of, cream());
        painter.galley(pos2(right - ow - cw, ty), count, cream());
        let clock_h = clock.size().y;
        painter.galley(pos2(right - clock.size().x, ty + ch * 0.9 + 8.0), clock, cream());
        if let Some(g) = ai_invite {
            let y = ty + ch * 0.9 + 8.0 + clock_h + 6.0;
            painter.galley(pos2(right - g.size().x, y), g, cream());
        }

        // `match-lobby__status-bar`: how full the room is.
        let bar = Rect::from_min_max(pos2(slab.left(), slab.bottom() - 8.0), slab.right_bottom());
        painter.rect_filled(bar, 0.0, hsla(0.0, 0.0, 100.0, 0.06));
        let share = (self.players.len() as f32 / self.max_players.max(1) as f32).min(1.0);
        let fill = Rect::from_min_size(bar.min, vec2(bar.width() * share, 8.0));
        if fill.width() > 0.5 {
            painter.add(gradient_rect_x(fill, &[(0.0, a.lo), (1.0, a.hi)]));
            let waiting = !error
                && !self.match_in_progress
                && (self.phase == Phase::Starting || !self.enough_to_start() || self.local_ready);
            let crawl = if !waiting {
                0.0
            } else if phase == StatusPhase::Starting {
                (time / 0.4).fract() as f32
            } else {
                (time / 0.8).fract() as f32
            };
            let stripes = painter.with_clip_rect(fill.intersect(painter.clip_rect()));
            let dark = hsla(250.0, 60.0, 5.0, 0.28);
            let mut x = fill.left() - 28.0 + crawl * 28.0;
            while x < fill.right() + 8.0 {
                stripes.add(Shape::convex_polygon(
                    vec![
                        pos2(x, fill.bottom()),
                        pos2(x + 8.0, fill.top()),
                        pos2(x + 15.0, fill.top()),
                        pos2(x + 7.0, fill.bottom()),
                    ],
                    dark,
                    Stroke::NONE,
                ));
                x += 14.0;
            }
        }
        h
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_radar(
        &self,
        painter: &Painter,
        c: Pos2,
        size: f32,
        phase: StatusPhase,
        a: &Accent,
        style: &RoomStyle<'_>,
        time: f64,
    ) {
        let r = size / 2.0;
        let k = size / 104.0;
        painter.circle_filled(c, r, hsla(250.0, 44.0, 5.0, 0.4));
        painter.circle_filled(c, r * 0.6 / 0.62, hsla(250.0, 44.0, 5.0, 0.9));
        painter.circle_stroke(c, r - 1.0, Stroke::new(2.0, a.face.gamma_multiply(0.55)));
        let ring = Stroke::new(1.5, a.face.gamma_multiply(0.35));
        let core_inset = match phase {
            StatusPhase::Searching => 45.0,
            StatusPhase::Ready => 16.0,
            StatusPhase::Starting => 30.0,
        } * k;
        if phase == StatusPhase::Ready {
            // The core swells into a signal that pulses outward.
            for delay in [0.0, 0.7] {
                let t = (((time + delay) / 1.4).fract()) as f32;
                let radius = (r - 16.0 * k) * (1.0 + 0.55 * t);
                painter.circle_stroke(c, radius, Stroke::new(3.0, a.face.gamma_multiply(0.9 * (1.0 - t))));
            }
        } else {
            painter.circle_stroke(c, r - 18.0 * k, ring);
            painter.circle_stroke(c, r - 36.0 * k, ring);
            // The sweep: a conic tail behind a turning edge.
            let period = if phase == StatusPhase::Starting { 0.7 } else { 2.2 };
            let turn = (time / period).fract() as f32 * std::f32::consts::TAU;
            let mut mesh = Mesh::default();
            let steps = 28;
            for i in 0..steps {
                let t0 = i as f32 / steps as f32;
                let t1 = (i + 1) as f32 / steps as f32;
                // `conic-gradient(transparent 0 70%, accent 100%)`
                let at = |t: f32| turn - std::f32::consts::TAU * 0.3 * (1.0 - t);
                let color = |t: f32| a.face.gamma_multiply(0.85 * t);
                let base = mesh.vertices.len() as u32;
                // CSS turns clockwise from the top.
                let point = |ang: f32| c + vec2(ang.sin(), -ang.cos()) * (r - 1.0);
                mesh.colored_vertex(c, color(t0));
                mesh.colored_vertex(point(at(t0)), color(t0));
                mesh.colored_vertex(point(at(t1)), color(t1));
                mesh.add_triangle(base, base + 1, base + 2);
            }
            painter.add(mesh);
            if phase == StatusPhase::Searching {
                for (dx, dy, delay) in [(66.0, 24.0, 0.0), (22.0, 62.0, 1.3)] {
                    let t = ((time + delay) / 2.2).fract();
                    let alpha = if (0.30..0.34).contains(&t) {
                        ((t - 0.30) / 0.04) as f32
                    } else if (0.34..0.8).contains(&t) {
                        1.0 - ((t - 0.34) / 0.46) as f32
                    } else {
                        0.0
                    };
                    if alpha > 0.0 {
                        let p = c - vec2(r, r) + vec2(dx + 4.5, dy + 4.5) * k;
                        painter.circle_filled(p, 9.0 * k, a.face.gamma_multiply(0.3 * alpha));
                        painter.circle_filled(p, 4.5 * k, a.hi.gamma_multiply(alpha));
                    }
                }
            }
        }
        let core_r = r - core_inset;
        painter.circle_filled(c, core_r, a.face);
        let mark = if self.enough_to_start() {
            "!".to_owned()
        } else {
            self.min_players.saturating_sub(self.players.len()).max(1).to_string()
        };
        let g = text(painter, &mark, 34.0 * k, style.display.clone(), ink(1.0), 0.0);
        painter.galley(c - g.size() / 2.0, g, ink(1.0));
    }

    /// The roster and the map vote, which scroll between them when short of room.
    pub(super) fn body(
        &mut self,
        ui: &mut egui::Ui,
        body: Rect,
        client: &LobbyClient,
        style: &RoomStyle<'_>,
        s: &Sizes,
        time: f64,
    ) {
        if body.height() < 20.0 {
            return;
        }
        let roster_h = self.roster_height(body.width(), style, s);
        let vote_head = 20.0 + 12.0;
        // Six to choose from sit three across, two down.
        let six = self.maps.len() > 3;
        let maps_min = if style.narrower {
            if six { 390.0 } else { 260.0 }
        } else {
            if six {
                // Two rows share what one took, so the room still fits.
                if style.short { 196.0 } else { 236.0 }
            } else if style.short {
                124.0
            } else {
                150.0
            }
        };
        let fit = body.height() - roster_h - s.gap - vote_head;
        let maps_h = fit.max(maps_min);
        let content_h = roster_h + s.gap + vote_head + maps_h;
        // The keys lift up and left and cast their side down and right, so the
        // scroll box reaches past the room on those edges.
        let outer = Rect::from_min_max(body.min - vec2(8.0, 8.0), body.max + vec2(24.0, 14.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(outer), |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("room-body", &self.lobby_id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let (area, _) =
                        ui.allocate_exact_size(vec2(outer.width() - 16.0, content_h + 22.0), egui::Sense::hover());
                    let inner = Rect::from_min_size(area.min + vec2(8.0, 8.0), vec2(body.width(), content_h));
                    let painter = ui.painter().clone();
                    self.roster(&painter, inner, style, s, time);
                    let vote_top = inner.top() + roster_h + s.gap;
                    self.vote(ui, &painter, inner, vote_top, maps_h, client, style);
                });
        });
    }

    pub(super) fn roster_height(&self, width: f32, style: &RoomStyle<'_>, s: &Sizes) -> f32 {
        if self.max_players == 2 {
            return s.vs_seat_h;
        }
        if self.has_teams {
            let per_team = self.max_players.div_ceil(2) as f32;
            let head = if style.short { 17.0 } else { 22.0 };
            let gap = if style.short { 6.0 } else { 8.0 };
            let seat = self.team_seat_h(width, style);
            return head + gap + per_team * seat + (per_team - 1.0) * gap;
        }
        let cols = if style.narrower { 2 } else { 4 };
        let rows = self.max_players.div_ceil(cols) as f32;
        rows * s.ffa_seat_h + (rows - 1.0) * 14.0
    }

    pub(super) fn team_seat_h(&self, width: f32, style: &RoomStyle<'_>) -> f32 {
        let column = (width - 22.0 * 2.0 - 40.0) / 2.0;
        match (style.short, column < 380.0) {
            (true, _) => 36.0,
            (false, true) => 52.0,
            (false, false) => 46.0,
        }
    }

    pub(super) fn roster(&self, painter: &Painter, r: Rect, style: &RoomStyle<'_>, s: &Sizes, time: f64) {
        let (you, them) = (hsl(322.0, 95.0, 62.0), hsl(182.0, 95.0, 52.0));
        if self.max_players == 2 {
            // Two seats face each other across the VS.
            let vs = vs_galley(painter, clamp_css(40.0, style.window_width * 0.036, 64.0), style);
            let vs_w = vs.size().x + 12.0;
            let seat_w = (r.width() - vs_w - 44.0) / 2.0;
            let mine = self.players.iter().find(|p| p.is_local).or(self.players.first());
            let other = self.players.iter().find(|p| Some(*p) != mine);
            let left = Rect::from_min_size(r.min, vec2(seat_w, s.vs_seat_h));
            let right = Rect::from_min_size(pos2(r.right() - seat_w, r.top()), vec2(seat_w, s.vs_seat_h));
            self.seat(painter, left, mine, you, SeatKind::Vs, 0, style, time);
            paint_vs(painter, pos2(r.center().x, left.center().y), vs);
            self.seat(painter, right, other, them, SeatKind::Vs, 1, style, time);
            return;
        }
        if self.has_teams {
            let vs = vs_galley(painter, 34.0, style);
            let vs_w = vs.size().x + 12.0;
            let col_w = (r.width() - vs_w - 44.0) / 2.0;
            let local = self.local_team();
            let per_team = self.max_players.div_ceil(2);
            let seat_h = self.team_seat_h(r.width(), style);
            let mut seats_top = r.top();
            for (i, team) in ["alpha", "bravo"].into_iter().enumerate() {
                let x = if i == 0 { r.left() } else { r.right() - col_w };
                let mine = local == Some(team);
                let colour = if mine { you } else { them };
                seats_top = self.team_column(
                    painter,
                    Rect::from_min_size(pos2(x, r.top()), vec2(col_w, r.height())),
                    team,
                    mine,
                    colour,
                    per_team,
                    seat_h,
                    style,
                    time,
                );
            }
            let seats_bottom = r.top() + self.roster_height(r.width(), style, &Sizes::new(style));
            paint_vs(painter, pos2(r.center().x, (seats_top + seats_bottom) / 2.0), vs);
            return;
        }
        let cols = if style.narrower { 2 } else { 4 };
        let w = (r.width() - 22.0 * (cols as f32 - 1.0)) / cols as f32;
        for i in 0..self.max_players {
            let (row, col) = (i / cols, i % cols);
            let rect = Rect::from_min_size(
                pos2(
                    r.left() + col as f32 * (w + 22.0),
                    r.top() + row as f32 * (s.ffa_seat_h + 14.0),
                ),
                vec2(w, s.ffa_seat_h),
            );
            let player = self.players.get(i);
            let colour = if player.is_some_and(|p| p.is_local) { you } else { them };
            self.seat(painter, rect, player, colour, SeatKind::Grid, i, style, time);
        }
    }

    /// `buildTeam`; returns where its seats start.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn team_column(
        &self,
        painter: &Painter,
        r: Rect,
        team: &str,
        mine: bool,
        colour: Color32,
        seats: usize,
        seat_h: f32,
        style: &RoomStyle<'_>,
        time: f64,
    ) -> f32 {
        let head_size = if style.short { 15.0 } else { 18.0 };
        let gap = if style.short { 6.0 } else { 8.0 };
        let name = text(
            painter,
            &self.team_name(team).to_uppercase(),
            head_size,
            style.display.clone(),
            colour,
            head_size * 0.1,
        );
        let base = r.top() + name.size().y;
        let mut x = r.left() + 4.0;
        let nw = name.size().x;
        painter.galley(pos2(x, r.top()), name, colour);
        x += nw + 12.0;
        if mine {
            let tag = text(
                painter,
                "YOUR SIDE",
                11.0,
                style.body.clone(),
                hsla(45.0, 100.0, 95.0, 0.55),
                11.0 * 0.22,
            );
            painter.galley(pos2(x, base - tag.size().y), tag, cream());
        }
        let members: Vec<&MatchLobbyPlayer> = self.players.iter().filter(|p| p.team == team).collect();
        let tally = text(
            painter,
            &format!("{}/{seats}", members.len()),
            15.0,
            style.display.clone(),
            hsla(45.0, 100.0, 95.0, 0.7),
            0.0,
        );
        painter.galley(
            pos2(r.right() - 4.0 - tally.size().x, base - tally.size().y),
            tally,
            cream(),
        );
        let top = r.top() + head_size + 4.0 + gap;
        let kind = if r.width() < 380.0 {
            SeatKind::Grid
        } else {
            SeatKind::Row
        };
        for i in 0..seats {
            let rect = Rect::from_min_size(pos2(r.left(), top + i as f32 * (seat_h + gap)), vec2(r.width(), seat_h));
            self.seat(painter, rect, members.get(i).copied(), colour, kind, i, style, time);
        }
        top
    }

    /// `buildSeat`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn seat(
        &self,
        painter: &Painter,
        r: Rect,
        player: Option<&MatchLobbyPlayer>,
        colour: Color32,
        kind: SeatKind,
        index: usize,
        style: &RoomStyle<'_>,
        time: f64,
    ) {
        let vs = kind == SeatKind::Vs;
        let slant = if vs { 22.0 } else { 12.0 };
        let border = if vs { 8.0 } else { 5.0 };
        let shape = [
            r.left_top(),
            r.right_top(),
            pos2(r.right() - slant, r.bottom()),
            r.left_bottom(),
        ];
        let open = player.is_none();
        let colour = if open { hsla(45.0, 100.0, 95.0, 0.22) } else { colour };
        if open {
            painter.add(Shape::convex_polygon(
                shape.to_vec(),
                hsla(250.0, 44.0, 6.0, 0.55),
                Stroke::NONE,
            ));
            // Hatched.
            let hatch = painter.with_clip_rect(r.intersect(painter.clip_rect()));
            let mut x = r.left() - r.height();
            while x < r.right() {
                hatch.add(Shape::convex_polygon(
                    vec![
                        pos2(x, r.bottom()),
                        pos2(x + r.height(), r.top()),
                        pos2(x + r.height() + 7.0, r.top()),
                        pos2(x + 7.0, r.bottom()),
                    ],
                    hsla(0.0, 0.0, 100.0, 0.035),
                    Stroke::NONE,
                ));
                x += 14.0;
            }
            // Dashed edge.
            let mut y = r.top();
            while y < r.bottom() {
                let to = (y + 6.0).min(r.bottom());
                painter.rect_filled(
                    Rect::from_min_max(pos2(r.left(), y), pos2(r.left() + border, to)),
                    0.0,
                    colour,
                );
                y += 10.0;
            }
            // A shimmer running through it while the room waits.
            let t = ease_in_out((((time - index as f64 * 0.18) / 2.4).rem_euclid(1.0)) as f32);
            let band = Rect::from_min_size(pos2(r.left() + r.width() * (t * 2.0 - 1.0), r.top()), r.size());
            horizontal_band(
                &hatch,
                band,
                &[
                    (0.35, Color32::TRANSPARENT),
                    (0.5, style.accent.face.gamma_multiply(0.18)),
                    (0.65, Color32::TRANSPARENT),
                ],
            );
        } else {
            let tinted = hsl(250.0, 40.0, 9.0).lerp_to_gamma(colour, 0.24);
            gradient_polygon_x(painter, &shape, &[(0.0, tinted), (0.7, hsla(250.0, 44.0, 7.0, 0.92))]);
            painter.rect_filled(
                Rect::from_min_max(r.min, pos2(r.left() + border, r.bottom())),
                0.0,
                colour,
            );
        }

        let (avatar, avatar_font, name_size) = match kind {
            SeatKind::Vs if style.narrow => (54.0, 24.0, clamp_css(20.0, style.window_width * 0.017, 30.0)),
            SeatKind::Vs => (76.0, 34.0, clamp_css(24.0, style.window_width * 0.02, 36.0)),
            SeatKind::Grid if style.short => (30.0, 14.0, 20.0),
            SeatKind::Grid => (36.0, 14.0, 20.0),
            SeatKind::Row if style.short => (24.0, 12.0, 20.0),
            SeatKind::Row => (30.0, 14.0, 20.0),
        };
        let pad_l = border + if vs { 16.0 } else { 12.0 };
        let pad_r = slant + if vs { 10.0 } else { 8.0 };
        let gap = if vs { 20.0 } else { 12.0 };
        let av = Rect::from_center_size(
            pos2(r.left() + pad_l + avatar / 2.0, r.center().y),
            vec2(avatar, avatar),
        );
        let av_shape = vec![
            av.left_top() + vec2(avatar * 0.14, 0.0),
            av.right_top() + vec2(0.0, avatar * 0.08),
            av.right_bottom() - vec2(avatar * 0.12, 0.0),
            av.left_bottom() - vec2(0.0, avatar * 0.1),
        ];
        let x0 = av.right() + gap;
        let right = r.right() - pad_r;
        let Some(p) = player else {
            let pulse = 0.5 + 0.5 * (time * std::f64::consts::TAU / 1.6).cos() as f32;
            let scale = 0.84 + 0.16 * pulse;
            let small = Rect::from_center_size(av.center(), av.size() * scale);
            painter.rect_filled(small, 0.0, hsla(0.0, 0.0, 100.0, 0.07));
            let q = text(
                painter,
                "?",
                avatar_font * scale,
                style.display.clone(),
                hsla(45.0, 100.0, 95.0, 0.45 * (0.55 + 0.45 * pulse)),
                0.0,
            );
            painter.galley(av.center() - q.size() / 2.0, q, cream());
            let label = if self.phase == Phase::Starting {
                "EMPTY SEAT"
            } else if self.max_players == 2 {
                "WAITING FOR A PLAYER"
            } else {
                "OPEN"
            };
            let size = if vs { name_size * 0.82 } else { 16.0 };
            let mut job = egui::text::LayoutJob::default();
            job.append(
                label,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::new(size, style.body.clone()),
                    color: hsla(45.0, 100.0, 95.0, 0.4),
                    italics: true,
                    extra_letter_spacing: size * 0.1,
                    ..Default::default()
                },
            );
            let g = painter.layout_job(job);
            painter.galley(pos2(x0, r.center().y - g.size().y / 2.0), g, cream());
            return;
        };
        gradient_polygon(
            painter,
            &av_shape,
            &[(0.0, colour), (1.0, colour.lerp_to_gamma(hsl(276.0, 90.0, 64.0), 0.6))],
        );
        let initial: String = p
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let g = text(painter, &initial, avatar_font, style.display.clone(), ink(1.0), 0.0);
        painter.galley(av.center() - g.size() / 2.0, g, ink(1.0));

        // Ready or not, drawn rather than said: an empty slanted pip at the
        // right, filled green with a check once ready. While a match is
        // played the flags belong to the next one, so the pip goes.
        let in_match = self.match_in_progress;
        let ready = p.ready && !in_match;
        let go = hsl(96.0, 85.0, 55.0);
        let (pip_w, pip_h) = match kind {
            SeatKind::Vs if !style.narrow && !style.short => (42.0, 30.0),
            _ => (34.0, 24.0),
        };
        let pip_room = if in_match { 0.0 } else { pip_w + 12.0 };
        let tagged = p.is_local || p.bot_level.is_some();
        let shown = elide(
            painter,
            &p.name,
            name_size,
            style.display.clone(),
            (right - x0 - pip_room).max(20.0),
        );
        let name = text(
            painter,
            &shown,
            name_size,
            style.display.clone(),
            cream(),
            name_size * 0.04,
        );
        let nh = name.size().y;
        painter.galley(pos2(x0, r.center().y - nh / 2.0), name, cream());
        if !in_match {
            // Under the corner tab when the seat has one.
            let drop = if tagged { (r.height() * 0.12).min(8.0) } else { 0.0 };
            let pr = Rect::from_min_size(
                pos2(right - pip_w, r.center().y + drop - pip_h / 2.0),
                vec2(pip_w, pip_h),
            );
            let cut = pip_w * 0.12;
            let pts = vec![
                pr.left_top() + vec2(cut, 0.0),
                pr.right_top(),
                pr.right_bottom() - vec2(cut, 0.0),
                pr.left_bottom(),
            ];
            if ready {
                for grow in [6.0, 3.0] {
                    let g = pr.expand(grow);
                    let c = g.width() * 0.12;
                    painter.add(Shape::convex_polygon(
                        vec![
                            g.left_top() + vec2(c, 0.0),
                            g.right_top(),
                            g.right_bottom() - vec2(c, 0.0),
                            g.left_bottom(),
                        ],
                        go.gamma_multiply(0.18),
                        Stroke::NONE,
                    ));
                }
                painter.add(Shape::convex_polygon(pts, go, Stroke::NONE));
                // `✓`, drawn: the face's font may not have it.
                let c = pr.center();
                let k = pip_h * 0.55;
                let stroke = Stroke::new(pip_h * 0.13, ink(1.0));
                painter.line_segment([pos2(c.x - k * 0.45, c.y), pos2(c.x - k * 0.1, c.y + k * 0.32)], stroke);
                painter.line_segment(
                    [
                        pos2(c.x - k * 0.1, c.y + k * 0.32),
                        pos2(c.x + k * 0.45, c.y - k * 0.38),
                    ],
                    stroke,
                );
            } else {
                painter.add(Shape::closed_line(pts, Stroke::new(2.0, hsla(45.0, 100.0, 95.0, 0.28))));
            }
        }

        // Who the seat is (You, a bot's level): a tab flush in the card's
        // top right corner, cut on its inner edge.
        let mut tags = Vec::new();
        if p.is_local {
            tags.push(("YOU".to_owned(), hsl(46.0, 100.0, 60.0)));
        }
        if let Some(level) = &p.bot_level {
            tags.push((
                format!("BOT · {}", capitalised(level).to_uppercase()),
                hsl(186.0, 90.0, 46.0),
            ));
        }
        let mut tx = r.right();
        for (t, c) in tags.iter().rev() {
            let g = text(painter, t, 11.0, style.display.clone(), ink(1.0), 11.0 * 0.12);
            let tw = g.size().x + 14.0 + 18.0;
            let th = g.size().y + 11.0;
            let tr = Rect::from_min_size(pos2(tx - tw, r.top()), vec2(tw, th));
            painter.add(Shape::convex_polygon(
                vec![
                    tr.left_top() + vec2(10.0, 0.0),
                    tr.right_top(),
                    tr.right_bottom(),
                    tr.left_bottom(),
                ],
                *c,
                Stroke::NONE,
            ));
            painter.galley(pos2(tr.left() + 18.0, tr.top() + 5.0), g, ink(1.0));
            tx -= tw - 4.0;
        }
    }

    /// `match-lobby__vote`: the head, and a card per map.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn vote(
        &mut self,
        ui: &mut egui::Ui,
        painter: &Painter,
        r: Rect,
        top: f32,
        maps_h: f32,
        client: &LobbyClient,
        style: &RoomStyle<'_>,
    ) {
        let title = text(painter, "VOTE MAP", 20.0, style.display.clone(), cream(), 20.0 * 0.14);
        painter.galley(pos2(r.left(), top), title, cream());
        let maps_top = top + 20.0 + 12.0;
        let slots = self.maps.len().max(3);
        let (cols, rows) = if style.narrower {
            (2, slots.div_ceil(2))
        } else if slots > 3 {
            (3, slots.div_ceil(3))
        } else {
            (slots, 1)
        };
        let gap = 26.0;
        let row_gap = if slots > 3 { 18.0 } else { gap };
        let w = (r.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let h = (maps_h - (rows as f32 - 1.0) * row_gap) / rows as f32;
        let most = self.maps.iter().map(|m| m.votes).max().unwrap_or(0);
        let leaders = self.maps.iter().filter(|m| m.votes == most).count();
        let starting = self.phase == Phase::Starting;
        let mut vote = None;
        for i in 0..slots {
            let (row, col) = (i / cols, i % cols);
            let depth = 7.8;
            let card = Rect::from_min_size(
                pos2(r.left() + col as f32 * (w + gap), maps_top + row as f32 * (h + row_gap)),
                vec2(w - depth * 0.75, h - depth - 8.0),
            );
            let Some(map) = self.maps.get(i) else {
                map_slot_empty(painter, card, style);
                continue;
            };
            let leading = map.votes > 0 && map.votes == most && leaders == 1;
            if map_card(ui, painter, card, map, leading, starting, style) {
                vote = Some(map.name.clone());
            }
        }
        if let Some(name) = vote {
            self.select_map_vote(&name, client);
        }
    }

    /// `match-lobby__bot-menu`: the skill menu, opening upwards from
    /// `bottom_left` over the room; the preset picked, if one was. A press
    /// outside it and the arrow (`caret`), or Esc, closes it.
    pub(super) fn bot_level_menu(
        &mut self,
        ui: &mut egui::Ui,
        bottom_left: Pos2,
        caret: Rect,
        style: &RoomStyle<'_>,
    ) -> Option<&'static str> {
        let (pad, item_pad, size) = (8.0, vec2(16.0, 10.0), 16.0);
        let painter = ui.painter().clone();
        let names: Vec<_> = BOT_LEVELS
            .iter()
            .map(|(_, name)| {
                text(
                    &painter,
                    &name.to_uppercase(),
                    size,
                    style.display.clone(),
                    ink(1.0),
                    size * 0.1,
                )
            })
            .collect();
        let widest = names.iter().map(|g| g.size().x).fold(0.0, f32::max);
        let item_h = size + item_pad.y * 2.0;
        // As wide as the key and its arrow, and never narrower than its words.
        let width = (widest + item_pad.x * 2.0 + pad * 2.0)
            .max(180.0)
            .max(caret.right() - bottom_left.x);
        let height = item_h * BOT_LEVELS.len() as f32 + pad * 2.0;
        let menu = Rect::from_min_size(bottom_left - vec2(0.0, height), vec2(width, height));
        let id = egui::Id::NULL.with(("match-lobby", "bot-menu"));
        let mut picked = None;
        let area = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(menu.min)
            .show(ui.ctx(), |ui| {
                let painter = ui.painter().clone();
                let slant = |r: Rect| {
                    vec![
                        r.left_top() + vec2(10.0, 0.0),
                        r.right_top(),
                        r.right_bottom() - vec2(10.0, 0.0),
                        r.left_bottom(),
                    ]
                };
                painter.add(Shape::convex_polygon(
                    slant(menu.translate(vec2(6.0, 6.0))),
                    ink(0.6),
                    Stroke::NONE,
                ));
                painter.add(Shape::convex_polygon(slant(menu), cream(), Stroke::NONE));
                for (i, ((level, _), name)) in BOT_LEVELS.iter().zip(names).enumerate() {
                    let r = Rect::from_min_size(
                        pos2(menu.left() + pad, menu.top() + pad + i as f32 * item_h),
                        vec2(width - pad * 2.0, item_h),
                    );
                    let response = ui.interact(r, id.with(i), egui::Sense::click());
                    let color = if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        painter.rect_filled(r, 0.0, ink(1.0));
                        cream()
                    } else {
                        ink(1.0)
                    };
                    painter.galley(
                        pos2(r.left() + item_pad.x, r.center().y - name.size().y / 2.0),
                        name,
                        color,
                    );
                    if response.clicked() {
                        picked = Some(*level);
                    }
                }
                ui.allocate_rect(menu, egui::Sense::hover());
            });
        let outside = ui
            .input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !caret.contains(p)))
            && !area.response.contains_pointer();
        if picked.is_none() && (outside || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
            self.bot_menu = false;
        }
        picked
    }

    /// `match-lobby__actions`: Leave, Add AI player, Switch and Shuffle,
    /// Rejoin, the ready tally and Ready.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn actions(
        &mut self,
        ui: &mut egui::Ui,
        painter: &Painter,
        rect: Rect,
        client: &LobbyClient,
        style: &RoomStyle<'_>,
        s: &Sizes,
        time: f64,
    ) -> Option<RoomAction> {
        let mut action = None;
        let gap = clamp_css(12.0, style.window_width * 0.011, 20.0);
        let label_size = clamp_css(14.0, style.window_width * 0.01, 18.0);
        let pad = (
            clamp_css(20.0, style.window_width * 0.018, 34.0),
            clamp_css(18.0, style.window_width * 0.017, 32.0),
        );
        let act_depth = 7.2;
        let bottom = rect.bottom() - 4.0;
        let act_top = bottom - act_depth - s.act_h;
        let mut x = rect.left();
        let starting = self.phase == Phase::Starting;
        let act = |ui: &mut egui::Ui,
                   x: &mut f32,
                   id: &str,
                   label: &str,
                   look: KeyLook,
                   hover: Option<(KeyLook, f32)>,
                   disabled: bool,
                   lead: f32,
                   cut_px: Option<f32>| {
            let lg = text(
                painter,
                &label.to_uppercase(),
                label_size,
                style.display.clone(),
                Color32::WHITE,
                label_size * 0.07,
            );
            let w = lg.size().x + lead + pad.0 + pad.1 + 8.0;
            let r = Rect::from_min_size(pos2(*x, act_top), vec2(w, s.act_h));
            // `--key-cut` in pixels rather than of the width.
            let cut = |look: KeyLook| match cut_px {
                Some(c) => KeyLook { cut: c / w, ..look },
                None => look,
            };
            let mut key = RaisedKey::new(("room-act", id), r, cut(look)).disabled(disabled);
            if let Some((h, p)) = hover {
                key = key.hover(cut(h), p);
            }
            let face = key.show(ui);
            // The label stands `lead` in, past a drawn mark.
            KeyText::new(label, "", style.display.clone(), style.body.clone())
                .size(label_size, 12.0)
                .pad(pad.0 + lead, pad.1)
                .layout(KeyTextLayout::Stacked)
                .paint(painter, &face);
            *x += w + act_depth * 0.75 + 2.0 + gap;
            face
        };

        // Leave, in the danger red.
        let leave_look = KeyLook {
            text: hsl(356.0, 88.0, 56.0),
            sub: hsla(356.0, 70.0, 38.0, 0.7),
            ..KeyLook::cream().depth(act_depth)
        };
        let leave_hover = KeyLook {
            face: vec![
                (0.0, hsl(356.0, 95.0, 68.0)),
                (0.55, hsl(356.0, 85.0, 54.0)),
                (1.0, hsl(352.0, 80.0, 44.0)),
            ],
            side: [hsl(350.0, 75.0, 30.0), hsl(345.0, 72.0, 18.0)],
            shadow: hsla(350.0, 80.0, 14.0, 0.5),
            text: hsl(45.0, 100.0, 96.0),
            sub: hsla(45.0, 100.0, 96.0, 0.72),
            ..leave_look.clone()
        };
        let arrow = label_size * 0.9;
        let leave = act(
            ui,
            &mut x,
            "leave",
            "Leave",
            leave_look,
            Some((leave_hover, -0.35)),
            false,
            arrow,
            None,
        );
        // `←`, drawn: the face's font may not have the arrow.
        {
            let y = leave.rect.center().y;
            let ax = leave.rect.left() + pad.0;
            let stroke = Stroke::new(2.4, leave.text);
            let k = label_size * 0.42;
            painter.line_segment([pos2(ax, y), pos2(ax + k * 1.4, y)], stroke);
            painter.line_segment([pos2(ax, y), pos2(ax + k * 0.6, y - k * 0.55)], stroke);
            painter.line_segment([pos2(ax, y), pos2(ax + k * 0.6, y + k * 0.55)], stroke);
        }
        if leave.clicked {
            client.leave_lobby(&self.lobby_id);
            action = Some(RoomAction::Leave);
        }

        let full = self.players.len() >= self.max_players;
        let bot_disabled = starting || self.invite_bot_pending.is_some() || full || self.bot_note.is_some();
        let cream_act = KeyLook::cream().depth(act_depth);
        // Add AI player and the skill arrow lean by the same fixed amount, as one
        // split key (the usual 3% would leave the narrow arrow upright).
        let key_cut = 5.0;
        let bot_left = x;
        let bot = act(
            ui,
            &mut x,
            "bot",
            self.bot_label(),
            cream_act.clone(),
            None,
            bot_disabled,
            0.0,
            Some(key_cut),
        );
        if bot.clicked {
            self.request_bot(client, None);
        }
        // The arrow starts where Add AI player's slanted edge ends, over its side.
        let caret_left = x - (act_depth * 0.75 + 2.0 + gap) - key_cut + 2.0;
        let caret_pad = clamp_css(12.0, style.window_width * 0.01, 18.0);
        let caret_w = caret_pad * 2.0 + 16.0;
        let caret_rect = Rect::from_min_size(pos2(caret_left, act_top), vec2(caret_w, s.act_h));
        let caret_look = KeyLook {
            cut: key_cut / caret_w,
            ..cream_act.clone()
        };
        let caret = RaisedKey::new(("room-act", "bot-level"), caret_rect, caret_look)
            .disabled(bot_disabled)
            .show(ui);
        {
            // `▴`, drawn: the face's font may not have it. It points up,
            // where its menu opens.
            let c = caret.rect.center();
            let (hw, hh) = (7.0, 5.0);
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x - hw, c.y + hh * 0.6),
                    pos2(c.x, c.y - hh * 1.4),
                    pos2(c.x + hw, c.y + hh * 0.6),
                ],
                caret.text,
                Stroke::NONE,
            ));
        }
        x = caret_left + caret_w + act_depth * 0.75 + 2.0 + gap;
        if bot_disabled {
            self.bot_menu = false;
        } else if caret.clicked {
            self.bot_menu = !self.bot_menu;
        } else if self.bot_menu
            && let Some(level) = self.bot_level_menu(ui, pos2(bot_left, act_top - 10.0), caret.rect, style)
        {
            self.bot_menu = false;
            self.request_bot(client, Some(level));
        }
        let flash_at = pos2(x - gap + 14.0, bot.rect.center().y);

        // Two seats facing each other have nothing to switch or shuffle.
        if self.has_teams && self.max_players > 2 {
            let to = if self.local_team() == Some("alpha") {
                "bravo"
            } else {
                "alpha"
            };
            let label = format!("⇄ To {}", self.team_name(to));
            let switch = act(
                ui,
                &mut x,
                "switch",
                &label,
                cream_act.clone(),
                None,
                starting,
                0.0,
                None,
            );
            if switch.clicked {
                client.switch_team(&self.lobby_id);
            }
            let label = if self.shuffle_sent { "Vote sent" } else { "⤨ Shuffle" };
            let shuffle = act(
                ui,
                &mut x,
                "shuffle",
                label,
                cream_act.clone(),
                None,
                starting || self.shuffle_sent,
                0.0,
                None,
            );
            if shuffle.clicked {
                client.vote_shuffle(&self.lobby_id);
                self.shuffle_sent = true;
            }
        }

        // Ready, at the right.
        let ready_label = self.ready_label();
        let ready_size = if style.short {
            clamp_css(22.0, style.window_width * 0.017, 30.0)
        } else {
            clamp_css(26.0, style.window_width * 0.019, 36.0)
        };
        let ready_depth = 10.2;
        let ready_w = clamp_css(200.0, style.window_width * 0.17, 330.0);
        let ready_rect = Rect::from_min_size(
            pos2(
                rect.right() - ready_w - ready_depth * 0.75 - 4.0,
                bottom - ready_depth - s.ready_h,
            ),
            vec2(ready_w, s.ready_h),
        );
        let a = style.accent;
        let calling = self.enough_to_start() && !self.local_ready && !starting;
        if calling {
            // `match-lobby__ready--calling`: a glow that breathes behind it.
            let breath = 0.5 - 0.5 * (time * std::f64::consts::TAU / 1.1).cos() as f32;
            for grow in [14.0, 9.0, 4.0] {
                let g = ready_rect.expand2(vec2(grow, grow * 0.7));
                let cut = g.width() * 0.05;
                painter.add(Shape::convex_polygon(
                    vec![
                        g.left_top() + vec2(cut, 0.0),
                        g.right_top(),
                        g.right_bottom() - vec2(cut, 0.0),
                        g.left_bottom(),
                    ],
                    a.face.gamma_multiply(0.25 * breath),
                    Stroke::NONE,
                ));
            }
        }
        let ready_look = KeyLook {
            face: vec![(0.0, a.hi), (0.58, a.face), (1.0, a.lo)],
            side: [a.side_hi, a.side_lo],
            shadow: hsla(250.0, 70.0, 3.0, 0.62),
            sub: hsla(250.0, 45.0, 8.0, 0.62),
            depth: ready_depth,
            cut: 0.05,
            ..KeyLook::gold()
        };
        let held_look = KeyLook {
            face: vec![
                (0.0, hsl(250.0, 28.0, 21.0)),
                (0.6, hsl(250.0, 40.0, 10.0)),
                (1.0, hsl(252.0, 44.0, 6.0)),
            ],
            side: [hsl(252.0, 44.0, 5.0), hsl(252.0, 44.0, 3.0)],
            text: a.hi,
            sub: a.hi.gamma_multiply(0.66),
            ..ready_look.clone()
        };
        let mut ready_key = if self.local_ready {
            RaisedKey::new(("room-ready", &self.lobby_id), ready_rect, held_look.clone())
                .rest_press(0.62)
                .hover(held_look, 0.62)
        } else {
            RaisedKey::new(("room-ready", &self.lobby_id), ready_rect, ready_look)
        };
        ready_key = ready_key.disabled(starting);
        let face = ready_key.show(ui);
        let label = text(
            painter,
            &ready_label.to_uppercase(),
            ready_size,
            style.display.clone(),
            face.text,
            ready_size * 0.12,
        );
        painter.galley(face.rect.center() - label.size() / 2.0, label, face.text);
        if face.clicked {
            let ready = !self.local_ready;
            self.set_ready(ready, client);
        }

        // `match-lobby__ready-tally`: how many are ready, beside the key that
        // adds you to them, over a pip per player lit when they are.
        let mut left_of_ready = ready_rect.left() - gap;
        if !self.match_in_progress && !self.players.is_empty() {
            let ready = self.players.iter().filter(|p| p.ready).count();
            let count_size = clamp_css(40.0, style.window_width * 0.032, 60.0);
            let count = text(
                painter,
                &ready.to_string(),
                count_size,
                style.display.clone(),
                a.hi,
                0.0,
            );
            let of = text(
                painter,
                &format!("/{}", self.players.len()),
                count_size * 0.5,
                style.display.clone(),
                a.hi,
                0.0,
            );
            let (pip_w, pip_h, pip_gap) = (20.0, 8.0, 5.0);
            let n = self.players.len() as f32;
            let pips_w = n * pip_w + (n - 1.0) * pip_gap;
            let count_w = count.size().x + of.size().x;
            let tally_w = count_w.max(pips_w);
            let margin = clamp_css(16.0, style.window_width * 0.016, 30.0);
            let right = ready_rect.left() - margin;
            let ch = count.size().y * 0.9;
            let total = ch + 10.0 + pip_h;
            let top = ready_rect.center().y - 5.0 - total / 2.0;
            let count_at = pos2(right - count_w, top);
            painter.galley_with_override_text_color(count_at + vec2(3.0, 4.0), count.clone(), hsl(250.0, 50.0, 3.0));
            let count_h = count.size().y;
            painter.galley(count_at, count, a.hi);
            painter.galley(pos2(right - of.size().x, top + count_h * 0.82 - of.size().y), of, a.hi);
            let py = top + ch + 10.0;
            for (i, player) in self.players.iter().enumerate() {
                let px = right - pips_w + i as f32 * (pip_w + pip_gap);
                let r = Rect::from_min_size(pos2(px, py), vec2(pip_w, pip_h));
                // `skewX(-20deg)`
                let skew = pip_h * 0.364;
                let pts = vec![
                    r.left_top() + vec2(skew / 2.0, 0.0),
                    r.right_top() + vec2(skew / 2.0, 0.0),
                    r.right_bottom() - vec2(skew / 2.0, 0.0),
                    r.left_bottom() - vec2(skew / 2.0, 0.0),
                ];
                if player.ready {
                    let glow: Vec<Pos2> = pts.clone();
                    painter.add(Shape::convex_polygon(
                        glow.iter().map(|p| *p + (*p - r.center()) * 0.35).collect(),
                        a.face.gamma_multiply(0.3),
                        Stroke::NONE,
                    ));
                    painter.add(Shape::convex_polygon(pts, a.face, Stroke::NONE));
                } else {
                    painter.add(Shape::convex_polygon(pts, hsla(45.0, 100.0, 95.0, 0.16), Stroke::NONE));
                }
            }
            left_of_ready = right - tally_w - gap;
        }

        // Rejoin, left of Ready, while this room's match runs.
        if let Some(match_id) = self.active_match_id.clone() {
            let label = self.recover_note.as_deref().unwrap_or("Rejoin match").to_owned();
            let lg = text(
                painter,
                &label.to_uppercase(),
                label_size,
                style.display.clone(),
                Color32::WHITE,
                label_size * 0.07,
            );
            let w = lg.size().x + pad.0 + pad.1 + 8.0;
            let rx = left_of_ready - act_depth * 0.75 - 2.0 - w;
            if rx > x {
                let mut rx_cursor = rx;
                let rejoin = act(
                    ui,
                    &mut rx_cursor,
                    "rejoin",
                    &label,
                    cream_act,
                    None,
                    !style.online,
                    0.0,
                    None,
                );
                if rejoin.clicked {
                    self.recover_note = Some("Checking seat…".into());
                    action = Some(RoomAction::Rejoin(match_id));
                }
            }
        }

        // A note beside Add AI player that fades by itself.
        if let Some((note, at)) = &self.flash {
            let t = at.elapsed().as_secs_f32() / FLASH.as_secs_f32();
            let alpha = if t < 0.1 {
                t / 0.1
            } else if t < 0.7 {
                1.0
            } else {
                (1.0 - t) / 0.3
            };
            let g = text(
                painter,
                &note.to_uppercase(),
                15.0,
                style.display.clone(),
                ink(alpha),
                1.5,
            );
            let slide = if t < 0.1 { -8.0 * (1.0 - t / 0.1) } else { 0.0 };
            let r = Rect::from_min_size(
                pos2(flash_at.x + slide, flash_at.y - (g.size().y + 24.0) / 2.0),
                vec2(g.size().x + 44.0, g.size().y + 24.0),
            );
            let shape = vec![
                r.left_top() + vec2(10.0, 0.0),
                r.right_top(),
                r.right_bottom() - vec2(10.0, 0.0),
                r.left_bottom(),
            ];
            let shadow: Vec<Pos2> = shape.iter().map(|p| *p + vec2(6.0, 6.0)).collect();
            painter.add(Shape::convex_polygon(
                shadow,
                hsla(250.0, 45.0, 4.0, 0.6 * alpha),
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                shape,
                cream().gamma_multiply(alpha),
                Stroke::NONE,
            ));
            painter.galley(pos2(r.left() + 22.0, r.top() + 12.0), g, ink(alpha));
        }
        action
    }
}

/// The CSS's sizes at this window size.
pub(super) struct Sizes {
    pub(super) gap: f32,
    pub(super) act_h: f32,
    pub(super) ready_h: f32,
    pub(super) ffa_seat_h: f32,
    pub(super) vs_seat_h: f32,
}

impl Sizes {
    fn new(style: &RoomStyle<'_>) -> Self {
        Self {
            gap: if style.short { 14.0 } else { 20.0 },
            act_h: if style.short { 56.0 } else { 66.0 },
            ready_h: if style.short { 76.0 } else { 100.0 },
            ffa_seat_h: if style.short { 50.0 } else { 62.0 },
            vs_seat_h: if style.short {
                80.0
            } else if style.narrow {
                96.0
            } else {
                116.0
            },
        }
    }
}

/// `--ink`.
pub(super) fn ink(a: f32) -> Color32 {
    hsla(250.0, 45.0, 7.0, a)
}

/// The invite: the server's share link on the game's site.
#[must_use]
pub fn invite_href(site: &str, share_url: &str, lobby_id: &str) -> String {
    let share = if share_url.is_empty() {
        format!("/?lobby={lobby_id}")
    } else {
        share_url.to_owned()
    };
    if share.contains("://") {
        return share;
    }
    let site = site.trim_end_matches('/');
    let path = if share.starts_with('/') {
        share
    } else {
        format!("/{share}")
    };
    format!("{site}{path}")
}

pub(super) fn ease_in_out(t: f32) -> f32 {
    // CSS `ease-in-out`, close enough for a light band.
    t * t * (3.0 - 2.0 * t)
}

/// A rectangle filled left to right through `stops`.
pub(super) fn gradient_rect_x(rect: Rect, stops: &[(f32, Color32)]) -> Mesh {
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
    mesh
}

/// A band across `rect`: transparent outside the stops.
pub(super) fn horizontal_band(painter: &Painter, rect: Rect, stops: &[(f32, Color32)]) {
    painter.add(gradient_rect_x(rect, stops));
}

pub(super) fn vs_galley(painter: &Painter, size: f32, style: &RoomStyle<'_>) -> std::sync::Arc<egui::Galley> {
    text(
        painter,
        "VS",
        size,
        style.display.clone(),
        hsl(46.0, 100.0, 60.0),
        -size * 0.02,
    )
}

/// `match-lobby__vs`: gold, skewed, on a hard shadow.
pub(super) fn paint_vs(painter: &Painter, centre: Pos2, g: std::sync::Arc<egui::Galley>) {
    let at = centre - g.size() / 2.0;
    let size = g.size().y;
    painter.galley_with_override_text_color(at + vec2(size * 0.08, size * 0.1), g.clone(), hsl(250.0, 50.0, 3.0));
    painter.galley(at, g, hsl(46.0, 100.0, 60.0));
}

/// The face's part from `y0` to `y1`, for a key cut by `cut` px.
pub(super) fn face_band(face: Rect, cut: f32, y0: f32, y1: f32) -> [Pos2; 4] {
    let h = face.height();
    let left = |y: f32| face.left() + cut * (1.0 - (y - face.top()) / h);
    let right = |y: f32| face.right() - cut * (y - face.top()) / h;
    [
        pos2(left(y0), y0),
        pos2(right(y0), y0),
        pos2(right(y1), y1),
        pos2(left(y1), y1),
    ]
}

/// A polygon showing `texture` as `background-size: cover` over `area`.
pub(super) fn textured_polygon(painter: &Painter, points: &[Pos2], texture: TextureId, area: Rect, image: [f32; 2]) {
    // Cover: crop the image to the area's aspect.
    let (iw, ih) = (image[0].max(1.0), image[1].max(1.0));
    let scale = (area.width() / iw).max(area.height() / ih);
    let shown = vec2(area.width() / (iw * scale), area.height() / (ih * scale));
    let uv0 = pos2((1.0 - shown.x) / 2.0, (1.0 - shown.y) / 2.0);
    let mut mesh = Mesh::with_texture(texture);
    for p in points {
        let t = vec2((p.x - area.left()) / area.width(), (p.y - area.top()) / area.height());
        mesh.vertices.push(Vertex {
            pos: *p,
            uv: pos2(uv0.x + t.x * shown.x, uv0.y + t.y * shown.y),
            color: Color32::WHITE,
        });
    }
    for i in 1..points.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    painter.add(mesh);
}

/// A map's card (`raised-key--map`); true when the player votes for it.
pub(super) fn map_card(
    ui: &mut egui::Ui,
    painter: &Painter,
    rect: Rect,
    map: &MatchLobbyMapOption,
    leading: bool,
    starting: bool,
    style: &RoomStyle<'_>,
) -> bool {
    let a = style.accent;
    let rest = KeyLook {
        face: vec![
            (0.0, hsl(45.0, 100.0, 97.0)),
            (0.62, hsl(42.0, 58.0, 89.0)),
            (1.0, hsl(40.0, 40.0, 80.0)),
        ],
        side: [hsl(36.0, 24.0, 56.0), hsl(30.0, 20.0, 38.0)],
        shadow: hsla(250.0, 70.0, 2.0, 0.6),
        depth: 7.8,
        cut: 0.024,
        ..KeyLook::cream()
    };
    let picked = KeyLook {
        face: vec![(0.0, a.hi), (0.58, a.face), (1.0, a.lo)],
        side: [a.side_hi, a.side_lo],
        text: hsl(250.0, 45.0, 8.0),
        sub: hsla(250.0, 45.0, 8.0, 0.66),
        ..rest.clone()
    };
    let key = if map.voted_by_local {
        RaisedKey::new(("room-map", map.name.as_str()), rect, picked.clone())
            .rest_press(-0.5)
            .hover(picked, -0.75)
            .active_face(vec![(0.0, a.face), (1.0, a.lo)])
    } else {
        RaisedKey::new(("room-map", map.name.as_str()), rect, rest)
    };
    let face: KeyFace = key.disabled(starting).show(ui);
    let f = face.rect;
    let cut = f.width() * 0.024;
    let name_size = if style.narrow {
        14.0
    } else {
        clamp_css(14.0, style.window_width * 0.0115, 20.0)
    };
    let (pad_t, pad_b) = if style.narrow { (11.0, 13.0) } else { (13.0, 15.0) };
    let info_h = name_size + pad_t + pad_b;
    let shot_bottom = f.bottom() - info_h;
    if shot_bottom - f.top() > 20.0 {
        let band = face_band(f, cut, f.top(), shot_bottom);
        gradient_polygon(
            painter,
            &band,
            &[(0.0, hsl(250.0, 30.0, 18.0)), (1.0, hsl(262.0, 34.0, 12.0))],
        );
        let area = Rect::from_min_max(f.min, pos2(f.right(), shot_bottom));
        if let Some(tex) = (style.map_shot)(&map.name) {
            let size = ui
                .ctx()
                .tex_manager()
                .read()
                .meta(tex)
                .map_or([16.0, 9.0], |m| [m.size[0] as f32, m.size[1] as f32]);
            // Grows a little under the pointer, as the browser's does.
            let zoom = if face.hovered { 1.06 } else { 1.0 };
            let zoomed = Rect::from_center_size(area.center(), area.size() * zoom);
            let dim = if starting { 0.6 } else { 1.0 };
            let mut mesh_painter = painter.clone();
            mesh_painter.set_opacity(dim);
            textured_polygon(&mesh_painter, &band, tex, zoomed, size);
        }
        let shade = face_band(f, cut, f.top() + (shot_bottom - f.top()) * 0.55, shot_bottom);
        gradient_polygon(
            painter,
            &shade,
            &[(0.0, Color32::TRANSPARENT), (1.0, hsla(250.0, 50.0, 4.0, 0.55))],
        );
        // Tabs flush in the shot's top corner, starting where the card's
        // slanted edge does: Your vote in gold, so the green of the leading
        // map stands out. A tab after another tucks under its slant.
        let mut fx = f.left() + cut;
        let mut flags = Vec::new();
        if map.voted_by_local {
            flags.push(("YOUR VOTE", hsl(46.0, 100.0, 60.0)));
        }
        if leading {
            flags.push(("LEADING", hsl(96.0, 85.0, 55.0)));
        }
        for (i, (label, bg)) in flags.into_iter().enumerate() {
            let g = text(painter, label, 13.0, style.display.clone(), ink(1.0), 13.0 * 0.12);
            let (pad_l, inset) = if i == 0 { (14.0, 0.0) } else { (20.0, 10.0) };
            if i > 0 {
                fx -= 6.0;
            }
            let r = Rect::from_min_size(pos2(fx, f.top()), vec2(g.size().x + pad_l + 22.0, g.size().y + 19.0));
            painter.add(Shape::convex_polygon(
                vec![
                    r.left_top() + vec2(inset, 0.0),
                    r.right_top(),
                    r.right_bottom() - vec2(10.0, 0.0),
                    r.left_bottom(),
                ],
                bg,
                Stroke::NONE,
            ));
            painter.galley(pos2(r.left() + pad_l, r.top() + 9.0), g, ink(1.0));
            fx = r.right();
        }
        // Votes, bottom right.
        if map.votes > 0 {
            let n = text(
                painter,
                &map.votes.to_string(),
                46.0,
                style.display.clone(),
                cream(),
                0.0,
            );
            let word = if map.votes == 1 { "vote" } else { "votes" };
            let w = text(
                painter,
                word,
                14.0,
                style.display.clone(),
                hsla(45.0, 100.0, 95.0, 0.7),
                14.0 * 0.14,
            );
            let total = n.size().x + 8.0 + w.size().x;
            let x = f.right() - 18.0 - total;
            let y = shot_bottom - 8.0 - n.size().y;
            let nh = n.size().y;
            let nw = n.size().x;
            painter.galley_with_override_text_color(pos2(x + 3.0, y + 4.0), n.clone(), hsl(250.0, 50.0, 3.0));
            painter.galley(pos2(x, y), n, cream());
            painter.galley(pos2(x + nw + 8.0, y + nh * 0.78 - w.size().y), w, cream());
        }
    }
    // The name under the shot.
    let ix = f.right() - cut - 26.0;
    let cy = shot_bottom + info_h / 2.0;
    let title = if map.display_name.is_empty() {
        &map.name
    } else {
        &map.display_name
    };
    let room = ix - 12.0 - (f.left() + 22.0);
    let shown = elide(
        painter,
        &title.to_uppercase(),
        name_size,
        style.display.clone(),
        room.max(20.0),
    );
    let name = text(
        painter,
        &shown,
        name_size,
        style.display.clone(),
        face.text,
        name_size * 0.04,
    );
    painter.galley(pos2(f.left() + 22.0, cy - name.size().y / 2.0), name, face.text);
    face.clicked
}

/// A slot without a map (`raised-key--map-empty`).
pub(super) fn map_slot_empty(painter: &Painter, rect: Rect, style: &RoomStyle<'_>) {
    let look = KeyLook {
        face: vec![(0.0, hsl(250.0, 23.0, 16.0)), (1.0, hsl(250.0, 23.0, 16.0))],
        side: [hsl(250.0, 20.0, 11.0), hsl(250.0, 20.0, 11.0)],
        shadow: hsla(250.0, 70.0, 2.0, 0.6),
        depth: 7.8,
        cut: 0.024,
        ..KeyLook::cream()
    };
    let f = super::super::raised_key::paint_key(painter, rect, &look, 0.0, 0.55);
    let dash = text(
        painter,
        "—",
        64.0,
        style.display.clone(),
        hsla(250.0, 15.0, 80.0, 0.25),
        0.0,
    );
    painter.galley(f.center() - dash.size() / 2.0 - vec2(0.0, 18.0), dash, cream());
    let name = text(
        painter,
        "UNAVAILABLE",
        14.0,
        style.display.clone(),
        hsla(45.0, 30.0, 90.0, 0.55 * 0.55),
        0.6,
    );
    painter.galley(pos2(f.left() + 22.0, f.bottom() - 15.0 - name.size().y), name, cream());
}
