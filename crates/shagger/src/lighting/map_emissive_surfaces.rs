//! Port of SHAG `src/engine/lighting/MapEmissiveSurfaces.ts`: the world's
//! light-emitting faces (shaders with `q3map_surfacelight`), each with its
//! centre, normal and size, largest first.

use std::collections::HashMap;

use glam::DVec3;

use crate::loader::bsp::bsp_loader::{BspFile, record_range};

/// `MIN_SURFACE_AREA`: faces smaller than this emit nothing worth placing.
const MIN_SURFACE_AREA: f64 = 256.0;

/// `EmissiveSurface`, in Babylon space.
#[derive(Debug, Clone, PartialEq)]
pub struct EmissiveSurface {
    pub position: DVec3,
    pub normal: DVec3,
    pub width: f64,
    pub height: f64,
    pub area: f64,
    pub texture_name: String,
    pub texture_index: i32,
    pub surface_light: f64,
}

/// `SurfaceEmission`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceEmission {
    pub emissive: bool,
    pub surface_light: f64,
}

/// `extractEmissiveSurfaces`: the world model's emissive faces.
#[must_use]
pub fn extract_emissive_surfaces(bsp: &BspFile, resolve: impl Fn(&str) -> SurfaceEmission) -> Vec<EmissiveSurface> {
    let Some(model) = bsp.models.first() else {
        return Vec::new();
    };
    let mut cache: HashMap<i32, SurfaceEmission> = HashMap::new();
    let mut surfaces = Vec::new();
    for i in record_range(model.first_face, model.num_faces, bsp.faces.len()) {
        let Some(face) = bsp.faces.get(i) else {
            continue;
        };
        if face.num_vertices <= 0 {
            continue;
        }
        let texture_name = usize::try_from(face.texture_index)
            .ok()
            .and_then(|t| bsp.textures.get(t))
            .map_or("", |t| t.name.as_str());
        let emission = *cache.entry(face.texture_index).or_insert_with(|| {
            if texture_name.is_empty() {
                SurfaceEmission {
                    emissive: false,
                    surface_light: 0.0,
                }
            } else {
                resolve(texture_name)
            }
        });
        if !emission.emissive {
            continue;
        }
        let mut sum = DVec3::ZERO;
        let mut min = DVec3::INFINITY;
        let mut max = DVec3::NEG_INFINITY;
        let mut count = 0;
        for v in record_range(face.first_vertex, face.num_vertices, bsp.vertices.len()) {
            let Some(p) = bsp.vertices.position(v) else {
                continue;
            };
            let b = DVec3::new(f64::from(p[0]), f64::from(p[2]), f64::from(p[1]));
            sum += b;
            min = min.min(b);
            max = max.max(b);
            count += 1;
        }
        if count == 0 {
            continue;
        }
        let mut extents = [max.x - min.x, max.y - min.y, max.z - min.z];
        extents.sort_by(|a, b| b.total_cmp(a));
        let (width, height) = (extents[0], extents[1]);
        let area = width * height;
        if area < MIN_SURFACE_AREA {
            continue;
        }
        let normal = DVec3::new(
            f64::from(face.normal[0]),
            f64::from(face.normal[2]),
            f64::from(face.normal[1]),
        );
        if normal.length_squared() < 1e-6 {
            continue;
        }
        surfaces.push(EmissiveSurface {
            position: sum / f64::from(count),
            normal: normal.normalize(),
            width,
            height,
            area,
            texture_name: texture_name.to_owned(),
            texture_index: face.texture_index,
            surface_light: emission.surface_light,
        });
    }
    surfaces.sort_by(|a, b| b.area.total_cmp(&a.area));
    surfaces
}
