//! Port of SHAG `src/engine/physics/PhysicsSystem.ts`: the physics world that
//! decoration (ragdolls) falls through, with the map as static triangles.
//!
//! SHAG runs Havok in the browser; here rapier3d stands in for it (RUST.md
//! 8.2). Gameplay never touches this world: players, projectiles and their
//! collision stay on the Q3 brush sweep, as in SHAG, and the server never
//! simulates bodies. Rapier's math types are glam 0.33 through `glamx`, a
//! different glam than ours, so values cross through arrays ([`to_rapier`],
//! [`from_rapier`]).
//!
//! What changed: Havok steps with the frame's delta; rapier steps fixed
//! 1/60 s substeps here ([`PhysicsSystem::step`]), so a ragdoll falls the
//! same at any frame rate. `createBspCollision` makes one trimesh per child
//! mesh of the world; here one trimesh holds every drawn batch. Ragdoll
//! boxes collide with the map only (Havok lets a ragdoll's boxes push each
//! other apart, which rapier turns explosive where they start overlapped).

use glam::{Quat, Vec3};
use rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, Group, InteractionGroups, InteractionTestMode, PhysicsWorld, Rotation, Vector,
};

use crate::rendering::world_geometry::WorldGeometry;

pub use rapier3d;

/// The fixed step rapier advances by, seconds.
pub const FIXED_STEP: f64 = 1.0 / 60.0;
/// Most substeps one [`PhysicsSystem::step`] runs; a longer frame drops the
/// rest, as Havok clamps its delta.
pub const MAX_SUBSTEPS: u32 = 4;
/// Rapier's `length_unit`: the size of a typical body (a ragdoll box is 4 to
/// 14 units across), which scales its contact tolerances.
pub const LENGTH_UNIT: f32 = 10.0;

/// The map's collision group.
pub const WORLD_GROUP: Group = Group::GROUP_1;
/// Decoration (ragdoll boxes): hits the map, not itself.
pub const DEBRIS_GROUP: Group = Group::GROUP_2;

/// The groups a decoration collider takes.
#[must_use]
pub fn debris_groups() -> InteractionGroups {
    InteractionGroups::new(DEBRIS_GROUP, WORLD_GROUP, InteractionTestMode::And)
}

#[must_use]
pub fn to_rapier(v: Vec3) -> Vector {
    Vector::from_array(v.to_array())
}

#[must_use]
pub fn from_rapier(v: Vector) -> Vec3 {
    Vec3::from_array(v.to_array())
}

#[must_use]
pub fn quat_to_rapier(q: Quat) -> Rotation {
    Rotation::from_array(q.to_array())
}

#[must_use]
pub fn quat_from_rapier(q: Rotation) -> Quat {
    Quat::from_array(q.to_array())
}

/// The triangles of every batch the world draws, minus `hidden` ones (the
/// liquid surfaces the water system took over), in Babylon space; slivers
/// with no area are dropped, which Havok's mesh shape skips too.
#[must_use]
pub fn world_triangles(geometry: &WorldGeometry, hidden: &[bool]) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let positions: Vec<[f32; 3]> = geometry.vertices.iter().map(|v| v.position).collect();
    let mut triangles = Vec::new();
    for (index, batch) in geometry.batches.iter().enumerate() {
        if hidden.get(index).copied().unwrap_or(false) {
            continue;
        }
        let start = batch.first_index as usize;
        let end = (start + batch.index_count as usize).min(geometry.indices.len());
        let Some(run) = geometry.indices.get(start..end) else {
            continue;
        };
        for t in run.as_chunks::<3>().0 {
            let p = |i: u32| Vec3::from(positions[i as usize]);
            let area = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length_squared();
            if area > 1e-6 {
                triangles.push(*t);
            }
        }
    }
    (positions, triangles)
}

/// `PhysicsSystem`: the world, its map collision, and the clock that steps it.
pub struct PhysicsSystem {
    pub world: PhysicsWorld,
    bsp_collision: Vec<ColliderHandle>,
    accumulator: f64,
}

impl PhysicsSystem {
    /// `enablePhysics`: gravity straight down at `sv_gravity`.
    #[must_use]
    pub fn new(sv_gravity: f32) -> Self {
        let mut world = PhysicsWorld::new();
        world.gravity = Vector::new(0.0, -sv_gravity, 0.0);
        world.integration_parameters.length_unit = LENGTH_UNIT;
        world.integration_parameters.dt = FIXED_STEP as f32;
        Self {
            world,
            bsp_collision: Vec::new(),
            accumulator: 0.0,
        }
    }

    /// `createBspCollision`: the map as one static trimesh. Returns how many
    /// colliders it made (none for an empty or broken mesh).
    pub fn create_bsp_collision(&mut self, positions: &[[f32; 3]], triangles: &[[u32; 3]]) -> usize {
        self.dispose_bsp_collision();
        if triangles.is_empty() {
            return 0;
        }
        let vertices = positions.iter().map(|p| Vector::from_array(*p)).collect();
        match ColliderBuilder::trimesh(vertices, triangles.to_vec()) {
            Ok(builder) => {
                let collider = builder.collision_groups(InteractionGroups::new(
                    WORLD_GROUP,
                    Group::ALL,
                    InteractionTestMode::And,
                ));
                self.bsp_collision.push(self.world.insert_collider(collider, None));
                1
            }
            Err(e) => {
                eprintln!("[PhysicsSystem] BSP collision failed: {e}");
                0
            }
        }
    }

    /// `hasBspCollision`.
    #[must_use]
    pub fn has_bsp_collision(&self) -> bool {
        !self.bsp_collision.is_empty()
    }

    /// `disposeBspCollision`.
    pub fn dispose_bsp_collision(&mut self) {
        for handle in self.bsp_collision.drain(..) {
            self.world.remove_collider(handle);
        }
    }

    /// `setGravity`.
    pub fn set_gravity(&mut self, gravity: Vec3) {
        self.world.gravity = to_rapier(gravity);
    }

    /// `getGravity`.
    #[must_use]
    pub fn gravity(&self) -> Vec3 {
        from_rapier(self.world.gravity)
    }

    /// Advances the world by `dt` seconds in fixed substeps; returns how
    /// many ran.
    pub fn step(&mut self, dt: f64) -> u32 {
        self.accumulator += dt.max(0.0);
        let mut steps = 0;
        while self.accumulator >= FIXED_STEP && steps < MAX_SUBSTEPS {
            self.world.step();
            self.accumulator -= FIXED_STEP;
            steps += 1;
        }
        if steps == MAX_SUBSTEPS {
            self.accumulator = self.accumulator.min(FIXED_STEP);
        }
        steps
    }

    /// How many dynamic or kinematic bodies the world holds.
    #[must_use]
    pub fn body_count(&self) -> usize {
        self.world.bodies.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier3d::prelude::RigidBodyBuilder;

    #[test]
    fn a_box_lands_on_the_floor() {
        let mut physics = PhysicsSystem::new(800.0);
        let floor = [
            [-100.0, 0.0, -100.0],
            [100.0, 0.0, -100.0],
            [100.0, 0.0, 100.0],
            [-100.0, 0.0, 100.0],
        ];
        assert_eq!(physics.create_bsp_collision(&floor, &[[0, 2, 1], [0, 3, 2]]), 1);
        let (body, _) = physics.world.insert(
            RigidBodyBuilder::dynamic().translation(Vector::new(0.0, 50.0, 0.0)),
            ColliderBuilder::cuboid(5.0, 5.0, 5.0).collision_groups(debris_groups()),
        );
        for _ in 0..180 {
            physics.step(FIXED_STEP);
        }
        let y = physics.world.bodies[body].translation().y;
        assert!((y - 5.0).abs() < 1.0, "rests on the floor, at {y}");
        physics.dispose_bsp_collision();
        assert!(!physics.has_bsp_collision());
    }

    #[test]
    fn long_frames_are_clamped() {
        let mut physics = PhysicsSystem::new(800.0);
        assert_eq!(physics.step(1.0), MAX_SUBSTEPS);
        assert_eq!(physics.step(0.5 * FIXED_STEP), 1);
        assert_eq!(physics.step(0.0), 0);
        assert_eq!(physics.gravity(), Vec3::new(0.0, -800.0, 0.0));
    }
}
