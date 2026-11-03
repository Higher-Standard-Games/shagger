//! Port of SHAG `src/shared/demo/SequenceBuilder.ts`: the highlight reel's
//! shot list. Each frag is filmed as a scene of three shots (a third-person
//! intro, the same moment from a player's eyes, a slow orbit); movement clips
//! are cut in between frag scenes.

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
    /// Freefly trailing a movement run along its route.
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
}

impl Transition {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cut => "cut",
            Self::Crossfade => "crossfade",
            Self::WhiteFlash => "white_flash",
            Self::FadeToBlack => "fade_to_black",
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

/// A change of playback speed from a tick on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedKeyframe {
    pub tick: i64,
    pub speed: f64,
}

/// `CinematicSegment`.
#[derive(Debug, Clone, PartialEq)]
pub struct CinematicSegment {
    pub start_tick: i64,
    pub end_tick: i64,
    /// Wall-clock ms at the segment's speeds.
    pub duration_ms: f64,
    pub camera_preset: CameraPreset,
    /// Entity id for the chase-cam presets.
    pub camera_target: Option<String>,
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
}

/// `CinematicReel`.
#[derive(Debug, Clone, PartialEq)]
pub struct CinematicReel {
    pub segments: Vec<CinematicSegment>,
    pub total_duration_ms: f64,
    pub total_segments: usize,
    pub total_scenes: usize,
    pub tick_rate: f64,
}

const AVG_SCENE_MS: f64 = 11000.0;
const AVG_MOVE_MS: f64 = 8000.0;
const MAX_MOVES_PER_GAP: usize = 2;
const MOVE_POV_SECONDS: f64 = 1.5;
const OVERLAP_BUFFER_SECONDS: f64 = 3.0;
const DEFAULT_TRANSITION_MS: f64 = 500.0;
const REWIND_FLASH_MS: f64 = 300.0;
const INTRO_LEADIN_SECONDS: f64 = 2.0;
const INTRO_SLOW_SECONDS: f64 = 0.6;
const INTRO_SLOW_SPEED: f64 = 0.35;
const POV_LEADIN_SECONDS: f64 = 2.0;
const ENDING_BEFORE_SECONDS: f64 = 0.3;
const ENDING_AFTER_SECONDS: f64 = 1.2;
const ENDING_SPEED: f64 = 0.4;
/// How a scene can end; the reel's last scene always fades to black.
const END_EFFECTS: [Transition; 3] = [Transition::FadeToBlack, Transition::WhiteFlash, Transition::Crossfade];

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
    let selected = select_highlights(&frags, &moves, target_ms, buffer);

    let mut segments = Vec::new();
    let mut previous_pov = None;
    for (scene, h) in selected.iter().enumerate() {
        if h.kind == HighlightType::Movement {
            segments.extend(build_move_scene(h, scene, tick_rate));
            continue;
        }
        let shots = build_scene(h, scene, tick_rate, previous_pov);
        previous_pov = shots.iter().find(|s| s.role == SceneRole::Pov).map(|s| s.camera_preset);
        segments.extend(shots);
    }
    assign_scene_ends(&mut segments, selected.len());
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

/// `_selectHighlights`: the best frags that fit, in match order, with the
/// best movement clips cut in between them.
fn select_highlights(
    frags: &[&HighlightCandidate],
    moves: &[&HighlightCandidate],
    target_ms: f64,
    buffer: i64,
) -> Vec<HighlightCandidate> {
    if frags.is_empty() {
        let max = ((target_ms / AVG_MOVE_MS).floor() as usize).max(1);
        return pick_non_overlapping(moves, max, buffer);
    }
    let per_scene = if moves.is_empty() {
        AVG_SCENE_MS
    } else {
        AVG_SCENE_MS + AVG_MOVE_MS / 2.0
    };
    let kills = pick_non_overlapping(frags, ((target_ms / per_scene).floor() as usize).max(1), buffer);
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

fn ticks(seconds: f64, tick_rate: f64) -> i64 {
    js_round(seconds * tick_rate) as i64
}

/// `_buildMoveScene`: a camera trailing the run, then a flash into the player's eyes.
fn build_move_scene(h: &HighlightCandidate, scene: usize, tick_rate: f64) -> Vec<CinematicSegment> {
    let mut follow = segment(
        h,
        scene,
        SceneRole::Intro,
        CameraPreset::TrickFollow,
        (h.start_tick, h.end_tick),
        tick_rate,
        1.0,
        &[
            SpeedKeyframe {
                tick: h.peak_tick - ticks(0.25, tick_rate),
                speed: 0.5,
            },
            SpeedKeyframe {
                tick: h.peak_tick + ticks(0.4, tick_rate),
                speed: 1.0,
            },
        ],
    );
    let mut eyes = segment(
        h,
        scene,
        SceneRole::Pov,
        CameraPreset::ChaseHero,
        (
            h.start_tick.max(h.peak_tick - ticks(MOVE_POV_SECONDS, tick_rate)),
            h.end_tick.min(h.peak_tick + ticks(MOVE_POV_SECONDS, tick_rate)),
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
    let clamp_start = |tick: i64| h.start_tick.max(tick.min(h.peak_tick));
    let clamp_end = |tick: i64| h.end_tick.min(tick.max(h.peak_tick));

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
        &[SpeedKeyframe {
            tick: h.peak_tick - t(INTRO_SLOW_SECONDS),
            speed: INTRO_SLOW_SPEED,
        }],
    );

    let pov_preset = pov_preset(h, previous_pov);
    let pov_start = if h.kill_count.unwrap_or(1) > 1 {
        h.start_tick
    } else {
        clamp_start(h.peak_tick - t(POV_LEADIN_SECONDS))
    };
    let mut pov = segment(
        h,
        scene,
        SceneRole::Pov,
        pov_preset,
        (pov_start, h.end_tick),
        tick_rate,
        1.0,
        &[
            SpeedKeyframe {
                tick: h.peak_tick - t(0.15),
                speed: 0.5,
            },
            SpeedKeyframe {
                tick: h.peak_tick + t(0.5),
                speed: 1.0,
            },
        ],
    );
    pov.camera_target = Some(if pov_preset == CameraPreset::VictimPov {
        h.victim_player.clone().unwrap_or_else(|| h.primary_player.clone())
    } else {
        h.primary_player.clone()
    });

    let mut ending = segment(
        h,
        scene,
        SceneRole::Ending,
        CameraPreset::ReplayOrbit,
        (
            clamp_start(h.peak_tick - t(ENDING_BEFORE_SECONDS)),
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
    ending.transition_in = Transition::Crossfade;
    vec![intro, pov, ending]
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
        .filter(|kf| kf.tick > start_tick && kf.tick < end_tick)
        .collect();
    CinematicSegment {
        start_tick,
        end_tick,
        duration_ms: compute_duration_ms(start_tick, end_tick, tick_rate, playback_speed, &keyframes),
        camera_preset,
        camera_target: None,
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

/// `_povPreset`: the victim's eyes when they saw it coming, never twice in a row.
fn pov_preset(h: &HighlightCandidate, previous: Option<CameraPreset>) -> CameraPreset {
    if h.victim_faced_killer == Some(true) && h.victim_player.is_some() && previous != Some(CameraPreset::VictimPov) {
        CameraPreset::VictimPov
    } else {
        CameraPreset::ChaseHero
    }
}

/// `_computeDurationMs`: wall-clock time at the base speed and each keyframe's speed from its tick.
fn compute_duration_ms(start: i64, end: i64, tick_rate: f64, speed: f64, keyframes: &[SpeedKeyframe]) -> f64 {
    let mut ms = 0.0;
    let mut from = start;
    let mut speed = speed;
    for kf in keyframes {
        ms += ((kf.tick - from) as f64 / tick_rate / speed) * 1000.0;
        from = kf.tick;
        speed = kf.speed;
    }
    ms + ((end - from) as f64 / tick_rate / speed) * 1000.0
}

/// `_assignSceneEnds`: fade in and out of black; between scenes, an end
/// effect picked from the highlight, so a demo always cuts the same.
fn assign_scene_ends(segments: &mut [CinematicSegment], total_scenes: usize) {
    if segments.is_empty() {
        return;
    }
    segments[0].transition_in = Transition::FadeToBlack;
    for i in 0..segments.len() {
        let next_scene = segments.get(i + 1).map(|n| n.scene);
        if next_scene == Some(segments[i].scene) {
            continue;
        }
        let effect = if segments[i].scene + 1 == total_scenes || next_scene.is_none() {
            Transition::FadeToBlack
        } else {
            END_EFFECTS[(scene_seed(&segments[i].highlight) % END_EFFECTS.len() as i64) as usize]
        };
        segments[i].transition_out = effect;
        if let Some(next) = segments.get_mut(i + 1) {
            next.transition_in = effect;
        }
    }
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
