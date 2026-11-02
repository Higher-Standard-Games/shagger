//! SHAG `src/engine/weapon/rail/RailBeamGeometry.ts`: the beam and impact
//! template meshes.

use super::rail_profiles::rail_part;
use crate::rendering::effect_quad_batch::EffectTemplate;

struct StripDefinition {
    part: f32,
    nodes: u32,
}

/// Straight parts (core, glow) only need their two endpoints; the helix
/// parts need many nodes so the spiral reads as a smooth curve.
const RAIL_STRIPS: [StripDefinition; 4] = [
    StripDefinition {
        part: rail_part::GLOW,
        nodes: 2,
    },
    StripDefinition {
        part: rail_part::CORE,
        nodes: 2,
    },
    StripDefinition {
        part: rail_part::HELIX_A,
        nodes: 56,
    },
    StripDefinition {
        part: rail_part::HELIX_B,
        nodes: 56,
    },
];

/// `appendStrip`: `nodes` pairs of vertices at (along, ±1, part).
fn append_strip(definition: &StripDefinition, out: &mut EffectTemplate) {
    let base = out.positions.len() as u32;
    let divisor = definition.nodes.saturating_sub(1).max(1);
    for node in 0..definition.nodes {
        let along = (f64::from(node) / f64::from(divisor)) as f32;
        out.positions.push([along, -1.0, definition.part]);
        out.positions.push([along, 1.0, definition.part]);
        out.uvs.push([along, 0.0]);
        out.uvs.push([along, 1.0]);
    }
    for node in 0..definition.nodes.saturating_sub(1) {
        let lower = base + node * 2;
        let upper = lower + 1;
        let next_lower = lower + 2;
        let next_upper = lower + 3;
        out.indices
            .extend_from_slice(&[lower, upper, next_lower, next_lower, upper, next_upper]);
    }
}

/// `buildRailBeamTemplate`: glow, core and the twin helix.
#[must_use]
pub fn build_rail_beam_template() -> EffectTemplate {
    let mut out = EffectTemplate::default();
    for definition in &RAIL_STRIPS {
        append_strip(definition, &mut out);
    }
    out
}

/// `buildRailImpactTemplate`: four quads at (±1, ±1, part); parts 0 and 1
/// are the terminal nucleus and crown, 2 and 3 the muzzle crack.
#[must_use]
pub fn build_rail_impact_template() -> EffectTemplate {
    let mut out = EffectTemplate::default();
    for part in 0..4u32 {
        let base = out.positions.len() as u32;
        let z = part as f32;
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
        let beam = build_rail_beam_template();
        assert_eq!(beam.positions.len(), 2 * (2 + 2 + 56 + 56));
        assert_eq!(beam.indices.len(), 6 * (1 + 1 + 55 + 55));
        assert_eq!(beam.positions[8], [0.0, -1.0, 2.0]);
        assert_eq!(beam.positions[10][0], (1.0f64 / 55.0) as f32);
        let impact = build_rail_impact_template();
        assert_eq!(impact.positions.len(), 16);
        assert_eq!(impact.indices[6..12], [4, 5, 6, 6, 5, 7]);
    }
}
