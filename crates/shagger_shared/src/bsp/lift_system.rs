//! Plats and vertical doors (SHAG `src/shared/bsp/LiftSystem.ts`).
//!
//! Fixed-step state shared by the server and client prediction. The
//! TypeScript keeps the trace function inside the system; here [`LiftSystem::step`]
//! takes it as an argument and hands it the system, because the collision
//! backend that answers it also reads the lift offsets.

use super::map_entities::{EntityLike, ModelLike, inline_model_index, origin_of};
use crate::js::{js_cos, js_hypot, js_max, js_min, js_sign, js_sin, js_string_to_number, to_int32};
use crate::server::simulation::types::Vec3;

/// Cooldown for a caller that must never rearm, finite so it still serializes.
const ONCE: f64 = 1e9;
/// Q3 dealt a mover's `dmg` once per 50 ms server frame; hold that rate.
const CRUSH_RATE_HZ: f64 = 20.0;
const DEFAULT_DOOR_SOUND: &str = "sounds/movers/door_start.ogg";
const DEFAULT_PLAT_SOUND: &str = "sounds/movers/elevator_move.ogg";
const DEFAULT_STOP_SOUND: &str = "sounds/movers/door_stop.ogg";

/// Something that calls a lift: a touch volume, or a `func_button`
/// (`LiftCaller`).
#[derive(Debug, Clone, PartialEq)]
pub struct LiftTrigger {
    pub min: Vec3,
    pub max: Vec3,
    pub wait: f64,
    /// A button with `health` fires when it takes damage and ignores
    /// touch; these are its brushes' boxes at rest, which a direct hit must
    /// land on.
    pub shot: Option<Vec<(Vec3, Vec3)>>,
    /// The button's inline model, which presses in while this caller rests.
    pub button: Option<LiftButton>,
}

/// How a `func_button` brush presses in: `push` is the full travel, at
/// `speed` units a second (`LiftButton`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiftButton {
    pub model_index: usize,
    pub push: Vec3,
    pub speed: f64,
}

/// Q3-space brush boxes (`mins`, `maxs`) of an inline model, for direct-hit
/// tests on shootable buttons (`InlineModelBrushBoxes`).
pub type InlineModelBrushBoxes<'f> = dyn Fn(usize) -> Vec<([f64; 3], [f64; 3])> + 'f;

#[derive(Debug, Clone, PartialEq)]
pub struct LiftDefinition {
    pub model_index: usize,
    pub min: Vec3,
    pub max: Vec3,
    pub origin: Vec3,
    pub height: f64,
    pub speed: f64,
    pub wait: f64,
    pub automatic: bool,
    pub start: f64,
    pub end: f64,
    pub plat: bool,
    pub toggle: bool,
    /// Presentation only; never simulated.
    pub sound: String,
    pub sound_stop: String,
    /// Q3 `dmg` dealt to whoever blocks this mover. 0 disables crushing.
    pub damage: f64,
    /// Q3 CRUSHER spawnflag: keep pushing when blocked instead of reversing.
    pub crusher: bool,
    pub triggers: Vec<LiftTrigger>,
}

/// Logical rest, outbound, activated and return states (a downward door's
/// `Up` goes down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiftPhase {
    Bottom,
    Up,
    Top,
    Down,
}

impl LiftPhase {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Up => "up",
            Self::Top => "top",
            Self::Down => "down",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiftState {
    pub model_index: usize,
    pub offset: f64,
    pub phase: LiftPhase,
    pub remaining: f64,
    pub cooldowns: Vec<f64>,
}

/// A body squashed by a blocked mover, for the caller to turn into damage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiftCrush {
    pub id: Option<i64>,
    pub model_index: usize,
    /// Damage for this step, already scaled to the tick length.
    pub damage: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiftActor {
    pub position: Vec3,
    pub velocity: Vec3,
    /// Caller's handle, echoed back in [`LiftCrush`].
    pub id: Option<i64>,
    /// Corpses and dropped items ride a lift but never call or hold one.
    pub passive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiftTrace {
    pub fraction: f64,
    pub solid: bool,
    pub normal_y: f64,
    /// The lift model hit, or -1.
    pub model_index: i64,
}

/// `number(value, fallback)` in `extractLifts`.
fn number(value: Option<&str>, fallback: f64) -> f64 {
    let parsed = value.map_or(f64::NAN, js_string_to_number);
    if parsed.is_finite() { parsed } else { fallback }
}

/// `extractLifts`. BSP plats are authored at the upper stop; simulation
/// starts one height below.
pub fn extract_lifts<E: EntityLike, M: ModelLike>(
    entities: &[E],
    models: &[M],
    brush_boxes: Option<&InlineModelBrushBoxes<'_>>,
) -> Vec<LiftDefinition> {
    let mut result = Vec::new();
    for entity in entities {
        let p = |k: &str| entity.property(k);
        let plat = entity.classname() == "func_plat";
        let angle = p("angle");
        if !plat && !(entity.classname() == "func_door" && matches!(angle, Some("-1" | "-2"))) {
            continue;
        }
        let Some(model_index) = inline_model_index(p("model"), false) else {
            continue;
        };
        let Some(model) = models.get(model_index) else { continue };
        if model_index == 0 {
            continue;
        }
        let Some(o) = origin_of(p("origin")) else { continue };
        let origin = Vec3::new(o[0], o[2], o[1]);
        let (mins, maxs) = (model.mins(), model.maxs());
        let height = number(
            if plat { p("height") } else { None },
            maxs[2] - mins[2] - number(p("lip"), 8.0),
        );
        if height <= 0.0 {
            continue;
        }
        let mut start = if plat { -height } else { 0.0 };
        let mut end = if plat {
            0.0
        } else if angle == Some("-2") {
            -height
        } else {
            height
        };
        let spawnflags = to_int32(number(p("spawnflags"), 0.0));
        if !plat && spawnflags & 1 != 0 {
            std::mem::swap(&mut start, &mut end);
        }
        let mut triggers = Vec::new();
        if let Some(targetname) = p("targetname").filter(|t| !t.is_empty()) {
            for trigger in entities {
                let t = |k: &str| trigger.property(k);
                let button = trigger.classname() == "func_button";
                let once = trigger.classname() == "trigger_once";
                if !button && !once && trigger.classname() != "trigger_multiple" {
                    continue;
                }
                if t("target") != Some(targetname) {
                    continue;
                }
                let Some(tm_index) = inline_model_index(t("model"), false).filter(|&i| i > 0) else {
                    continue;
                };
                let Some(tm) = models.get(tm_index) else { continue };
                let Some(to) = origin_of(t("origin")) else { continue };
                // A button stays pressed 3 s unless told otherwise, as in
                // Warsow; a `wait` of 0 also means the default there.
                let wait = if button {
                    match number(t("wait"), 3.0) {
                        0.0 => 3.0,
                        w => w,
                    }
                } else {
                    number(t("wait"), 0.5)
                };
                let (tmin, tmax) = (tm.mins(), tm.maxs());
                let box_of = |mins: [f64; 3], maxs: [f64; 3]| {
                    (
                        Vec3::new(mins[0] + to[0], mins[2] + to[2], mins[1] + to[1]),
                        Vec3::new(maxs[0] + to[0], maxs[2] + to[2], maxs[1] + to[1]),
                    )
                };
                let (min, max) = box_of(tmin, tmax);
                let mut caller = LiftTrigger {
                    min,
                    max,
                    wait: if once || wait < 0.0 { ONCE } else { js_max(0.1, wait) },
                    shot: None,
                    button: None,
                };
                if button {
                    caller.button = Some(button_travel(tm_index, tmin, tmax, trigger));
                    // Any nonzero health makes the button shootable, and then
                    // touch does nothing.
                    if number(t("health"), 0.0) != 0.0 {
                        let boxes = brush_boxes.map(|f| f(tm_index)).unwrap_or_default();
                        let parts = if boxes.is_empty() {
                            vec![box_of(tmin, tmax)]
                        } else {
                            boxes.into_iter().map(|(mins, maxs)| box_of(mins, maxs)).collect()
                        };
                        caller.shot = Some(parts);
                    }
                }
                triggers.push(caller);
            }
        }
        let targetname = p("targetname").unwrap_or("");
        result.push(LiftDefinition {
            model_index,
            origin,
            height,
            min: Vec3::new(mins[0] + origin.x, mins[2] + origin.y, mins[1] + origin.z),
            max: Vec3::new(maxs[0] + origin.x, maxs[2] + origin.y, maxs[1] + origin.z),
            speed: js_max(1.0, number(p("speed"), if plat { 200.0 } else { 100.0 })),
            wait: number(p("wait"), if plat { 1.0 } else { 2.0 }),
            sound: p("noise_start")
                .or_else(|| p("noise"))
                .or_else(|| p("sound"))
                .unwrap_or(if plat { DEFAULT_PLAT_SOUND } else { DEFAULT_DOOR_SOUND })
                .to_owned(),
            sound_stop: p("noise_stop").unwrap_or(DEFAULT_STOP_SOUND).to_owned(),
            damage: js_max(0.0, number(p("dmg"), 2.0)),
            crusher: spawnflags & 4 != 0,
            automatic: targetname.is_empty(),
            start,
            end,
            plat,
            triggers,
            toggle: !plat && spawnflags & 32 != 0,
        });
    }
    result
}

/// `buttonTravel`: Q3 `func_button` travel along its `angle` (-1 up, -2
/// down, else a yaw), by the model's size on that axis less `lip` (default
/// 4), at `speed` (default 40).
fn button_travel<E: EntityLike>(model_index: usize, mins: [f64; 3], maxs: [f64; 3], entity: &E) -> LiftButton {
    let to_number = |k: &str| entity.property(k).map_or(f64::NAN, js_string_to_number);
    let angle = entity.property("angle").map_or(0.0, js_string_to_number);
    let dir = if angle == -1.0 {
        [0.0, 0.0, 1.0]
    } else if angle == -2.0 {
        [0.0, 0.0, -1.0]
    } else {
        let yaw = (if angle.is_finite() { angle } else { 0.0 }) * std::f64::consts::PI / 180.0;
        [js_cos(yaw), js_sin(yaw), 0.0]
    };
    // `lip || 4`: missing, unreadable or 0 is the default.
    let lip = match to_number("lip") {
        l if l.is_nan() || l == 0.0 => 4.0,
        l => l,
    };
    let size = [maxs[0] - mins[0], maxs[1] - mins[1], maxs[2] - mins[2]];
    let distance = js_max(
        0.0,
        dir[0].abs() * size[0] + dir[1].abs() * size[1] + dir[2].abs() * size[2] - lip,
    );
    let speed = to_number("speed");
    LiftButton {
        model_index,
        push: Vec3::new(dir[0] * distance, dir[2] * distance, dir[1] * distance),
        speed: if speed > 0.0 { speed } else { 40.0 },
    }
}

/// A shot's impact point sits on the brush face, give or take the trace's
/// 2-unit box.
const STRIKE_EPSILON: f64 = 3.0;

/// Distance from `p` to the box `min`..`max` moved by `shift`; 0 inside it.
fn distance_to_box(p: Vec3, min: Vec3, max: Vec3, shift: Vec3) -> f64 {
    let dx = js_max(js_max(min.x + shift.x - p.x, 0.0), p.x - max.x - shift.x);
    let dy = js_max(js_max(min.y + shift.y - p.y, 0.0), p.y - max.y - shift.y);
    let dz = js_max(js_max(min.z + shift.z - p.z, 0.0), p.z - max.z - shift.z);
    js_hypot(&[dx, dy, dz])
}

/// The ray a lift asks the collision world for: `(system, start, end,
/// exclude model or -1, only model or -1)`.
pub type LiftTraceFn<'f> = dyn FnMut(&LiftSystem, Vec3, Vec3, i64, i64) -> LiftTrace + 'f;

/// `LiftSystem`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiftSystem {
    pub definitions: Vec<LiftDefinition>,
    states: Vec<LiftState>,
    /// Every `func_button` that calls a lift, once each, for collision and
    /// rendering.
    pub buttons: Vec<LiftButton>,
    /// A button targeting two doors is a caller of both; its press reads
    /// either. By model: (definition, caller) pairs.
    button_callers: Vec<(usize, Vec<(usize, usize)>)>,
}

fn overlaps(p: Vec3, min: Vec3, max: Vec3) -> bool {
    p.x + 16.0 > min.x
        && p.x - 16.0 < max.x
        && p.y + 40.0 > min.y
        && p.y - 24.0 < max.y
        && p.z + 16.0 > min.z
        && p.z - 16.0 < max.z
}

impl LiftSystem {
    #[must_use]
    pub fn new(definitions: Vec<LiftDefinition>) -> Self {
        let states = definitions
            .iter()
            .map(|d| LiftState {
                model_index: d.model_index,
                offset: d.start,
                phase: LiftPhase::Bottom,
                remaining: 0.0,
                cooldowns: vec![0.0; d.triggers.len()],
            })
            .collect();
        let mut buttons: Vec<LiftButton> = Vec::new();
        let mut button_callers: Vec<(usize, Vec<(usize, usize)>)> = Vec::new();
        for (i, d) in definitions.iter().enumerate() {
            for (j, t) in d.triggers.iter().enumerate() {
                let Some(button) = t.button else { continue };
                match button_callers.iter_mut().find(|(m, _)| *m == button.model_index) {
                    Some((_, callers)) => callers.push((i, j)),
                    None => {
                        button_callers.push((button.model_index, vec![(i, j)]));
                        buttons.push(button);
                    }
                }
            }
        }
        Self {
            definitions,
            states,
            buttons,
            button_callers,
        }
    }

    /// `buttonOffset`: how far a button is pressed in now, from its
    /// caller's cooldown: it travels in at its speed and holds until the
    /// caller rearms. Derived from simulated state, so prediction,
    /// collision and rendering agree without extra data.
    #[must_use]
    pub fn button_offset(&self, model_index: usize) -> Vec3 {
        let Some((_, callers)) = self.button_callers.iter().find(|(m, _)| *m == model_index) else {
            return Vec3::new(0.0, 0.0, 0.0);
        };
        // The first caller's button: the one the entry was made with.
        let Some(button) = self.definitions[callers[0].0].triggers[callers[0].1].button else {
            return Vec3::new(0.0, 0.0, 0.0);
        };
        let mut held = -1.0;
        for &(i, j) in callers {
            let cooldown = self.states[i].cooldowns[j];
            if cooldown > 0.0 {
                held = js_max(held, self.definitions[i].triggers[j].wait - cooldown);
            }
        }
        if held < 0.0 {
            return Vec3::new(0.0, 0.0, 0.0);
        }
        let push = button.push;
        let length = js_hypot(&[push.x, push.y, push.z]);
        let fraction = if length > 0.0 {
            js_min(1.0, held * button.speed / length)
        } else {
            1.0
        };
        Vec3::new(push.x * fraction, push.y * fraction, push.z * fraction)
    }

    /// `strike`: damage landed at `point`. A direct hit (`radius` 0) fires
    /// a shootable button whose brush it is on; a blast fires one whose
    /// bounds lie within `radius`, as Q3 radius damage reached any entity's
    /// box. Returns how many lifts were set going.
    pub fn strike(&mut self, point: Vec3, radius: f64) -> usize {
        if !(point.x + point.y + point.z).is_finite() {
            return 0;
        }
        let mut fired = 0;
        for i in 0..self.definitions.len() {
            for j in 0..self.definitions[i].triggers.len() {
                let t = &self.definitions[i].triggers[j];
                let Some(parts) = &t.shot else { continue };
                if self.states[i].cooldowns[j] > 0.0 {
                    continue;
                }
                let offset = t
                    .button
                    .map_or(Vec3::new(0.0, 0.0, 0.0), |b| self.button_offset(b.model_index));
                let hit = if radius > 0.0 {
                    distance_to_box(point, t.min, t.max, offset) < radius
                } else {
                    parts
                        .iter()
                        .any(|&(min, max)| distance_to_box(point, min, max, offset) <= STRIKE_EPSILON)
                };
                let wait = t.wait;
                if !hit {
                    continue;
                }
                self.states[i].cooldowns[j] = wait;
                self.activate(i);
                fired += 1;
            }
        }
        fired
    }

    /// A caller fired: start the lift, reverse a toggle, or hold an open
    /// door (`activate`).
    fn activate(&mut self, i: usize) {
        let d = &self.definitions[i];
        let s = &mut self.states[i];
        if d.toggle && matches!(s.phase, LiftPhase::Up | LiftPhase::Top) {
            s.phase = LiftPhase::Down;
        } else if matches!(s.phase, LiftPhase::Bottom | LiftPhase::Down) {
            s.phase = LiftPhase::Up;
        } else if s.phase == LiftPhase::Top && !d.plat {
            s.remaining = js_max(0.0, d.wait);
        }
    }

    fn index_of(&self, model_index: usize) -> Option<usize> {
        // Like the TypeScript Map, a repeated model keeps its last index.
        self.definitions.iter().rposition(|d| d.model_index == model_index)
    }

    #[must_use]
    pub fn phase(&self, model_index: usize) -> Option<LiftPhase> {
        self.index_of(model_index).map(|i| self.states[i].phase)
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<LiftState> {
        self.states.clone()
    }

    /// Only map-known, finite state. Never lets a packet create geometry.
    pub fn restore(&mut self, states: &[LiftState]) {
        for i in 0..self.states.len() {
            let d = &self.definitions[i];
            let Some(s) = states.iter().find(|s| s.model_index == d.model_index) else {
                continue;
            };
            if !s.offset.is_finite() || !s.remaining.is_finite() {
                continue;
            }
            let (lo, hi) = (js_min(d.start, d.end), js_max(d.start, d.end));
            self.states[i] = LiftState {
                model_index: s.model_index,
                offset: js_max(lo, js_min(hi, s.offset)),
                phase: s.phase,
                remaining: js_max(0.0, s.remaining),
                cooldowns: d
                    .triggers
                    .iter()
                    .enumerate()
                    .map(|(j, t)| match s.cooldowns.get(j) {
                        Some(&c) if c.is_finite() => js_max(0.0, js_min(t.wait, c)),
                        _ => 0.0,
                    })
                    .collect(),
            };
        }
    }

    /// Travel offsets, in definition order, for lag-compensation history.
    #[must_use]
    pub fn offsets(&self) -> Vec<f64> {
        self.states.iter().map(|s| s.offset).collect()
    }

    /// Runs `query` with the movers where they were on a recorded tick.
    pub fn rewind<T>(&mut self, offsets: &[f64], query: impl FnOnce(&Self) -> T) -> T {
        let saved = self.offsets();
        for (s, &o) in self.states.iter_mut().zip(offsets) {
            if o.is_finite() {
                s.offset = o;
            }
        }
        let result = query(self);
        for (s, o) in self.states.iter_mut().zip(saved) {
            s.offset = o;
        }
        result
    }

    /// Current vertical travel of the mover at `definitions[index]`.
    #[must_use]
    pub fn travel_at(&self, index: usize) -> f64 {
        self.states[index].offset
    }

    /// The mover's current origin in game space.
    #[must_use]
    pub fn offset(&self, model_index: usize) -> Option<Vec3> {
        let i = self.index_of(model_index)?;
        let o = self.definitions[i].origin;
        Some(Vec3::new(o.x, o.y + self.states[i].offset, o.z))
    }

    fn in_trigger(d: &LiftDefinition, p: Vec3) -> bool {
        let x0 = js_min(d.min.x + 33.0, (d.min.x + d.max.x) / 2.0);
        let x1 = js_max(d.max.x - 33.0, x0 + 1.0);
        let z0 = js_min(d.min.z + 33.0, (d.min.z + d.max.z) / 2.0);
        let z1 = js_max(d.max.z - 33.0, z0 + 1.0);
        if !d.plat {
            let lo = Vec3::new(d.min.x - 16.0, d.min.y + d.start - 16.0, d.min.z - 16.0);
            let hi = Vec3::new(d.max.x + 16.0, d.max.y + d.start + 16.0, d.max.z + 16.0);
            return overlaps(p, lo, hi);
        }
        p.x + 16.0 > x0
            && p.x - 16.0 < x1
            && p.z + 16.0 > z0
            && p.z - 16.0 < z1
            && p.y + 40.0 > d.min.y - d.height
            && p.y - 24.0 < d.max.y - d.height + 8.0
    }

    /// `step`: advance every mover one fixed step, carrying and pushing the
    /// given bodies. Returns whoever got squashed.
    #[allow(clippy::too_many_lines)]
    pub fn step(&mut self, dt: f64, actors: &mut [LiftActor], trace: &mut LiftTraceFn<'_>) -> Vec<LiftCrush> {
        let mut crushes = Vec::new();
        if !(dt.is_finite() && dt > 0.0) {
            return crushes;
        }
        for i in 0..self.states.len() {
            let mut activated = false;
            for j in 0..self.definitions[i].triggers.len() {
                let cooldown = js_max(0.0, self.states[i].cooldowns[j] - dt);
                self.states[i].cooldowns[j] = cooldown;
                let t = &self.definitions[i].triggers[j];
                if t.shot.is_none()
                    && cooldown == 0.0
                    && actors.iter().any(|a| !a.passive && overlaps(a.position, t.min, t.max))
                {
                    activated = true;
                    self.states[i].cooldowns[j] = t.wait;
                }
            }
            let d = &self.definitions[i];
            let offset = self.states[i].offset;
            let padding = d.speed * dt + 2.0;
            let lo = Vec3::new(d.min.x, d.min.y + offset - padding, d.min.z);
            let hi = Vec3::new(d.max.x, d.max.y + offset + padding, d.max.z);
            let nearby: Vec<usize> = (0..actors.len())
                .filter(|&a| overlaps(actors[a].position, lo, hi))
                .collect();
            let model_index = d.model_index;
            let (automatic, toggle, plat, wait, start, end, speed, damage, crusher) = (
                d.automatic,
                d.toggle,
                d.plat,
                d.wait,
                d.start,
                d.end,
                d.speed,
                d.damage,
                d.crusher,
            );
            // Riders in first-seen order, as the TypeScript Set iterates.
            let mut riders: Vec<usize> = Vec::new();
            for &a in &nearby {
                let actor = actors[a];
                if actor.velocity.y > 1.0 {
                    continue;
                }
                let below = Vec3 {
                    y: actor.position.y - 2.0,
                    ..actor.position
                };
                let hit = trace(self, actor.position, below, -1, -1);
                if hit.fraction < 1.0 && hit.normal_y > 0.7 && hit.model_index == model_index as i64 {
                    riders.push(a);
                }
            }
            let d = &self.definitions[i];
            activated =
                activated || (automatic && actors.iter().any(|a| !a.passive && Self::in_trigger(d, a.position)));
            let s = &mut self.states[i];
            if activated {
                if toggle && matches!(s.phase, LiftPhase::Up | LiftPhase::Top) {
                    s.phase = LiftPhase::Down;
                } else if matches!(s.phase, LiftPhase::Bottom | LiftPhase::Down) {
                    s.phase = LiftPhase::Up;
                }
            }
            if s.phase == LiftPhase::Top {
                let hold = if plat {
                    riders.iter().any(|&a| !actors[a].passive)
                } else {
                    activated
                };
                s.remaining = if hold {
                    js_max(0.0, wait)
                } else {
                    js_max(0.0, s.remaining - dt)
                };
                if !toggle && wait >= 0.0 && s.remaining <= 0.0 && !hold {
                    s.phase = LiftPhase::Down;
                }
            }
            if s.phase != LiftPhase::Up && s.phase != LiftPhase::Down {
                continue;
            }
            let old_offset = s.offset;
            let target = if s.phase == LiftPhase::Up { end } else { start };
            let next = old_offset + js_sign(target - old_offset) * js_min((target - old_offset).abs(), speed * dt);
            let delta = next - old_offset;
            let mut pushed: Vec<(usize, Vec3)> = Vec::new();
            let mut blocker: Option<usize> = None;
            for &a in &nearby {
                let position = actors[a].position;
                let contact = trace(
                    self,
                    position,
                    Vec3 {
                        y: position.y - delta,
                        ..position
                    },
                    -1,
                    model_index as i64,
                );
                if !riders.contains(&a) && contact.fraction == 1.0 && !contact.solid {
                    continue;
                }
                let target_position = Vec3 {
                    y: position.y + delta,
                    ..position
                };
                let obstruction = trace(self, position, target_position, model_index as i64, -1);
                if obstruction.fraction < 1.0 || obstruction.solid {
                    blocker = Some(a);
                    break;
                }
                pushed.push((a, target_position));
            }
            let s = &mut self.states[i];
            if let Some(b) = blocker {
                if damage > 0.0 && !actors[b].passive {
                    crushes.push(LiftCrush {
                        id: actors[b].id,
                        model_index,
                        damage: damage * dt * CRUSH_RATE_HZ,
                    });
                }
                if !crusher {
                    s.phase = if s.phase == LiftPhase::Up {
                        LiftPhase::Down
                    } else {
                        LiftPhase::Up
                    };
                }
                continue;
            }
            s.offset = next;
            for (a, position) in pushed {
                actors[a].position = position;
            }
            if s.offset == end {
                s.phase = LiftPhase::Top;
                s.remaining = js_max(0.0, wait);
            } else if s.offset == start {
                s.phase = LiftPhase::Bottom;
            }
        }
        crushes
    }
}

#[cfg(test)]
mod tests {
    //! SHAG `src/shared/bsp/__tests__/LiftButtons.test.ts`.
    use super::*;

    struct E(&'static str, Vec<(&'static str, &'static str)>);

    impl EntityLike for E {
        fn classname(&self) -> &str {
            self.0
        }
        fn property(&self, key: &str) -> Option<&str> {
            self.1.iter().rev().find(|(k, _)| *k == key).map(|(_, v)| *v)
        }
    }

    struct M([f64; 3], [f64; 3]);

    impl ModelLike for M {
        fn mins(&self) -> [f64; 3] {
            self.0
        }
        fn maxs(&self) -> [f64; 3] {
            self.1
        }
    }

    // Q3 space: a split door like wdm16's techdoor3 (two halves, one going
    // up and one down) and a button made of two corner panels either side.
    fn models() -> Vec<M> {
        vec![
            M([-4096.0; 3], [4096.0; 3]),
            M([-64.0, -656.0, -256.0], [128.0, -624.0, -176.0]),
            M([-64.0, -656.0, -192.0], [128.0, -624.0, -64.0]),
            M([-106.0, -710.0, -222.0], [170.0, -570.0, -194.0]),
            M([-106.0, -576.0, -256.0], [-78.0, -568.0, -222.0]),
        ]
    }

    fn panels(i: usize) -> Vec<([f64; 3], [f64; 3])> {
        if i == 3 {
            vec![
                ([-106.0, -576.0, -222.0], [-78.0, -570.0, -194.0]),
                ([142.0, -710.0, -222.0], [170.0, -704.0, -194.0]),
            ]
        } else {
            Vec::new()
        }
    }

    fn door(model: &'static str, angle: &'static str) -> E {
        E(
            "func_door",
            vec![
                ("model", model),
                ("angle", angle),
                ("targetname", "techdoor3"),
                ("speed", "1500"),
                ("health", "-1"),
            ],
        )
    }

    fn button(extra: &[(&'static str, &'static str)]) -> E {
        let mut p = vec![
            ("model", "*3"),
            ("target", "techdoor3"),
            ("angle", "-2"),
            ("speed", "1000"),
            ("health", "1"),
        ];
        p.extend_from_slice(extra);
        E("func_button", p)
    }

    fn make(entities: &[E]) -> LiftSystem {
        LiftSystem::new(extract_lifts(entities, &models(), Some(&panels)))
    }

    fn step(lifts: &mut LiftSystem, dt: f64, actors: &mut [LiftActor]) {
        let mut trace = |_: &LiftSystem, _: Vec3, _: Vec3, _: i64, _: i64| LiftTrace {
            fraction: 1.0,
            solid: false,
            normal_y: 0.0,
            model_index: -1,
        };
        lifts.step(dt, actors, &mut trace);
    }

    /// Engine space is Q3 (x, z, y).
    fn at(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, z, y)
    }

    fn body(x: f64, y: f64, z: f64) -> LiftActor {
        LiftActor {
            position: at(x, y, z),
            velocity: Vec3::new(0.0, 0.0, 0.0),
            id: None,
            passive: false,
        }
    }

    fn phases(lifts: &LiftSystem) -> Vec<LiftPhase> {
        lifts.snapshot().iter().map(|s| s.phase).collect()
    }

    fn halves() -> Vec<E> {
        vec![door("*1", "-2"), door("*2", "-1")]
    }

    fn with(mut entities: Vec<E>, extra: E) -> Vec<E> {
        entities.push(extra);
        entities
    }

    #[test]
    fn a_shootable_button_ignores_touch() {
        let mut lifts = make(&with(halves(), button(&[])));
        // Standing in the doorway is inside the button's bounds.
        for _ in 0..30 {
            step(&mut lifts, 1.0 / 60.0, &mut [body(32.0, -640.0, -232.0)]);
        }
        assert_eq!(phases(&lifts), [LiftPhase::Bottom, LiftPhase::Bottom]);
    }

    #[test]
    fn a_shot_on_a_panel_opens_both_halves_and_between_them_does_not() {
        let mut lifts = make(&with(halves(), button(&[])));
        assert_eq!(lifts.strike(at(32.0, -640.0, -208.0), 0.0), 0);
        assert_eq!(lifts.strike(at(-92.0, -569.0, -208.0), 0.0), 2);
        assert_eq!(phases(&lifts), [LiftPhase::Up, LiftPhase::Up]);
        // It rests for its default 3 s, so a second shot does nothing.
        assert_eq!(lifts.strike(at(-92.0, -569.0, -208.0), 0.0), 0);
    }

    #[test]
    fn a_blast_reaches_the_buttons_bounds() {
        let mut lifts = make(&with(halves(), button(&[])));
        assert_eq!(lifts.strike(at(32.0, -760.0, -208.0), 40.0), 0);
        assert_eq!(lifts.strike(at(32.0, -740.0, -208.0), 40.0), 2);
    }

    #[test]
    fn presses_in_at_its_speed_holds_while_resting_and_comes_back() {
        let mut lifts = make(&with(halves(), button(&[])));
        assert_eq!(lifts.buttons.iter().map(|b| b.model_index).collect::<Vec<_>>(), [3]);
        assert_eq!(lifts.button_offset(3).y, 0.0);
        lifts.strike(at(155.0, -705.0, -208.0), 0.0);
        step(&mut lifts, 0.01, &mut []);
        // 1000 u/s for 10 ms is 10 of its 24 units (28 tall less a 4 lip), downward.
        assert!((lifts.button_offset(3).y + 10.0).abs() < 1e-6);
        for _ in 0..60 {
            step(&mut lifts, 1.0 / 60.0, &mut []);
        }
        assert!((lifts.button_offset(3).y + 24.0).abs() < 1e-6);
        for _ in 0..130 {
            step(&mut lifts, 1.0 / 60.0, &mut []);
        }
        assert_eq!(lifts.button_offset(3).y, 0.0);
    }

    #[test]
    fn touch_volumes_and_a_button_without_health_still_open_by_contact() {
        let touch = E("trigger_multiple", vec![("model", "*4"), ("target", "techdoor3")]);
        let mut lifts = make(&with(halves(), touch));
        step(&mut lifts, 1.0 / 60.0, &mut [body(-92.0, -572.0, -240.0)]);
        assert_eq!(phases(&lifts), [LiftPhase::Up, LiftPhase::Up]);
        let mut plain = make(&with(halves(), button(&[("health", "0")])));
        step(&mut plain, 1.0 / 60.0, &mut [body(32.0, -640.0, -232.0)]);
        assert_eq!(phases(&plain), [LiftPhase::Up, LiftPhase::Up]);
    }

    #[test]
    fn restores_a_buttons_press_from_the_serialized_caller_cooldown() {
        let mut server = make(&with(halves(), button(&[])));
        server.strike(at(-92.0, -569.0, -208.0), 0.0);
        for _ in 0..30 {
            step(&mut server, 1.0 / 60.0, &mut []);
        }
        let mut client = make(&with(halves(), button(&[])));
        client.restore(&server.snapshot());
        assert!((client.button_offset(3).y - server.button_offset(3).y).abs() < 1e-9);
        assert!((client.button_offset(3).y + 24.0).abs() < 1e-6);
    }
}
