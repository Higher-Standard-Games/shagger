//! The Vanguard player model's package and clips (SHAG
//! `src/engine/animation/PlayerModelAssets.ts`, from
//! `config/player-model-vanguard.json`).

/// A clip file and the name the animation controller plays it by
/// (`AnimFileConfig`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimFileConfig {
    pub name: &'static str,
    pub file: &'static str,
    pub optional: bool,
}

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
