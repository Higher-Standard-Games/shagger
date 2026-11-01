//! Plats and vertical doors (SHAG `src/shared/bsp/LiftSystem.ts`).
//!
//! Fixed-step state shared by the server and client prediction. The
//! TypeScript keeps the trace function inside the system; here [`LiftSystem::step`]
//! takes it as an argument and hands it the system, because the collision
//! backend that answers it also reads the lift offsets.

use super::map_entities::{EntityLike, ModelLike, inline_model_index, origin_of};
use crate::js::{js_max, js_min, js_sign, js_string_to_number, to_int32};
use crate::server::simulation::types::Vec3;

/// Cooldown for a caller that must never rearm, finite so it still serializes.
const ONCE: f64 = 1e9;
/// Q3 dealt a mover's `dmg` once per 50 ms server frame; hold that rate.
const CRUSH_RATE_HZ: f64 = 20.0;
const DEFAULT_DOOR_SOUND: &str = "sounds/movers/door_start.ogg";
const DEFAULT_PLAT_SOUND: &str = "sounds/movers/elevator_move.ogg";
const DEFAULT_STOP_SOUND: &str = "sounds/movers/door_stop.ogg";

#[derive(Debug, Clone, PartialEq)]
pub struct LiftTrigger {
    pub min: Vec3,
    pub max: Vec3,
    pub wait: f64,
}

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
pub fn extract_lifts<E: EntityLike, M: ModelLike>(entities: &[E], models: &[M]) -> Vec<LiftDefinition> {
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
                let Some(tm) = inline_model_index(t("model"), false).and_then(|i| models.get(i)) else {
                    continue;
                };
                let Some(to) = origin_of(t("origin")) else { continue };
                let wait = number(t("wait"), if button { 1.0 } else { 0.5 });
                let (tmin, tmax) = (tm.mins(), tm.maxs());
                triggers.push(LiftTrigger {
                    min: Vec3::new(tmin[0] + to[0], tmin[2] + to[2], tmin[1] + to[1]),
                    max: Vec3::new(tmax[0] + to[0], tmax[2] + to[2], tmax[1] + to[1]),
                    wait: if once || wait < 0.0 { ONCE } else { js_max(0.1, wait) },
                });
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

/// The ray a lift asks the collision world for: `(system, start, end,
/// exclude model or -1, only model or -1)`.
pub type LiftTraceFn<'f> = dyn FnMut(&LiftSystem, Vec3, Vec3, i64, i64) -> LiftTrace + 'f;

/// `LiftSystem`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiftSystem {
    pub definitions: Vec<LiftDefinition>,
    states: Vec<LiftState>,
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
        Self { definitions, states }
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
                if cooldown == 0.0 && actors.iter().any(|a| !a.passive && overlaps(a.position, t.min, t.max)) {
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
