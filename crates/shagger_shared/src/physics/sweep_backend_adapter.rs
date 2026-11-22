//! `SweepBackend` over the Q3 backend (SHAG `src/engine/physics/SweepBackendAdapter.ts`):
//! the shared set of contents out for the raw Q3 ones the backend reports.

use super::collision_backend::TraceOptions;
use super::content_flags::content;
use super::sweep_collision_backend::SweepCollisionBackend;
use crate::bsp::brush_model_state::BrushModelState;
use crate::bsp::lift_system::LiftSystem;
use crate::collision::collision_world::{SweepBackend, SweepBackendTraceOptions, SweepMove, SweepTrace, contents};
use crate::server::simulation::types::Vec3;

const CONTENTS_TRANSLATION: [(i32, u32); 7] = [
    (content::SOLID, contents::SOLID),
    (content::WATER, contents::WATER),
    (content::LAVA, contents::LAVA),
    (content::SLIME, contents::SLIME),
    (content::PLAYERCLIP, contents::PLAYERCLIP),
    (content::MONSTERCLIP, contents::MONSTERCLIP),
    (content::TRIGGER, contents::TRIGGER),
];

/// `toSharedContents`. Note raw LAVA (0x8) and shared SLIME (1 << 3) are the
/// same number; the two sets must not mix.
#[must_use]
pub fn to_shared_contents(raw: i32) -> u32 {
    CONTENTS_TRANSLATION
        .iter()
        .filter(|(bsp, _)| raw & bsp != 0)
        .fold(0, |out, (_, shared)| out | shared)
}

fn options(o: SweepBackendTraceOptions) -> TraceOptions {
    TraceOptions {
        radius: Some(o.radius),
        height: Some(o.height),
        content_mask: o.content_mask,
    }
}

/// `SweepBackendAdapter`.
#[derive(Debug, Clone)]
pub struct SweepBackendAdapter {
    pub backend: SweepCollisionBackend,
}

impl SweepBackend for SweepBackendAdapter {
    fn lifts(&self) -> Option<&LiftSystem> {
        self.backend.lifts.as_ref()
    }

    fn brush_models(&self) -> Option<&BrushModelState> {
        self.backend.brush_models.as_ref()
    }

    fn trace(&mut self, start: Vec3, end: Vec3, o: SweepBackendTraceOptions) -> SweepTrace {
        let r = self.backend.trace(start, end, options(o));
        SweepTrace {
            fraction: r.fraction,
            end_pos: Some(r.end_pos),
            normal: r.normal,
            contents: Some(to_shared_contents(r.contents)),
        }
    }

    fn step_slide_move(&mut self, start: Vec3, movement: Vec3, o: SweepBackendTraceOptions) -> SweepMove {
        let r = self.backend.step_slide_move(start, movement, options(o));
        SweepMove {
            end_pos: r.end_pos,
            velocity: r.velocity,
            hit_count: r.hit_count,
        }
    }

    fn point_contents(&mut self, point: Vec3) -> u32 {
        to_shared_contents(self.backend.point_contents(point, None))
    }

    fn is_in_ladder(&mut self, min: Vec3, max: Vec3) -> Option<bool> {
        Some(self.backend.is_in_ladder(min, max))
    }
}
