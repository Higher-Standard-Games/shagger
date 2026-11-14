//! SHAG `src/engine/network/client/SnapshotBuffer.ts`: the time-based
//! interpolation buffer remote entities are drawn from.
//!
//! Snapshots are stamped when they arrive, and a frame renders
//! `interp_delay` behind the clock, between the two snapshots around that
//! time; past the newest one an entity is dead-reckoned for at most
//! [`MAX_EXTRAPOLATION_SEC`]. The delay follows the arrival jitter.
//!
//! The browser reads `performance.now()` and the `cl_interp` and
//! `cl_timenudge` cvars itself; here the caller passes the clock (ms) to
//! each call that reads it and sets the two cvars as fields.

use std::collections::{HashMap, VecDeque};

/// One entity as the buffer interpolates it for a frame, in Babylon's
/// space (y up) with the yaw Babylon turns the body by.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InterpolatedEntity {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    /// View pitch in radians, as the player aims.
    pub pitch: Option<f64>,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub grounded: bool,
    pub crouching: bool,
    pub sliding: bool,
    pub wall_jumping: bool,
    pub aiming: Option<bool>,
    pub crouch_view_offset: Option<f64>,
    /// Strafe keys: -1 left, 0 none, 1 right.
    pub strafe_input: Option<i8>,
    /// Forward keys: -1 back, 0 none, 1 forward.
    pub forward_input: Option<i8>,
    pub slide_phase: Option<f64>,
    pub ledge_grab_phase: Option<f64>,
    /// Held powerups as a bitmask over the game's `POWERUP_KINDS`.
    pub powerup_mask: Option<u32>,
    /// How deep in liquid: 0 dry, 1 wading, 2 swimming, 3 submerged.
    pub water_level: Option<u8>,
    pub on_ladder: Option<bool>,
}

/// A single entity's state in one snapshot: the same fields as what the
/// buffer gives back.
pub type EntitySnapshot = InterpolatedEntity;

/// A timestamped snapshot for the entire frame.
#[derive(Debug, Clone)]
pub struct BufferedSnapshot {
    /// Local reception time (ms).
    pub receive_time: f64,
    pub tick: u32,
    pub entities: HashMap<String, EntitySnapshot>,
    /// The server's timestamp in local time through clock sync.
    pub server_local_time: Option<f64>,
}

/// Pitch stays within ±90°, so it never wraps; either end missing holds the other.
pub fn lerp_pitch(a: Option<f64>, b: Option<f64>, t: f64) -> Option<f64> {
    lerp_optional(a, b, t)
}

/// A value either end may lack: interpolated when both have it, else the one that does.
pub fn lerp_optional(a: Option<f64>, b: Option<f64>, t: f64) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + (b - a) * t),
        _ => b.or(a),
    }
}

/// Server gravity constant (must match server physics).
const GRAVITY: f64 = -800.0;

/// How far past the newest snapshot a remote entity may be dead-reckoned:
/// one snapshot interval at 30 Hz plus a little margin. Past that a body
/// freezes where it was last seen.
pub const MAX_EXTRAPOLATION_SEC: f64 = 0.06;

/// Past this gap between a server timestamp and its arrival, the clock sync is not trusted.
const MAX_CLOCK_DRIFT_MS: f64 = 500.0;

/// Safe starting delay for 60 Hz snapshots (about 3 intervals) until jitter is measured.
pub const DEFAULT_INTERP_DELAY_MS: f64 = 50.0;

/// Inter-snapshot jump beyond this is a teleport: snap instead of streaking across the map.
const TELEPORT_SNAP_DIST: f64 = 256.0;

const MAX_BUFFER_SIZE: usize = 32;
const MAX_JITTER_SAMPLES: usize = 20;

#[derive(Debug, Clone, Copy)]
struct Frame {
    render_time: f64,
    before: Option<usize>,
    after: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct SnapshotBuffer {
    /// Oldest first; full ones drop from the front (`shift`).
    buffer: VecDeque<BufferedSnapshot>,
    /// Base interpolation delay in ms (auto-adjusts to jitter).
    interp_delay: f64,
    /// `cl_interp`: a manual delay in ms, 0 for automatic.
    pub cl_interp: f64,
    /// `cl_timenudge`: render time nudge in ms.
    pub cl_timenudge: f64,
    transit_samples: VecDeque<f64>,
    transit: f64,
    last_receive_time: f64,
    jitter_samples: VecDeque<f64>,
    frame: Option<Frame>,
    /// While the match is paused: the newest snapshot is where everything stands.
    held: bool,
    last_tick: Option<u32>,
}

impl Default for SnapshotBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotBuffer {
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::new(),
            interp_delay: DEFAULT_INTERP_DELAY_MS,
            cl_interp: 0.0,
            cl_timenudge: 0.0,
            transit_samples: VecDeque::new(),
            transit: 0.0,
            last_receive_time: 0.0,
            jitter_samples: VecDeque::new(),
            frame: None,
            held: false,
            last_tick: None,
        }
    }

    /// `hold`: holds the render time at the newest snapshot while the match
    /// is paused, so nothing is extrapolated past where the server stopped
    /// it. On release the pause's gap is not taken for network jitter.
    pub fn hold(&mut self, held: bool) {
        if held == self.held {
            return;
        }
        self.held = held;
        if !held {
            self.last_receive_time = 0.0;
        }
        self.frame = None;
    }

    /// Whether [`Self::hold`] holds the render time.
    #[must_use]
    pub fn is_held(&self) -> bool {
        self.held
    }

    /// Pushes a snapshot that arrived at `now`. A tick not newer than the
    /// newest one buffered is rejected; returns whether it was accepted.
    pub fn push(
        &mut self,
        now: f64,
        tick: u32,
        entities: HashMap<String, EntitySnapshot>,
        server_local_time: Option<f64>,
    ) -> bool {
        if self.last_tick.is_some_and(|last| tick <= last) {
            return false;
        }
        self.last_tick = Some(tick);

        if self.last_receive_time > 0.0 {
            let interval = now - self.last_receive_time;
            self.jitter_samples.push_back(interval);
            if self.jitter_samples.len() > MAX_JITTER_SAMPLES {
                self.jitter_samples.pop_front();
            }
            self.update_interp_delay();
        }
        self.last_receive_time = now;

        if let Some(server) = server_local_time
            && (server - now).abs() < MAX_CLOCK_DRIFT_MS
        {
            self.transit_samples.push_back(now - server);
            if self.transit_samples.len() > MAX_JITTER_SAMPLES {
                self.transit_samples.pop_front();
            }
            self.transit = self.transit_samples.iter().copied().fold(f64::INFINITY, f64::min);
        }

        self.buffer.push_back(BufferedSnapshot {
            receive_time: now,
            tick,
            entities,
            server_local_time,
        });
        while self.buffer.len() > MAX_BUFFER_SIZE {
            self.buffer.pop_front();
        }
        // Indices into the buffer moved.
        self.frame = None;
        true
    }

    /// Drops everything tied to the previous connection. Call when a
    /// session starts, not when a player respawns.
    pub fn reset(&mut self) {
        let (cl_interp, cl_timenudge) = (self.cl_interp, self.cl_timenudge);
        *self = Self::new();
        self.cl_interp = cl_interp;
        self.cl_timenudge = cl_timenudge;
    }

    /// Samples an entity at the render time for `now`; `None` when it is in
    /// no buffered snapshot.
    pub fn sample(&self, now: f64, entity_id: &str, cl_interp_override: Option<f64>) -> Option<InterpolatedEntity> {
        if self.buffer.is_empty() {
            return None;
        }
        let render_time = self.render_time(now, cl_interp_override);
        let (before, after) = self.bracket(render_time);
        self.sample_between(entity_id, render_time, before, after)
    }

    /// Starts sampling a frame: the render time and the two snapshots
    /// around it, found once for every entity [`Self::sample_in_frame`] is
    /// asked for.
    pub fn begin_frame(&mut self, now: f64) {
        if self.buffer.is_empty() {
            self.frame = None;
            return;
        }
        let render_time = self.render_time(now, None);
        let (before, after) = self.bracket(render_time);
        self.frame = Some(Frame {
            render_time,
            before,
            after,
        });
    }

    /// Samples an entity in the frame [`Self::begin_frame`] started.
    pub fn sample_in_frame(&self, entity_id: &str) -> Option<InterpolatedEntity> {
        let frame = self.frame?;
        self.sample_between(entity_id, frame.render_time, frame.before, frame.after)
    }

    fn render_time(&self, now: f64, cl_interp_override: Option<f64>) -> f64 {
        let delay = cl_interp_override.unwrap_or(if self.cl_interp > 0.0 {
            self.cl_interp
        } else {
            self.interp_delay
        });
        let time = now - delay + self.cl_timenudge;
        match self.buffer.back() {
            Some(newest) if self.held => time.min(self.snap_time(newest)),
            _ => time,
        }
    }

    fn bracket(&self, render_time: f64) -> (Option<usize>, Option<usize>) {
        let mut before = None;
        for (i, snapshot) in self.buffer.iter().enumerate() {
            if self.snap_time(snapshot) <= render_time {
                before = Some(i);
            } else {
                return (before, Some(i));
            }
        }
        (before, None)
    }

    fn sample_between(
        &self,
        entity_id: &str,
        render_time: f64,
        before: Option<usize>,
        after: Option<usize>,
    ) -> Option<InterpolatedEntity> {
        let before = before.map(|i| &self.buffer[i]);
        let after = after.map(|i| &self.buffer[i]);
        match (before, after) {
            (Some(before), Some(after)) => {
                let a = before.entities.get(entity_id);
                let b = after.entities.get(entity_id);
                match (a, b) {
                    (Some(a), Some(b)) => {
                        let range = self.snap_time(after) - self.snap_time(before);
                        let t = if range > 0.0 {
                            (render_time - self.snap_time(before)) / range
                        } else {
                            0.0
                        };
                        Some(lerp_entity(a, b, t))
                    }
                    (Some(a), None) => Some(*a),
                    (None, b) => b.copied(),
                }
            }
            (Some(before), None) => {
                let entity = before.entities.get(entity_id)?;
                let elapsed = ((render_time - self.snap_time(before)) / 1000.0).min(MAX_EXTRAPOLATION_SEC);
                let y = if entity.grounded {
                    entity.y
                } else {
                    entity.y + entity.vy * elapsed + 0.5 * GRAVITY * elapsed * elapsed
                };
                Some(InterpolatedEntity {
                    x: entity.x + entity.vx * elapsed,
                    y,
                    z: entity.z + entity.vz * elapsed,
                    vy: if entity.grounded {
                        entity.vy
                    } else {
                        entity.vy + GRAVITY * elapsed
                    },
                    // Not carried over by extrapolation.
                    wall_jumping: false,
                    ..*entity
                })
            }
            (None, Some(after)) => after.entities.get(entity_id).copied(),
            (None, None) => None,
        }
    }

    /// All entity ids in the most recent snapshot, in no order, without
    /// collecting them.
    pub fn latest_entity_ids(&self) -> impl Iterator<Item = &str> {
        self.buffer
            .back()
            .into_iter()
            .flat_map(|latest| latest.entities.keys().map(String::as_str))
    }

    /// The current interpolation delay in ms.
    pub fn interp_delay(&self) -> f64 {
        self.interp_delay
    }

    pub fn buffer_size(&self) -> usize {
        self.buffer.len()
    }

    /// Keeps only the latest snapshot as the baseline (a tab return in the
    /// browser; a window regaining focus here).
    pub fn flush(&mut self) {
        if self.buffer.len() > 1 {
            self.buffer.drain(..self.buffer.len() - 1);
        }
        self.jitter_samples.clear();
        self.last_receive_time = 0.0;
        self.frame = None;
    }

    /// When a snapshot counts as having arrived: its server time shifted by
    /// the network's transit, unless that drifted past
    /// [`MAX_CLOCK_DRIFT_MS`] from when it arrived.
    fn snap_time(&self, snap: &BufferedSnapshot) -> f64 {
        if let Some(server) = snap.server_local_time
            && (server - snap.receive_time).abs() < MAX_CLOCK_DRIFT_MS
        {
            return server + self.transit;
        }
        snap.receive_time
    }

    /// delay = average interval + one standard deviation, within 16 to 150 ms.
    fn update_interp_delay(&mut self) {
        let n = self.jitter_samples.len();
        if n < 3 {
            return;
        }
        let avg = self.jitter_samples.iter().fold(0.0, |s, v| s + v) / n as f64;
        let variance = self.jitter_samples.iter().fold(0.0, |s, v| s + (v - avg).powi(2)) / n as f64;
        let stddev = variance.sqrt();
        self.interp_delay = (avg + stddev).clamp(16.0, 150.0);
    }
}

fn lerp_entity(a: &EntitySnapshot, b: &EntitySnapshot, t: f64) -> InterpolatedEntity {
    let t = t.clamp(0.0, 1.0);
    let jump_sq = (b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2);
    if jump_sq > TELEPORT_SNAP_DIST * TELEPORT_SNAP_DIST {
        return *b;
    }
    let mut yaw_delta = b.yaw - a.yaw;
    while yaw_delta > std::f64::consts::PI {
        yaw_delta -= std::f64::consts::PI * 2.0;
    }
    while yaw_delta < -std::f64::consts::PI {
        yaw_delta += std::f64::consts::PI * 2.0;
    }
    let near = if t < 0.5 { a } else { b };
    InterpolatedEntity {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        z: a.z + (b.z - a.z) * t,
        yaw: a.yaw + yaw_delta * t,
        pitch: lerp_pitch(a.pitch, b.pitch, t),
        vx: a.vx + (b.vx - a.vx) * t,
        vy: a.vy + (b.vy - a.vy) * t,
        vz: a.vz + (b.vz - a.vz) * t,
        grounded: near.grounded,
        crouching: near.crouching,
        sliding: near.sliding,
        wall_jumping: near.wall_jumping,
        aiming: near.aiming,
        crouch_view_offset: lerp_optional(a.crouch_view_offset, b.crouch_view_offset, t),
        strafe_input: near.strafe_input,
        forward_input: near.forward_input,
        slide_phase: lerp_optional(a.slide_phase, b.slide_phase, t),
        ledge_grab_phase: lerp_optional(a.ledge_grab_phase, b.ledge_grab_phase, t),
        // The newer mask: a quad expiring reads the moment the server says so.
        powerup_mask: b.powerup_mask.or(a.powerup_mask),
        water_level: near.water_level,
        on_ladder: near.on_ladder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(x: f64, y: f64, z: f64) -> EntitySnapshot {
        EntitySnapshot {
            x,
            y,
            z,
            grounded: true,
            ..Default::default()
        }
    }

    fn one(id: &str, e: EntitySnapshot) -> HashMap<String, EntitySnapshot> {
        HashMap::from([(id.to_string(), e)])
    }

    #[test]
    fn held_for_a_pause_stops_at_the_newest_snapshot() {
        // SnapshotBuffer.test.ts "SnapshotBuffer held for a pause".
        let mut b = SnapshotBuffer::new();
        let moving = |x: f64| EntitySnapshot {
            vx: 600.0,
            ..entity(x, 0.0, 0.0)
        };
        let mut now = 0.0;
        for i in 0..6_u32 {
            now = 1000.0 + f64::from(i) * 16.0;
            b.push(now, i + 1, one("a", moving(f64::from(i) * 10.0)), None);
        }
        let delay = b.interp_delay();
        b.hold(true);
        now += 5000.0;
        assert_eq!(b.sample(now, "a", None).unwrap().x, 50.0);
        b.hold(false);
        now += 16.0;
        b.push(now, 7, one("a", moving(60.0)), None);
        assert_eq!(b.interp_delay(), delay);
    }

    #[test]
    fn empty_and_single() {
        let mut b = SnapshotBuffer::new();
        assert!(b.sample(0.0, "p", None).is_none());
        assert!(b.latest_entity_ids().next().is_none());
        b.push(1000.0, 1, one("e1", entity(100.0, 50.0, -200.0)), None);
        let s = b.sample(1000.0, "e1", None).unwrap();
        assert_eq!((s.x, s.y, s.z), (100.0, 50.0, -200.0));
        assert!(b.sample(1000.0, "nope", None).is_none());
    }

    #[test]
    fn interpolates_and_wraps_yaw() {
        let mut b = SnapshotBuffer::new();
        let mut a = entity(0.0, 0.0, 0.0);
        a.yaw = 3.0;
        let mut c = entity(100.0, 0.0, 0.0);
        c.yaw = -3.0;
        b.push(1000.0, 1, one("e", a), None);
        b.push(1100.0, 2, one("e", c), None);
        // Render at 1050 with an explicit 0 delay: halfway.
        let s = b.sample(1050.0, "e", Some(0.0)).unwrap();
        assert_eq!(s.x, 50.0);
        // Shortest path through π, not back through 0.
        assert!((s.yaw - (3.0 + (2.0 * std::f64::consts::PI - 6.0) / 2.0)).abs() < 1e-12);
    }

    #[test]
    fn extrapolates_with_gravity_and_cap() {
        let mut b = SnapshotBuffer::new();
        let mut e = entity(0.0, 100.0, 0.0);
        e.vx = 100.0;
        e.vy = 50.0;
        e.grounded = false;
        e.wall_jumping = true;
        b.push(1000.0, 1, one("e", e), None);
        let s = b.sample(1030.0, "e", Some(0.0)).unwrap();
        assert!((s.x - 3.0).abs() < 1e-12);
        assert!((s.y - (100.0 + 50.0 * 0.03 - 400.0 * 0.03 * 0.03)).abs() < 1e-12);
        assert!(!s.wall_jumping);
        // Capped at 60 ms.
        let s = b.sample(2000.0, "e", Some(0.0)).unwrap();
        assert!((s.x - 6.0).abs() < 1e-12);
    }

    #[test]
    fn teleport_snaps() {
        let mut b = SnapshotBuffer::new();
        b.push(1000.0, 1, one("e", entity(0.0, 0.0, 0.0)), None);
        b.push(1100.0, 2, one("e", entity(1000.0, 0.0, 0.0)), None);
        assert_eq!(b.sample(1010.0, "e", Some(0.0)).unwrap().x, 1000.0);
    }

    #[test]
    fn rejects_stale_ticks_until_reset() {
        let mut b = SnapshotBuffer::new();
        assert!(b.push(1000.0, 5, one("e", entity(0.0, 0.0, 0.0)), None));
        assert!(!b.push(1010.0, 5, one("e", entity(0.0, 0.0, 0.0)), None));
        assert!(!b.push(1020.0, 4, one("e", entity(0.0, 0.0, 0.0)), None));
        b.reset();
        assert!(b.push(1030.0, 1, one("e", entity(0.0, 0.0, 0.0)), None));
        assert_eq!(b.interp_delay(), DEFAULT_INTERP_DELAY_MS);
    }

    #[test]
    fn evicts_and_flushes() {
        let mut b = SnapshotBuffer::new();
        for i in 0..40 {
            b.push(1000.0 + f64::from(i) * 16.0, i, one("e", entity(0.0, 0.0, 0.0)), None);
        }
        assert_eq!(b.buffer_size(), 32);
        // Steady 16 ms arrivals: the delay floors at 16.
        assert_eq!(b.interp_delay(), 16.0);
        b.flush();
        assert_eq!(b.buffer_size(), 1);
    }

    #[test]
    fn frame_sampling_matches_sample() {
        let mut b = SnapshotBuffer::new();
        b.push(1000.0, 1, one("e", entity(0.0, 0.0, 0.0)), None);
        b.push(1100.0, 2, one("e", entity(100.0, 0.0, 0.0)), None);
        for now in [900.0, 1080.0, 1130.0, 1300.0] {
            b.begin_frame(now);
            assert_eq!(b.sample_in_frame("e"), b.sample(now, "e", None));
        }
    }

    #[test]
    fn server_time_draws_behind_the_newest_arrival() {
        // Snapshots sent every 33 ms that take 80 ms to arrive.
        let mut b = SnapshotBuffer::new();
        for i in 0..10u32 {
            let sent = 1000.0 + f64::from(i) * 33.0;
            b.push(sent + 80.0, i + 1, one("e", entity(f64::from(i), 0.0, 0.0)), Some(sent));
        }
        let now = 1000.0 + 9.0 * 33.0 + 80.0;
        let s = b.sample(now, "e", None).unwrap();
        assert!(s.x < 9.0, "{}", s.x);
    }

    #[test]
    fn pitch_lerp() {
        assert_eq!(lerp_pitch(Some(0.0), Some(1.0), 0.25), Some(0.25));
        assert_eq!(lerp_pitch(None, Some(1.0), 0.25), Some(1.0));
        assert_eq!(lerp_pitch(Some(0.5), None, 0.25), Some(0.5));
        assert_eq!(lerp_pitch(None, None, 0.25), None);
    }
}
