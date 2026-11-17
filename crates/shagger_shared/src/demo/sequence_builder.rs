//! Port of SHAG `src/shared/demo/SequenceBuilder.ts`: the highlight reel's
//! shot list. Each frag is filmed as a scene of three shots (a third-person
//! intro, the same moment from a player's eyes, a slow orbit); a frag
//! sequence as it happened (a chase between frags, the killer's eyes for each
//! with a counter, the orbit); movement clips are cut in between frag scenes.
//! Three frag scenes end on a freeze frame, and some cuts jump ahead through
//! speed lines.
//!
//! `alignSceneCutsToBeats` (the video export's cuts on the music's beats) is
//! not ported: the native client has no video export.

use crate::demo::demo_analyzer::{DemoAnalysis, HighlightCandidate, HighlightTag, HighlightType};
use crate::js::js_round;

/// `CameraPresetId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraPreset {
    /// Spectator eye on the killer.
    ChaseHero,
    /// Freefly orbit round the action's midpoint.
    OrbitKill,
    /// Freefly below the jump arc, looking up.
    RocketJumpHero,
    /// Spectator eye on the victim before death.
    VictimPov,
    /// Freefly dolly from wide to tight.
    DramaticZoom,
    /// Slow orbit for the instant replay.
    ReplayOrbit,
    /// Freefly trailing a player along their route.
    TrickFollow,
}

impl CameraPreset {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChaseHero => "chase_hero",
            Self::OrbitKill => "orbit_kill",
            Self::RocketJumpHero => "rocket_jump_hero",
            Self::VictimPov => "victim_pov",
            Self::DramaticZoom => "dramatic_zoom",
            Self::ReplayOrbit => "replay_orbit",
            Self::TrickFollow => "trick_follow",
        }
    }
}

/// `TransitionType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Cut,
    Crossfade,
    WhiteFlash,
    FadeToBlack,
    /// Speed lines: the cut jumps ahead in time.
    SpeedWipe,
}

impl Transition {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cut => "cut",
            Self::Crossfade => "crossfade",
            Self::WhiteFlash => "white_flash",
            Self::FadeToBlack => "fade_to_black",
            Self::SpeedWipe => "speed_wipe",
        }
    }
}

/// `SceneRole`: which of a scene's shots a segment is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneRole {
    Intro,
    Pov,
    Ending,
}

impl SceneRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Intro => "intro",
            Self::Pov => "pov",
            Self::Ending => "ending",
        }
    }
}

/// `SpeedKeyframe`: a change of playback speed. The speed eases from the
/// one before over the `ramp` ticks up to `tick`, and holds from `tick` on;
/// with no ramp it changes at `tick`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedKeyframe {
    pub tick: i64,
    pub speed: f64,
    pub ramp: Option<i64>,
}

const fn key(tick: i64, speed: f64, ramp: Option<i64>) -> SpeedKeyframe {
    SpeedKeyframe { tick, speed, ramp }
}

/// `ReelFreeze`: the picture held still as a comic panel, with a caption.
#[derive(Debug, Clone, PartialEq)]
pub struct ReelFreeze {
    pub tick: i64,
    pub ms: f64,
    pub caption: String,
}

/// `ReelStamp`: a counter stamped on the picture as a frag lands.
#[derive(Debug, Clone, PartialEq)]
pub struct ReelStamp {
    pub tick: i64,
    pub text: String,
}

/// `CinematicSegment`.
#[derive(Debug, Clone, PartialEq)]
pub struct CinematicSegment {
    pub start_tick: i64,
    pub end_tick: i64,
    /// Wall-clock ms at the segment's speeds, freeze included.
    pub duration_ms: f64,
    pub camera_preset: CameraPreset,
    /// Entity id for the chase-cam presets.
    pub camera_target: Option<String>,
    /// The victim a fly-by frames with the killer, when not the
    /// highlight's own.
    pub victim_target: Option<String>,
    pub playback_speed: f64,
    pub speed_keyframes: Vec<SpeedKeyframe>,
    pub highlight: HighlightCandidate,
    pub is_replay: bool,
    pub segment_index: usize,
    /// The scene (one highlight) this shot belongs to, 0-based.
    pub scene: usize,
    pub role: SceneRole,
    pub transition_in: Transition,
    pub transition_out: Transition,
    pub transition_duration_ms: f64,
    pub freeze: Option<ReelFreeze>,
    pub stamps: Vec<ReelStamp>,
}

/// `CinematicReel`.
#[derive(Debug, Clone, PartialEq)]
pub struct CinematicReel {
    pub segments: Vec<CinematicSegment>,
    pub total_duration_ms: f64,
    pub total_segments: usize,
    /// Highlights in the reel; each is one scene of several shots.
    pub total_scenes: usize,
    pub tick_rate: f64,
}

/// About how long one scene runs, all three shots, for the selection budget.
const AVG_SCENE_MS: f64 = 11000.0;
/// A frag sequence's ending orbit and its way out, past its own window, for
/// the selection budget.
const SEQUENCE_ENDING_MS: f64 = 3500.0;
/// About how long a movement clip runs, both shots.
const AVG_MOVE_MS: f64 = 8000.0;
const MAX_MOVES_PER_GAP: usize = 2;
const MOVE_POV_SECONDS: f64 = 1.5;
const OVERLAP_BUFFER_SECONDS: f64 = 3.0;
const DEFAULT_TRANSITION_MS: f64 = 500.0;
/// The rewind from the fly-by into the player's eyes.
const REWIND_FLASH_MS: f64 = 300.0;
/// Speed lines either side of a cut that jumps ahead.
pub const SPEED_WIPE_MS: f64 = 380.0;
/// How long a change into or out of slow motion takes, in game seconds.
const SPEED_RAMP_SECONDS: f64 = 0.2;
const INTRO_LEADIN_SECONDS: f64 = 2.0;
const INTRO_SLOW_SECONDS: f64 = 0.6;
const INTRO_SLOW_SPEED: f64 = 0.35;
const POV_LEADIN_SECONDS: f64 = 2.0;
/// The victim's eyes slow this long before the frag; the shot ends on it.
const VICTIM_SLOW_SECONDS: f64 = 0.35;
const VICTIM_SLOW_SPEED: f64 = 0.45;
const ENDING_BEFORE_SECONDS: f64 = 0.3;
const ENDING_AFTER_SECONDS: f64 = 1.2;
const ENDING_SPEED: f64 = 0.4;
/// A frag sequence: the killer's eyes from this long before each frag to
/// this long after.
const SEQ_EYES_BEFORE_SECONDS: f64 = 1.2;
const SEQ_EYES_AFTER_SECONDS: f64 = 0.5;
/// Frags closer than this between their eyes shots share one.
const SEQ_MERGE_GAP_SECONDS: f64 = 0.8;
/// The chase between two frags runs at most this long; a longer gap jumps
/// ahead.
const SEQ_CHASE_MAX_SECONDS: f64 = 3.0;
/// A chase between frags plays this fast, back to full speed by the next
/// frag.
const SEQ_CHASE_SPEED: f64 = 1.35;
/// An opening chase shorter than this is left out.
const SEQ_CHASE_MIN_SECONDS: f64 = 0.8;
/// How long a freeze frame holds, and how many a reel has.
pub const FREEZE_MS: f64 = 1400.0;
const FREEZE_COUNT: usize = 3;
/// A freeze lands just after the frag, when the hit is on screen.
const FREEZE_AFTER_SECONDS: f64 = 0.1;
/// How a scene can end; the reel's last scene always fades to black.
const END_EFFECTS: [Transition; 3] = [Transition::FadeToBlack, Transition::WhiteFlash, Transition::Crossfade];

// ── Planned speed ──

/// `plannedSpeedAt`: the speed a segment plays at, at `tick`: its base
/// speed and keyframes with their ramps.
#[must_use]
pub fn planned_speed_at(playback_speed: f64, keyframes: &[SpeedKeyframe], tick: f64) -> f64 {
    let mut speed = playback_speed;
    let mut previous_tick = f64::NEG_INFINITY;
    for kf in keyframes {
        let k = kf.tick as f64;
        let ramp = (kf.ramp.unwrap_or(0) as f64).min(k - previous_tick).max(0.0);
        if tick >= k {
            speed = kf.speed;
        } else {
            if ramp > 0.0 && tick > k - ramp {
                let t = (tick - (k - ramp)) / ramp;
                speed += (kf.speed - speed) * t * t * (3.0 - 2.0 * t);
            }
            break;
        }
        previous_tick = k;
    }
    speed
}

/// `plannedDurationMs`: wall-clock ms from `start_tick` to `end_tick` at
/// the planned speeds.
#[must_use]
pub fn planned_duration_ms(
    playback_speed: f64,
    keyframes: &[SpeedKeyframe],
    start_tick: i64,
    end_tick: i64,
    tick_rate: f64,
) -> f64 {
    let ramps = keyframes.iter().any(|kf| kf.ramp.unwrap_or(0) > 0);
    if !ramps {
        // Steps: exact.
        let mut ms = 0.0;
        let mut from = start_tick;
        let mut speed = playback_speed;
        for kf in keyframes {
            if kf.tick <= start_tick {
                speed = kf.speed;
                continue;
            }
            if kf.tick >= end_tick {
                break;
            }
            ms += ((kf.tick - from) as f64 / tick_rate / speed) * 1000.0;
            from = kf.tick;
            speed = kf.speed;
        }
        return ms + ((end_tick - from) as f64 / tick_rate / speed) * 1000.0;
    }
    // Ramps: midpoint steps of a quarter tick.
    let step: f64 = 0.25;
    let end = end_tick as f64;
    let mut ms = 0.0;
    let mut tick = start_tick as f64;
    while tick < end {
        let span = step.min(end - tick);
        ms += (span / tick_rate / planned_speed_at(playback_speed, keyframes, tick + span / 2.0).max(0.05)) * 1000.0;
        tick += step;
    }
    ms
}

impl CinematicSegment {
    /// `plannedSpeedAt` for this segment.
    #[must_use]
    pub fn planned_speed_at(&self, tick: f64) -> f64 {
        planned_speed_at(self.playback_speed, &self.speed_keyframes, tick)
    }
}

/// `SequenceBuilder.build`.
#[must_use]
pub fn build(analysis: &DemoAnalysis, target_duration_minutes: f64, tick_rate: f64) -> CinematicReel {
    let target_ms = target_duration_minutes * 60.0 * 1000.0;
    let tick_rate = if tick_rate > 0.0 { tick_rate } else { 20.0 };
    if analysis.highlights.is_empty() {
        return CinematicReel {
            segments: Vec::new(),
            total_duration_ms: 0.0,
            total_segments: 0,
            total_scenes: 0,
            tick_rate,
        };
    }
    let (moves, frags): (Vec<&HighlightCandidate>, Vec<&HighlightCandidate>) = analysis
        .highlights
        .iter()
        .partition(|h| h.kind == HighlightType::Movement);
    let buffer = js_round(OVERLAP_BUFFER_SECONDS * tick_rate) as i64;
    let selected = select_highlights(&frags, &moves, target_ms, buffer, tick_rate);
    let frozen = freeze_scenes(&selected);

    let mut segments = Vec::new();
    let mut previous_pov = None;
    for (scene, h) in selected.iter().enumerate() {
        if h.kind == HighlightType::Movement {
            segments.extend(build_move_scene(h, scene, tick_rate));
            continue;
        }
        let mut shots = if h.kind == HighlightType::FragSequence && h.kills.as_ref().is_some_and(|k| k.len() >= 2) {
            build_sequence_scene(h, scene, tick_rate)
        } else {
            build_scene(h, scene, tick_rate, previous_pov)
        };
        if h.kind != HighlightType::FragSequence {
            previous_pov = shots.iter().find(|s| s.role == SceneRole::Pov).map(|s| s.camera_preset);
        }
        if frozen.contains(&scene) {
            freeze_ending(&mut shots, tick_rate);
        }
        segments.extend(shots);
    }
    assign_scene_ends(&mut segments, &selected);
    for (i, s) in segments.iter_mut().enumerate() {
        s.segment_index = i;
    }
    let total_duration_ms = segments
        .iter()
        .fold(0.0, |sum, s| sum + s.duration_ms + s.transition_duration_ms);
    CinematicReel {
        total_segments: segments.len(),
        segments,
        total_duration_ms,
        total_scenes: selected.len(),
        tick_rate,
    }
}

fn by_score_desc<'a>(list: &[&'a HighlightCandidate]) -> Vec<&'a HighlightCandidate> {
    let mut sorted = list.to_vec();
    sorted.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    sorted
}

/// `_selectHighlights`: the best frags that fit the length, in match order,
/// with the best movement clips cut in between them.
fn select_highlights(
    frags: &[&HighlightCandidate],
    moves: &[&HighlightCandidate],
    target_ms: f64,
    buffer: i64,
    tick_rate: f64,
) -> Vec<HighlightCandidate> {
    if frags.is_empty() {
        let max = ((target_ms / AVG_MOVE_MS).floor() as usize).max(1);
        return pick_non_overlapping(moves, max, buffer);
    }
    // Each frag scene costs its own length, and a share of the movement
    // clips between them.
    let move_share = if moves.is_empty() { 0.0 } else { AVG_MOVE_MS / 2.0 };
    let cost = |h: &HighlightCandidate| scene_cost_ms(h, tick_rate) + move_share;
    let kills = pick_within_budget(frags, target_ms, cost, buffer);
    if kills.len() < 2 || moves.is_empty() {
        return kills;
    }
    let max_moves = (kills.len() / 2).max(1);
    let mut per_gap: Vec<usize> = vec![0; kills.len()];
    let mut chosen: Vec<HighlightCandidate> = Vec::new();
    for mv in by_score_desc(moves) {
        if chosen.len() >= max_moves {
            break;
        }
        let mut gap: isize = -1;
        while ((gap + 1) as usize) < kills.len() && kills[(gap + 1) as usize].end_tick + buffer < mv.start_tick {
            gap += 1;
        }
        if gap < 0 || gap as usize >= kills.len() - 1 {
            continue;
        }
        let gap = gap as usize;
        if mv.end_tick + buffer >= kills[gap + 1].start_tick
            || per_gap[gap] >= MAX_MOVES_PER_GAP
            || overlaps_any(mv, &chosen, buffer)
        {
            continue;
        }
        chosen.push(mv.clone());
        per_gap[gap] += 1;
    }
    let mut all = kills;
    all.extend(chosen);
    all.sort_by_key(|h| h.peak_tick);
    all
}

/// `_pickWithinBudget`: the best-scoring candidates that fit `budget_ms`
/// (at least one), none within the buffer of another; in match order.
fn pick_within_budget(
    candidates: &[&HighlightCandidate],
    budget_ms: f64,
    cost: impl Fn(&HighlightCandidate) -> f64,
    buffer: i64,
) -> Vec<HighlightCandidate> {
    let mut selected: Vec<HighlightCandidate> = Vec::new();
    let mut spent = 0.0;
    for c in by_score_desc(candidates) {
        if overlaps_any(c, &selected, buffer) {
            continue;
        }
        let price = cost(c);
        if !selected.is_empty() && spent + price > budget_ms {
            continue;
        }
        selected.push(c.clone());
        spent += price;
    }
    selected.sort_by_key(|h| h.peak_tick);
    selected
}

/// `_pickNonOverlapping`: the best, up to `max`, none within the buffer of another; in match order.
fn pick_non_overlapping(candidates: &[&HighlightCandidate], max: usize, buffer: i64) -> Vec<HighlightCandidate> {
    let mut selected: Vec<HighlightCandidate> = Vec::new();
    for c in by_score_desc(candidates) {
        if selected.len() >= max {
            break;
        }
        if !overlaps_any(c, &selected, buffer) {
            selected.push(c.clone());
        }
    }
    selected.sort_by_key(|h| h.peak_tick);
    selected
}

fn overlaps_any(c: &HighlightCandidate, others: &[HighlightCandidate], buffer: i64) -> bool {
    others
        .iter()
        .any(|s| c.start_tick <= s.end_tick + buffer && c.end_tick >= s.start_tick - buffer)
}

/// `_freezeScenes`: the scenes (by index) that end on a freeze frame: the
/// reel's `FREEZE_COUNT` coolest frag scenes, the higher score first among
/// equals.
fn freeze_scenes(selected: &[HighlightCandidate]) -> Vec<usize> {
    let mut frags: Vec<usize> = (0..selected.len())
        .filter(|&i| selected[i].kind != HighlightType::Movement)
        .collect();
    frags.sort_by(|&a, &b| {
        let (a, b) = (&selected[a], &selected[b]);
        freeze_rank(b)
            .cmp(&freeze_rank(a))
            .then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });
    frags.truncate(FREEZE_COUNT);
    frags
}

fn ticks(seconds: f64, tick_rate: f64) -> i64 {
    js_round(seconds * tick_rate) as i64
}

/// `_buildMoveScene`: a camera trailing the run, then a flash into the player's eyes.
fn build_move_scene(h: &HighlightCandidate, scene: usize, tick_rate: f64) -> Vec<CinematicSegment> {
    let t = |s: f64| ticks(s, tick_rate);
    let ramp = Some(t(SPEED_RAMP_SECONDS));
    let mut follow = segment(
        h,
        scene,
        SceneRole::Intro,
        CameraPreset::TrickFollow,
        (h.start_tick, h.end_tick),
        tick_rate,
        1.0,
        &[
            key(h.peak_tick - t(0.25), 0.5, ramp),
            key(h.peak_tick + t(0.4), 1.0, ramp),
        ],
    );
    let mut eyes = segment(
        h,
        scene,
        SceneRole::Pov,
        CameraPreset::ChaseHero,
        (
            h.start_tick.max(h.peak_tick - t(MOVE_POV_SECONDS)),
            h.end_tick.min(h.peak_tick + t(MOVE_POV_SECONDS)),
        ),
        tick_rate,
        1.0,
        &[],
    );
    eyes.camera_target = Some(h.primary_player.clone());
    follow.transition_out = Transition::WhiteFlash;
    eyes.transition_in = Transition::WhiteFlash;
    follow.transition_duration_ms = REWIND_FLASH_MS;
    eyes.transition_duration_ms = REWIND_FLASH_MS;
    vec![follow, eyes]
}

/// `_buildScene`: intro, eyes, ending.
fn build_scene(
    h: &HighlightCandidate,
    scene: usize,
    tick_rate: f64,
    previous_pov: Option<CameraPreset>,
) -> Vec<CinematicSegment> {
    let t = |s: f64| ticks(s, tick_rate);
    let ramp = t(SPEED_RAMP_SECONDS);
    let clamp_start = |tick: i64| h.start_tick.max(tick.min(h.peak_tick));
    let clamp_end = |tick: i64| h.end_tick.min(tick.max(h.peak_tick));

    // 1. Intro: third person up to the frag, slowing into it.
    let intro_preset = intro_preset(h);
    let intro_start = if intro_preset == CameraPreset::RocketJumpHero {
        h.start_tick
    } else {
        clamp_start(h.peak_tick - t(INTRO_LEADIN_SECONDS))
    };
    let mut intro = segment(
        h,
        scene,
        SceneRole::Intro,
        intro_preset,
        (intro_start, h.peak_tick),
        tick_rate,
        1.0,
        &[key(
            h.peak_tick - t(INTRO_SLOW_SECONDS),
            INTRO_SLOW_SPEED,
            Some(ramp * 2),
        )],
    );

    // 2. The same moment from a player's eyes: the killer's through the frag
    // with a beat of slow motion; the victim's end on it.
    let pov_preset = pov_preset(h, previous_pov);
    let victim = pov_preset == CameraPreset::VictimPov;
    let pov_start = if h.kill_count.unwrap_or(1) > 1 {
        h.start_tick
    } else {
        clamp_start(h.peak_tick - t(POV_LEADIN_SECONDS))
    };
    let mut pov = if victim {
        segment(
            h,
            scene,
            SceneRole::Pov,
            pov_preset,
            (pov_start, h.peak_tick),
            tick_rate,
            1.0,
            &[key(h.peak_tick - t(VICTIM_SLOW_SECONDS), VICTIM_SLOW_SPEED, Some(ramp))],
        )
    } else {
        segment(
            h,
            scene,
            SceneRole::Pov,
            pov_preset,
            (pov_start, h.end_tick),
            tick_rate,
            1.0,
            &[
                key(h.peak_tick - t(0.15), 0.5, Some(ramp)),
                key(h.peak_tick + t(0.5), 1.0, Some(ramp)),
            ],
        )
    };
    pov.camera_target = Some(if victim {
        h.victim_player.clone().unwrap_or_else(|| h.primary_player.clone())
    } else {
        h.primary_player.clone()
    });
    if h.kills.as_ref().is_some_and(|k| k.len() >= 2) {
        pov.stamps = kill_stamps(h, pov.start_tick, pov.end_tick);
    }

    // 3. Ending: a slow orbit round the frag. After the victim's eyes it
    // takes over at the frag itself.
    let mut ending = segment(
        h,
        scene,
        SceneRole::Ending,
        CameraPreset::ReplayOrbit,
        (
            if victim {
                h.peak_tick
            } else {
                clamp_start(h.peak_tick - t(ENDING_BEFORE_SECONDS))
            },
            clamp_end(h.peak_tick + t(ENDING_AFTER_SECONDS)),
        ),
        tick_rate,
        ENDING_SPEED,
        &[],
    );
    ending.is_replay = true;

    intro.transition_out = Transition::WhiteFlash;
    pov.transition_in = Transition::WhiteFlash;
    intro.transition_duration_ms = REWIND_FLASH_MS;
    pov.transition_duration_ms = REWIND_FLASH_MS;
    ending.transition_in = if victim { Transition::Cut } else { Transition::Crossfade };
    vec![intro, pov, ending]
}

/// `_buildSequenceScene`: a frag sequence as it happened: the killer chased
/// in third person between frags, seen through their eyes for each frag,
/// then the ending orbit round the last. A long wait between two frags jumps
/// ahead through speed lines.
fn build_sequence_scene(h: &HighlightCandidate, scene: usize, tick_rate: f64) -> Vec<CinematicSegment> {
    let t = |s: f64| ticks(s, tick_rate);
    let ramp = Some(t(SPEED_RAMP_SECONDS));
    let kills = h.kills.as_deref().unwrap_or_default();

    // The killer's eyes around each frag, frags close together sharing one
    // shot.
    struct Run {
        start: i64,
        end: i64,
        kills: Vec<usize>,
    }
    let mut runs: Vec<Run> = Vec::new();
    for (i, kill) in kills.iter().enumerate() {
        let start = h.start_tick.max(kill.tick - t(SEQ_EYES_BEFORE_SECONDS));
        let end = h.end_tick.min(kill.tick + t(SEQ_EYES_AFTER_SECONDS));
        match runs.last_mut() {
            Some(last) if start <= last.end + t(SEQ_MERGE_GAP_SECONDS) => {
                last.end = end;
                last.kills.push(i);
            }
            _ => runs.push(Run {
                start,
                end,
                kills: vec![i],
            }),
        }
    }

    let chase = |from: i64, to: i64, kill_index: usize, fast: bool| {
        let keys = [key(to, 1.0, Some((to - from).min(t(0.6))))];
        let mut shot = segment(
            h,
            scene,
            SceneRole::Intro,
            CameraPreset::TrickFollow,
            (from, to),
            tick_rate,
            if fast { SEQ_CHASE_SPEED } else { 1.0 },
            if fast { &keys } else { &[] },
        );
        shot.victim_target = Some(kills[kill_index].victim_player.clone());
        shot
    };

    let mut shots: Vec<CinematicSegment> = Vec::new();
    // Opening: chasing the killer into the first frag.
    if runs[0].start - h.start_tick >= t(SEQ_CHASE_MIN_SECONDS) {
        shots.push(chase(h.start_tick, runs[0].start, 0, false));
    }
    for (r, run) in runs.iter().enumerate() {
        let mut keyframes = Vec::new();
        for &i in &run.kills {
            keyframes.push(key(kills[i].tick - t(0.1), 0.5, ramp));
            keyframes.push(key(kills[i].tick + t(0.35), 1.0, ramp));
        }
        let mut eyes = segment(
            h,
            scene,
            SceneRole::Pov,
            CameraPreset::ChaseHero,
            (run.start, run.end),
            tick_rate,
            1.0,
            &keyframes,
        );
        eyes.camera_target = Some(h.primary_player.clone());
        eyes.stamps = kill_stamps(h, run.start, run.end);
        let Some(next) = runs.get(r + 1) else {
            shots.push(eyes);
            continue;
        };
        let gap = next.start - run.end;
        if gap <= 0 {
            shots.push(eyes);
            continue;
        }
        let long = gap > t(SEQ_CHASE_MAX_SECONDS);
        let from = if long {
            next.start - t(SEQ_CHASE_MAX_SECONDS - 0.5)
        } else {
            run.end
        };
        let mut between = chase(from, next.start, next.kills[0], true);
        if long {
            eyes.transition_out = Transition::SpeedWipe;
            between.transition_in = Transition::SpeedWipe;
            eyes.transition_duration_ms = SPEED_WIPE_MS;
            between.transition_duration_ms = SPEED_WIPE_MS;
        }
        shots.push(eyes);
        shots.push(between);
    }

    // Ending: the slow orbit round the last frag.
    let last_kill = &kills[kills.len() - 1];
    let mut ending = segment(
        h,
        scene,
        SceneRole::Ending,
        CameraPreset::ReplayOrbit,
        (
            h.start_tick.max(last_kill.tick - t(ENDING_BEFORE_SECONDS)),
            h.end_tick
                .min(last_kill.tick.max(last_kill.tick + t(ENDING_AFTER_SECONDS))),
        ),
        tick_rate,
        ENDING_SPEED,
        &[],
    );
    ending.is_replay = true;
    ending.transition_in = Transition::Crossfade;
    shots.push(ending);
    shots
}

/// `_killStamps`: the frag counter over a grouped highlight's frags inside
/// a shot: "1", "2", ...
fn kill_stamps(h: &HighlightCandidate, from: i64, to: i64) -> Vec<ReelStamp> {
    h.kills
        .iter()
        .flatten()
        .enumerate()
        .map(|(i, kill)| ReelStamp {
            tick: kill.tick,
            text: (i + 1).to_string(),
        })
        .filter(|s| s.tick >= from && s.tick < to)
        .collect()
}

/// `_freezeEnding`: hold the scene's ending still just after its frag, as a
/// comic panel.
fn freeze_ending(shots: &mut [CinematicSegment], tick_rate: f64) {
    let Some(ending) = shots.iter_mut().find(|s| s.role == SceneRole::Ending) else {
        return;
    };
    let tick = (ending.end_tick - 1).min(
        ending
            .start_tick
            .max(ending.highlight.peak_tick + js_round(FREEZE_AFTER_SECONDS * tick_rate) as i64),
    );
    if tick < ending.start_tick {
        return;
    }
    ending.freeze = Some(ReelFreeze {
        tick,
        ms: FREEZE_MS,
        caption: freeze_caption(&ending.highlight),
    });
    ending.duration_ms += FREEZE_MS;
}

#[allow(clippy::too_many_arguments)]
fn segment(
    h: &HighlightCandidate,
    scene: usize,
    role: SceneRole,
    camera_preset: CameraPreset,
    (start_tick, end_tick): (i64, i64),
    tick_rate: f64,
    playback_speed: f64,
    speed_keyframes: &[SpeedKeyframe],
) -> CinematicSegment {
    let keyframes: Vec<SpeedKeyframe> = speed_keyframes
        .iter()
        .copied()
        .filter(|kf| kf.tick > start_tick && kf.tick <= end_tick)
        .collect();
    CinematicSegment {
        start_tick,
        end_tick,
        duration_ms: planned_duration_ms(playback_speed, &keyframes, start_tick, end_tick, tick_rate),
        camera_preset,
        camera_target: None,
        victim_target: None,
        playback_speed,
        speed_keyframes: keyframes,
        highlight: h.clone(),
        is_replay: false,
        segment_index: 0,
        scene,
        role,
        transition_in: Transition::Cut,
        transition_out: Transition::Cut,
        transition_duration_ms: DEFAULT_TRANSITION_MS,
        freeze: None,
        stamps: Vec::new(),
    }
}

/// `_introPreset`: the third-person shot that suits the play.
fn intro_preset(h: &HighlightCandidate) -> CameraPreset {
    if h.has(HighlightTag::RocketJump) || h.has(HighlightTag::HeightGain) {
        CameraPreset::RocketJumpHero
    } else if h.has(HighlightTag::LongRange) {
        CameraPreset::DramaticZoom
    } else {
        CameraPreset::OrbitKill
    }
}

/// `_povPreset`: the victim's eyes when they saw it coming, never twice in a
/// row; a multi-kill is always the killer's.
fn pov_preset(h: &HighlightCandidate, previous: Option<CameraPreset>) -> CameraPreset {
    if h.kill_count.unwrap_or(1) <= 1
        && h.victim_faced_killer == Some(true)
        && h.victim_player.is_some()
        && previous != Some(CameraPreset::VictimPov)
    {
        CameraPreset::VictimPov
    } else {
        CameraPreset::ChaseHero
    }
}

/// `_assignSceneEnds`: the reel fades in from black and out to black.
/// Between scenes, each scene ends with an end effect the next comes in
/// through: some on speed lines, the rest picked from the highlight, so the
/// same demo always cuts the same.
fn assign_scene_ends(segments: &mut [CinematicSegment], selected: &[HighlightCandidate]) {
    if segments.is_empty() {
        return;
    }
    let total_scenes = selected.len();
    let wiped = speed_line_scenes(selected);
    segments[0].transition_in = Transition::FadeToBlack;
    for i in 0..segments.len() {
        let next_scene = segments.get(i + 1).map(|n| n.scene);
        // A scene's last shot.
        if next_scene == Some(segments[i].scene) {
            continue;
        }
        let effect = match next_scene {
            _ if segments[i].scene + 1 == total_scenes => Transition::FadeToBlack,
            None => Transition::FadeToBlack,
            Some(next) if wiped.contains(&next) => Transition::SpeedWipe,
            Some(_) => END_EFFECTS[(scene_seed(&segments[i].highlight) % END_EFFECTS.len() as i64) as usize],
        };
        segments[i].transition_out = effect;
        if effect == Transition::SpeedWipe {
            segments[i].transition_duration_ms = SPEED_WIPE_MS;
        }
        if let Some(next) = segments.get_mut(i + 1) {
            next.transition_in = effect;
            if effect == Transition::SpeedWipe {
                next.transition_duration_ms = SPEED_WIPE_MS;
            }
        }
    }
}

/// `_speedLineScenes`: the scenes (by index) that come in on speed lines:
/// about one cut in three at most, never the first, never two running, the
/// best frag scenes first, then movement clips.
fn speed_line_scenes(selected: &[HighlightCandidate]) -> Vec<usize> {
    let cuts = selected.len().saturating_sub(1);
    let budget = if cuts >= 2 { (cuts / 3).max(1) } else { cuts };
    let mut frags: Vec<usize> = (1..selected.len())
        .filter(|&i| selected[i].kind != HighlightType::Movement)
        .collect();
    frags.sort_by(|&a, &b| {
        selected[b]
            .score
            .partial_cmp(&selected[a].score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let moves = (1..selected.len()).filter(|&i| selected[i].kind == HighlightType::Movement);
    let mut chosen: Vec<usize> = Vec::new();
    for scene in frags.into_iter().chain(moves) {
        if chosen.len() >= budget {
            break;
        }
        if chosen.contains(&(scene - 1)) || chosen.contains(&(scene + 1)) {
            continue;
        }
        chosen.push(scene);
    }
    chosen
}

/// `sceneCostMs`: about how long a frag scene runs, for the selection
/// budget: a frag sequence plays its whole window and an ending.
fn scene_cost_ms(h: &HighlightCandidate, tick_rate: f64) -> f64 {
    if h.kind != HighlightType::FragSequence {
        return AVG_SCENE_MS;
    }
    ((h.end_tick - h.start_tick) as f64 / tick_rate) * 1000.0 + SEQUENCE_ENDING_MS
}

/// `freezeRank`: how much a frag deserves a freeze frame: the rarer the
/// play, the higher.
fn freeze_rank(h: &HighlightCandidate) -> u8 {
    let kills = h.kill_count.unwrap_or(1);
    if kills >= 3 {
        4
    } else if h.has(HighlightTag::AirRocket) || h.has(HighlightTag::Blade) {
        3
    } else if h.has(HighlightTag::RocketJump) {
        2
    } else if kills == 2 || h.has(HighlightTag::Airborne) || h.has(HighlightTag::LongRange) {
        1
    } else {
        0
    }
}

/// `freezeCaption`: the word on a freeze frame.
#[must_use]
pub fn freeze_caption(h: &HighlightCandidate) -> String {
    let kills = h.kill_count.unwrap_or(1);
    if kills >= 5 {
        return format!("{kills} FRAGS!");
    }
    let word = match kills {
        4 => "QUAD!",
        3 => "TRIPLE!",
        2 => "DOUBLE!",
        _ if h.has(HighlightTag::AirRocket) => "AIR ROCKET!",
        _ if h.has(HighlightTag::Blade) => "SLICED!",
        _ if h.has(HighlightTag::RocketJump) => "ROCKET JUMP!",
        _ if h.has(HighlightTag::Airborne) => "AIRSHOT!",
        _ if h.has(HighlightTag::LongRange) => "SNIPED!",
        _ if h.has(HighlightTag::LowHp) => "CLUTCH!",
        _ if h.has(HighlightTag::Revenge) => "REVENGE!",
        _ => "FRAG!",
    };
    word.to_owned()
}

/// `sceneSeed`: a stable number from a highlight.
#[must_use]
pub fn scene_seed(h: &HighlightCandidate) -> i64 {
    let mut hash = h.peak_tick as i32;
    for unit in h.primary_player.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    i64::from(hash).abs()
}
