//! SHAG `src/engine/weapon/voltaic/VoltaicProfiles.ts`: the impact kinds and
//! the presentation seed the Voltaic beam and the rail share.

use glam::{DVec3, Vec3};

/// `VoltaicImpactKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoltaicImpactKind {
    #[default]
    None,
    World,
    Player,
}

impl VoltaicImpactKind {
    /// `"none"`, `"world"` or `"player"`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Self::None),
            "world" => Some(Self::World),
            "player" => Some(Self::Player),
            _ => None,
        }
    }
}

/// `VoltaicEffectSample`, in Babylon space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoltaicEffectSample {
    pub start: DVec3,
    pub end: DVec3,
    pub color: Vec3,
    pub impact_kind: VoltaicImpactKind,
    pub impact_normal: Option<DVec3>,
    pub intensity: Option<f64>,
}

/// `VOLTAIC_PART`: the presentation-part ids in `position.z`.
pub mod voltaic_part {
    pub const SHEATH: f32 = 0.0;
    pub const CORE: f32 = 1.0;
    pub const DOMINANT: f32 = 2.0;
    pub const SECONDARY_A: f32 = 3.0;
    pub const SECONDARY_B: f32 = 4.0;
    pub const GHOST: f32 = 5.0;
    pub const FORK_A: f32 = 6.0;
    pub const FORK_B: f32 = 7.0;
    pub const FORK_C: f32 = 8.0;
    pub const FORK_D: f32 = 9.0;
}

pub const VOLTAIC_REMOTE_FADE_START: f64 = 0.15;
pub const VOLTAIC_REMOTE_REMOVE_TIME: f64 = 0.21;
pub const VOLTAIC_HERO_RELEASE_TIME: f64 = 0.05;

/// `impactKindToFloat`.
#[must_use]
pub fn impact_kind_to_float(kind: VoltaicImpactKind) -> f64 {
    match kind {
        VoltaicImpactKind::World => 1.0,
        VoltaicImpactKind::Player => 2.0,
        VoltaicImpactKind::None => 0.0,
    }
}

/// `makeVoltaicSeed`: FNV-1a over `key:activation` (UTF-16 code units, as
/// `charCodeAt` reads them), folded into 1..=8191.
#[must_use]
pub fn make_voltaic_seed(key: &str, activation: u32) -> f64 {
    let mut hash: u32 = 0x811c_9dc5;
    for unit in format!("{key}:{activation}").encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    f64::from(hash % 8191 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_match_v8() {
        // node: makeVoltaicSeed("rail", 1), ("rail", 2), ("rail", 1000), ("hero", 7)
        assert_eq!(make_voltaic_seed("rail", 1), 7875.0);
        assert_eq!(make_voltaic_seed("rail", 2), 7037.0);
        assert_eq!(make_voltaic_seed("rail", 1000), 7850.0);
        assert_eq!(make_voltaic_seed("hero", 7), 7449.0);
    }
}
