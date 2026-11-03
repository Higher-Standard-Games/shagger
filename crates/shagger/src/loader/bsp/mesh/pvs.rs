//! SHAG `src/engine/loader/bsp/mesh/pvs.ts`: which clusters each face
//! belongs to, from the compiled leaf membership.

use crate::loader::bsp::bsp_loader::{BspFile, record_range};

/// `buildFaceClusters`: every cluster whose leaves list each face, in the
/// order they are first met; `[-1]` for a face no leaf lists (and `-1` for
/// a leaf outside the clusters), which keeps it drawn rather than guessed.
#[must_use]
pub fn build_face_clusters(bsp: &BspFile) -> Vec<Vec<i32>> {
    let faces = bsp.common.faces.len();
    let mut clusters: Vec<Vec<i32>> = vec![Vec::new(); faces];
    let count = bsp.common.vis_data.as_ref().map_or(0, |v| v.num_clusters);
    for leaf in &bsp.common.leafs {
        let cluster = if leaf.cluster >= 0 && leaf.cluster < count {
            leaf.cluster
        } else {
            -1
        };
        for at in record_range(leaf.first_leaf_face, leaf.num_leaf_faces, bsp.common.leaf_faces.len()) {
            let Some(&face) = bsp.common.leaf_faces.get(at) else {
                continue;
            };
            // A `Set` keeps insertion order and drops repeats.
            if let Some(members) = usize::try_from(face).ok().and_then(|f| clusters.get_mut(f))
                && !members.contains(&cluster)
            {
                members.push(cluster);
            }
        }
    }
    for members in &mut clusters {
        if members.is_empty() {
            members.push(-1);
        }
    }
    clusters
}
