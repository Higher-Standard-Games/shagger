//! The player bodies' packages and the clips they share (SHAG
//! `src/engine/animation/PlayerModelAssets.ts`, from `config/player-model.json`
//! and `config/player-animations.json`).
//!
//! The packaged bodies (Police by default, Female Police, Cartel, RiotKitty
//! and Robot) are Draco-compressed and share one set of clips in their own
//! package, retargeted to each body at load. Vanguard, the body the clips
//! were made on, keeps its own copy of them beside it; the native client
//! falls back to it while the published packages do not have the new
//! bodies yet.

/// A clip file and the name the animation controller plays it by
/// (`AnimFileConfig`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimFileConfig {
    pub name: &'static str,
    pub file: &'static str,
    pub optional: bool,
}

/// `config/player-model.json`: the default body and its archive.
pub const PLAYER_MODEL_PACKAGE: &str = "washoi_player_model.pk3";
pub const PLAYER_MODEL_ROOT: &str = "models/players/police/";
pub const PLAYER_MODEL_FILE: &str = "police.glb";
pub const FEMALE_MODEL_ROOT: &str = "models/players/female/";
pub const CARTEL_MODEL_ROOT: &str = "models/players/cartel/";
pub const RIOTKITTY_MODEL_ROOT: &str = "models/players/riotkitty/";
pub const ROBOT_MODEL_ROOT: &str = "models/players/robot/";

/// `config/player-animations.json`: the clips every packaged body shares.
pub const PLAYER_ANIMATION_PACKAGE: &str = "washoi_player_animations.pk3";
pub const PLAYER_ANIMATION_ROOT: &str = "models/players/animations/";

pub const VANGUARD_PACKAGE: &str = "washoi_player_model.pk3";
pub const VANGUARD_ROOT: &str = "models/players/vanguard/";
pub const VANGUARD_MODEL: &str = "vanguard.glb";

const fn clip(name: &'static str, file: &'static str) -> AnimFileConfig {
    AnimFileConfig {
        name,
        file,
        optional: false,
    }
}

const fn optional(name: &'static str, file: &'static str) -> AnimFileConfig {
    AnimFileConfig {
        name,
        file,
        optional: true,
    }
}

/// `PLAYER_ANIMATIONS`, in `config/player-animations.json`'s order.
pub const PLAYER_ANIMATIONS: &[AnimFileConfig] = &[
    clip("idle", "rifle aiming idle.glb"),
    clip("run", "rifle run.glb"),
    clip("run_back", "backwards_run.glb"),
    clip("strafe_l", "strafe left.glb"),
    clip("strafe_r", "strafe right.glb"),
    clip("jump", "rifle jump.glb"),
    clip("walk", "walking.glb"),
    clip("reload", "reloading.glb"),
    clip("blade_slash", "blade_slash.glb"),
    clip("blade_idle", "blade_idle.glb"),
    clip("slide", "slide.glb"),
    clip("crouch_walk", "crouch_walk_forward.glb"),
    clip("crouch_walk_back", "crouch_walk_backward.glb"),
    clip("crouch_strafe_l", "crouch_walk_left.glb"),
    clip("crouch_strafe_r", "crouch_walk_right.glb"),
    clip("walljump", "wallrun.glb"),
    clip("swim", "swimming.glb"),
    clip("swim_idle", "swimming_idle.glb"),
    clip("climb", "climbing_ladder.glb"),
    clip("death_flying_back", "death_flying_back.glb"),
    clip("death_from_back", "death_from_back.glb"),
    clip("death_headshot", "death_headshot.glb"),
    optional("dance_1", "dance_1.glb"),
    optional("dance_2", "dance_2.glb"),
    optional("dance_3", "dance_3.glb"),
    optional("taunt_1", "taunt_1.glb"),
    optional("taunt_2", "taunt_2.glb"),
    optional("taunt_3", "taunt_3.glb"),
];

/// The clips beside the Vanguard body (`config/player-model-vanguard.json`
/// before the shared package): the fallback's.
pub const VANGUARD_ANIMATIONS: &[AnimFileConfig] = &[
    clip("idle", "rifle aiming idle.glb"),
    clip("run", "rifle run.glb"),
    clip("strafe_l", "strafe left.glb"),
    clip("strafe_r", "strafe right.glb"),
    clip("jump", "rifle jump.glb"),
    clip("walk", "walking.glb"),
    clip("reload", "reloading.glb"),
    clip("blade_slash", "blade_slash.glb"),
    clip("slide", "slide.glb"),
    clip("crouch_walk", "crouch_walk_forward.glb"),
    clip("crouch_walk_back", "crouch_walk_backward.glb"),
    clip("crouch_strafe_l", "crouch_walk_left.glb"),
    clip("crouch_strafe_r", "crouch_walk_right.glb"),
    clip("walljump", "wallrun.glb"),
    clip("swim", "swimming.glb"),
    clip("swim_idle", "swimming_idle.glb"),
    clip("climb", "climbing_ladder.glb"),
    optional("dance_1", "dance_1.glb"),
    optional("dance_2", "dance_2.glb"),
    optional("dance_3", "dance_3.glb"),
];
