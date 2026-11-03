//! Port of SHAG `src/engine/rendering/water/WaterSurfaceLighting.ts`:
//! bounded, map-authored water highlights with cached static visibility.
//! Each lit patch gets up to [`MAX_WATER_LIGHTS`] sources and a 16 by 16
//! mask whose channels are each source's visibility, traced once at load.

use glam::DVec3;
use shagger_shared::js::{js_round, js_string_to_number};

use super::static_visibility_index::{Ray, StaticVisibilityIndex};
use super::unified_water_material::MAX_WATER_LIGHTS;
use super::water_probe_cache::WaterPatch;
use crate::lighting::map_emissive_surfaces::EmissiveSurface;
use crate::loader::bsp::bsp_loader::BspEntity;

/// The mask's side in texels.
pub const MASK_SIZE: usize = 16;
const MAX_LIT_PATCHES: usize = 4;

/// `WaterMapLight`, in Babylon space.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterMapLight {
    pub position: DVec3,
    pub range: f64,
    pub colour: [f64; 3],
    pub normal: Option<DVec3>,
    pub sample_environment: bool,
}

/// `waterSurfaceLights`: shader-authored emitting panels, not arbitrary
/// fullbright paint.
#[must_use]
pub fn water_surface_lights(surfaces: &[EmissiveSurface]) -> Vec<WaterMapLight> {
    surfaces
        .iter()
        .filter(|s| s.surface_light > 0.0 && s.surface_light.is_finite())
        .map(|s| WaterMapLight {
            position: s.position + s.normal * 2.0,
            normal: Some(s.normal),
            range: 768f64.min(160f64.max(s.area.sqrt() * 6.0)),
            colour: [3f64.min(s.surface_light / 150.0); 3],
            sample_environment: true,
        })
        .collect()
}

/// `value.trim().split(/\s+/).map(Number)`.
fn numbers(value: &str) -> Vec<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return vec![0.0];
    }
    trimmed.split_whitespace().map(js_string_to_number).collect()
}

/// `waterMapLights`: untargeted `light` entities as points. Never the
/// scene's lights, so weapon and projectile flashes cannot enter.
#[must_use]
pub fn water_map_lights(entities: &[BspEntity]) -> Vec<WaterMapLight> {
    let mut result = Vec::new();
    for entity in entities {
        let p = &entity.properties;
        // Targeted lights need spot or sun semantics; not points.
        let truthy = |v: Option<&str>| v.is_some_and(|v| !v.is_empty());
        if entity.classname != "light" || truthy(p.get("target")) || !truthy(p.get("origin")) {
            continue;
        }
        let xyz = numbers(p.get("origin").unwrap_or(""));
        let intensity = p.get("light").map_or(300.0, js_string_to_number);
        let range = p.get("range").map_or(intensity, js_string_to_number);
        let rgb = numbers(p.get("_color").unwrap_or("1 1 1"));
        if xyz.len() != 3
            || rgb.len() != 3
            || !xyz
                .iter()
                .chain(&rgb)
                .chain([&intensity, &range])
                .all(|v| v.is_finite())
            || intensity <= 0.0
            || range <= 0.0
        {
            continue;
        }
        let scale = if rgb.iter().any(|&c| c > 1.0) { 1.0 / 255.0 } else { 1.0 };
        result.push(WaterMapLight {
            position: DVec3::new(xyz[0], xyz[2], xyz[1]),
            range: range.min(2048.0),
            colour: [0, 1, 2].map(|i| 0f64.max(1f64.min(rgb[i] * scale)) * 4f64.min(intensity / 100.0)),
            normal: None,
            sample_environment: false,
        });
    }
    result
}

/// `PatchLighting`: the mask's RGBA bytes and the uniforms it pairs with.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchLighting {
    pub mask: Vec<u8>,
    pub lights: [[f32; 4]; MAX_WATER_LIGHTS],
    pub colours: [[f32; 4]; MAX_WATER_LIGHTS],
    pub count: usize,
}

/// `WaterSurfaceLighting`, keyed by patch index.
#[derive(Debug, Clone, Default)]
pub struct WaterSurfaceLighting {
    entries: Vec<(usize, PatchLighting)>,
}

impl WaterSurfaceLighting {
    /// `prepare`: `patches` are `(index, patch)` of the reactive surfaces,
    /// `blockers` the static opaque triangles, built only when a patch is lit.
    pub fn prepare(
        &mut self,
        blockers: impl FnOnce() -> Vec<[DVec3; 3]>,
        patches: &[(usize, &WaterPatch)],
        sources: &[WaterMapLight],
    ) {
        let mut blockers = Some(blockers);
        let mut visibility: Option<StaticVisibilityIndex> = None;
        let mut seen = Vec::new();
        for &(key, patch) in patches {
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            if self.entry(key).is_some() || self.entries.len() >= MAX_LIT_PATCHES {
                continue;
            }
            let mut nearest: Vec<(&WaterMapLight, f64)> = sources
                .iter()
                .filter(|light| {
                    light.position.y > patch.min.y + 1.0
                        && light
                            .normal
                            .is_none_or(|n| n.dot(patch.position - light.position) > 0.0)
                })
                .map(|light| {
                    let near = light.position.clamp(patch.min, patch.max);
                    (light, light.position.distance(near))
                })
                .filter(|(light, distance)| *distance < light.range)
                .collect();
            // Stable, as `Array.prototype.sort`.
            nearest.sort_by(|a, b| (a.1 / a.0.range).total_cmp(&(b.1 / b.0.range)));
            nearest.truncate(MAX_WATER_LIGHTS);
            if nearest.is_empty() {
                continue;
            }
            let visibility = visibility
                .get_or_insert_with(|| StaticVisibilityIndex::new(blockers.take().map_or_else(Vec::new, |b| b())));
            let mut lights = [[0.0f32; 4]; MAX_WATER_LIGHTS];
            let mut colours = [[0.0f32; 4]; MAX_WATER_LIGHTS];
            for (i, (light, _)) in nearest.iter().enumerate() {
                let p = light.position.as_vec3();
                lights[i] = [p.x, p.y, p.z, light.range as f32];
                let c = light.colour.map(|c| c as f32);
                colours[i] = [c[0], c[1], c[2], if light.sample_environment { 1.0 } else { 0.0 }];
            }
            // Each channel is one light's visibility, traced at claim only,
            // through hidden static geometry too.
            let mut mask = vec![0u8; MASK_SIZE * MASK_SIZE * 4];
            let size = MASK_SIZE as f64;
            for y in 0..MASK_SIZE {
                for x in 0..MASK_SIZE {
                    let mut point = DVec3::new(
                        patch.min.x + ((x as f64 + 0.5) / size) * (patch.max.x - patch.min.x),
                        patch.min.y,
                        patch.min.z + ((y as f64 + 0.5) / size) * (patch.max.z - patch.min.z),
                    );
                    if !patch.contains(point.x, point.y, point.z) {
                        continue;
                    }
                    point.y += 0.5;
                    for (i, (light, _)) in nearest.iter().enumerate() {
                        let delta = light.position - point;
                        let distance = delta.length();
                        if distance >= light.range {
                            continue;
                        }
                        let ray = Ray {
                            origin: point,
                            direction: delta * (1.0 / distance),
                            length: 0f64.max(distance - 0.5),
                        };
                        let facing = light.normal.map_or(1.0, |n| 0f64.max(-n.dot(ray.direction)));
                        if facing > 0.0 && !visibility.hits(&ray) {
                            mask[(y * MASK_SIZE + x) * 4 + i] = js_round(255.0 * facing) as u8;
                        }
                    }
                }
            }
            self.entries.push((
                key,
                PatchLighting {
                    mask,
                    lights,
                    colours,
                    count: nearest.len(),
                },
            ));
        }
    }

    /// A lit patch's mask and uniforms.
    #[must_use]
    pub fn entry(&self, patch: usize) -> Option<&PatchLighting> {
        self.entries.iter().find(|(k, _)| *k == patch).map(|(_, e)| e)
    }

    /// `lightCount`.
    #[must_use]
    pub fn light_count(&self) -> usize {
        self.entries.iter().map(|(_, e)| e.count).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::bsp::bsp_loader::Properties;

    fn source(pairs: &[(&str, &str)]) -> BspEntity {
        let mut properties = Properties::default();
        for (k, v) in pairs {
            properties.set(k, v);
        }
        BspEntity {
            classname: "light".to_owned(),
            properties,
        }
    }

    /// `CreateGround` 80 by 80: two triangles at y 0.
    fn pool() -> WaterPatch {
        let a = DVec3::new(-40.0, 0.0, -40.0);
        let b = DVec3::new(40.0, 0.0, -40.0);
        let c = DVec3::new(40.0, 0.0, 40.0);
        let d = DVec3::new(-40.0, 0.0, 40.0);
        WaterPatch {
            batches: vec![0],
            min: a,
            max: c,
            position: DVec3::ZERO,
            triangles: vec![[a, b, c], [a, c, d]],
        }
    }

    #[test]
    fn parses_authored_lights_and_excludes_malformed_or_targeted() {
        let mut other = source(&[("origin", "0 0 100")]);
        other.classname = "projectile".to_owned();
        let lights = water_map_lights(&[
            source(&[("origin", "10 20 100"), ("_color", "255 128 0"), ("light", "200")]),
            source(&[("origin", "0 0 100"), ("target", "sun")]),
            source(&[("origin", "NaN 0 0")]),
            source(&[("origin", "0 0 0"), ("light", "-1")]),
            other,
        ]);
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].position, DVec3::new(10.0, 100.0, 20.0));
        assert_eq!(lights[0].range, 200.0);
        assert_eq!(lights[0].colour, [2.0, 256.0 / 255.0, 0.0]);
    }

    #[test]
    fn caps_lights_and_walls_block() {
        let patch = pool();
        let sources = water_map_lights(&vec![source(&[("origin", "0 0 100")]); 8]);
        let mut lighting = WaterSurfaceLighting::default();
        lighting.prepare(Vec::new, &[(0, &patch)], &sources);
        assert_eq!(lighting.light_count(), 4);
        assert!(lighting.entry(0).unwrap().mask.iter().all(|&v| v == 255));
        // A slab at y 45 to 55 over the whole pool.
        let wall = || {
            let (lo, hi) = (-100.0, 100.0);
            let quad = |y| {
                let a = DVec3::new(lo, y, lo);
                let b = DVec3::new(hi, y, lo);
                let c = DVec3::new(hi, y, hi);
                let d = DVec3::new(lo, y, hi);
                [[a, b, c], [a, c, d]]
            };
            [quad(45.0), quad(55.0)].concat()
        };
        let mut blocked = WaterSurfaceLighting::default();
        blocked.prepare(
            wall,
            &[(0, &patch)],
            &water_map_lights(&[source(&[("origin", "0 0 100")])]),
        );
        assert!(blocked.entry(0).unwrap().mask.iter().all(|&v| v == 0));
    }

    #[test]
    fn rejects_underwater_and_out_of_range_sources() {
        let patch = pool();
        let mut lighting = WaterSurfaceLighting::default();
        lighting.prepare(
            Vec::new,
            &[(0, &patch)],
            &water_map_lights(&[
                source(&[("origin", "0 0 -100")]),
                source(&[("origin", "0 0 100"), ("range", "20")]),
            ]),
        );
        assert_eq!(lighting.light_count(), 0);
        assert!(lighting.entry(0).is_none());
    }

    #[test]
    fn only_panels_facing_the_pool() {
        let panel = EmissiveSurface {
            position: DVec3::new(0.0, 100.0, 0.0),
            normal: DVec3::new(0.0, -1.0, 0.0),
            width: 32.0,
            height: 32.0,
            area: 1024.0,
            texture_name: "light-panel".to_owned(),
            texture_index: 0,
            surface_light: 300.0,
        };
        let sources = water_surface_lights(&[
            panel.clone(),
            EmissiveSurface {
                surface_light: 0.0,
                ..panel.clone()
            },
            EmissiveSurface {
                normal: DVec3::Y,
                ..panel
            },
        ]);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].position.y, 98.0);
        let patch = pool();
        let mut lighting = WaterSurfaceLighting::default();
        lighting.prepare(Vec::new, &[(0, &patch)], &sources);
        assert_eq!(lighting.light_count(), 1);
        assert_eq!(lighting.entry(0).unwrap().colours[0][3], 1.0);
    }
}
