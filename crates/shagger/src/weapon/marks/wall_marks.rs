//! SHAG `src/engine/weapon/HitscanEffects.ts`'s wall marks (`spawnWallMark`,
//! `ensureWallMarkMaterial`, `ensureWallMarkBatch`, `updateWallMarks`):
//! bullet holes and scorches where shots hit, a flat triangle (Babylon's
//! disc of three segments) facing out of the wall, fading over three
//! seconds, fifty at most, oldest first. One material serves them all, so
//! every mark wears the first one's colour.

use glam::{DVec3, Vec3};

use crate::rendering::effect_quad_batch::{EffectFrame, EffectQuadBatch, EffectTemplate, instance5};
use crate::rendering::gpu::Gpu;
use crate::weapon::weapon_barrels::babylon_euler;

/// `MAX_WALL_MARKS`.
pub const MAX_WALL_MARKS: usize = 50;
/// `WALL_MARK_DURATION`: seconds to a full fade.
pub const WALL_MARK_DURATION: f64 = 3.0;
/// The material's alpha.
const WALL_MARK_ALPHA: f64 = 0.7;

/// `MeshBuilder.CreateDisc({ radius: 1, tessellation: 3 })`: a fan from the
/// centre to three rim points, closed back on the first.
#[must_use]
pub fn wall_mark_template() -> EffectTemplate {
    let mut positions = vec![[0.0, 0.0, 0.0]];
    let step = std::f32::consts::TAU / 3.0;
    for i in 0..3 {
        let a = step * i as f32;
        positions.push([a.cos(), a.sin(), 0.0]);
    }
    positions.push(positions[1]);
    let uvs = positions.iter().map(|p| [p[0], p[1]]).collect();
    let indices = (0..3).flat_map(|i| [i + 1, 0, i + 2]).collect();
    EffectTemplate {
        positions,
        uvs,
        indices,
    }
}

/// The disc's x and y axes for a mark facing out along `normal`:
/// `lookAt(position + normal)` (`setDirection`) turns the node's +Z to the
/// normal by yaw and pitch, no roll.
#[must_use]
pub fn wall_mark_axes(normal: DVec3) -> (Vec3, Vec3) {
    let dv = normal;
    let yaw = -dv.z.atan2(dv.x) + std::f64::consts::FRAC_PI_2;
    let len = (dv.x * dv.x + dv.z * dv.z).sqrt();
    let pitch = -dv.y.atan2(len);
    let rotation = babylon_euler(Vec3::new(pitch as f32, yaw as f32, 0.0));
    (rotation * Vec3::X, rotation * Vec3::Y)
}

#[derive(Debug, Clone, Copy)]
struct WallMark {
    spawn_time: f64,
    position: DVec3,
    axes: (Vec3, Vec3),
    size: f64,
}

/// The marks and their batch.
pub struct WallMarks {
    marks: Vec<WallMark>,
    /// The shared material's emissive: the first mark's colour times 0.3.
    emissive: Option<Vec3>,
    batch: EffectQuadBatch,
}

impl WallMarks {
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        Self {
            marks: Vec::new(),
            emissive: None,
            batch: EffectQuadBatch::with_template(
                gpu,
                color_format,
                "wall_marks",
                include_str!("wall_marks.wgsl"),
                MAX_WALL_MARKS,
                &wall_mark_template(),
            ),
        }
    }

    /// `spawnWallMark` at `now`: a mark 0.05 off the wall, `size` units
    /// across, the oldest dropped past the limit.
    pub fn spawn(&mut self, now: f64, position: DVec3, normal: DVec3, color: Vec3, size: f64) {
        self.emissive.get_or_insert(color * 0.3);
        while self.marks.len() >= MAX_WALL_MARKS {
            self.marks.remove(0);
        }
        self.marks.push(WallMark {
            spawn_time: now,
            position: position + normal * 0.05,
            axes: wall_mark_axes(normal),
            size,
        });
    }

    /// `updateWallMarks`: drops the marks that have faded.
    pub fn update(&mut self, now: f64) {
        self.marks.retain(|m| now - m.spawn_time < WALL_MARK_DURATION);
    }

    /// The marks' alpha at `now`, oldest first.
    #[must_use]
    pub fn alphas(&self, now: f64) -> Vec<f64> {
        self.marks
            .iter()
            .map(|m| WALL_MARK_ALPHA * (1.0 - (now - m.spawn_time) / WALL_MARK_DURATION))
            .collect()
    }

    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame) {
        let emissive = self.emissive.unwrap_or(Vec3::ZERO).as_dvec3();
        let instances: Vec<_> = self
            .marks
            .iter()
            .zip(self.alphas(frame.time))
            .map(|(m, alpha)| {
                let (x, y) = (m.axes.0.as_dvec3(), m.axes.1.as_dvec3());
                let p = m.position;
                instance5([
                    [p.x, p.y, p.z, m.size],
                    [x.x, x.y, x.z, 0.0],
                    [y.x, y.y, y.z, 0.0],
                    [emissive.x, emissive.y, emissive.z, alpha.max(0.0)],
                    [0.0; 4],
                ])
            })
            .collect();
        self.batch.prepare(gpu, frame, &instances, true);
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.batch.draw(pass);
    }

    pub fn clear(&mut self) {
        self.marks.clear();
    }

    /// Live marks, for tests and debugging.
    #[must_use]
    pub fn count(&self) -> usize {
        self.marks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_disc_of_three_segments_is_a_triangle_fan() {
        let t = wall_mark_template();
        assert_eq!(t.positions.len(), 5);
        assert_eq!(t.indices, vec![1, 0, 2, 2, 0, 3, 3, 0, 4]);
        assert_eq!(t.positions[4], t.positions[1]);
    }

    #[test]
    fn a_mark_lies_flat_on_its_wall() {
        for normal in [DVec3::X, DVec3::Y, -DVec3::Z, DVec3::new(1.0, 1.0, 0.5).normalize()] {
            let (x, y) = wall_mark_axes(normal);
            let n = normal.as_vec3();
            assert!(x.dot(n).abs() < 1e-5 && y.dot(n).abs() < 1e-5, "{normal}");
            assert!((x.length() - 1.0).abs() < 1e-5 && (y.length() - 1.0).abs() < 1e-5);
            // Its +Z faces out of the wall.
            assert!((x.cross(y) - n).length() < 1e-5, "{normal}");
        }
    }
}
