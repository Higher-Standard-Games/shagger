//! Conservative bounds and a static BVH over brushes and facets
//! (SHAG `src/engine/physics/CollisionSpatialIndex.ts`).

use shagger_shared::js::{js_max, js_min};
use std::cmp::Ordering;

/// Q3-space bounds `[minX, minY, minZ, maxX, maxY, maxZ]`. Missing
/// constraints stay infinite (unculled).
pub type CollisionBounds = [f64; 6];

/// `planeBounds` over `[nx, ny, nz, dist]` quads `first..last`. Only exactly
/// axial planes prove a bound.
#[must_use]
pub fn plane_bounds(planes: &[f64], first: usize, last: usize, allow_partial: bool) -> Option<CollisionBounds> {
    let mut b = [
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
    ];
    for i in first..last {
        let n = [planes[i * 4], planes[i * 4 + 1], planes[i * 4 + 2]];
        let dist = planes[i * 4 + 3];
        if !n.iter().all(|v| v.is_finite()) || !dist.is_finite() {
            return None;
        }
        for axis in 0..3 {
            if n[axis] == 0.0 || n[(axis + 1) % 3] != 0.0 || n[(axis + 2) % 3] != 0.0 {
                continue;
            }
            let limit = (dist + 0.125) / n[axis];
            if n[axis] > 0.0 {
                b[axis + 3] = js_min(b[axis + 3], limit);
            } else {
                b[axis] = js_max(b[axis], limit);
            }
        }
    }
    let valid = (0..3).all(|i| b[i] <= b[i + 3]);
    let constrained = if allow_partial {
        b.iter().any(|v| v.is_finite())
    } else {
        b.iter().all(|v| v.is_finite())
    };
    (valid && constrained).then_some(b)
}

#[must_use]
pub fn bounds_overlap(a: &CollisionBounds, b: &CollisionBounds) -> bool {
    a[0] <= b[3] && a[3] >= b[0] && a[1] <= b[4] && a[4] >= b[1] && a[2] <= b[5] && a[5] >= b[2]
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionBoundItem {
    pub id: usize,
    pub bounds: Option<CollisionBounds>,
}

#[derive(Debug, Clone)]
struct Node {
    bounds: CollisionBounds,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
    items: Option<Vec<(usize, CollisionBounds)>>,
}

/// A finite partition anchor for full boxes and half-infinite slabs alike.
fn split_position(b: &CollisionBounds, axis: usize) -> f64 {
    let (low, high) = (b[axis], b[axis + 3]);
    if low.is_finite() {
        if high.is_finite() { low / 2.0 + high / 2.0 } else { low }
    } else if high.is_finite() {
        high
    } else {
        0.0
    }
}

/// `CollisionSpatialIndex`: a static median-split BVH. Results come back in
/// ascending id order, so equal-time contacts resolve as in source order.
#[derive(Debug, Clone)]
pub struct CollisionSpatialIndex {
    root: Option<Box<Node>>,
    unbounded: Vec<usize>,
}

fn build(mut entries: Vec<(usize, CollisionBounds)>) -> Option<Box<Node>> {
    if entries.is_empty() {
        return None;
    }
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (_, b) in &entries {
        for j in 0..3 {
            bounds[j] = js_min(bounds[j], b[j]);
            bounds[j + 3] = js_max(bounds[j + 3], b[j + 3]);
        }
    }
    if entries.len() <= 8 {
        return Some(Box::new(Node {
            bounds,
            left: None,
            right: None,
            items: Some(entries),
        }));
    }
    let mut axis = 0;
    let mut widest = -1.0;
    for j in 0..3 {
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for (_, b) in &entries {
            let anchor = split_position(b, j);
            low = js_min(low, anchor);
            high = js_max(high, anchor);
        }
        if high - low > widest {
            widest = high - low;
            axis = j;
        }
    }
    // `sort((a, b) => pos(a) - pos(b) || a.id - b.id)`; JavaScript's sort is stable.
    entries.sort_by(|a, b| {
        let d = split_position(&a.1, axis) - split_position(&b.1, axis);
        if d != 0.0 && !d.is_nan() {
            if d < 0.0 { Ordering::Less } else { Ordering::Greater }
        } else {
            a.0.cmp(&b.0)
        }
    });
    let right = entries.split_off(entries.len() / 2);
    Some(Box::new(Node {
        bounds,
        left: build(entries),
        right: build(right),
        items: None,
    }))
}

impl CollisionSpatialIndex {
    #[must_use]
    pub fn new(items: &[CollisionBoundItem]) -> Self {
        let unbounded = items.iter().filter(|i| i.bounds.is_none()).map(|i| i.id).collect();
        let bounded = items.iter().filter_map(|i| i.bounds.map(|b| (i.id, b))).collect();
        Self {
            root: build(bounded),
            unbounded,
        }
    }

    #[must_use]
    pub fn query(&self, bounds: &CollisionBounds) -> Vec<usize> {
        let mut result = Vec::new();
        self.query_into(bounds, &mut result);
        result
    }

    /// Clears and refills `result`, sorted by id.
    pub fn query_into(&self, bounds: &CollisionBounds, result: &mut Vec<usize>) {
        result.clear();
        result.extend_from_slice(&self.unbounded);
        Self::visit(self.root.as_deref(), bounds, result);
        result.sort_unstable();
    }

    fn visit(node: Option<&Node>, bounds: &CollisionBounds, result: &mut Vec<usize>) {
        let Some(node) = node else { return };
        if !bounds_overlap(bounds, &node.bounds) {
            return;
        }
        if let Some(items) = &node.items {
            result.extend(
                items
                    .iter()
                    .filter(|(_, b)| bounds_overlap(bounds, b))
                    .map(|(id, _)| *id),
            );
        } else {
            Self::visit(node.left.as_deref(), bounds, result);
            Self::visit(node.right.as_deref(), bounds, result);
        }
    }
}
