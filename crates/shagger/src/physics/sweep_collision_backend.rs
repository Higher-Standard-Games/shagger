//! The Q3 swept-box collision backend (SHAG
//! `src/engine/physics/SweepCollisionBackend.ts`).
//!
//! BSP traversal, brush and patch-facet sweeps, clip brush fallback, inline
//! movers (lifts and `func_explosive`), Q3 slide and step-slide moves, point
//! contents and ladder tests. Public methods take and return Babylon axes
//! (y up); everything inside is Q3 axes (z up), as in the TypeScript.
//!
//! The order of brush and facet tests decides which normal wins between
//! equal-time contacts, so the visit bookkeeping (generations, resets on
//! embedded contacts) is kept exactly.
//!
//! Where the TypeScript would crash on a corrupt map (an inline model whose
//! brush range runs off the lump), this skips the missing brush instead.

use super::collision_backend::{TraceOptions, TraceResult};
use super::collision_spatial_index::{CollisionBoundItem, CollisionBounds, CollisionSpatialIndex, plane_bounds};
use super::facet_bevels::build_facet_planes;
use super::patch_tessellator::{PatchCollisionMesh, build_patch};
use super::player_physics_constants::{PLAYER_HEIGHT, PLAYER_MINS_Z, PLAYER_RADIUS, STEPSIZE};
use crate::babylon::maths::math_vector::Vector3;
use crate::loader::bsp::bsp_loader::{BspBrush, BspCollisionData, BspEntity, BspLeaf, BspModel};
use shagger_shared::bsp::brush_model_state::{BrushModelState, extract_brush_models};
use shagger_shared::bsp::lift_system::{LiftActor, LiftCrush, LiftSystem, LiftTrace, extract_lifts};
use shagger_shared::bsp::map_entities::{EntityLike, ModelLike};
use shagger_shared::js::{js_max, js_min};
use shagger_shared::server::simulation::types::Vec3;
use std::collections::{HashMap, HashSet};

mod content {
    pub const SOLID: i32 = 0x1;
    pub const PLAYERCLIP: i32 = 0x10000;
}

mod surface {
    pub const LADDER: i32 = 0x8;
    pub const HINT: i32 = 0x100;
    pub const NONSOLID: i32 = 0x4000;
}

const PLAYER_CONTENTS: i32 = content::SOLID | content::PLAYERCLIP;
const OVERCLIP: f64 = 1.001;
const MAX_CLIP_PLANES: usize = 5;
const MAX_BUMPS: usize = 4;

impl EntityLike for BspEntity {
    fn classname(&self) -> &str {
        &self.classname
    }
    fn property(&self, key: &str) -> Option<&str> {
        self.properties.get(key)
    }
}

impl ModelLike for BspModel {
    fn mins(&self) -> [f64; 3] {
        self.mins.map(f64::from)
    }
    fn maxs(&self) -> [f64; 3] {
        self.maxs.map(f64::from)
    }
}

#[derive(Debug, Clone)]
struct CollisionTexture {
    name: String,
    content_flags: i32,
    surface_flags: i32,
    collision_mask: i32,
}

/// `FaceCollisionGeometry`: a patch's tessellation and its facets.
#[derive(Debug, Clone)]
pub struct FaceCollisionGeometry {
    pub mesh: PatchCollisionMesh,
    /// `[nx, ny, nz, dist]` per plane, Q3 space.
    pub facet_planes: Vec<f64>,
    /// Start of each triangle's planes, plus a final end marker.
    pub facet_ranges: Vec<usize>,
    facet_index: CollisionSpatialIndex,
}

#[derive(Debug, Clone, Copy)]
struct TraceResultQ3 {
    fraction: f64,
    end_pos: [f64; 3],
    normal: Option<[f64; 3]>,
    start_solid: bool,
    all_solid: bool,
    contents: i32,
    hit_brush_index: i64,
    /// Index into `collision_textures`, or none.
    hit_texture: Option<usize>,
    hit_surface_flags: i32,
}

#[derive(Debug, Clone, Copy)]
struct TraceState {
    start: [f64; 3],
    end: [f64; 3],
    mins: [f64; 3],
    maxs: [f64; 3],
    extents: [f64; 3],
    content_mask: i32,
    face_index: usize,
    result: TraceResultQ3,
}

/// Lift data a trace reads: the definitions and current offsets.
type Lifts<'l> = Option<&'l LiftSystem>;

/// What a slide move returns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideMove {
    pub end_pos: Vector3,
    pub velocity: Vector3,
    pub hit_count: u32,
    pub stepped: bool,
}

/// Counters, as `getStats` reports them (trace timing left out).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CollisionStats {
    pub trace_count: u64,
    pub hit_count: u64,
    pub brush_count: usize,
    pub bounded_brush_count: usize,
    pub clip_brush_count: usize,
    pub collision_triangle_count: usize,
    pub bounded_triangle_count: usize,
    pub partially_bounded_triangle_count: usize,
    pub redundant_brush_count: usize,
    pub pruned_brush_tests: u64,
}

/// `SweepCollisionBackend`.
#[derive(Debug, Clone)]
pub struct SweepCollisionBackend {
    bsp: BspCollisionData,
    pub lifts: Option<LiftSystem>,
    pub brush_models: Option<BrushModelState>,
    brush_model_brushes: HashSet<i64>,
    brush_model_faces: HashSet<i64>,
    lift_brushes: HashMap<i64, usize>,
    lift_faces: HashSet<i64>,
    ladder_brushes: Vec<usize>,
    face_collision: Vec<Option<FaceCollisionGeometry>>,
    clip_brush_indices: Vec<usize>,
    brush_visits: Vec<u32>,
    face_visits: Vec<u32>,
    trace_generation: u32,
    collision_textures: Vec<CollisionTexture>,
    brush_bounds: Vec<Option<CollisionBounds>>,
    clip_index: Option<CollisionSpatialIndex>,
    facet_candidates: Vec<usize>,
    clip_candidates: Vec<usize>,
    brush_parents: Vec<i64>,
    stats: CollisionStats,
}

fn idx(i: i64) -> Option<usize> {
    usize::try_from(i).ok()
}

fn get<T>(list: &[T], i: impl Into<i64>) -> Option<&T> {
    idx(i.into()).and_then(|i| list.get(i))
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn plane_normal(n: [f32; 3]) -> [f64; 3] {
    n.map(f64::from)
}

/// `[from, from + count)` as i64, the way the TypeScript loops it.
fn range(first: i32, count: i32) -> std::ops::Range<i64> {
    i64::from(first)..i64::from(first) + i64::from(count)
}

impl SweepCollisionBackend {
    /// `init`.
    #[must_use]
    pub fn new(bsp: BspCollisionData) -> Self {
        let brush_models = BrushModelState::new(extract_brush_models(&bsp.entities, &bsp.models));
        let mut brush_model_brushes = HashSet::new();
        let mut brush_model_faces = HashSet::new();
        for d in &brush_models.definitions {
            let m = &bsp.models[d.model_index];
            brush_model_brushes.extend(range(m.first_brush, m.num_brushes));
            brush_model_faces.extend(range(m.first_face, m.num_faces));
        }
        let definitions = extract_lifts(&bsp.entities, &bsp.models);
        let mut lift_brushes = HashMap::new();
        let mut lift_faces = HashSet::new();
        for d in &definitions {
            let m = &bsp.models[d.model_index];
            for i in range(m.first_brush, m.num_brushes) {
                lift_brushes.insert(i, d.model_index);
            }
            lift_faces.extend(range(m.first_face, m.num_faces));
        }
        let collision_textures = bsp
            .textures
            .iter()
            .map(|t| {
                let name = t.name.to_lowercase();
                let (flags, surface_flags) = (t.content_flags, t.surface_flags);
                let mut mask = 0;
                if surface_flags & (surface::NONSOLID | surface::HINT) == 0 {
                    if flags & content::SOLID != 0
                        || name.contains("fullclip")
                        || (name.contains("clip") && !name.contains("playerclip"))
                    {
                        mask |= content::SOLID;
                    }
                    if flags & content::PLAYERCLIP != 0 || name.contains("playerclip") {
                        mask |= content::PLAYERCLIP;
                    }
                }
                CollisionTexture {
                    name: t.name.clone(),
                    content_flags: flags,
                    surface_flags,
                    collision_mask: mask,
                }
            })
            .collect();
        let brush_bounds = bsp
            .brushes
            .iter()
            .map(|brush| {
                let mut planes = Vec::new();
                for i in range(brush.first_side, brush.num_sides) {
                    let side = get(&bsp.brush_sides, i)?;
                    let plane = get(&bsp.planes, side.plane_index)?;
                    planes.extend(plane_normal(plane.normal));
                    planes.push(f64::from(plane.dist));
                }
                plane_bounds(&planes, 0, planes.len() / 4, false)
            })
            .collect();
        let brushes = bsp.brushes.len();
        let faces = bsp.faces.len();
        let mut backend = Self {
            bsp,
            lifts: Some(LiftSystem::new(definitions)),
            brush_models: Some(brush_models),
            brush_model_brushes,
            brush_model_faces,
            lift_brushes,
            lift_faces,
            ladder_brushes: Vec::new(),
            face_collision: Vec::new(),
            clip_brush_indices: Vec::new(),
            brush_visits: vec![0; brushes],
            face_visits: vec![0; faces],
            trace_generation: 0,
            collision_textures,
            brush_bounds,
            clip_index: None,
            facet_candidates: Vec::new(),
            clip_candidates: Vec::new(),
            brush_parents: Vec::new(),
            stats: CollisionStats::default(),
        };
        backend.process_faces();
        backend.build_clip_brush_index();
        backend.build_ladder_brush_index();
        backend.build_brush_containment();
        backend
    }

    #[must_use]
    pub fn bsp(&self) -> &BspCollisionData {
        &self.bsp
    }

    fn brush_texture(&self, brush: &BspBrush) -> Option<usize> {
        let ok = |i: i64| idx(i).filter(|&i| i < self.collision_textures.len());
        ok(i64::from(brush.shader_index))
            .or_else(|| get(&self.bsp.brush_sides, brush.first_side).and_then(|side| ok(i64::from(side.shader_index))))
    }

    fn should_collide_with(&self, texture: usize, mask: i32) -> bool {
        self.collision_textures[texture].collision_mask & mask != 0
    }

    /// `buildBrushContainment`: strict containment only.
    fn build_brush_containment(&mut self) {
        let bsp = &self.bsp;
        let mut parents = vec![-1i64; bsp.brushes.len()];
        let mut inline = HashSet::new();
        for m in bsp.models.iter().skip(1) {
            inline.extend(range(m.first_brush, m.num_brushes));
        }
        let ladders: HashSet<usize> = self.ladder_brushes.iter().copied().collect();
        let clips: HashSet<usize> = self.clip_brush_indices.iter().copied().collect();
        // Leaves that reference each brush, in leaf order; and each leaf's brushes.
        let mut leaves: Vec<Vec<usize>> = vec![Vec::new(); bsp.brushes.len()];
        let leaf_brushes: Vec<HashSet<i64>> = bsp
            .leafs
            .iter()
            .enumerate()
            .map(|(li, leaf)| {
                let mut refs = HashSet::new();
                for i in range(leaf.first_leaf_brush, leaf.num_leaf_brushes) {
                    if let Some(&bi) = get(&bsp.leaf_brushes, i) {
                        refs.insert(i64::from(bi));
                        if let Some(l) = idx(i64::from(bi)).and_then(|b| leaves.get_mut(b)) {
                            if !l.contains(&li) {
                                l.push(li);
                            }
                        }
                    }
                }
                refs
            })
            .collect();
        let eligible: Vec<(usize, CollisionBounds, usize)> = bsp
            .brushes
            .iter()
            .enumerate()
            .filter_map(|(id, brush)| {
                let bounds = self.brush_bounds[id]?;
                let texture = self.brush_texture(brush)?;
                (self.collision_textures[texture].collision_mask != 0
                    && !inline.contains(&(id as i64))
                    && !ladders.contains(&id))
                .then_some((id, bounds, texture))
            })
            .collect();
        let index = CollisionSpatialIndex::new(
            &eligible
                .iter()
                .map(|&(id, bounds, _)| CollisionBoundItem {
                    id,
                    bounds: Some(bounds),
                })
                .collect::<Vec<_>>(),
        );
        for &(inner, inner_bounds, inner_texture) in &eligible {
            for id in index.query(&inner_bounds) {
                if id == inner || self.brush_bounds[id].is_none() {
                    continue;
                }
                let outer = &bsp.brushes[id];
                let Some(texture) = self.brush_texture(outer) else {
                    continue;
                };
                let (t, it) = (
                    &self.collision_textures[texture],
                    &self.collision_textures[inner_texture],
                );
                if t.content_flags != it.content_flags || t.collision_mask != it.collision_mask {
                    continue;
                }
                if clips.contains(&inner) && !clips.contains(&id) {
                    continue;
                }
                if leaves[inner].is_empty() && !clips.contains(&inner) {
                    continue;
                }
                if leaves[inner].iter().any(|&li| !leaf_brushes[li].contains(&(id as i64))) {
                    continue;
                }
                let mut contained = true;
                for i in range(outer.first_side, outer.num_sides) {
                    // Bounds exist, so every side and plane does.
                    let side = get(&bsp.brush_sides, i).expect("bounded brush side");
                    let plane = get(&bsp.planes, side.plane_index).expect("bounded brush plane");
                    let n = plane_normal(plane.normal);
                    let mut distance = 0.0;
                    for axis in 0..3 {
                        distance += n[axis] * inner_bounds[axis + if n[axis] < 0.0 { 0 } else { 3 }];
                    }
                    if !(distance < f64::from(plane.dist) - 0.125) {
                        contained = false;
                        break;
                    }
                }
                if contained {
                    parents[inner] = id as i64;
                    break;
                }
            }
        }
        self.brush_parents = parents;
    }

    fn hull_starts_outside_brush(&self, brush: &BspBrush, state: &TraceState) -> bool {
        for i in range(brush.first_side, brush.num_sides) {
            let side = get(&self.bsp.brush_sides, i).expect("parent brush side");
            let plane = get(&self.bsp.planes, side.plane_index).expect("parent brush plane");
            let n = plane_normal(plane.normal);
            let mut distance = -f64::from(plane.dist);
            for axis in 0..3 {
                distance += (state.start[axis]
                    + if n[axis] < 0.0 {
                        state.maxs[axis]
                    } else {
                        state.mins[axis]
                    })
                    * n[axis];
            }
            if distance > 0.0 {
                return true;
            }
        }
        false
    }

    fn build_ladder_brush_index(&mut self) {
        self.ladder_brushes = (0..self.bsp.brushes.len())
            .filter(|&bi| {
                let b = &self.bsp.brushes[bi];
                b.num_sides >= 4 && self.is_ladder_brush(b)
            })
            .collect();
    }

    fn is_ladder_brush(&self, brush: &BspBrush) -> bool {
        let flags = |shader: i32| get(&self.bsp.textures, shader).map_or(0, |t| t.surface_flags);
        if flags(brush.shader_index) & surface::LADDER != 0 {
            return true;
        }
        range(brush.first_side, brush.num_sides)
            .filter_map(|i| get(&self.bsp.brush_sides, i))
            .any(|side| flags(side.shader_index) & surface::LADDER != 0)
    }

    /// `isInLadder`: is any part of this Babylon-space box inside a climbable brush?
    pub fn is_in_ladder(&self, min: Vector3, max: Vector3) -> bool {
        if self.ladder_brushes.is_empty() {
            return false;
        }
        let centre = [(min.x + max.x) / 2.0, (min.z + max.z) / 2.0, (min.y + max.y) / 2.0];
        let half = [(max.x - min.x) / 2.0, (max.z - min.z) / 2.0, (max.y - min.y) / 2.0];
        self.ladder_brushes
            .iter()
            .any(|&bi| self.point_in_brush(centre, &self.bsp.brushes[bi], Some(half)))
    }

    /// `getFaceCollision`: diagnostics only.
    #[must_use]
    pub fn face_collision(&self, face: usize) -> Option<&FaceCollisionGeometry> {
        self.face_collision.get(face).and_then(Option::as_ref)
    }

    fn build_clip_brush_index(&mut self) {
        self.clip_brush_indices = (0..self.bsp.brushes.len())
            .filter(|&bi| {
                if self.lift_brushes.contains_key(&(bi as i64)) || self.brush_model_brushes.contains(&(bi as i64)) {
                    return false;
                }
                let brush = &self.bsp.brushes[bi];
                if brush.num_sides < 4 {
                    return false;
                }
                self.brush_texture(brush).is_some_and(|t| {
                    let name = self.collision_textures[t].name.to_lowercase();
                    name.contains("fullclip") || name.contains("playerclip")
                })
            })
            .collect();
        let items: Vec<CollisionBoundItem> = self
            .clip_brush_indices
            .iter()
            .map(|&id| CollisionBoundItem {
                id,
                bounds: self.brush_bounds[id],
            })
            .collect();
        self.clip_index = Some(CollisionSpatialIndex::new(&items));
    }

    fn swept_bounds(state: &TraceState) -> CollisionBounds {
        let mut b = [0.0; 6];
        for axis in 0..3 {
            b[axis] = js_min(state.start[axis], state.end[axis]) + state.mins[axis];
            b[axis + 3] = js_max(state.start[axis], state.end[axis]) + state.maxs[axis];
        }
        b
    }

    fn process_faces(&mut self) {
        let mut out = vec![None; self.bsp.faces.len()];
        for (i, face) in self.bsp.faces.iter().enumerate() {
            if self.lift_faces.contains(&(i as i64)) || self.brush_model_faces.contains(&(i as i64)) {
                continue;
            }
            if face.face_type != 2 {
                continue;
            }
            let Some(texture) = get(&self.bsp.textures, face.texture_index) else {
                continue;
            };
            if texture.content_flags & (content::SOLID | content::PLAYERCLIP) == 0 {
                continue;
            }
            if let Some(mesh) = build_patch(&self.bsp.vertices, face, 5) {
                out[i] = Some(Self::build_facets(mesh, &mut self.stats));
            }
        }
        self.face_collision = out;
    }

    fn build_facets(mesh: PatchCollisionMesh, stats: &mut CollisionStats) -> FaceCollisionGeometry {
        let triangles = mesh.indices.len() / 3;
        let mut packed = Vec::new();
        let mut ranges = Vec::with_capacity(triangles + 1);
        for t in 0..triangles {
            ranges.push(packed.len() / 4);
            let v = |k: usize| mesh.vertices[mesh.indices[t * 3 + k]];
            let (a, b, c) = (v(0), v(1), v(2));
            for p in build_facet_planes([a.x, a.z, a.y], [b.x, b.z, b.y], [c.x, c.z, c.y]) {
                packed.extend_from_slice(&[p.normal[0], p.normal[1], p.normal[2], p.dist]);
            }
        }
        ranges.push(packed.len() / 4);
        let mut items = Vec::new();
        for t in 0..triangles {
            if ranges[t + 1] <= ranges[t] {
                continue;
            }
            let bounds = plane_bounds(&packed, ranges[t], ranges[t + 1], true);
            items.push(CollisionBoundItem { id: t, bounds });
            stats.collision_triangle_count += 1;
            if let Some(b) = bounds {
                if b.iter().all(|v| v.is_finite()) {
                    stats.bounded_triangle_count += 1;
                } else {
                    stats.partially_bounded_triangle_count += 1;
                }
            }
        }
        FaceCollisionGeometry {
            mesh,
            facet_planes: packed,
            facet_ranges: ranges,
            facet_index: CollisionSpatialIndex::new(&items),
        }
    }

    /// `trace`, with no excluded or exclusive model.
    pub fn trace(&mut self, start: Vector3, end: Vector3, options: TraceOptions) -> TraceResult {
        self.trace_models(start, end, options, -1, -1)
    }

    /// `trace(start, end, options, excludeModel, onlyModel)`.
    pub fn trace_models(
        &mut self,
        start: Vector3,
        end: Vector3,
        options: TraceOptions,
        exclude_model: i64,
        only_model: i64,
    ) -> TraceResult {
        let lifts = self.lifts.take();
        let result = self.trace_with(lifts.as_ref(), start, end, options, exclude_model, only_model);
        self.lifts = lifts;
        result
    }

    #[allow(clippy::too_many_lines)]
    fn trace_with(
        &mut self,
        lifts: Lifts<'_>,
        start: Vector3,
        end: Vector3,
        options: TraceOptions,
        exclude_model: i64,
        only_model: i64,
    ) -> TraceResult {
        self.stats.trace_count += 1;
        self.trace_generation = self.trace_generation.wrapping_add(1);
        if self.trace_generation == 0 {
            self.brush_visits.fill(0);
            self.face_visits.fill(0);
            self.trace_generation = 1;
        }
        let q3_start = [start.x, start.z, start.y];
        let q3_end = [end.x, end.z, end.y];
        let radius = options.radius.unwrap_or(PLAYER_RADIUS);
        let height = options.height.unwrap_or(PLAYER_HEIGHT);
        let mins_z = -(height * (-PLAYER_MINS_Z / PLAYER_HEIGHT));
        let maxs_z = height + mins_z;
        let mins = [-radius, -radius, mins_z];
        let maxs = [radius, radius, maxs_z];
        let content_mask = options.content_mask.unwrap_or(PLAYER_CONTENTS);
        let mut state = TraceState {
            start: q3_start,
            end: q3_end,
            mins,
            maxs,
            extents: std::array::from_fn(|i| js_max(mins[i].abs(), maxs[i].abs())),
            content_mask,
            face_index: 0,
            result: TraceResultQ3 {
                fraction: 1.0,
                end_pos: q3_end,
                normal: None,
                start_solid: false,
                all_solid: false,
                contents: 0,
                hit_brush_index: -1,
                hit_texture: None,
                hit_surface_flags: 0,
            },
        };

        if only_model < 0 {
            self.trace_node(0, 0.0, 1.0, q3_start, q3_end, &mut state);
            self.trace_all_clip_brushes(&mut state);
        }

        if let Some(lifts) = lifts {
            for (li, d) in lifts.definitions.iter().enumerate() {
                let model_index = d.model_index as i64;
                if model_index == exclude_model || (only_model >= 0 && model_index != only_model) {
                    continue;
                }
                let shift = [d.origin.x, d.origin.z, d.origin.y + lifts.travel_at(li)];
                let model = self.bsp.models[d.model_index];
                let (mmin, mmax) = (model.mins.map(f64::from), model.maxs.map(f64::from));
                let apart = (0..3).any(|j| {
                    js_min(q3_start[j], q3_end[j]) + mins[j] > mmax[j] + shift[j] + 0.1
                        || js_max(q3_start[j], q3_end[j]) + maxs[j] < mmin[j] + shift[j] - 0.1
                });
                if apart {
                    continue;
                }
                for j in 0..3 {
                    state.start[j] = q3_start[j] - shift[j];
                    state.end[j] = q3_end[j] - shift[j];
                }
                self.trace_model_brushes(&model, &mut state);
            }
        }
        if let Some(models) = &self.brush_models {
            let enabled: Vec<(usize, Vec3)> = models
                .definitions
                .iter()
                .filter(|d| models.is_enabled(d.model_index))
                .map(|d| (d.model_index, d.origin))
                .collect();
            for (model_index, origin) in enabled {
                let mi = model_index as i64;
                if mi == exclude_model || (only_model >= 0 && mi != only_model) {
                    continue;
                }
                state.start = [q3_start[0] - origin.x, q3_start[1] - origin.z, q3_start[2] - origin.y];
                state.end = [q3_end[0] - origin.x, q3_end[1] - origin.z, q3_end[2] - origin.y];
                let model = self.bsp.models[model_index];
                self.trace_model_brushes(&model, &mut state);
            }
        }
        state.start = q3_start;
        state.end = q3_end;

        let r = state.result;
        let mut end_pos = r.end_pos;
        if r.fraction < 1.0 {
            for i in 0..3 {
                end_pos[i] = q3_start[i] + r.fraction * (q3_end[i] - q3_start[i]);
            }
            self.stats.hit_count += 1;
        }
        TraceResult {
            fraction: r.fraction,
            end_pos: Vector3::new(end_pos[0], end_pos[2], end_pos[1]),
            normal: r.normal.map(|n| Vector3::new(n[0], n[2], n[1])),
            start_solid: r.start_solid,
            all_solid: r.all_solid,
            contents: r.contents,
            hit_brush_index: r.hit_brush_index,
            hit_texture_name: r
                .hit_texture
                .map_or_else(String::new, |t| self.collision_textures[t].name.clone()),
            hit_surface_flags: r.hit_surface_flags,
        }
    }

    fn trace_model_brushes(&mut self, model: &BspModel, state: &mut TraceState) {
        for i in range(model.first_brush, model.num_brushes) {
            let Some(&brush) = get(&self.bsp.brushes, i) else {
                continue;
            };
            let Some(texture) = self.brush_texture(&brush) else {
                continue;
            };
            if self.should_collide_with(texture, state.content_mask) {
                self.trace_brush(&brush, i as usize, texture, state);
            }
        }
    }

    fn trace_node(
        &mut self,
        node_index: i32,
        start_fraction: f64,
        end_fraction: f64,
        start: [f64; 3],
        end: [f64; 3],
        state: &mut TraceState,
    ) {
        if state.result.fraction <= start_fraction {
            return;
        }
        if node_index < 0 {
            let leaf_index = -(i64::from(node_index) + 1);
            if let Some(&leaf) = get(&self.bsp.leafs, leaf_index) {
                self.trace_leaf(&leaf, state);
            }
            return;
        }
        let Some(&node) = get(&self.bsp.nodes, node_index) else {
            return;
        };
        let Some(&plane) = get(&self.bsp.planes, node.plane_index) else {
            return;
        };
        let n = plane_normal(plane.normal);
        let dist = f64::from(plane.dist);
        let (start_dist, end_dist, offset);
        if n[0] == 1.0 || n[0] == -1.0 {
            start_dist = start[0] - dist;
            end_dist = end[0] - dist;
            offset = state.extents[0];
        } else if n[1] == 1.0 || n[1] == -1.0 {
            start_dist = start[1] - dist;
            end_dist = end[1] - dist;
            offset = state.extents[1];
        } else if n[2] == 1.0 || n[2] == -1.0 {
            start_dist = start[2] - dist;
            end_dist = end[2] - dist;
            offset = state.extents[2];
        } else {
            start_dist = dot(start, n) - dist;
            end_dist = dot(end, n) - dist;
            offset =
                (state.extents[0] * n[0]).abs() + (state.extents[1] * n[1]).abs() + (state.extents[2] * n[2]).abs();
        }

        if start_dist >= offset && end_dist >= offset {
            self.trace_node(node.children[0], start_fraction, end_fraction, start, end, state);
        } else if start_dist < -offset && end_dist < -offset {
            self.trace_node(node.children[1], start_fraction, end_fraction, start, end, state);
        } else {
            let (side, mut fraction1, mut fraction2);
            if start_dist < end_dist {
                side = 1;
                let inv = 1.0 / (start_dist - end_dist);
                fraction1 = (start_dist - offset + 0.03125) * inv;
                fraction2 = (start_dist + offset + 0.03125) * inv;
            } else if start_dist > end_dist {
                side = 0;
                let inv = 1.0 / (start_dist - end_dist);
                fraction1 = (start_dist + offset + 0.03125) * inv;
                fraction2 = (start_dist - offset - 0.03125) * inv;
            } else {
                side = 0;
                fraction1 = 1.0;
                fraction2 = 0.0;
            }
            fraction1 = js_max(0.0, js_min(1.0, fraction1));
            fraction2 = js_max(0.0, js_min(1.0, fraction2));

            let mid_fraction = start_fraction + (end_fraction - start_fraction) * fraction1;
            let mid = std::array::from_fn(|k| start[k] + fraction1 * (end[k] - start[k]));
            self.trace_node(node.children[side], start_fraction, mid_fraction, start, mid, state);

            let mid_fraction2 = start_fraction + (end_fraction - start_fraction) * fraction2;
            let mid2 = std::array::from_fn(|k| start[k] + fraction2 * (end[k] - start[k]));
            self.trace_node(node.children[side ^ 1], mid_fraction2, end_fraction, mid2, end, state);
        }
    }

    fn trace_leaf(&mut self, leaf: &BspLeaf, state: &mut TraceState) {
        for i in range(leaf.first_leaf_brush, leaf.num_leaf_brushes) {
            let Some(&bi) = get(&self.bsp.leaf_brushes, i) else {
                continue;
            };
            let bi64 = i64::from(bi);
            if self.lift_brushes.contains_key(&bi64) || self.brush_model_brushes.contains(&bi64) {
                continue;
            }
            let Some(brush_index) = idx(bi64).filter(|&b| b < self.bsp.brushes.len()) else {
                continue;
            };
            if self.brush_visits[brush_index] == self.trace_generation {
                continue;
            }
            let brush = self.bsp.brushes[brush_index];
            if brush.num_sides < 1 {
                continue;
            }
            let Some(texture) = self.brush_texture(&brush) else {
                continue;
            };
            if !self.should_collide_with(texture, state.content_mask) {
                continue;
            }
            self.brush_visits[brush_index] = self.trace_generation;
            self.trace_brush(&brush, brush_index, texture, state);
        }
        self.trace_leaf_faces(leaf, state);
    }

    fn trace_leaf_faces(&mut self, leaf: &BspLeaf, state: &mut TraceState) {
        if leaf.num_leaf_faces == 0 {
            return;
        }
        let bounds = Self::swept_bounds(state);
        for i in range(leaf.first_leaf_face, leaf.num_leaf_faces) {
            let Some(&fi) = get(&self.bsp.leaf_faces, i) else {
                continue;
            };
            let Some(face_index) = idx(i64::from(fi)).filter(|&f| f < self.face_collision.len()) else {
                continue;
            };
            let Some(geometry) = self.face_collision[face_index].take() else {
                continue;
            };
            self.trace_face(face_index, &geometry, &bounds, state);
            self.face_collision[face_index] = Some(geometry);
        }
    }

    fn trace_face(
        &mut self,
        face_index: usize,
        geometry: &FaceCollisionGeometry,
        bounds: &CollisionBounds,
        state: &mut TraceState,
    ) {
        if self.face_visits[face_index] == self.trace_generation {
            return;
        }
        self.face_visits[face_index] = self.trace_generation;
        let (mn, mx) = (geometry.mesh.mins, geometry.mesh.maxs);
        if bounds[3] < mn.x
            || bounds[0] > mx.x
            || bounds[4] < mn.z
            || bounds[1] > mx.z
            || bounds[5] < mn.y
            || bounds[2] > mx.y
        {
            return;
        }
        let texture_index = self.bsp.faces[face_index].texture_index;
        let Some(texture) = idx(i64::from(texture_index)).filter(|&t| t < self.collision_textures.len()) else {
            return;
        };
        if !self.should_collide_with(texture, state.content_mask) {
            return;
        }
        state.face_index = face_index;
        let mut candidates = std::mem::take(&mut self.facet_candidates);
        geometry.facet_index.query_into(bounds, &mut candidates);
        for &t in &candidates {
            self.trace_facet(
                &geometry.facet_planes,
                geometry.facet_ranges[t],
                geometry.facet_ranges[t + 1],
                texture,
                state,
            );
        }
        self.facet_candidates = candidates;
    }

    fn support_offset(state: &TraceState, n: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|k| if n[k] < 0.0 { state.maxs[k] } else { state.mins[k] })
    }

    fn trace_facet(&mut self, planes: &[f64], first: usize, last: usize, texture: usize, state: &mut TraceState) {
        let mut enter = -1.0;
        let mut leave = 1.0;
        let (mut start_out, mut end_out) = (false, false);
        let mut hit_plane: Option<usize> = None;
        for i in first..last {
            let n = [planes[i * 4], planes[i * 4 + 1], planes[i * 4 + 2]];
            let dist = planes[i * 4 + 3];
            let o = Self::support_offset(state, n);
            let sd =
                (state.start[0] + o[0]) * n[0] + (state.start[1] + o[1]) * n[1] + (state.start[2] + o[2]) * n[2] - dist;
            let ed = (state.end[0] + o[0]) * n[0] + (state.end[1] + o[1]) * n[1] + (state.end[2] + o[2]) * n[2] - dist;
            if sd > 0.0 {
                start_out = true;
            }
            if ed > 0.0 {
                end_out = true;
            }
            if sd > 0.0 && ed > 0.0 {
                return;
            }
            if sd <= 0.0 && ed <= 0.0 {
                continue;
            }
            if sd > ed {
                let f = (sd - 0.03125) / (sd - ed);
                if f > enter {
                    enter = f;
                    hit_plane = Some(i);
                }
            } else {
                let f = (sd + 0.03125) / (sd - ed);
                if f < leave {
                    leave = f;
                }
            }
        }
        let t = &self.collision_textures[texture];
        if !start_out {
            self.face_visits[state.face_index] = 0;
            let mut best = f64::NEG_INFINITY;
            let mut best_plane: Option<usize> = None;
            for i in first..last {
                let n = [planes[i * 4], planes[i * 4 + 1], planes[i * 4 + 2]];
                let o = Self::support_offset(state, n);
                let d =
                    (state.start[0] + o[0]) * n[0] + (state.start[1] + o[1]) * n[1] + (state.start[2] + o[2]) * n[2]
                        - planes[i * 4 + 3];
                if d > best {
                    best = d;
                    best_plane = Some(i);
                }
            }
            state.result.start_solid = true;
            state.result.contents = t.content_flags;
            if let Some(p) = best_plane {
                state.result.normal = Some([planes[p * 4], planes[p * 4 + 1], planes[p * 4 + 2]]);
            }
            if !end_out {
                state.result.all_solid = true;
                state.result.fraction = 0.0;
            }
            return;
        }
        if enter < leave && enter > -1.0 && enter < state.result.fraction {
            state.result.fraction = if enter < 0.0 { 0.0 } else { enter };
            state.result.normal = hit_plane.map(|p| [planes[p * 4], planes[p * 4 + 1], planes[p * 4 + 2]]);
            state.result.contents = t.content_flags;
            state.result.hit_texture = Some(texture);
            state.result.hit_surface_flags = t.surface_flags;
        }
    }

    fn trace_all_clip_brushes(&mut self, state: &mut TraceState) {
        if state.content_mask & content::PLAYERCLIP == 0 {
            return;
        }
        let mut candidates = std::mem::take(&mut self.clip_candidates);
        match &self.clip_index {
            Some(index) => index.query_into(&Self::swept_bounds(state), &mut candidates),
            None => candidates.clone_from(&self.clip_brush_indices),
        }
        for &bi in &candidates {
            if self.brush_visits[bi] == self.trace_generation {
                continue;
            }
            self.brush_visits[bi] = self.trace_generation;
            let brush = self.bsp.brushes[bi];
            if brush.num_sides < 4 {
                continue;
            }
            let Some(texture) = self.brush_texture(&brush) else {
                continue;
            };
            self.trace_brush(&brush, bi, texture, state);
        }
        self.clip_candidates = candidates;
    }

    #[allow(clippy::too_many_lines)]
    fn trace_brush(&mut self, brush: &BspBrush, brush_index: usize, texture: usize, state: &mut TraceState) {
        if let Some(b) = self.brush_bounds.get(brush_index).copied().flatten() {
            for axis in 0..3 {
                if js_max(state.start[axis], state.end[axis]) + state.maxs[axis] < b[axis]
                    || js_min(state.start[axis], state.end[axis]) + state.mins[axis] > b[axis + 3]
                {
                    return;
                }
            }
        }
        let parent = self.brush_parents.get(brush_index).copied().unwrap_or(-1);
        if parent >= 0 {
            let parent_brush = self.bsp.brushes[parent as usize];
            if self.hull_starts_outside_brush(&parent_brush, state) {
                self.stats.pruned_brush_tests += 1;
                return;
            }
        }
        let mut enter = -1.0;
        let mut leave = 1.0;
        let (mut start_out, mut end_out) = (false, false);
        let mut hit_normal: Option<[f64; 3]> = None;
        let mut hit_side_shader: i32 = -1;
        for i in range(brush.first_side, brush.num_sides) {
            let Some(&side) = get(&self.bsp.brush_sides, i) else {
                continue;
            };
            let Some(&plane) = get(&self.bsp.planes, side.plane_index) else {
                continue;
            };
            let n = plane_normal(plane.normal);
            let dist = f64::from(plane.dist);
            let o = Self::support_offset(state, n);
            let sd =
                (state.start[0] + o[0]) * n[0] + (state.start[1] + o[1]) * n[1] + (state.start[2] + o[2]) * n[2] - dist;
            let ed = (state.end[0] + o[0]) * n[0] + (state.end[1] + o[1]) * n[1] + (state.end[2] + o[2]) * n[2] - dist;
            if sd > 0.0 {
                start_out = true;
            }
            if ed > 0.0 {
                end_out = true;
            }
            if sd > 0.0 && ed > 0.0 {
                return;
            }
            if sd <= 0.0 && ed <= 0.0 {
                continue;
            }
            if sd > ed {
                let f = (sd - 0.03125) / (sd - ed);
                if f > enter {
                    enter = f;
                    hit_normal = Some(n);
                    hit_side_shader = side.shader_index;
                }
            } else {
                let f = (sd + 0.03125) / (sd - ed);
                if f < leave {
                    leave = f;
                }
            }
        }
        let (content_flags, surface_flags) = (
            self.collision_textures[texture].content_flags,
            self.collision_textures[texture].surface_flags,
        );
        if !start_out {
            self.brush_visits[brush_index] = 0;
            let mut best = f64::NEG_INFINITY;
            let mut best_normal: Option<[f64; 3]> = None;
            for i in range(brush.first_side, brush.num_sides) {
                let Some(&side) = get(&self.bsp.brush_sides, i) else {
                    continue;
                };
                let Some(&plane) = get(&self.bsp.planes, side.plane_index) else {
                    continue;
                };
                let n = plane_normal(plane.normal);
                let o = Self::support_offset(state, n);
                let d =
                    (state.start[0] + o[0]) * n[0] + (state.start[1] + o[1]) * n[1] + (state.start[2] + o[2]) * n[2]
                        - f64::from(plane.dist);
                if d > best {
                    best = d;
                    best_normal = Some(n);
                }
            }
            state.result.start_solid = true;
            state.result.contents = content_flags;
            if best_normal.is_some() {
                state.result.normal = best_normal;
            }
            if !end_out {
                state.result.all_solid = true;
                state.result.fraction = 0.0;
            }
            return;
        }
        if enter < leave && enter > -1.0 && enter < state.result.fraction {
            state.result.fraction = if enter < 0.0 { 0.0 } else { enter };
            state.result.normal = hit_normal;
            state.result.contents = content_flags;
            state.result.hit_brush_index = brush_index as i64;
            state.result.hit_texture = Some(texture);
            state.result.hit_surface_flags =
                get(&self.bsp.textures, hit_side_shader).map_or(surface_flags, |t| t.surface_flags);
        }
    }

    /// `clipVelocity` (Q3 `PM_ClipVelocity`).
    #[must_use]
    pub fn clip_velocity(velocity: Vector3, normal: Vector3, overbounce: f64) -> Vector3 {
        let backoff = Vector3::dot(velocity, normal) * overbounce;
        velocity.subtract(normal.scale(backoff))
    }

    /// `slideMove` (Q3 `PM_SlideMove`): `velocity` is this step's displacement.
    #[allow(clippy::too_many_lines)]
    pub fn slide_move(&mut self, start_pos: Vector3, velocity: Vector3, options: TraceOptions) -> SlideMove {
        let mut planes: Vec<Vector3> = Vec::new();
        let mut pos = start_pos;
        let mut vel = velocity;
        let mut time_left = 1.0;
        let mut hit_count = 0;
        let done = |pos, vel, hit_count| SlideMove {
            end_pos: pos,
            velocity: vel,
            hit_count,
            stepped: false,
        };
        for _ in 0..MAX_BUMPS {
            if vel.length_squared() < 0.0001 {
                break;
            }
            let end = pos.add(vel.scale(time_left));
            let trace = self.trace(pos, end, options);
            if trace.all_solid {
                const NUDGE: f64 = 2.0;
                let nudges = [
                    Vector3::new(NUDGE, 0.0, 0.0),
                    Vector3::new(-NUDGE, 0.0, 0.0),
                    Vector3::new(0.0, NUDGE, 0.0),
                    Vector3::new(0.0, -NUDGE, 0.0),
                    Vector3::new(0.0, 0.0, NUDGE),
                    Vector3::new(0.0, 0.0, -NUDGE),
                    Vector3::new(NUDGE, NUDGE, 0.0),
                    Vector3::new(-NUDGE, NUDGE, 0.0),
                    Vector3::new(0.0, NUDGE, NUDGE),
                    Vector3::new(0.0, NUDGE, -NUDGE),
                ];
                let mut escaped = false;
                for nudge in nudges {
                    let test = pos.add(nudge);
                    let t = self.trace(test, test, options);
                    if !t.all_solid && !t.start_solid {
                        pos = test;
                        escaped = true;
                        break;
                    }
                }
                if !escaped {
                    return done(pos, Vector3::zero(), hit_count);
                }
                continue;
            }
            if trace.start_solid {
                if let Some(n) = trace.normal {
                    pos.add_in_place(n.scale(0.05));
                }
            }
            if trace.fraction > 0.0 {
                pos = trace.end_pos;
            }
            if trace.fraction >= 1.0 {
                break;
            }
            hit_count += 1;
            time_left -= time_left * trace.fraction;
            let Some(normal) = trace.normal else {
                vel = Vector3::zero();
                break;
            };
            if planes.iter().any(|&p| Vector3::dot(normal, p) > 0.99) {
                vel = vel.add(normal);
                continue;
            }
            if planes.len() >= MAX_CLIP_PLANES {
                vel = Vector3::zero();
                break;
            }
            planes.push(normal);
            let mut clipped;
            for i in 0..planes.len() {
                if Vector3::dot(vel, planes[i]) >= 0.0 {
                    continue;
                }
                clipped = Self::clip_velocity(vel, planes[i], OVERCLIP);
                for j in 0..planes.len() {
                    if j == i || Vector3::dot(clipped, planes[j]) >= 0.0 {
                        continue;
                    }
                    clipped = Self::clip_velocity(clipped, planes[j], OVERCLIP);
                    if Vector3::dot(clipped, planes[i]) >= 0.0 {
                        continue;
                    }
                    let dir = Vector3::cross(planes[i], planes[j]).normalize();
                    let d = Vector3::dot(dir, vel);
                    clipped = dir.scale(d);
                    for k in 0..planes.len() {
                        if k == i || k == j || Vector3::dot(clipped, planes[k]) >= 0.0 {
                            continue;
                        }
                        return done(pos, Vector3::zero(), hit_count);
                    }
                }
                vel = clipped;
                break;
            }
        }
        done(pos, vel, hit_count)
    }

    /// `stepSlideMove`: slide, and try stepping up stairs and small ledges.
    /// `start_pos` is the hull origin, not the feet.
    pub fn step_slide_move(&mut self, start_pos: Vector3, velocity: Vector3, options: TraceOptions) -> SlideMove {
        let (start_o, start_v) = (start_pos, velocity);
        let first = self.slide_move(start_pos, velocity, options);
        if first.hit_count == 0 {
            return SlideMove { hit_count: 0, ..first };
        }
        let down = self.trace(
            start_o,
            Vector3::new(start_o.x, start_o.y - STEPSIZE, start_o.z),
            options,
        );
        if down.fraction >= 1.0 {
            return first;
        }
        if down.normal.is_some_and(|n| n.y < 0.7) {
            return first;
        }
        let up = self.trace(
            start_o,
            Vector3::new(start_o.x, start_o.y + STEPSIZE, start_o.z),
            options,
        );
        if up.all_solid {
            return first;
        }
        let step = up.end_pos.y - start_o.y;
        let raised = self.slide_move(up.end_pos, start_v, options);
        let lowered = Vector3::new(raised.end_pos.x, raised.end_pos.y - step, raised.end_pos.z);
        let step_down = self.trace(raised.end_pos, lowered, options);
        if !step_down.all_solid {
            let stepped = step_down.end_pos;
            let mut velocity = raised.velocity;
            if step_down.fraction < 1.0 {
                if let Some(n) = step_down.normal {
                    velocity = Self::clip_velocity(raised.velocity, n, OVERCLIP);
                }
            }
            let flat = Vector3::distance(start_o, first.end_pos);
            let climbed = Vector3::distance(start_o, stepped);
            if climbed > flat + 0.1 {
                return SlideMove {
                    end_pos: stepped,
                    velocity,
                    hit_count: raised.hit_count,
                    stepped: true,
                };
            }
        }
        first
    }

    /// `pointContents`: the raw content flags of every volume containing a
    /// Babylon-space point, OR-ed. `None` for the mask means everything.
    pub fn point_contents(&self, point: Vector3, content_mask: Option<i32>) -> i32 {
        let q3 = [point.x, point.z, point.y];
        let Some(leaf) = get(&self.bsp.leafs, self.point_leaf_index(q3)) else {
            return 0;
        };
        let mut contents = 0;
        for i in range(leaf.first_leaf_brush, leaf.num_leaf_brushes) {
            let Some(&bi) = get(&self.bsp.leaf_brushes, i) else {
                continue;
            };
            let bi64 = i64::from(bi);
            if self.lift_brushes.contains_key(&bi64) || self.brush_model_brushes.contains(&bi64) {
                continue;
            }
            let Some(brush) = get(&self.bsp.brushes, bi64) else {
                continue;
            };
            if brush.num_sides < 1 {
                continue;
            }
            let Some(texture) = self.brush_texture(brush) else {
                continue;
            };
            let flags = self.collision_textures[texture].content_flags;
            if flags == 0 || contents & flags == flags {
                continue;
            }
            if self.point_in_brush(q3, brush, None) {
                contents |= flags;
            }
        }
        if let Some(lifts) = &self.lifts {
            for (li, d) in lifts.definitions.iter().enumerate() {
                let local = [
                    point.x - d.origin.x,
                    point.z - d.origin.z,
                    point.y - (d.origin.y + lifts.travel_at(li)),
                ];
                let model = &self.bsp.models[d.model_index];
                if (0..3).any(|j| local[j] < f64::from(model.mins[j]) || local[j] > f64::from(model.maxs[j])) {
                    continue;
                }
                contents |= self.model_contents(model, local);
            }
        }
        if let Some(models) = &self.brush_models {
            for d in models.definitions.iter().filter(|d| models.is_enabled(d.model_index)) {
                let local = [q3[0] - d.origin.x, q3[1] - d.origin.z, q3[2] - d.origin.y];
                contents |= self.model_contents(&self.bsp.models[d.model_index], local);
            }
        }
        content_mask.map_or(contents, |m| contents & m)
    }

    fn model_contents(&self, model: &BspModel, local: [f64; 3]) -> i32 {
        let mut contents = 0;
        for i in range(model.first_brush, model.num_brushes) {
            let Some(brush) = get(&self.bsp.brushes, i) else {
                continue;
            };
            if let Some(texture) = self.brush_texture(brush) {
                if self.point_in_brush(local, brush, None) {
                    contents |= self.collision_textures[texture].content_flags;
                }
            }
        }
        contents
    }

    fn point_leaf_index(&self, q3: [f64; 3]) -> i64 {
        let mut node_index: i32 = 0;
        while node_index >= 0 {
            let Some(node) = get(&self.bsp.nodes, node_index) else {
                return -1;
            };
            let Some(plane) = get(&self.bsp.planes, node.plane_index) else {
                return -1;
            };
            let d = dot(q3, plane_normal(plane.normal)) - f64::from(plane.dist);
            node_index = if d >= 0.0 { node.children[0] } else { node.children[1] };
        }
        -(i64::from(node_index) + 1)
    }

    /// Behind every side plane, pushed out by `half`'s reach when given.
    fn point_in_brush(&self, q3: [f64; 3], brush: &BspBrush, half: Option<[f64; 3]>) -> bool {
        for i in range(brush.first_side, brush.num_sides) {
            let Some(side) = get(&self.bsp.brush_sides, i) else {
                return false;
            };
            let Some(plane) = get(&self.bsp.planes, side.plane_index) else {
                return false;
            };
            let n = plane_normal(plane.normal);
            let offset = half.map_or(0.0, |h| (h[0] * n[0]).abs() + (h[1] * n[1]).abs() + (h[2] * n[2]).abs());
            if dot(q3, n) - f64::from(plane.dist) > offset {
                return false;
            }
        }
        true
    }

    /// `findGround`: the floor within `max_distance` below, with the player
    /// hull unless `options` say otherwise.
    pub fn find_ground(
        &mut self,
        point: Vector3,
        max_distance: f64,
        options: TraceOptions,
    ) -> Option<(Vector3, Vector3)> {
        let end = point.add(Vector3::new(0.0, -max_distance, 0.0));
        let options = TraceOptions {
            radius: Some(options.radius.unwrap_or(PLAYER_RADIUS)),
            height: Some(options.height.unwrap_or(PLAYER_HEIGHT)),
            content_mask: options.content_mask,
        };
        let r = self.trace(point, end, options);
        if r.fraction < 1.0 {
            if let Some(n) = r.normal {
                return Some((r.end_pos, n));
            }
        }
        None
    }

    /// `isOnGround`.
    pub fn is_on_ground(&mut self, point: Vector3) -> bool {
        self.find_ground(point, 2.0, TraceOptions::default()).is_some()
    }

    /// `LiftSystem.step` with this backend answering the lifts' traces.
    pub fn step_lifts(&mut self, dt: f64, actors: &mut [LiftActor]) -> Vec<LiftCrush> {
        let Some(mut lifts) = self.lifts.take() else {
            return Vec::new();
        };
        let crushes = lifts.step(dt, actors, &mut |system, start, end, exclude, only| {
            let options = TraceOptions {
                radius: Some(16.0),
                height: Some(64.0),
                content_mask: None,
            };
            let hit = self.trace_with(
                Some(system),
                Vector3::new(start.x, start.y, start.z),
                Vector3::new(end.x, end.y, end.z),
                options,
                exclude,
                only,
            );
            LiftTrace {
                fraction: hit.fraction,
                solid: hit.start_solid,
                normal_y: hit.normal.map_or(0.0, |n| n.y),
                model_index: self.lift_brushes.get(&hit.hit_brush_index).map_or(-1, |&m| m as i64),
            }
        });
        self.lifts = Some(lifts);
        crushes
    }

    #[must_use]
    pub fn stats(&self) -> CollisionStats {
        CollisionStats {
            brush_count: self.bsp.brushes.len(),
            bounded_brush_count: self.brush_bounds.iter().filter(|b| b.is_some()).count(),
            clip_brush_count: self.clip_brush_indices.len(),
            redundant_brush_count: self.brush_parents.iter().filter(|&&p| p >= 0).count(),
            ..self.stats
        }
    }
}
