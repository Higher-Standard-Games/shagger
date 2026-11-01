//! SHAG `src/engine/loader/bsp/PVSCuller.ts`: static BSP visibility from the
//! compiled face-to-leaf membership. Babylon meshes become mesh ids (the
//! renderer's batches); `isVisible` becomes [`PvsCuller::is_mesh_visible`].

use std::collections::{BTreeSet, HashMap};

use glam::Vec3;

use super::bsp_loader::{BspFile, BspLeaf, BspNode, BspPlane, BspVisData, find_leaf_for_point, is_cluster_visible};

/// `PVSCullerStats`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PvsCullerStats {
    pub enabled: bool,
    pub current_cluster: i32,
    pub total_clusters: i32,
    pub visible_clusters: usize,
    pub total_meshes: usize,
    pub visible_meshes: usize,
    pub culled_meshes: usize,
    /// Milliseconds the last scan took.
    pub last_update_time: f64,
}

/// `ClusterMeshGroup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterMeshGroup {
    pub cluster: i32,
    pub meshes: Vec<usize>,
}

/// `PVSCuller`.
#[derive(Debug, Clone)]
pub struct PvsCuller {
    vis: Option<BspVisData>,
    nodes: Vec<BspNode>,
    leafs: Vec<BspLeaf>,
    planes: Vec<BspPlane>,
    /// Each registered mesh's clusters, in registration order.
    mesh_clusters: Vec<(usize, Vec<i32>)>,
    index: HashMap<usize, usize>,
    /// `mesh.isVisible` for the meshes this culler manages.
    visible: HashMap<usize, bool>,
    original_visibility: HashMap<usize, bool>,
    enabled: bool,
    current_cluster: i32,
    visible_clusters: BTreeSet<i32>,
    last_update_time: f64,
    cached_visible_count: usize,
    cached_culled_count: usize,
    has_pvs: bool,
    last_camera_pos: [f32; 3],
    force_update: bool,
}

impl PvsCuller {
    #[must_use]
    pub fn new(bsp: &BspFile) -> Self {
        let vis = bsp.common.vis_data.clone();
        let has_pvs = vis.as_ref().is_some_and(|v| {
            v.num_clusters > 0
                && i64::from(v.bytes_per_cluster) >= (i64::from(v.num_clusters) + 7).div_euclid(8)
                && v.bit_vectors.len() as i64 >= i64::from(v.num_clusters) * i64::from(v.bytes_per_cluster)
        });
        Self {
            vis,
            nodes: bsp.common.nodes.clone(),
            leafs: bsp.common.leafs.clone(),
            planes: bsp.common.planes.clone(),
            mesh_clusters: Vec::new(),
            index: HashMap::new(),
            visible: HashMap::new(),
            original_visibility: HashMap::new(),
            enabled: true,
            current_cluster: -1,
            visible_clusters: BTreeSet::new(),
            last_update_time: 0.0,
            cached_visible_count: 0,
            cached_culled_count: 0,
            has_pvs,
            last_camera_pos: [f32::NAN; 3],
            force_update: true,
        }
    }

    #[must_use]
    pub fn has_pvs(&self) -> bool {
        self.has_pvs
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.invalidate();
        if !enabled {
            self.restore_visibility();
        }
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled && self.has_pvs
    }

    /// `mesh.isVisible`: true for a mesh this culler does not manage.
    #[must_use]
    pub fn is_mesh_visible(&self, mesh: usize) -> bool {
        self.visible.get(&mesh).copied().unwrap_or(true)
    }

    fn num_clusters(&self) -> i32 {
        self.vis.as_ref().map_or(0, |v| v.num_clusters)
    }

    fn valid_cluster(&self, cluster: i32) -> bool {
        cluster >= 0 && cluster < self.num_clusters()
    }

    /// `registerClusterMeshes`: every mesh starts visible.
    pub fn register_cluster_meshes(&mut self, groups: &[ClusterMeshGroup]) {
        self.clear_registration();
        for group in groups {
            let cluster = if self.valid_cluster(group.cluster) {
                group.cluster
            } else {
                -1
            };
            for &mesh in &group.meshes {
                let at = *self.index.entry(mesh).or_insert_with(|| {
                    self.mesh_clusters.push((mesh, Vec::new()));
                    self.original_visibility.insert(mesh, true);
                    self.mesh_clusters.len() - 1
                });
                let clusters = &mut self.mesh_clusters[at].1;
                if !clusters.contains(&cluster) {
                    clusters.push(cluster);
                }
            }
        }
        self.refresh_counts();
    }

    /// `registerFromWorldMesh`: mesh `i` is a world batch whose faces belong
    /// to `clusters[i]` (`bspPvsClusters`).
    pub fn register_from_world_meshes(&mut self, clusters: &[Vec<i32>]) {
        let mut groups: Vec<ClusterMeshGroup> = Vec::new();
        for (mesh, members) in clusters.iter().enumerate() {
            let fallback = [-1];
            let members: &[i32] = if members.is_empty() { &fallback } else { members };
            for &cluster in members {
                let key = if self.valid_cluster(cluster) { cluster } else { -1 };
                match groups.iter_mut().find(|g| g.cluster == key) {
                    Some(group) => group.meshes.push(mesh),
                    None => groups.push(ClusterMeshGroup {
                        cluster: key,
                        meshes: vec![mesh],
                    }),
                }
            }
        }
        self.register_cluster_meshes(&groups);
    }

    /// `findClusterAtPosition`, for a Babylon-space position.
    #[must_use]
    pub fn find_cluster_at_position(&self, position: Vec3) -> i32 {
        if !position.is_finite() {
            return -1;
        }
        let point = [f64::from(position.x), f64::from(position.z), f64::from(position.y)];
        let cluster = find_leaf_for_point(point, &self.nodes, &self.leafs, &self.planes).map_or(-1, |l| l.cluster);
        if self.valid_cluster(cluster) { cluster } else { -1 }
    }

    /// `isPositionVisible`: fails open.
    #[must_use]
    pub fn is_position_visible(&self, position: Vec3) -> bool {
        if !self.enabled || !self.has_pvs || !self.valid_cluster(self.current_cluster) {
            return true;
        }
        self.is_cluster_visible(self.find_cluster_at_position(position))
    }

    /// `isClusterVisible`: fails open.
    #[must_use]
    pub fn is_cluster_visible(&self, cluster: i32) -> bool {
        if !self.enabled || !self.has_pvs || !self.valid_cluster(self.current_cluster) {
            return true;
        }
        cluster < 0 || cluster == self.current_cluster || self.visible_clusters.contains(&cluster)
    }

    /// `getVisibleClusters`.
    #[must_use]
    pub fn get_visible_clusters(&self, from_cluster: i32) -> BTreeSet<i32> {
        (0..self.num_clusters())
            .filter(|&i| {
                !self.has_pvs
                    || !self.valid_cluster(from_cluster)
                    || i == from_cluster
                    || is_cluster_visible(self.vis.as_ref(), from_cluster, i)
            })
            .collect()
    }

    /// `update`: only a camera that changed cluster rescans the meshes;
    /// `None` shows every registered mesh.
    pub fn update(&mut self, camera_position: Option<Vec3>) -> PvsCullerStats {
        if !self.is_enabled() {
            return self.cached_stats();
        }
        if !self.force_update && camera_position.is_some_and(|p| p.to_array() == self.last_camera_pos) {
            return self.cached_stats();
        }
        let start = std::time::Instant::now();
        let new_cluster = camera_position.map_or(-1, |p| self.find_cluster_at_position(p));
        self.last_camera_pos = camera_position.map_or([f32::NAN; 3], |p| p.to_array());
        if !self.force_update && new_cluster == self.current_cluster {
            return self.cached_stats();
        }
        self.force_update = false;
        self.current_cluster = new_cluster;
        self.visible_clusters = self.get_visible_clusters(new_cluster);
        let (mut visible_count, mut culled_count) = (0, 0);
        for (mesh, clusters) in &self.mesh_clusters {
            let visible = clusters.iter().any(|&c| c < 0 || self.visible_clusters.contains(&c))
                && self.original_visibility.get(mesh) == Some(&true);
            self.visible.insert(*mesh, visible);
            if visible {
                visible_count += 1;
            } else {
                culled_count += 1;
            }
        }
        self.cached_visible_count = visible_count;
        self.cached_culled_count = culled_count;
        self.last_update_time = start.elapsed().as_secs_f64() * 1000.0;
        self.cached_stats()
    }

    fn cached_stats(&self) -> PvsCullerStats {
        PvsCullerStats {
            enabled: self.is_enabled(),
            current_cluster: self.current_cluster,
            total_clusters: self.num_clusters(),
            visible_clusters: self.visible_clusters.len(),
            total_meshes: self.cached_visible_count + self.cached_culled_count,
            visible_meshes: self.cached_visible_count,
            culled_meshes: self.cached_culled_count,
            last_update_time: self.last_update_time,
        }
    }

    /// `getStats`.
    pub fn get_stats(&mut self) -> PvsCullerStats {
        self.refresh_counts();
        self.cached_stats()
    }

    fn refresh_counts(&mut self) {
        let visible = self
            .mesh_clusters
            .iter()
            .filter(|(mesh, _)| self.is_mesh_visible(*mesh))
            .count();
        self.cached_visible_count = visible;
        self.cached_culled_count = self.mesh_clusters.len() - visible;
    }

    pub fn invalidate(&mut self) {
        self.force_update = true;
    }

    fn restore_visibility(&mut self) {
        for (mesh, visible) in &self.original_visibility {
            self.visible.insert(*mesh, *visible);
        }
        self.refresh_counts();
    }

    fn clear_registration(&mut self) {
        self.restore_visibility();
        self.mesh_clusters.clear();
        self.index.clear();
        self.visible.clear();
        self.original_visibility.clear();
        self.visible_clusters.clear();
        self.current_cluster = -1;
        self.cached_visible_count = 0;
        self.cached_culled_count = 0;
        self.last_update_time = 0.0;
        self.invalidate();
    }
}

/// `createPVSCuller`: `None` for a map without usable vis data.
#[must_use]
pub fn create_pvs_culler(bsp: &BspFile) -> Option<PvsCuller> {
    let culler = PvsCuller::new(bsp);
    culler.has_pvs().then_some(culler)
}
