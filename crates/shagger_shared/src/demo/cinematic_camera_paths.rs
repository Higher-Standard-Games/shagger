//! Port of SHAG `src/shared/demo/CinematicCameraPaths.ts`: the camera
//! keyframes of a reel's freefly shots, or the entity a chase shot follows.

use std::f64::consts::PI;

use crate::demo::demo_analyzer::HighlightCandidate;
use crate::demo::sequence_builder::CameraPreset;
use crate::js::{js_atan2, js_cos, js_hypot, js_sin};
use crate::net::game_state::Vec3;

/// `CameraKeyframe`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraKeyframe {
    pub tick: f64,
    pub position: Vec3,
    pub look_at: Vec3,
    /// Degrees; 90 when absent.
    pub fov: Option<f64>,
}

/// How a freefly path runs between its keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    CatmullRom,
}

/// `CameraPathResult`.
#[derive(Debug, Clone, PartialEq)]
pub enum CameraPath {
    Chase {
        target_entity_id: String,
    },
    Freefly {
        keyframes: Vec<CameraKeyframe>,
        interpolation: Interpolation,
    },
}

/// A camera at one tick of a path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub position: Vec3,
    pub look_at: Vec3,
    pub fov: f64,
}

const ORBIT_DEG_PER_SECOND: f64 = 90.0;
const ORBIT_MAX_ARC: f64 = 270.0;
const REPLAY_BOTH_WITHIN: f64 = 600.0;
const JUMP_HEADING_MIN_SPEED: f64 = 50.0;
const TRAIL_BACK: f64 = 170.0;
const TRAIL_SIDE: f64 = 80.0;
const TRAIL_UP: f64 = 60.0;
const TRAIL_KEYFRAME_SECONDS: f64 = 0.3;

fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn scale(v: Vec3, s: f64) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

fn lerp(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    add(a, scale(sub(b, a), t))
}

fn normalize(v: Vec3) -> Vec3 {
    let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if len < 1e-10 {
        return Vec3::new(0.0, 0.0, 0.0);
    }
    scale(v, 1.0 / len)
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

fn distance(a: Vec3, b: Vec3) -> f64 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Catmull-Rom between p1 and p2.
fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f64) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    let axis = |a: f64, b: f64, c: f64, d: f64| {
        0.5 * (2.0 * b + (c - a) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (3.0 * b - a - 3.0 * c + d) * t3)
    };
    Vec3::new(
        axis(p0.x, p1.x, p2.x, p3.x),
        axis(p0.y, p1.y, p2.y, p3.y),
        axis(p0.z, p1.z, p2.z, p3.z),
    )
}

fn lerp_tick(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn keyframe(tick: f64, position: Vec3, look_at: Vec3) -> CameraKeyframe {
    CameraKeyframe {
        tick,
        position,
        look_at,
        fov: None,
    }
}

/// `generateCameraPath`.
#[must_use]
pub fn generate_camera_path(
    preset: CameraPreset,
    h: &HighlightCandidate,
    start_tick: f64,
    end_tick: f64,
    tick_rate: f64,
) -> CameraPath {
    match preset {
        CameraPreset::ChaseHero => CameraPath::Chase {
            target_entity_id: h.primary_player.clone(),
        },
        CameraPreset::VictimPov => CameraPath::Chase {
            target_entity_id: h.victim_player.clone().unwrap_or_else(|| h.primary_player.clone()),
        },
        CameraPreset::OrbitKill => orbit_kill(h, start_tick, end_tick, tick_rate),
        CameraPreset::RocketJumpHero => rocket_jump_hero(h, start_tick, end_tick),
        CameraPreset::DramaticZoom => dramatic_zoom(h, start_tick, end_tick),
        CameraPreset::ReplayOrbit => replay_orbit(h, start_tick, end_tick),
        CameraPreset::TrickFollow => trick_follow(h, start_tick, end_tick, tick_rate),
    }
}

fn orbit_kill(h: &HighlightCandidate, start_tick: f64, end_tick: f64, tick_rate: f64) -> CameraPath {
    let killer = h.killer_pos.unwrap_or_default();
    let victim = h.victim_pos.unwrap_or(Vec3::new(200.0, 0.0, 0.0));
    let midpoint = scale(add(killer, victim), 0.5);
    let radius = (distance(killer, victim) * 0.8).max(200.0);
    let height = midpoint.y + 80.0;
    let start = start_angle_behind(killer, midpoint);
    let arc = ORBIT_MAX_ARC.min(((end_tick - start_tick) / tick_rate) * ORBIT_DEG_PER_SECOND) * (PI / 180.0);
    let keyframes = (0..=4)
        .map(|i| {
            let t = f64::from(i) / 4.0;
            let angle = start + t * arc;
            let pos = Vec3::new(
                midpoint.x + js_cos(angle) * radius,
                height,
                midpoint.z + js_sin(angle) * radius,
            );
            keyframe(lerp_tick(start_tick, end_tick, t), pos, midpoint)
        })
        .collect();
    CameraPath::Freefly {
        keyframes,
        interpolation: Interpolation::CatmullRom,
    }
}

fn rocket_jump_hero(h: &HighlightCandidate, start_tick: f64, end_tick: f64) -> CameraPath {
    let killer = h.killer_pos.unwrap_or(Vec3::new(0.0, 100.0, 0.0));
    let apex = Vec3::new(killer.x, killer.y + 150.0, killer.z);
    let (forward, right) = jump_frame(h);
    let at = |base: Vec3, ahead: f64, side: f64, up: f64| {
        Vec3::new(
            base.x + forward.x * ahead + right.x * side,
            base.y + up,
            base.z + forward.z * ahead + right.z * side,
        )
    };
    let keyframes = vec![
        keyframe(start_tick, at(killer, 150.0, 0.0, -100.0), killer),
        keyframe(
            lerp_tick(start_tick, end_tick, 0.5),
            at(killer, 0.0, 200.0, 60.0),
            Vec3::new(killer.x, killer.y + 75.0, killer.z),
        ),
        keyframe(end_tick, at(apex, -120.0, 60.0, 80.0), apex),
    ];
    CameraPath::Freefly {
        keyframes,
        interpolation: Interpolation::CatmullRom,
    }
}

fn dramatic_zoom(h: &HighlightCandidate, start_tick: f64, end_tick: f64) -> CameraPath {
    let killer = h.killer_pos.unwrap_or_default();
    let victim = h.victim_pos.unwrap_or(Vec3::new(400.0, 0.0, 0.0));
    let midpoint = scale(add(killer, victim), 0.5);
    let direction = normalize(sub(victim, killer));
    let perp = normalize(cross(direction, Vec3::new(0.0, 1.0, 0.0)));
    let wide = Vec3::new(
        midpoint.x + perp.x * 400.0,
        midpoint.y + 200.0,
        midpoint.z + perp.z * 400.0,
    );
    let tight = Vec3::new(victim.x + perp.x * 80.0, victim.y + 40.0, victim.z + perp.z * 80.0);
    let keyframes = vec![
        CameraKeyframe {
            tick: start_tick,
            position: wide,
            look_at: midpoint,
            fov: Some(90.0),
        },
        CameraKeyframe {
            tick: lerp_tick(start_tick, end_tick, 0.7),
            position: lerp(wide, tight, 0.7),
            look_at: victim,
            fov: Some(60.0),
        },
        CameraKeyframe {
            tick: end_tick,
            position: tight,
            look_at: victim,
            fov: Some(50.0),
        },
    ];
    CameraPath::Freefly {
        keyframes,
        interpolation: Interpolation::Linear,
    }
}

fn replay_orbit(h: &HighlightCandidate, start_tick: f64, end_tick: f64) -> CameraPath {
    let killer = h.killer_pos.unwrap_or_default();
    let victim = h.victim_pos;
    let dist = victim.map_or(0.0, |v| distance(killer, v));
    let center = match victim {
        None => killer,
        Some(v) if dist <= REPLAY_BOTH_WITHIN => scale(add(killer, v), 0.5),
        Some(v) => v,
    };
    let height = center.y + 60.0;
    let radius = if victim.is_some() && dist <= REPLAY_BOTH_WITHIN {
        (dist * 0.6).max(250.0)
    } else {
        250.0
    };
    let start = start_angle_behind(killer, center);
    let keyframes = (0..=8)
        .map(|i| {
            let t = f64::from(i) / 8.0;
            let angle = start + t * 270.0 * (PI / 180.0);
            let pos = Vec3::new(
                center.x + js_cos(angle) * radius,
                height,
                center.z + js_sin(angle) * radius,
            );
            keyframe(lerp_tick(start_tick, end_tick, t), pos, center)
        })
        .collect();
    CameraPath::Freefly {
        keyframes,
        interpolation: Interpolation::CatmullRom,
    }
}

/// `_trickFollow`: trail a movement run along its recorded route.
fn trick_follow(h: &HighlightCandidate, start_tick: f64, end_tick: f64, tick_rate: f64) -> CameraPath {
    let route: Vec<_> = h
        .path
        .iter()
        .flatten()
        .copied()
        .filter(|p| p.tick as f64 >= start_tick && p.tick as f64 <= end_tick)
        .collect();
    let fallback = jump_frame(h).0;
    let trail = |i: usize| {
        let p = route[i];
        let from = route[i.saturating_sub(2)];
        let to = route[(i + 2).min(route.len() - 1)];
        let (dx, dz) = (to.x - from.x, to.z - from.z);
        let length = js_hypot(&[dx, dz]);
        let forward = if length > 1.0 {
            Vec3::new(dx / length, 0.0, dz / length)
        } else {
            fallback
        };
        let right = Vec3::new(forward.z, 0.0, -forward.x);
        keyframe(
            p.tick as f64,
            Vec3::new(
                p.x - forward.x * TRAIL_BACK + right.x * TRAIL_SIDE,
                p.y + TRAIL_UP,
                p.z - forward.z * TRAIL_BACK + right.z * TRAIL_SIDE,
            ),
            Vec3::new(p.x + forward.x * 60.0, p.y + 20.0, p.z + forward.z * 60.0),
        )
    };

    if route.len() < 2 {
        let at = h.killer_pos.unwrap_or_default();
        let position = Vec3::new(
            at.x - fallback.x * TRAIL_BACK,
            at.y + TRAIL_UP,
            at.z - fallback.z * TRAIL_BACK,
        );
        return CameraPath::Freefly {
            keyframes: vec![keyframe(start_tick, position, at), keyframe(end_tick, position, at)],
            interpolation: Interpolation::Linear,
        };
    }

    let mut keyframes: Vec<CameraKeyframe> = Vec::new();
    let spacing = TRAIL_KEYFRAME_SECONDS * tick_rate;
    for i in 0..route.len() {
        let take = keyframes
            .last()
            .is_none_or(|last| route[i].tick as f64 - last.tick >= spacing || i == route.len() - 1);
        if take {
            keyframes.push(trail(i));
        }
    }
    if keyframes[0].tick > start_tick {
        let first = CameraKeyframe {
            tick: start_tick,
            ..keyframes[0]
        };
        keyframes.insert(0, first);
    }
    let last = keyframes[keyframes.len() - 1];
    if last.tick < end_tick {
        keyframes.push(CameraKeyframe { tick: end_tick, ..last });
    }
    CameraPath::Freefly {
        keyframes,
        interpolation: Interpolation::CatmullRom,
    }
}

/// The orbit angle that puts the camera on the killer's side of `center`.
fn start_angle_behind(killer: Vec3, center: Vec3) -> f64 {
    let (dx, dz) = (killer.x - center.x, killer.z - center.z);
    if dx == 0.0 && dz == 0.0 { 0.0 } else { js_atan2(dz, dx) }
}

/// The jump's heading on x/z and its right: horizontal velocity, else the killer's view, else +z.
fn jump_frame(h: &HighlightCandidate) -> (Vec3, Vec3) {
    let mut forward = Vec3::new(0.0, 0.0, 1.0);
    if let Some(v) = h.killer_velocity
        && js_hypot(&[v.x, v.z]) > JUMP_HEADING_MIN_SPEED
    {
        forward = normalize(Vec3::new(v.x, 0.0, v.z));
    } else if let Some(yaw) = h.killer_yaw {
        forward = Vec3::new(js_sin(yaw), 0.0, js_cos(yaw));
    }
    (forward, Vec3::new(forward.z, 0.0, -forward.x))
}

/// `interpolatePath`.
#[must_use]
pub fn interpolate_path(keyframes: &[CameraKeyframe], tick: f64, interpolation: Interpolation) -> CameraPose {
    let pose = |kf: &CameraKeyframe| CameraPose {
        position: kf.position,
        look_at: kf.look_at,
        fov: kf.fov.unwrap_or(90.0),
    };
    let (Some(first), Some(last)) = (keyframes.first(), keyframes.last()) else {
        return CameraPose {
            position: Vec3::default(),
            look_at: Vec3::default(),
            fov: 90.0,
        };
    };
    if tick <= first.tick {
        return pose(first);
    }
    if tick >= last.tick {
        return pose(last);
    }
    let (mut lo, mut hi) = (0, keyframes.len() - 1);
    while hi - lo > 1 {
        let mid = (lo + hi) >> 1;
        if keyframes[mid].tick <= tick {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let (a, b) = (keyframes[lo], keyframes[hi]);
    let span = b.tick - a.tick;
    let t = if span > 0.0 { (tick - a.tick) / span } else { 0.0 };
    let (fov_a, fov_b) = (a.fov.unwrap_or(90.0), b.fov.unwrap_or(90.0));
    let position = match interpolation {
        Interpolation::CatmullRom => catmull_rom(
            keyframes[lo.saturating_sub(1)].position,
            a.position,
            b.position,
            keyframes[(hi + 1).min(keyframes.len() - 1)].position,
            t,
        ),
        Interpolation::Linear => lerp(a.position, b.position, t),
    };
    CameraPose {
        position,
        look_at: lerp(a.look_at, b.look_at, t),
        fov: fov_a + (fov_b - fov_a) * t,
    }
}
