//! SHAG `src/engine/weapon/voltaic/VoltaicBeamGeometry.ts`: the beam
//! templates for the three profiles and the impact template.

use super::voltaic_profiles::voltaic_part as part;
use crate::rendering::effect_quad_batch::EffectTemplate;

/// `VoltaicBeamProfile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoltaicBeamProfile {
    Hero,
    World,
    Chain,
}

const HERO_VOLUME_LAYERS: u32 = 2;
const VOLUME_LAYER_PART_OFFSET: f32 = 16.0;

/// (part, nodes) per strip.
const HERO_STRIPS: [(f32, u32); 10] = [
    (part::SHEATH, 24),
    (part::CORE, 2),
    (part::DOMINANT, 32),
    (part::SECONDARY_A, 28),
    (part::SECONDARY_B, 24),
    (part::GHOST, 32),
    (part::FORK_A, 8),
    (part::FORK_B, 8),
    (part::FORK_C, 9),
    (part::FORK_D, 9),
];

const WORLD_STRIPS: [(f32, u32); 5] = [
    (part::SHEATH, 20),
    (part::CORE, 2),
    (part::DOMINANT, 24),
    (part::SECONDARY_A, 20),
    (part::FORK_A, 7),
];

const CHAIN_STRIPS: [(f32, u32); 3] = [(part::CORE, 2), (part::DOMINANT, 24), (part::SECONDARY_A, 20)];

/// `appendStrip`: `nodes` pairs at (along, ±1, part + layer × 16).
fn append_strip((strip_part, nodes): (f32, u32), out: &mut EffectTemplate, volume_layer: u32) {
    let base = out.positions.len() as u32;
    let divisor = nodes.saturating_sub(1).max(1);
    let encoded_part = strip_part + volume_layer as f32 * VOLUME_LAYER_PART_OFFSET;
    for node in 0..nodes {
        let along = (f64::from(node) / f64::from(divisor)) as f32;
        out.positions.push([along, -1.0, encoded_part]);
        out.positions.push([along, 1.0, encoded_part]);
        out.uvs.push([along, 0.0]);
        out.uvs.push([along, 1.0]);
    }
    for node in 0..nodes.saturating_sub(1) {
        let lower = base + node * 2;
        out.indices
            .extend_from_slice(&[lower, lower + 1, lower + 2, lower + 2, lower + 1, lower + 3]);
    }
}

/// `buildVoltaicBeamTemplate`: the hero carries two copies of every strip.
#[must_use]
pub fn build_voltaic_beam_template(profile: VoltaicBeamProfile) -> EffectTemplate {
    let definitions: &[(f32, u32)] = match profile {
        VoltaicBeamProfile::Hero => &HERO_STRIPS,
        VoltaicBeamProfile::World => &WORLD_STRIPS,
        VoltaicBeamProfile::Chain => &CHAIN_STRIPS,
    };
    let layers = if profile == VoltaicBeamProfile::Hero {
        HERO_VOLUME_LAYERS
    } else {
        1
    };
    let mut out = EffectTemplate::default();
    for layer in 0..layers {
        for &definition in definitions {
            append_strip(definition, &mut out, layer);
        }
    }
    out
}

/// `buildVoltaicImpactTemplate`: endpoint nucleus and crown, muzzle corona
/// and petals, then eight contact streaks, as twelve quads.
#[must_use]
pub fn build_voltaic_impact_template() -> EffectTemplate {
    let mut out = EffectTemplate::default();
    for quad in 0..12u32 {
        let base = out.positions.len() as u32;
        let z = quad as f32;
        out.positions
            .extend_from_slice(&[[-1.0, -1.0, z], [1.0, -1.0, z], [-1.0, 1.0, z], [1.0, 1.0, z]]);
        out.uvs
            .extend_from_slice(&[[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]]);
        out.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_have_the_browser_counts() {
        let hero = build_voltaic_beam_template(VoltaicBeamProfile::Hero);
        assert_eq!(hero.positions.len(), 2 * 2 * 176);
        assert_eq!(hero.positions[2 * 176][2], 16.0);
        let world = build_voltaic_beam_template(VoltaicBeamProfile::World);
        assert_eq!(world.positions.len(), 2 * 73);
        assert_eq!(world.indices.len(), 6 * (73 - 5));
        let chain = build_voltaic_beam_template(VoltaicBeamProfile::Chain);
        assert_eq!(chain.positions[4], [0.0, -1.0, 2.0]);
        assert_eq!(build_voltaic_impact_template().positions.len(), 48);
    }
}
