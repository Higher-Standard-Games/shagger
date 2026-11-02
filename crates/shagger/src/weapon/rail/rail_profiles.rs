//! SHAG `src/engine/weapon/rail/RailProfiles.ts`: the rail's presentation
//! constants. It reuses the Voltaic seed hash and impact kinds.

pub use crate::weapon::voltaic::voltaic_profiles::{
    VoltaicEffectSample as RailEffectSample, VoltaicImpactKind as RailImpactKind, impact_kind_to_float,
    make_voltaic_seed as make_rail_seed,
};

/// `RAIL_PART`: the presentation-part ids in the strip templates' `position.z`.
pub mod rail_part {
    /// Wide, soft outer sheath (straight).
    pub const GLOW: f32 = 0.0;
    /// Thin white-blue hot core (straight).
    pub const CORE: f32 = 1.0;
    pub const HELIX_A: f32 = 2.0;
    /// The helix, opposite phase.
    pub const HELIX_B: f32 = 3.0;
}

/// One-shot lifetime, seconds. Holds full for the first ~35% then fades out.
pub const RAIL_DURATION: f64 = 0.55;

/// How many times the helix wraps the core over the beam length.
pub const RAIL_HELIX_TURNS: f64 = 7.0;

/// Helix radius in world units (the `amplitude` instance parameter).
pub const RAIL_HELIX_RADIUS: f64 = 4.0;
