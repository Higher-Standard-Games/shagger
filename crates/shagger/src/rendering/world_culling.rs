//! Which world batches to draw this frame: SHAG's static PVS culling
//! (`PVSCuller` over the batches' `bspPvsClusters`) and Babylon's frustum
//! culling of each mesh's bounds.

use glam::Vec3;

use super::camera::Camera;
use super::frustum::Frustum;
use super::world_geometry::WorldGeometry;
use crate::loader::bsp::bsp_loader::BspFile;
use crate::loader::bsp::pvs_culler::{PvsCuller, PvsCullerStats, create_pvs_culler};

pub struct WorldCulling {
    pvs: Option<PvsCuller>,
    bounds: Vec<[Vec3; 2]>,
    visible: Vec<bool>,
    /// Batches left out by the PVS and by the frustum on the last update.
    pub pvs_culled: usize,
    pub frustum_culled: usize,
}

impl WorldCulling {
    #[must_use]
    pub fn new(bsp: &BspFile, geometry: &WorldGeometry) -> Self {
        let pvs = create_pvs_culler(bsp).map(|mut culler| {
            let clusters: Vec<Vec<i32>> = geometry.batches.iter().map(|b| b.pvs_clusters.clone()).collect();
            culler.register_from_world_meshes(&clusters);
            culler
        });
        Self {
            pvs,
            bounds: geometry
                .batches
                .iter()
                .map(|b| [Vec3::from(b.bounds[0]), Vec3::from(b.bounds[1])])
                .collect(),
            visible: vec![true; geometry.batches.len()],
            pvs_culled: 0,
            frustum_culled: 0,
        }
    }

    /// Turns PVS culling on or off (`r_novis`).
    pub fn set_pvs_enabled(&mut self, enabled: bool) {
        if let Some(pvs) = &mut self.pvs {
            pvs.set_enabled(enabled);
        }
    }

    /// The PVS state after the last update, when the map has vis data.
    #[must_use]
    pub fn pvs_stats(&mut self) -> Option<PvsCullerStats> {
        self.pvs.as_mut().map(PvsCuller::get_stats)
    }

    /// Recomputes the visible batches for this camera.
    pub fn update(&mut self, camera: &Camera, aspect: f32) -> &[bool] {
        if let Some(pvs) = &mut self.pvs {
            pvs.update(Some(camera.position));
        }
        let frustum = Frustum::from_view_projection(camera.view_projection(aspect));
        self.pvs_culled = 0;
        self.frustum_culled = 0;
        for (batch, visible) in self.visible.iter_mut().enumerate() {
            *visible = if !self.pvs.as_ref().is_none_or(|p| p.is_mesh_visible(batch)) {
                self.pvs_culled += 1;
                false
            } else if !frustum.intersects_box(self.bounds[batch][0], self.bounds[batch][1]) {
                self.frustum_culled += 1;
                false
            } else {
                true
            };
        }
        &self.visible
    }

    #[must_use]
    pub fn visible(&self) -> &[bool] {
        &self.visible
    }
}
