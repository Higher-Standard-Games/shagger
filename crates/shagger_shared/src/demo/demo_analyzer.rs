//! Port of SHAG `src/shared/demo/DemoAnalyzer.ts`: finds the highlights in a
//! demo (frags, multi-kills, airborne frags, movement runs) and scores them
//! for the highlight reel.
//!
//! The TypeScript reads its game's events by name (`death`, `award`,
//! `combat_state`); here the game's event type says which of those an event
//! is through [`HighlightEvent`]. Every map the TypeScript iterates keeps its
//! insertion order, so the highlights come out in the same order.

use std::collections::HashMap;

use crate::demo::demo_reader::{DemoFrame, DemoHeader};
use crate::js::{js_cos, js_hypot, js_round, js_sin};
use crate::net::game_state::{OrderedMap, Vec3};

/// `HighlightType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightType {
    Frag,
    MultiKill,
    AirborneFrag,
    StreakKill,
    FirstBlood,
    Movement,
}

impl HighlightType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frag => "frag",
            Self::MultiKill => "multi_kill",
            Self::AirborneFrag => "airborne_frag",
            Self::StreakKill => "streak_kill",
            Self::FirstBlood => "first_blood",
            Self::Movement => "movement",
        }
    }
}

/// `HighlightTag`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightTag {
    Airborne,
    RocketJump,
    LowHp,
    CloseRange,
    LongRange,
    MultiKill,
    Revenge,
    FirstBlood,
    OnFire,
    Raging,
    Unstoppable,
    HighSpeed,
    TrickJump,
    HeightGain,
    WallJump,
    Slide,
    BunnyHop,
}

impl HighlightTag {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Airborne => "airborne",
            Self::RocketJump => "rocket_jump",
            Self::LowHp => "low_hp",
            Self::CloseRange => "close_range",
            Self::LongRange => "long_range",
            Self::MultiKill => "multi_kill",
            Self::Revenge => "revenge",
            Self::FirstBlood => "first_blood",
            Self::OnFire => "on_fire",
            Self::Raging => "raging",
            Self::Unstoppable => "unstoppable",
            Self::HighSpeed => "high_speed",
            Self::TrickJump => "trick_jump",
            Self::HeightGain => "height_gain",
            Self::WallJump => "wall_jump",
            Self::Slide => "slide",
            Self::BunnyHop => "bunny_hop",
        }
    }

    /// `TAG_MULTIPLIERS`.
    #[must_use]
    pub const fn multiplier(self) -> f64 {
        match self {
            Self::Airborne | Self::FirstBlood => 1.5,
            Self::RocketJump => 1.8,
            Self::LowHp | Self::TrickJump | Self::WallJump => 1.3,
            Self::CloseRange | Self::Revenge | Self::Slide => 1.1,
            Self::LongRange | Self::OnFire | Self::HighSpeed | Self::HeightGain | Self::BunnyHop => 1.2,
            // Handled separately through the kill count.
            Self::MultiKill => 1.0,
            Self::Raging => 1.4,
            Self::Unstoppable => 1.6,
        }
    }
}

/// A point of a movement highlight's route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathPoint {
    pub tick: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// `HighlightCandidate`.
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightCandidate {
    pub start_tick: i64,
    pub peak_tick: i64,
    pub end_tick: i64,
    pub score: f64,
    pub kind: HighlightType,
    /// Entity id of the primary actor (the killer).
    pub primary_player: String,
    pub primary_player_name: String,
    pub victim_player: Option<String>,
    pub victim_player_name: Option<String>,
    pub weapon_id: Option<String>,
    pub kill_count: Option<u32>,
    pub tags: Vec<HighlightTag>,
    pub killer_pos: Option<Vec3>,
    pub victim_pos: Option<Vec3>,
    pub killer_velocity: Option<Vec3>,
    /// Where the killer was looking at the frag: yaw in radians, forward = (sin, cos) on x/z.
    pub killer_yaw: Option<f64>,
    /// The victim was looking at the killer when the frag landed.
    pub victim_faced_killer: Option<bool>,
    /// A movement highlight's route, sampled about every 0.1 s.
    pub path: Option<Vec<PathPoint>>,
}

impl HighlightCandidate {
    #[must_use]
    pub fn has(&self, tag: HighlightTag) -> bool {
        self.tags.contains(&tag)
    }
}

/// `DemoAnalysis`.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoAnalysis {
    pub highlights: Vec<HighlightCandidate>,
    pub total_ticks: i64,
    pub total_duration_ms: f64,
    /// entity id → display name.
    pub player_names: OrderedMap<String>,
    /// entity id → frags over the whole demo (suicides and world deaths left out).
    pub frags: Vec<(String, u32)>,
}

/// What the analyzer reads of a game event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyzerEvent<'a> {
    /// `combat_state`.
    CombatState {
        entity_id: &'a str,
        health: f64,
    },
    /// `award`.
    Award {
        player_id: &'a str,
        award_type: &'a str,
    },
    /// `death`, with the victim's position.
    Death {
        entity_id: &'a str,
        killer_id: &'a str,
        weapon_id: &'a str,
        at: Vec3,
    },
    Other,
}

/// A game's snapshot event, as the analyzer sees it.
pub trait HighlightEvent {
    fn analyzer_event(&self) -> AnalyzerEvent<'_>;
}

/// How far off the line to the killer the victim may look and still have seen them.
fn facing_cone() -> f64 {
    js_cos(std::f64::consts::PI / 4.0)
}

const MULTI_KILL_WINDOW: i64 = 200;
const LEADIN_SECONDS: f64 = 3.0;
const FOLLOWTHROUGH_SECONDS: f64 = 2.0;
const LONG_RANGE_THRESHOLD: f64 = 1500.0;
const CLOSE_RANGE_THRESHOLD: f64 = 150.0;
/// Rising faster than a plain jump (270) or a wall jump (420) can.
const ROCKET_JUMP_VEL_Y: f64 = 450.0;
/// Feet at least this far above the ground make a frag airborne.
pub const AIRBORNE_MIN_CLEARANCE: f64 = 32.0;
const HIGH_SPEED_THRESHOLD: f64 = 600.0;
const LOW_HP_THRESHOLD: f64 = 30.0;

const MOVE_MIN_SPEED: f64 = 450.0;
const MOVE_GRACE_SECONDS: f64 = 0.35;
const MOVE_MIN_SECONDS: f64 = 1.2;
const MOVE_MAX_SECONDS: f64 = 8.0;
const MOVE_PAD_SECONDS: f64 = 0.5;
const MOVE_SAMPLE_SECONDS: f64 = 0.1;
const BUNNY_HOP_MIN: u32 = 3;
const MOVE_HEIGHT_GAIN: f64 = 96.0;
const MOVE_HIGH_SPEED: f64 = 700.0;

const BASE_SCORE_FRAG: f64 = 10.0;
const BASE_SCORE_MOVEMENT: f64 = 5.0;

#[derive(Debug, Clone, Copy)]
struct RunFrame {
    pos: Vec3,
    vel: Vec3,
    yaw: Option<f64>,
}

/// A stretch of fast movement in the air or on a slide, while it lasts.
#[derive(Debug, Clone)]
struct MovementRun {
    start_tick: i64,
    last_active_tick: i64,
    start_y: f64,
    max_y: f64,
    max_y_tick: i64,
    max_speed: f64,
    max_speed_tick: i64,
    wall_jump_ticks: Vec<i64>,
    slides: u32,
    hops: u32,
    was_sliding: bool,
    was_grounded: bool,
    was_charging: bool,
    path: Vec<PathPoint>,
    frames: HashMap<i64, RunFrame>,
}

#[derive(Debug, Clone)]
struct PlayerTracker {
    last_kill_tick: i64,
    last_death_tick: i64,
    last_killed_by: String,
    last_grounded_y: Option<f64>,
    current_health: f64,
    frags: u32,
}

impl Default for PlayerTracker {
    fn default() -> Self {
        Self {
            last_kill_tick: -1,
            last_death_tick: -1,
            last_killed_by: String::new(),
            last_grounded_y: None,
            current_health: 100.0,
            frags: 0,
        }
    }
}

/// Where the ground is, when the caller has the map: how far the feet of a
/// player at `origin` are above the ground below them, or `None`.
pub type GroundClearance<'f> = dyn Fn(Vec3) -> Option<f64> + 'f;

/// `DemoAnalyzer`.
#[derive(Default)]
pub struct DemoAnalyzer<'a> {
    ground_clearance: Option<&'a GroundClearance<'a>>,
}

/// Runs in the order the TypeScript `Map` keeps them: a run that ends and
/// starts again goes to the back.
type Runs = Vec<(String, MovementRun)>;

fn run_index(runs: &Runs, id: &str) -> Option<usize> {
    runs.iter().position(|(k, _)| k == id)
}

impl<'a> DemoAnalyzer<'a> {
    /// `groundClearance` measures a killer's height above the ground; without
    /// it, height is taken against where the killer last stood.
    #[must_use]
    pub fn new(ground_clearance: Option<&'a GroundClearance<'a>>) -> Self {
        Self { ground_clearance }
    }

    /// `analyze`.
    #[allow(clippy::too_many_lines)]
    pub fn analyze<E: HighlightEvent>(
        &self,
        frames: &[DemoFrame<E>],
        header: &DemoHeader,
        duration_ms: f64,
    ) -> DemoAnalysis {
        if frames.is_empty() {
            return DemoAnalysis {
                highlights: Vec::new(),
                total_ticks: 0,
                total_duration_ms: duration_ms,
                player_names: OrderedMap::default(),
                frags: Vec::new(),
            };
        }

        let tick_rate = if header.tick_rate > 0 {
            f64::from(header.tick_rate)
        } else {
            20.0
        };
        let leadin = js_round(LEADIN_SECONDS * tick_rate) as i64;
        let followthrough = js_round(FOLLOWTHROUGH_SECONDS * tick_rate) as i64;

        let last_tick = i64::from(frames[frames.len() - 1].tick);
        let first_tick = i64::from(frames[0].tick);
        let total_ticks = last_tick - first_tick + 1;

        let player_names = build_player_names(frames, header);

        let mut trackers: OrderedMap<PlayerTracker> = OrderedMap::default();
        let mut raw_frags: Vec<HighlightCandidate> = Vec::new();
        let mut last_yaw: HashMap<String, f64> = HashMap::new();
        let mut runs: Runs = Vec::new();
        let mut moves: Vec<HighlightCandidate> = Vec::new();

        for frame in frames {
            let tick = i64::from(frame.tick);
            let state = &frame.state;

            for ev in &state.events {
                if let AnalyzerEvent::CombatState { entity_id, health } = ev.analyzer_event()
                    && !entity_id.is_empty()
                {
                    trackers
                        .get_or_insert_with(entity_id, PlayerTracker::default)
                        .current_health = health;
                }
            }

            let mut awards: Vec<(String, Vec<&str>)> = Vec::new();
            for ev in &state.events {
                if let AnalyzerEvent::Award { player_id, award_type } = ev.analyzer_event() {
                    let entity_id = format!("player_{player_id}");
                    match awards.iter_mut().find(|(k, _)| *k == entity_id) {
                        Some((_, set)) => {
                            if !set.contains(&award_type) {
                                set.push(award_type);
                            }
                        }
                        None => awards.push((entity_id, vec![award_type])),
                    }
                }
            }

            for ev in &state.events {
                let AnalyzerEvent::Death {
                    entity_id: victim_id,
                    killer_id,
                    weapon_id,
                    at: victim_pos,
                } = ev.analyzer_event()
                else {
                    continue;
                };
                if killer_id.is_empty() || killer_id == victim_id {
                    continue;
                }
                trackers.get_or_insert_with(killer_id, PlayerTracker::default);
                trackers.get_or_insert_with(victim_id, PlayerTracker::default);

                let mut killer_pos = None;
                let mut killer_velocity = None;
                let mut killer_grounded = true;
                let mut killer_yaw = last_yaw.get(killer_id).copied();
                let mut killer_linear_y = 0.0;
                if let Some(killer) = state.entities.get(killer_id) {
                    if let Some(t) = killer.transform {
                        killer_pos = Some(t.position);
                        killer_yaw = Some(t.rotation[1]);
                    }
                    if let Some(v) = killer.velocity {
                        killer_grounded = v.grounded;
                        killer_linear_y = v.linear.y;
                        killer_velocity = Some(v.linear);
                    }
                }

                let mut victim_faced_killer = None;
                let victim_yaw = state
                    .entities
                    .get(victim_id)
                    .and_then(|e| e.transform)
                    .map(|t| t.rotation[1])
                    .or_else(|| last_yaw.get(victim_id).copied());
                if let (Some(kp), Some(yaw)) = (killer_pos, victim_yaw) {
                    let to_x = kp.x - victim_pos.x;
                    let to_z = kp.z - victim_pos.z;
                    let length = js_hypot(&[to_x, to_z]);
                    if length > 0.0 {
                        victim_faced_killer = Some((js_sin(yaw) * to_x + js_cos(yaw) * to_z) / length >= facing_cone());
                    }
                }

                let mut tags = Vec::new();
                if !killer_grounded
                    && let Some(kp) = killer_pos
                    && self.clearance(kp, trackers.get(killer_id).expect("tracked")) >= AIRBORNE_MIN_CLEARANCE
                {
                    tags.push(if killer_linear_y > ROCKET_JUMP_VEL_Y {
                        HighlightTag::RocketJump
                    } else {
                        HighlightTag::Airborne
                    });
                }
                if let Some(kp) = killer_pos {
                    let (dx, dy, dz) = (kp.x - victim_pos.x, kp.y - victim_pos.y, kp.z - victim_pos.z);
                    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                    if dist > LONG_RANGE_THRESHOLD {
                        tags.push(HighlightTag::LongRange);
                    } else if dist < CLOSE_RANGE_THRESHOLD {
                        tags.push(HighlightTag::CloseRange);
                    }
                }
                let killer = trackers.get(killer_id).expect("tracked");
                let victim = trackers.get(victim_id).expect("tracked");
                if killer.current_health > 0.0 && killer.current_health <= LOW_HP_THRESHOLD {
                    tags.push(HighlightTag::LowHp);
                }
                if killer.last_death_tick >= 0 && victim.last_kill_tick >= 0 && killer.last_killed_by == victim_id {
                    tags.push(HighlightTag::Revenge);
                }
                if let Some(v) = killer_velocity
                    && (v.x * v.x + v.z * v.z).sqrt() > HIGH_SPEED_THRESHOLD
                {
                    tags.push(HighlightTag::HighSpeed);
                }
                if let Some((_, set)) = awards.iter().find(|(k, _)| k == killer_id) {
                    for (name, tag) in [
                        ("first_blood", HighlightTag::FirstBlood),
                        ("on_fire", HighlightTag::OnFire),
                        ("raging", HighlightTag::Raging),
                        ("unstoppable", HighlightTag::Unstoppable),
                    ] {
                        if set.contains(&name) {
                            tags.push(tag);
                        }
                    }
                }

                let kind = if tags.contains(&HighlightTag::RocketJump) || tags.contains(&HighlightTag::Airborne) {
                    HighlightType::AirborneFrag
                } else if tags.contains(&HighlightTag::FirstBlood) {
                    HighlightType::FirstBlood
                } else {
                    HighlightType::Frag
                };
                raw_frags.push(HighlightCandidate {
                    start_tick: tick,
                    peak_tick: tick,
                    end_tick: tick,
                    score: 0.0,
                    kind,
                    primary_player: killer_id.to_owned(),
                    primary_player_name: player_names
                        .get(killer_id)
                        .cloned()
                        .unwrap_or_else(|| killer_id.to_owned()),
                    victim_player: Some(victim_id.to_owned()),
                    victim_player_name: Some(
                        player_names
                            .get(victim_id)
                            .cloned()
                            .unwrap_or_else(|| victim_id.to_owned()),
                    ),
                    weapon_id: (!weapon_id.is_empty()).then(|| weapon_id.to_owned()),
                    kill_count: None,
                    tags,
                    killer_pos,
                    victim_pos: Some(victim_pos),
                    killer_velocity,
                    killer_yaw,
                    victim_faced_killer,
                    path: None,
                });

                let killer = trackers.get_mut(killer_id).expect("tracked");
                killer.frags += 1;
                killer.last_kill_tick = tick;
                let victim = trackers.get_mut(victim_id).expect("tracked");
                victim.last_death_tick = tick;
                victim.last_killed_by = killer_id.to_owned();
            }

            for (entity_id, entity) in state.entities.iter() {
                if !entity_id.starts_with("player_") {
                    continue;
                }
                let yaw = entity.transform.map(|t| t.rotation[1]);
                if let Some(yaw) = yaw {
                    last_yaw.insert(entity_id.to_owned(), yaw);
                }
                let Some(velocity) = entity.velocity else { continue };
                let t = trackers.get_or_insert_with(entity_id, PlayerTracker::default);
                let position = entity.transform.map(|t| t.position);
                // The TypeScript also counts air frames and kill streaks, which nothing reads.
                if velocity.grounded
                    && let Some(p) = position
                {
                    t.last_grounded_y = Some(p.y);
                }
                if let Some(position) = position {
                    let movement = entity.movement_state;
                    let sliding = movement.is_some_and(|m| m.is_sliding);
                    let charging = movement.is_some_and(|m| m.is_wall_jumping);
                    let step = Step {
                        position,
                        vel: velocity.linear,
                        grounded: velocity.grounded,
                        sliding,
                        charging,
                        yaw,
                        tick,
                    };
                    if let Some(finished) = track_movement(&mut runs, entity_id, step, tick_rate) {
                        moves.push(finished);
                    }
                }
            }

            let mut i = 0;
            while i < runs.len() {
                if state.entities.get(&runs[i].0).is_some() {
                    i += 1;
                    continue;
                }
                let (entity_id, run) = runs.remove(i);
                if let Some(finished) = finish_run(&run, &entity_id, tick_rate) {
                    moves.push(finished);
                }
            }
        }

        for (entity_id, run) in &runs {
            if let Some(finished) = finish_run(run, entity_id, tick_rate) {
                moves.push(finished);
            }
        }

        let mut grouped = group_multi_kills(raw_frags);
        for c in &mut grouped {
            c.start_tick = first_tick.max(c.peak_tick - leadin);
            c.end_tick = last_tick.min(c.peak_tick + followthrough);
        }
        for c in &mut grouped {
            c.score = compute_score(c);
        }
        for m in &mut moves {
            m.start_tick = first_tick.max(m.start_tick);
            m.end_tick = last_tick.min(m.end_tick);
            m.score = compute_score(m);
            m.primary_player_name = player_names
                .get(&m.primary_player)
                .cloned()
                .unwrap_or_else(|| m.primary_player.clone());
        }

        let mut highlights = deduplicate_overlapping(&grouped);
        highlights.extend(deduplicate_overlapping(&moves));
        sort_by_score(&mut highlights);

        let frags = trackers
            .iter()
            .filter(|(_, t)| t.frags > 0)
            .map(|(id, t)| (id.to_owned(), t.frags))
            .collect();
        DemoAnalysis {
            highlights,
            total_ticks,
            total_duration_ms: duration_ms,
            player_names,
            frags,
        }
    }

    /// Feet height above the ground below, or above where the player last stood.
    fn clearance(&self, origin: Vec3, tracker: &PlayerTracker) -> f64 {
        if let Some(measured) = self.ground_clearance.and_then(|f| f(origin)) {
            return measured;
        }
        tracker.last_grounded_y.map_or(f64::INFINITY, |y| origin.y - y)
    }
}

/// Sorts by score, best first, keeping ties in order (`Array.prototype.sort` is stable).
fn sort_by_score(list: &mut [HighlightCandidate]) {
    list.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
}

fn build_player_names<E>(frames: &[DemoFrame<E>], header: &DemoHeader) -> OrderedMap<String> {
    let mut names = OrderedMap::default();
    for p in &header.players {
        names.insert(&format!("player_{}", p.player_id), p.name.clone());
    }
    for frame in frames {
        for (entity_id, _) in frame.state.entities.iter() {
            if entity_id.starts_with("player_") && names.get(entity_id).is_none() {
                let raw = &entity_id["player_".len()..];
                let name = frame
                    .state
                    .players
                    .get(raw)
                    .filter(|p| !p.name.is_empty())
                    .map_or_else(|| entity_id.to_owned(), |p| p.name.clone());
                names.insert(entity_id, name);
            }
        }
    }
    names
}

fn group_multi_kills(frags: Vec<HighlightCandidate>) -> Vec<HighlightCandidate> {
    let mut by_player: Vec<(String, Vec<HighlightCandidate>)> = Vec::new();
    for f in frags {
        match by_player.iter_mut().find(|(k, _)| *k == f.primary_player) {
            Some((_, list)) => list.push(f),
            None => by_player.push((f.primary_player.clone(), vec![f])),
        }
    }
    let mut result = Vec::new();
    for (_, mut player_frags) in by_player {
        player_frags.sort_by_key(|f| f.peak_tick);
        let mut i = 0;
        while i < player_frags.len() {
            let mut j = i + 1;
            while j < player_frags.len() && player_frags[j].peak_tick - player_frags[i].peak_tick <= MULTI_KILL_WINDOW {
                j += 1;
            }
            let group = &player_frags[i..j];
            if group.len() >= 2 {
                let first = &group[0];
                let last = &group[group.len() - 1];
                let mut tags: Vec<HighlightTag> = Vec::new();
                for g in group {
                    for &t in &g.tags {
                        if !tags.contains(&t) {
                            tags.push(t);
                        }
                    }
                }
                if !tags.contains(&HighlightTag::MultiKill) {
                    tags.push(HighlightTag::MultiKill);
                }
                result.push(HighlightCandidate {
                    start_tick: first.start_tick,
                    peak_tick: last.peak_tick,
                    end_tick: last.end_tick,
                    score: 0.0,
                    kind: HighlightType::MultiKill,
                    primary_player: first.primary_player.clone(),
                    primary_player_name: first.primary_player_name.clone(),
                    victim_player: last.victim_player.clone(),
                    victim_player_name: last.victim_player_name.clone(),
                    weapon_id: last.weapon_id.clone(),
                    kill_count: Some(group.len() as u32),
                    tags,
                    killer_pos: last.killer_pos,
                    victim_pos: last.victim_pos,
                    killer_velocity: last.killer_velocity,
                    killer_yaw: last.killer_yaw,
                    victim_faced_killer: None,
                    path: None,
                });
            } else {
                result.push(group[0].clone());
            }
            i = j;
        }
    }
    result
}

/// One player's movement in one frame.
#[derive(Debug, Clone, Copy)]
struct Step {
    position: Vec3,
    vel: Vec3,
    grounded: bool,
    sliding: bool,
    charging: bool,
    yaw: Option<f64>,
    tick: i64,
}

/// `_trackMovement`: returns the highlight when a run ends worth keeping.
fn track_movement(runs: &mut Runs, entity_id: &str, s: Step, tick_rate: f64) -> Option<HighlightCandidate> {
    let speed = js_hypot(&[s.vel.x, s.vel.z]);
    // A wall jump's charge holds the player still on the wall: it keeps the run alive
    let active = (speed >= MOVE_MIN_SPEED && (!s.grounded || s.sliding)) || s.charging;

    let mut index = run_index(runs, entity_id);
    let mut finished = None;
    if let Some(i) = index
        && (s.tick - runs[i].1.last_active_tick) as f64 > MOVE_GRACE_SECONDS * tick_rate
    {
        let (_, run) = runs.remove(i);
        finished = finish_run(&run, entity_id, tick_rate);
        index = None;
    }
    if index.is_none() && active {
        runs.push((
            entity_id.to_owned(),
            MovementRun {
                start_tick: s.tick,
                last_active_tick: s.tick,
                start_y: s.position.y,
                max_y: s.position.y,
                max_y_tick: s.tick,
                max_speed: 0.0,
                max_speed_tick: s.tick,
                wall_jump_ticks: Vec::new(),
                slides: 0,
                hops: 0,
                was_sliding: false,
                was_grounded: s.grounded,
                was_charging: false,
                path: Vec::new(),
                frames: HashMap::new(),
            },
        ));
        index = Some(runs.len() - 1);
    }
    if let Some(i) = index {
        let run = &mut runs[i].1;
        if active {
            run.last_active_tick = s.tick;
        }
        step_run(run, s, speed, tick_rate);
    }
    finished
}

fn step_run(run: &mut MovementRun, s: Step, speed: f64, tick_rate: f64) {
    if s.sliding && !run.was_sliding {
        run.slides += 1;
    }
    if s.charging && !run.was_charging {
        run.wall_jump_ticks.push(s.tick);
    }
    if !s.grounded && run.was_grounded {
        run.hops += 1;
    }
    run.was_sliding = s.sliding;
    run.was_charging = s.charging;
    run.was_grounded = s.grounded;
    if s.position.y > run.max_y {
        run.max_y = s.position.y;
        run.max_y_tick = s.tick;
    }
    if speed > run.max_speed {
        run.max_speed = speed;
        run.max_speed_tick = s.tick;
    }
    let sample = run
        .path
        .last()
        .is_none_or(|last| (s.tick - last.tick) as f64 >= MOVE_SAMPLE_SECONDS * tick_rate);
    if sample {
        run.path.push(PathPoint {
            tick: s.tick,
            x: s.position.x,
            y: s.position.y,
            z: s.position.z,
        });
        run.frames.insert(
            s.tick,
            RunFrame {
                pos: s.position,
                vel: s.vel,
                yaw: s.yaw,
            },
        );
    }
}

/// `_finishRun`: a finished run, as a highlight if it was more than just running fast.
fn finish_run(run: &MovementRun, entity_id: &str, tick_rate: f64) -> Option<HighlightCandidate> {
    let seconds = (run.last_active_tick - run.start_tick) as f64 / tick_rate;
    if seconds < MOVE_MIN_SECONDS || run.path.len() < 2 {
        return None;
    }
    let mut tags = Vec::new();
    let wall_jumps = run.wall_jump_ticks.len();
    if wall_jumps > 0 {
        tags.push(HighlightTag::WallJump);
    }
    if run.slides > 0 {
        tags.push(HighlightTag::Slide);
    }
    if run.hops >= BUNNY_HOP_MIN {
        tags.push(HighlightTag::BunnyHop);
    }
    // A climb only counts off the walls: a jump pad gains height for anyone
    let climbed = wall_jumps > 0 && run.max_y - run.start_y >= MOVE_HEIGHT_GAIN;
    if climbed {
        tags.push(HighlightTag::HeightGain);
    }
    if run.max_speed >= MOVE_HIGH_SPEED {
        tags.push(HighlightTag::HighSpeed);
    }
    let techniques = tags
        .iter()
        .filter(|t| matches!(t, HighlightTag::WallJump | HighlightTag::Slide | HighlightTag::BunnyHop))
        .count();
    if techniques >= 2 {
        tags.push(HighlightTag::TrickJump);
    }
    if wall_jumps == 0
        && run.hops < BUNNY_HOP_MIN
        && !(run.slides > 0 && run.hops > 0)
        && !tags.contains(&HighlightTag::HighSpeed)
    {
        return None;
    }

    let peak_tick = if climbed {
        run.max_y_tick
    } else if wall_jumps > 0 {
        run.wall_jump_ticks[0]
    } else {
        run.max_speed_tick
    };
    let end = run
        .last_active_tick
        .min(run.start_tick + js_round(MOVE_MAX_SECONDS * tick_rate) as i64);
    let path: Vec<PathPoint> = run.path.iter().copied().filter(|p| p.tick <= end).collect();
    let nearest = path.iter().copied().reduce(|best, p| {
        if (p.tick - peak_tick).abs() < (best.tick - peak_tick).abs() {
            p
        } else {
            best
        }
    })?;
    let at = run.frames.get(&nearest.tick);
    let pad = js_round(MOVE_PAD_SECONDS * tick_rate) as i64;
    Some(HighlightCandidate {
        start_tick: run.start_tick - pad,
        peak_tick: peak_tick.min(end),
        end_tick: end + pad,
        score: 0.0,
        kind: HighlightType::Movement,
        primary_player: entity_id.to_owned(),
        primary_player_name: entity_id.to_owned(),
        victim_player: None,
        victim_player_name: None,
        weapon_id: None,
        kill_count: None,
        tags,
        killer_pos: Some(at.map_or(Vec3::new(nearest.x, nearest.y, nearest.z), |f| f.pos)),
        victim_pos: None,
        killer_velocity: at.map(|f| f.vel),
        killer_yaw: at.and_then(|f| f.yaw),
        victim_faced_killer: None,
        path: Some(path),
    })
}

/// `_computeScore`.
fn compute_score(c: &HighlightCandidate) -> f64 {
    let mut score = if c.kind == HighlightType::Movement {
        BASE_SCORE_MOVEMENT
    } else {
        BASE_SCORE_FRAG
    };
    let has_rocket_jump = c.has(HighlightTag::RocketJump);
    for &tag in &c.tags {
        if (tag == HighlightTag::Airborne && has_rocket_jump) || tag == HighlightTag::MultiKill {
            continue;
        }
        score *= tag.multiplier();
    }
    if let Some(n) = c.kill_count
        && n > 1
    {
        score *= 1.0 + 0.5 * f64::from(n - 1);
    }
    js_round(score * 100.0) / 100.0
}

/// `_deduplicateOverlapping`: the best of each overlapping set.
fn deduplicate_overlapping(candidates: &[HighlightCandidate]) -> Vec<HighlightCandidate> {
    let mut sorted = candidates.to_vec();
    sort_by_score(&mut sorted);
    let mut kept: Vec<HighlightCandidate> = Vec::new();
    for c in sorted {
        if !kept
            .iter()
            .any(|k| c.start_tick <= k.end_tick && c.end_tick >= k.start_tick)
        {
            kept.push(c);
        }
    }
    kept
}
