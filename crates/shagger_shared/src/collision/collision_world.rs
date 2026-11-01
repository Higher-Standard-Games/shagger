//! The simulation's view of collision (SHAG `src/shared/collision/CollisionWorld.ts`).
//!
//! Game space (y up) throughout. The content flags here are the shared,
//! compact set ([`contents`]), not the raw Q3 bits the backend reads from the
//! map; `shagger::physics::sweep_backend_adapter` translates.

use crate::bsp::brush_model_state::BrushModelState;
use crate::bsp::lift_system::LiftSystem;
use crate::server::simulation::types::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TraceResult {
    pub hit: bool,
    pub fraction: f64,
    pub hit_point: Option<Vec3>,
    pub normal: Option<Vec3>,
    pub contents: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StepSlideMoveResult {
    pub end_pos: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
}

/// `CollisionWorld`.
pub trait CollisionWorld {
    fn lifts(&self) -> Option<&LiftSystem>;
    fn brush_models(&self) -> Option<&BrushModelState>;
    fn trace(&mut self, start: Vec3, end: Vec3, bounds: Option<Aabb>, content_mask: Option<i32>) -> TraceResult;
    fn step_slide_move(&mut self, start: Vec3, velocity: Vec3, delta_time: f64, bounds: Aabb) -> StepSlideMoveResult;
    fn point_contents(&mut self, point: Vec3) -> u32;
    fn is_in_ladder(&mut self, _min: Vec3, _max: Vec3) -> bool {
        false
    }
    fn world_bounds(&self) -> Aabb;
}

/// `SweepBackendTraceOptions`. `content_mask` holds Q3 values; `None` means
/// the backend's player default, SOLID | PLAYERCLIP.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SweepBackendTraceOptions {
    pub radius: f64,
    pub height: f64,
    pub content_mask: Option<i32>,
}

/// What `SweepBackend.trace` returns.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SweepTrace {
    pub fraction: f64,
    pub end_pos: Option<Vec3>,
    pub normal: Option<Vec3>,
    pub contents: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SweepMove {
    pub end_pos: Vec3,
    pub velocity: Vec3,
    pub hit_count: u32,
}

/// Q3 SOLID: what a shot collides with.
pub const SHOT_CONTENT_MASK: i32 = 0x1;

/// `SweepBackend`: the engine's swept-box tracer.
pub trait SweepBackend {
    fn lifts(&self) -> Option<&LiftSystem>;
    fn brush_models(&self) -> Option<&BrushModelState>;
    fn trace(&mut self, start: Vec3, end: Vec3, options: SweepBackendTraceOptions) -> SweepTrace;
    /// `movement` is a displacement in units, not units per second.
    fn step_slide_move(&mut self, start: Vec3, movement: Vec3, options: SweepBackendTraceOptions) -> SweepMove;
    fn point_contents(&mut self, point: Vec3) -> u32;
    fn is_in_ladder(&mut self, min: Vec3, max: Vec3) -> Option<bool>;
}

/// `Contents`: the shared content flags.
pub mod contents {
    pub const EMPTY: u32 = 0;
    pub const SOLID: u32 = 1 << 0;
    pub const WATER: u32 = 1 << 1;
    pub const LAVA: u32 = 1 << 2;
    pub const SLIME: u32 = 1 << 3;
    pub const PLAYERCLIP: u32 = 1 << 4;
    pub const MONSTERCLIP: u32 = 1 << 5;
    pub const TRIGGER: u32 = 1 << 6;
}

/// Any liquid: water, slime or lava.
pub const MASK_LIQUID: u32 = contents::WATER | contents::LAVA | contents::SLIME;

/// What stops a body.
pub const MASK_SOLID: u32 = contents::SOLID | contents::PLAYERCLIP;

/// `NullCollisionWorld`: an infinite floor at the hull's feet.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullCollisionWorld;

impl CollisionWorld for NullCollisionWorld {
    fn lifts(&self) -> Option<&LiftSystem> {
        None
    }

    fn brush_models(&self) -> Option<&BrushModelState> {
        None
    }

    fn trace(&mut self, _start: Vec3, _end: Vec3, _bounds: Option<Aabb>, _mask: Option<i32>) -> TraceResult {
        TraceResult {
            hit: false,
            fraction: 1.0,
            ..TraceResult::default()
        }
    }

    fn step_slide_move(&mut self, start: Vec3, velocity: Vec3, delta_time: f64, bounds: Aabb) -> StepSlideMoveResult {
        let mut end = Vec3::new(
            start.x + velocity.x * delta_time,
            start.y + velocity.y * delta_time,
            start.z + velocity.z * delta_time,
        );
        let mut vel = velocity;
        let floor = -bounds.min.y;
        let mut grounded = false;
        if end.y <= floor {
            end.y = floor;
            if vel.y < 0.0 {
                vel.y = 0.0;
            }
            grounded = true;
        }
        StepSlideMoveResult {
            end_pos: end,
            velocity: vel,
            grounded,
        }
    }

    fn point_contents(&mut self, _point: Vec3) -> u32 {
        contents::EMPTY
    }

    fn world_bounds(&self) -> Aabb {
        Aabb {
            min: Vec3::new(-10000.0, -10000.0, -10000.0),
            max: Vec3::new(10000.0, 10000.0, 10000.0),
        }
    }
}
