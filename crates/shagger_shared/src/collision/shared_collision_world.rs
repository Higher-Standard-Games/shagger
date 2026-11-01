//! `CollisionWorld` over a sweep backend, with ground detection
//! (SHAG `src/shared/collision/SharedCollisionWorld.ts`).
//!
//! Note (RUST.md 19.4): the slide move uses the fixed player hull for
//! anything not well under the player's radius, so crouched and swimming
//! bounds do not reach it. The server does the same.

use super::collision_world::{
    Aabb, CollisionWorld, StepSlideMoveResult, SweepBackend, SweepBackendTraceOptions, TraceResult,
};
use crate::bsp::brush_model_state::BrushModelState;
use crate::bsp::lift_system::LiftSystem;
use crate::server::simulation::types::Vec3;

const PLAYER_RADIUS: f64 = 16.0;
const PLAYER_HEIGHT: f64 = 64.0;

/// `SharedCollisionWorld`.
#[derive(Debug, Clone)]
pub struct SharedCollisionWorld<B> {
    pub backend: B,
}

impl<B: SweepBackend> SharedCollisionWorld<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    fn ground_check_end(from: Vec3) -> Vec3 {
        Vec3::new(from.x, from.y - 2.0, from.z)
    }
}

impl<B: SweepBackend> CollisionWorld for SharedCollisionWorld<B> {
    fn lifts(&self) -> Option<&LiftSystem> {
        self.backend.lifts()
    }

    fn brush_models(&self) -> Option<&BrushModelState> {
        self.backend.brush_models()
    }

    fn trace(&mut self, start: Vec3, end: Vec3, bounds: Option<Aabb>, content_mask: Option<i32>) -> TraceResult {
        let options = SweepBackendTraceOptions {
            radius: bounds.map_or(0.0, |b| (b.max.x - b.min.x) / 2.0),
            height: bounds.map_or(0.0, |b| b.max.y - b.min.y),
            content_mask,
        };
        let result = self.backend.trace(start, end, options);
        TraceResult {
            hit: result.fraction < 1.0,
            fraction: result.fraction,
            hit_point: result.end_pos,
            normal: result.normal,
            contents: result.contents.unwrap_or(0),
        }
    }

    fn step_slide_move(&mut self, start: Vec3, velocity: Vec3, delta_time: f64, bounds: Aabb) -> StepSlideMoveResult {
        let movement = Vec3::new(
            velocity.x * delta_time,
            velocity.y * delta_time,
            velocity.z * delta_time,
        );
        let bounds_radius = (bounds.max.x - bounds.min.x) / 2.0;
        let bounds_height = bounds.max.y - bounds.min.y;
        let small = bounds_radius > 0.0 && bounds_radius < PLAYER_RADIUS * 0.75;
        let options = SweepBackendTraceOptions {
            radius: if small { bounds_radius } else { PLAYER_RADIUS },
            height: if small { bounds_height } else { PLAYER_HEIGHT },
            content_mask: None,
        };
        let result = self.backend.step_slide_move(start, movement, options);

        let expected_y = movement.y;
        let actual_y = result.end_pos.y - start.y;
        let mut grounded = false;
        if expected_y < -0.1 && actual_y > expected_y * 0.5 {
            grounded = true;
        } else if movement.y.abs() < 0.1 {
            let ground = self
                .backend
                .trace(result.end_pos, Self::ground_check_end(result.end_pos), options);
            grounded = ground.fraction < 1.0;
        } else if movement.y > 0.1 {
            let ground = self
                .backend
                .trace(result.end_pos, Self::ground_check_end(result.end_pos), options);
            if let Some(n) = ground.normal.filter(|n| ground.fraction < 1.0 && n.y >= 0.7) {
                let v = result.velocity;
                grounded = v.x * n.x + v.y * n.y + v.z * n.z <= 0.01;
            }
        }

        let velocity = if delta_time > 0.0 {
            let inv = 1.0 / delta_time;
            Vec3::new(
                result.velocity.x * inv,
                result.velocity.y * inv,
                result.velocity.z * inv,
            )
        } else {
            Vec3::default()
        };
        StepSlideMoveResult {
            end_pos: result.end_pos,
            velocity,
            grounded,
        }
    }

    fn point_contents(&mut self, point: Vec3) -> u32 {
        self.backend.point_contents(point)
    }

    fn is_in_ladder(&mut self, min: Vec3, max: Vec3) -> bool {
        self.backend.is_in_ladder(min, max).unwrap_or(false)
    }

    fn world_bounds(&self) -> Aabb {
        Aabb {
            min: Vec3::new(-8192.0, -8192.0, -8192.0),
            max: Vec3::new(8192.0, 8192.0, 8192.0),
        }
    }
}
