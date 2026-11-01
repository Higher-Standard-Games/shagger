//! Facet bevel planes for triangle collision (SHAG
//! `src/engine/physics/FacetBevels.ts`, after Quake 3's `CM_AddFacetBevels`).
//!
//! Each triangle becomes a closed convex plane set with outward normals: its
//! own plane, a thin back plane, border planes, axial bevels and the
//! edge-by-axis bevels that support it. Q3 space (z up).

use shagger_shared::js::js_max;

/// How thick a triangle is made before sweeping it.
const FACET_THICKNESS: f64 = 0.5;
/// Planes closer than this in orientation are the same plane.
const PARALLEL_EPSILON: f64 = 0.999;
/// A facet point this far in front of a candidate plane disqualifies it.
const FRONT_EPSILON: f64 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FacetPlane {
    pub normal: [f64; 3],
    pub dist: f64,
}

type Vec3 = [f64; 3];

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Normalizes in place by division, returning the original length, or 0
/// (leaving `v` alone) when shorter than 1e-6.
fn normalize(v: &mut Vec3) -> f64 {
    let length = dot(*v, *v).sqrt();
    if length < 1e-6 {
        return 0.0;
    }
    v[0] /= length;
    v[1] /= length;
    v[2] /= length;
    length
}

fn is_duplicate(planes: &[FacetPlane], normal: Vec3, dist: f64) -> bool {
    planes
        .iter()
        .any(|p| dot(p.normal, normal) > PARALLEL_EPSILON && (p.dist - dist).abs() < 0.01)
}

fn supports_facet(points: &[Vec3; 3], normal: Vec3, dist: f64) -> bool {
    points.iter().all(|&p| !(dot(p, normal) - dist > FRONT_EPSILON))
}

/// `buildFacetPlanes`: the convex plane set for one triangle, or none for a
/// degenerate one.
#[must_use]
pub fn build_facet_planes(v1: Vec3, v2: Vec3, v3: Vec3) -> Vec<FacetPlane> {
    let mut normal = cross(sub(v2, v1), sub(v3, v1));
    if normalize(&mut normal) == 0.0 {
        return Vec::new();
    }
    let dist = dot(normal, v1);
    let points = [v1, v2, v3];
    // `Math.max(...points.map(...)) + Math.max(0, -T * dot(direction, normal))`
    let support_distance = |direction: Vec3| {
        let far = points
            .iter()
            .fold(f64::NEG_INFINITY, |m, &p| js_max(m, dot(direction, p)));
        far + js_max(0.0, -FACET_THICKNESS * dot(direction, normal))
    };

    let mut planes = vec![
        FacetPlane { normal, dist },
        FacetPlane {
            normal: [-normal[0], -normal[1], -normal[2]],
            dist: -(dist - FACET_THICKNESS),
        },
    ];

    let edges = [(v1, v2, v3), (v2, v3, v1), (v3, v1, v2)];
    for &(a, b, opposite) in &edges {
        let mut border = cross(sub(b, a), normal);
        if normalize(&mut border) == 0.0 {
            continue;
        }
        let mut border_dist = dot(border, a);
        if dot(border, opposite) - border_dist > 0.0 {
            border = [-border[0], -border[1], -border[2]];
            border_dist = -border_dist;
        }
        if !is_duplicate(&planes, border, border_dist) {
            planes.push(FacetPlane {
                normal: border,
                dist: border_dist,
            });
        }
    }

    for axis in 0..3 {
        for dir in [1.0, -1.0] {
            let mut axial = [0.0; 3];
            axial[axis] = dir;
            let axial_dist = support_distance(axial);
            if !is_duplicate(&planes, axial, axial_dist) {
                planes.push(FacetPlane {
                    normal: axial,
                    dist: axial_dist,
                });
            }
        }
    }

    for &(a, b, _) in &edges {
        let mut edge = sub(b, a);
        if normalize(&mut edge) == 0.0 {
            continue;
        }
        for axis in 0..3 {
            let mut axial = [0.0; 3];
            axial[axis] = 1.0;
            let mut bevel = cross(edge, axial);
            if normalize(&mut bevel) == 0.0 {
                continue;
            }
            for dir in [1.0, -1.0] {
                let candidate = [bevel[0] * dir, bevel[1] * dir, bevel[2] * dir];
                let candidate_dist = dot(candidate, a);
                if !supports_facet(&points, candidate, candidate_dist) {
                    continue;
                }
                let support = support_distance(candidate);
                if is_duplicate(&planes, candidate, support) {
                    continue;
                }
                planes.push(FacetPlane {
                    normal: candidate,
                    dist: support,
                });
            }
        }
    }
    planes
}
