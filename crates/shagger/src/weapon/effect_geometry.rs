//! SHAG `src/engine/weapon/EffectGeometry.ts`: streaming an effect's
//! positions into its fixed vertex layout.

use glam::DVec3;

/// `updateUnlitTrailRibbon`'s positions: a double-sided ribbon's two paths,
/// once per side. Normals stay as the ribbon was built; the trails are
/// unlit.
#[must_use]
pub fn unlit_trail_ribbon_positions(path_a: &[DVec3], path_b: &[DVec3]) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity((path_a.len() + path_b.len()) * 2);
    unlit_trail_ribbon_positions_into(path_a, path_b, &mut out);
    out
}

/// [`unlit_trail_ribbon_positions`] into `out`, which it clears first, for
/// a trail that streams its ribbon every frame.
pub fn unlit_trail_ribbon_positions_into(path_a: &[DVec3], path_b: &[DVec3], out: &mut Vec<[f32; 3]>) {
    out.clear();
    for _side in 0..2 {
        for path in [path_a, path_b] {
            out.extend(path.iter().map(|p| [p.x as f32, p.y as f32, p.z as f32]));
        }
    }
}
