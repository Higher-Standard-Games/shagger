//! Port of SHAG `src/engine/rendering/water/WaterProbeCache.ts`: one cached
//! room cubemap per connected, flat pool, captured once at load from a point
//! just above the water, with the room's bounds for parallax. Nothing
//! recaptures while playing.
//!
//! The patches and capture placement are here; the cube itself is drawn by
//! [`LiquidSurfaceSystem::capture_reflections`](super::liquid_surface_system::LiquidSurfaceSystem::capture_reflections).
//! `scene.pickWithRay` over the static meshes becomes [`StaticTriangles`],
//! the world's triangles with their batches' flags.

use std::collections::{HashMap, HashSet};

use glam::DVec3;
use shagger_shared::js::js_round;

use crate::rendering::world_geometry::WorldGeometry;

/// `MAX_PROBES`.
pub const MAX_PROBES: usize = 4;
/// `PROBE_SIZE`: pixels per cube face.
pub const PROBE_SIZE: u32 = 256;

/// `WaterPatch`: a connected flat liquid surface, by the batches it spans.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterPatch {
    pub batches: Vec<usize>,
    pub min: DVec3,
    pub max: DVec3,
    pub position: DVec3,
    pub triangles: Vec<[DVec3; 3]>,
}

/// `inTriangle`: inside in XZ, either winding, with a hundredth of slack.
fn in_triangle(x: f64, z: f64, t: &[DVec3; 3]) -> bool {
    let cross = |a: DVec3, b: DVec3| (b.x - a.x) * (z - a.z) - (b.z - a.z) * (x - a.x);
    let (a, b, c) = (cross(t[0], t[1]), cross(t[1], t[2]), cross(t[2], t[0]));
    (a >= -0.01 && b >= -0.01 && c >= -0.01) || (a <= 0.01 && b <= 0.01 && c <= 0.01)
}

fn vertex_key(p: DVec3) -> (i64, i64) {
    (js_round(p.x * 100.0) as i64, js_round(p.z * 100.0) as i64)
}

impl WaterPatch {
    /// `contains`: within three units of the surface's height and over one
    /// of its triangles.
    #[must_use]
    pub fn contains(&self, x: f64, y: f64, z: f64) -> bool {
        (y - self.min.y).abs() < 3.0 && self.triangles.iter().any(|t| in_triangle(x, z, t))
    }
}

/// `waterPatch`: a batch's surface when it is flat and connected.
#[must_use]
pub fn water_patch(batch: usize, vertices: &[DVec3], indices: &[u32]) -> Option<WaterPatch> {
    if indices.is_empty() {
        return None;
    }
    let mut min = DVec3::INFINITY;
    let mut max = DVec3::NEG_INFINITY;
    for &i in indices {
        let p = *vertices.get(i as usize)?;
        min = min.min(p);
        max = max.max(p);
    }
    if max.y - min.y > 0.1 || max.x - min.x < 1.0 || max.z - min.z < 1.0 {
        return None;
    }
    let mut triangles: Vec<[DVec3; 3]> = Vec::new();
    let mut parents: Vec<usize> = Vec::new();
    let mut seen: HashMap<(i64, i64), usize> = HashMap::new();
    fn find(parents: &mut [usize], mut i: usize) -> usize {
        while parents[i] != i {
            parents[i] = parents[parents[i]];
            i = parents[i];
        }
        i
    }
    let mut largest = 0.0;
    let mut position = (min + max) * 0.5;
    for tri in indices.as_chunks::<3>().0 {
        let t = [
            vertices[tri[0] as usize],
            vertices[tri[1] as usize],
            vertices[tri[2] as usize],
        ];
        let area = ((t[1].x - t[0].x) * (t[2].z - t[0].z) - (t[1].z - t[0].z) * (t[2].x - t[0].x)).abs();
        if area < 0.001 {
            continue;
        }
        let id = triangles.len();
        parents.push(id);
        triangles.push(t);
        for p in t {
            let key = vertex_key(p);
            if let Some(&other) = seen.get(&key) {
                let (a, b) = (find(&mut parents, id), find(&mut parents, other));
                parents[a] = b;
            } else {
                seen.insert(key, id);
            }
        }
        if area > largest {
            largest = area;
            position = (t[0] + t[1] + t[2]) * (1.0 / 3.0);
        }
    }
    if triangles.is_empty() {
        return None;
    }
    let root = find(&mut parents, 0);
    if (0..parents.len()).any(|i| find(&mut parents, i) != root) {
        return None;
    }
    let centre = (min + max) * 0.5;
    if triangles.iter().any(|t| in_triangle(centre.x, centre.z, t)) {
        position = centre;
    }
    position.y += 2.0;
    Some(WaterPatch {
        batches: vec![batch],
        min,
        max,
        position,
        triangles,
    })
}

/// A static triangle for picking: corners, vertex normals, and its batch.
#[derive(Debug, Clone, Copy)]
struct PickTriangle {
    p: [DVec3; 3],
    n: [DVec3; 3],
    batch: usize,
}

/// A pick: distance along the ray and the raw interpolated vertex normal
/// (`getNormal(true)` with the ray cleared).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickHit {
    pub distance: f64,
    pub normal: DVec3,
}

/// The static meshes `claim` gathers (the world without liquids or
/// autosprites), for `scene.pickWithRay`.
pub struct StaticTriangles {
    triangles: Vec<PickTriangle>,
    /// Per batch: drawn with alpha blending (`needAlphaBlendingForMesh`).
    blended: Vec<bool>,
}

impl StaticTriangles {
    /// `liquid` marks the liquid batches.
    #[must_use]
    pub fn new(geometry: &WorldGeometry, liquid: &[bool]) -> Self {
        let mut triangles = Vec::new();
        let mut blended = Vec::with_capacity(geometry.batches.len());
        for (index, batch) in geometry.batches.iter().enumerate() {
            let first = batch.passes.first();
            blended.push(first.is_some_and(|p| p.is_transparent));
            if liquid.get(index).copied().unwrap_or(false) || first.is_some_and(|p| p.autosprite || p.autosprite2) {
                continue;
            }
            let start = batch.first_index as usize;
            let end = start + batch.index_count as usize;
            for tri in geometry.indices[start..end].as_chunks::<3>().0 {
                let v = tri.map(|i| &geometry.vertices[i as usize]);
                triangles.push(PickTriangle {
                    p: v.map(|v| DVec3::from(v.position.map(f64::from))),
                    n: v.map(|v| DVec3::from(v.normal.map(f64::from))),
                    batch: index,
                });
            }
        }
        Self { triangles, blended }
    }

    /// The closest hit within `length` (two-sided, as Babylon's ray test),
    /// skipping blended batches when `opaque_only`.
    #[must_use]
    pub fn pick(&self, origin: DVec3, direction: DVec3, length: f64, opaque_only: bool) -> Option<PickHit> {
        let mut best: Option<(f64, PickHit)> = None;
        for t in &self.triangles {
            if opaque_only && self.blended[t.batch] {
                continue;
            }
            // Möller–Trumbore, as `Ray.intersectsTriangle`.
            let edge1 = t.p[1] - t.p[0];
            let edge2 = t.p[2] - t.p[0];
            let pvec = direction.cross(edge2);
            let det = edge1.dot(pvec);
            if det == 0.0 {
                continue;
            }
            let inv = 1.0 / det;
            let tvec = origin - t.p[0];
            let bu = tvec.dot(pvec) * inv;
            if !(0.0..=1.0).contains(&bu) {
                continue;
            }
            let qvec = tvec.cross(edge1);
            let bv = direction.dot(qvec) * inv;
            if bv < 0.0 || bu + bv > 1.0 {
                continue;
            }
            let distance = edge2.dot(qvec) * inv;
            if distance > length || distance < 0.0 {
                continue;
            }
            if best.is_none_or(|(d, _)| distance < d) {
                let normal = (t.n[0] * (1.0 - bu - bv) + t.n[1] * bu + t.n[2] * bv).normalize_or_zero();
                best = Some((distance, PickHit { distance, normal }));
            }
        }
        best.map(|(_, hit)| hit)
    }
}

/// A capture the cache plans: where, and the room it parallaxes against.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbePlan {
    /// Index into [`WaterProbeCache::patches`].
    pub patch: usize,
    pub position: DVec3,
    pub room_min: DVec3,
    pub room_max: DVec3,
}

/// `WaterProbeCache`'s patches, merged across batches that share an edge.
#[derive(Debug, Clone, Default)]
pub struct WaterProbeCache {
    pub patches: Vec<WaterPatch>,
    /// The patch of each claimed batch, in claiming order.
    by_batch: Vec<(usize, usize)>,
    probe_lift: f64,
}

impl WaterProbeCache {
    /// `claim`: a patch per flat, connected reactive batch, then patches at
    /// one height that share a vertex merged.
    #[must_use]
    pub fn new(geometry: &WorldGeometry, liquids: &[usize], probe_lift: f64) -> Self {
        let vertices: Vec<DVec3> = geometry
            .vertices
            .iter()
            .map(|v| DVec3::from(v.position.map(f64::from)))
            .collect();
        let mut patches: Vec<WaterPatch> = Vec::new();
        let mut by_batch = Vec::new();
        for &batch in liquids {
            let b = &geometry.batches[batch];
            let start = b.first_index as usize;
            let indices = &geometry.indices[start..start + b.index_count as usize];
            if let Some(patch) = water_patch(batch, &vertices, indices) {
                by_batch.push((batch, patches.len()));
                patches.push(patch);
            }
        }
        // Merge until nothing merges: the first pair found each round.
        'merging: loop {
            for i in 0..patches.len() {
                let keys: HashSet<(i64, i64)> = patches[i].triangles.iter().flatten().map(|&p| vertex_key(p)).collect();
                for j in i + 1..patches.len() {
                    let (a, b) = (&patches[i], &patches[j]);
                    if (a.min.y - b.min.y).abs() > 0.1
                        || !b
                            .triangles
                            .iter()
                            .any(|t| t.iter().any(|&p| keys.contains(&vertex_key(p))))
                    {
                        continue;
                    }
                    let mut triangles = a.triangles.clone();
                    triangles.extend_from_slice(&b.triangles);
                    let (min, max) = (a.min.min(b.min), a.max.max(b.max));
                    let centre = (min + max) * 0.5;
                    let position = if triangles.iter().any(|t| in_triangle(centre.x, centre.z, t)) {
                        centre + DVec3::new(0.0, 2.0, 0.0)
                    } else {
                        a.position
                    };
                    let mut batches = a.batches.clone();
                    batches.extend_from_slice(&b.batches);
                    patches[i] = WaterPatch {
                        batches,
                        min,
                        max,
                        position,
                        triangles,
                    };
                    patches.remove(j);
                    for entry in &mut by_batch {
                        if entry.1 == j {
                            entry.1 = i;
                        } else if entry.1 > j {
                            entry.1 -= 1;
                        }
                    }
                    continue 'merging;
                }
            }
            break;
        }
        Self {
            patches,
            by_batch,
            probe_lift,
        }
    }

    /// `patchFor`.
    #[must_use]
    pub fn patch_for(&self, batch: usize) -> Option<usize> {
        self.by_batch.iter().find(|(b, _)| *b == batch).map(|(_, p)| *p)
    }

    /// `surfaceAt`: the nearest patch surface within 64 units under or over
    /// a point.
    #[must_use]
    pub fn surface_at(&self, position: DVec3) -> Option<DVec3> {
        self.patches
            .iter()
            .filter(|p| (p.min.y - position.y).abs() <= 64.0 && p.contains(position.x, p.min.y, position.z))
            .min_by(|a, b| (a.min.y - position.y).abs().total_cmp(&(b.min.y - position.y).abs()))
            .map(|p| DVec3::new(position.x, p.min.y, position.z))
    }

    /// `captureAll`'s order (`nextPatch`): up to [`MAX_PROBES`] patches, in
    /// claiming order, those with an open spot above the water.
    #[must_use]
    pub fn plan(&self, picks: &StaticTriangles) -> Vec<ProbePlan> {
        let mut plans: Vec<ProbePlan> = Vec::new();
        for &(_, patch) in &self.by_batch {
            if plans.len() >= MAX_PROBES {
                break;
            }
            if plans.iter().any(|p| p.patch == patch) {
                continue;
            }
            if let Some(plan) = self.create_entry(patch, picks) {
                plans.push(plan);
            }
        }
        plans
    }

    /// `createEntry`: the capture point and the room around it.
    fn create_entry(&self, index: usize, picks: &StaticTriangles) -> Option<ProbePlan> {
        let patch = &self.patches[index];
        let position = self.capture_position(patch, picks)?;
        let mut room_min = position - DVec3::new(1024.0, 256.0, 1024.0);
        let mut room_max = position + DVec3::new(1024.0, 1024.0, 1024.0);
        for axis in 0..3 {
            for sign in [-1.0, 1.0] {
                let mut direction = DVec3::ZERO;
                direction[axis] = sign;
                if let Some(hit) = picks.pick(position, direction, 2048.0, false)
                    && hit.distance > 4.0
                {
                    let bound = if sign > 0.0 { &mut room_max } else { &mut room_min };
                    bound[axis] = position[axis] + sign * hit.distance;
                }
            }
        }
        room_min.x = room_min.x.min(patch.min.x - 1.0);
        room_min.z = room_min.z.min(patch.min.z - 1.0);
        room_max.x = room_max.x.max(patch.max.x + 1.0);
        room_max.z = room_max.z.max(patch.max.z + 1.0);
        room_min.y = room_min.y.min(patch.min.y - 1.0);
        Some(ProbePlan {
            patch: index,
            position,
            room_min,
            room_max,
        })
    }

    /// `capturePosition`: the first candidate that sees no back face in any
    /// axis direction, lifted by `r_water_probe_height` below any ceiling.
    fn capture_position(&self, patch: &WaterPatch, picks: &StaticTriangles) -> Option<DVec3> {
        let mut candidates = vec![patch.position];
        for x in [0.25, 0.75, 0.125, 0.875] {
            for z in [0.25, 0.75, 0.125, 0.875] {
                let point = DVec3::new(
                    patch.min.x + (patch.max.x - patch.min.x) * x,
                    patch.min.y + 2.0,
                    patch.min.z + (patch.max.z - patch.min.z) * z,
                );
                if patch.contains(point.x, patch.min.y, point.z) {
                    candidates.push(point);
                }
            }
        }
        for t in patch.triangles.iter().take(24) {
            candidates.push((t[0] + t[1] + t[2]) * (1.0 / 3.0) + DVec3::new(0.0, 2.0, 0.0));
        }
        let directions = [DVec3::X, DVec3::NEG_X, DVec3::Y, DVec3::NEG_Y, DVec3::Z, DVec3::NEG_Z];
        let anchor = candidates.into_iter().find(|&position| {
            directions
                .iter()
                .all(|&direction| match picks.pick(position, direction, 2048.0, true) {
                    None => true,
                    Some(hit) => hit.distance > 1.0 && (hit.normal == DVec3::ZERO || hit.normal.dot(direction) <= 0.01),
                })
        })?;
        let lift = if self.probe_lift.is_finite() {
            self.probe_lift.clamp(0.0, 128.0)
        } else {
            0.0
        };
        let allowed = match picks.pick(anchor, DVec3::Y, lift + 4.0, true) {
            Some(hit) => lift.min((hit.distance - 4.0).max(0.0)),
            None => lift,
        };
        Some(anchor + DVec3::new(0.0, allowed, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_square_is_one_patch() {
        let v = [
            DVec3::new(0.0, 10.0, 0.0),
            DVec3::new(100.0, 10.0, 0.0),
            DVec3::new(100.0, 10.0, 100.0),
            DVec3::new(0.0, 10.0, 100.0),
        ];
        let patch = water_patch(3, &v, &[0, 1, 2, 0, 2, 3]).unwrap();
        assert_eq!(patch.position, DVec3::new(50.0, 12.0, 50.0));
        assert!(patch.contains(10.0, 11.0, 90.0));
        assert!(!patch.contains(10.0, 20.0, 90.0));
        // Two separate triangles are not one pool.
        let far = [
            v[0],
            v[1],
            v[2],
            DVec3::new(500.0, 10.0, 500.0),
            DVec3::new(600.0, 10.0, 500.0),
            DVec3::new(600.0, 10.0, 600.0),
        ];
        assert!(water_patch(0, &far, &[0, 1, 2, 3, 4, 5]).is_none());
        // A slope is not flat.
        let slope = [v[0], v[1], DVec3::new(100.0, 30.0, 100.0)];
        assert!(water_patch(0, &slope, &[0, 1, 2]).is_none());
    }
}
