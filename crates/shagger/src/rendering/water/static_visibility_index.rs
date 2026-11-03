//! Port of SHAG `src/engine/rendering/water/StaticVisibilityIndex.ts`: a
//! bounding-volume tree over static triangles, answering "does this ray hit
//! anything" for the water's light visibility masks at load.

use glam::DVec3;

/// Babylon's `Epsilon`, a ray's default edge slack.
pub const EPSILON: f64 = 0.001;

/// A ray as Babylon's: origin, unit direction, length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: DVec3,
    pub direction: DVec3,
    pub length: f64,
}

impl Ray {
    /// `Ray.intersectsBoxMinMax` (no threshold): the slab test from the
    /// origin on, without the length.
    fn intersects_box(&self, min: DVec3, max: DVec3) -> bool {
        let mut d = 0.0f64;
        let mut max_value = f64::MAX;
        for axis in 0..3 {
            let (o, dir) = (self.origin[axis], self.direction[axis]);
            if dir.abs() < 0.000_000_1 {
                if o < min[axis] || o > max[axis] {
                    return false;
                }
            } else {
                let inv = 1.0 / dir;
                let mut lo = (min[axis] - o) * inv;
                let mut hi = (max[axis] - o) * inv;
                if hi == f64::NEG_INFINITY {
                    hi = f64::INFINITY;
                }
                if lo > hi {
                    std::mem::swap(&mut lo, &mut hi);
                }
                d = d.max(lo);
                max_value = max_value.min(hi);
                if d > max_value {
                    return false;
                }
            }
        }
        true
    }

    /// `Ray.intersectsTriangle`: two-sided, from the origin to the length,
    /// with Babylon's edge slack (`Epsilon`, 0.001).
    fn intersects_triangle(&self, a: DVec3, b: DVec3, c: DVec3) -> bool {
        let edge1 = b - a;
        let edge2 = c - a;
        let pvec = self.direction.cross(edge2);
        let det = edge1.dot(pvec);
        if det == 0.0 {
            return false;
        }
        let inv = 1.0 / det;
        let tvec = self.origin - a;
        let bv = tvec.dot(pvec) * inv;
        if !(-EPSILON..=1.0 + EPSILON).contains(&bv) {
            return false;
        }
        let qvec = tvec.cross(edge1);
        let bw = self.direction.dot(qvec) * inv;
        if bw < -EPSILON || bv + bw > 1.0 + EPSILON {
            return false;
        }
        let distance = edge2.dot(qvec) * inv;
        distance <= self.length && distance >= 0.0
    }
}

enum Branch {
    Leaf {
        min: DVec3,
        max: DVec3,
        triangles: Vec<usize>,
    },
    Node {
        min: DVec3,
        max: DVec3,
        left: Box<Branch>,
        right: Box<Branch>,
    },
}

/// `StaticVisibilityIndex`.
pub struct StaticVisibilityIndex {
    triangles: Vec<[DVec3; 3]>,
    root: Option<Branch>,
}

impl StaticVisibilityIndex {
    #[must_use]
    pub fn new(triangles: Vec<[DVec3; 3]>) -> Self {
        let mut index = Self { triangles, root: None };
        if !index.triangles.is_empty() {
            index.root = Some(index.build((0..index.triangles.len()).collect()));
        }
        index
    }

    /// `hits`.
    #[must_use]
    pub fn hits(&self, ray: &Ray) -> bool {
        self.root.as_ref().is_some_and(|root| self.intersects(ray, root))
    }

    fn build(&self, mut ids: Vec<usize>) -> Branch {
        let mut min = DVec3::INFINITY;
        let mut max = DVec3::NEG_INFINITY;
        for &id in &ids {
            for p in self.triangles[id] {
                min = min.min(p);
                max = max.max(p);
            }
        }
        if ids.len() <= 16 {
            return Branch::Leaf {
                min,
                max,
                triangles: ids,
            };
        }
        let size = max - min;
        let axis = if size.x >= size.y && size.x >= size.z {
            0
        } else if size.y >= size.z {
            1
        } else {
            2
        };
        let centre = |id: usize| {
            let t = &self.triangles[id];
            t[0][axis] + t[1][axis] + t[2][axis]
        };
        // `Array.prototype.sort` is stable, as `sort_by` is.
        ids.sort_by(|&a, &b| centre(a).partial_cmp(&centre(b)).unwrap_or(std::cmp::Ordering::Equal));
        let right = ids.split_off(ids.len() / 2);
        Branch::Node {
            min,
            max,
            left: Box::new(self.build(ids)),
            right: Box::new(self.build(right)),
        }
    }

    fn intersects(&self, ray: &Ray, branch: &Branch) -> bool {
        match branch {
            Branch::Node { min, max, left, right } => {
                ray.intersects_box(*min, *max) && (self.intersects(ray, left) || self.intersects(ray, right))
            }
            Branch::Leaf { min, max, triangles } => {
                ray.intersects_box(*min, *max)
                    && triangles.iter().any(|&id| {
                        let [a, b, c] = self.triangles[id];
                        ray.intersects_triangle(a, b, c)
                    })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_only_between_the_origin_and_the_length() {
        // Many floor triangles at y 0 and one roof triangle at y 10.
        let mut triangles: Vec<[DVec3; 3]> = (0..40)
            .map(|i| {
                let x = f64::from(i) * 10.0 - 200.0;
                [
                    DVec3::new(x, 0.0, -100.0),
                    DVec3::new(x + 10.0, 0.0, -100.0),
                    DVec3::new(x, 0.0, 100.0),
                ]
            })
            .collect();
        triangles.push([
            DVec3::new(-50.0, 10.0, -50.0),
            DVec3::new(50.0, 10.0, -50.0),
            DVec3::new(0.0, 10.0, 50.0),
        ]);
        let index = StaticVisibilityIndex::new(triangles);
        let up = |length| Ray {
            origin: DVec3::new(0.0, 5.0, 0.0),
            direction: DVec3::Y,
            length,
        };
        assert!(index.hits(&up(20.0)));
        assert!(!index.hits(&up(4.0)));
        // The floor lies behind the origin: no hit.
        let sideways = Ray {
            origin: DVec3::new(300.0, 5.0, 0.0),
            direction: DVec3::Y,
            length: 100.0,
        };
        assert!(!index.hits(&sideways));
    }
}
