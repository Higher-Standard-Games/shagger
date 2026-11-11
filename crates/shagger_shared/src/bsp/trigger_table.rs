//! Teleporter and jumppad volumes from the BSP entities
//! (SHAG `src/shared/bsp/TriggerTable.ts`).
//!
//! The server and the predicting client both build their trigger list here
//! from the same entities and models, so trigger evaluation agrees on both
//! sides. Triggers with a bad model or an unresolvable target are dropped,
//! as SHAG drops them (it also logs a warning, which this does not).

use super::map_entities::{EntityLike, ModelLike, split_whitespace_runs};
use crate::js::{js_parse_int10, js_string_to_number};
use crate::server::simulation::types::Vec3;
use std::f64::consts::PI;

/// `TriggerType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerType {
    Teleport,
    Push,
}

impl TriggerType {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Teleport => "teleport",
            Self::Push => "push",
        }
    }
}

/// `TriggerVolume`, in Babylon space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriggerVolume {
    /// Index in BSP order over accepted triggers.
    pub id: usize,
    pub kind: TriggerType,
    pub min: Vec3,
    pub max: Vec3,
    pub dest: Vec3,
    /// Exit yaw in game radians, teleporters only.
    pub dest_yaw: Option<f64>,
}

const DEST_CLASSNAMES: [&str; 6] = [
    "target_position",
    // A point origin used as a ballistic target (SHAG beta9).
    "target_push",
    "misc_teleporter_dest",
    "info_teleport_destination",
    "target_location",
    "info_notnull",
];

fn trigger_type(classname: &str) -> Option<TriggerType> {
    match classname {
        "trigger_teleport" => Some(TriggerType::Teleport),
        "trigger_push" => Some(TriggerType::Push),
        _ => None,
    }
}

/// `q3AngleToGameYaw`: Q3 yaw degrees to game radians in (-pi, pi].
///
/// Where the TypeScript's normalising loops would never finish (an infinite
/// angle, or one so large that adding 2pi changes nothing), this returns the
/// unnormalised value instead of hanging.
#[must_use]
pub fn q3_angle_to_game_yaw(q3_angle_deg: f64) -> f64 {
    let deg = if q3_angle_deg == -1.0 || q3_angle_deg == -2.0 {
        0.0
    } else {
        q3_angle_deg
    };
    let mut yaw = PI / 2.0 - (deg * PI) / 180.0;
    while yaw <= -PI {
        let next = yaw + 2.0 * PI;
        if next == yaw {
            return yaw;
        }
        yaw = next;
    }
    while yaw > PI {
        let next = yaw - 2.0 * PI;
        if next == yaw {
            return yaw;
        }
        yaw = next;
    }
    yaw
}

/// `parseModelIndex`: `"*N"` with N a positive integer.
fn parse_model_index(model: Option<&str>) -> Option<f64> {
    let index = js_parse_int10(model?.strip_prefix('*')?);
    (index.fract() == 0.0 && index > 0.0).then_some(index)
}

/// `parseOrigin`: `"x y z"` in Q3 axes to Babylon axes.
fn parse_origin(origin: Option<&str>) -> Option<Vec3> {
    let origin = origin.filter(|o| !o.is_empty())?;
    let parts = split_whitespace_runs(origin);
    let n = |i: usize| parts.get(i).map_or(f64::NAN, |p| js_string_to_number(p));
    let (x, y, z) = (n(0), n(1), n(2));
    (x.is_finite() && y.is_finite() && z.is_finite()).then(|| Vec3::new(x, z, y))
}

/// `extractTriggerVolumes`.
#[must_use]
pub fn extract_trigger_volumes<E: EntityLike, M: ModelLike>(entities: &[E], models: &[M]) -> Vec<TriggerVolume> {
    let mut out = Vec::new();
    for entity in entities {
        let Some(kind) = trigger_type(entity.classname()) else {
            continue;
        };
        let Some(model_index) = parse_model_index(entity.property("model")) else {
            continue;
        };
        if model_index >= models.len() as f64 {
            continue;
        }
        let Some(target) = entity.property("target").filter(|t| !t.is_empty()) else {
            continue;
        };
        let Some(dest_entity) = entities
            .iter()
            .find(|e| DEST_CLASSNAMES.contains(&e.classname()) && e.property("targetname") == Some(target))
        else {
            continue;
        };
        let Some(dest) = parse_origin(dest_entity.property("origin")) else {
            continue;
        };
        let model = &models[model_index as usize];
        let (mins, maxs) = (model.mins(), model.maxs());
        let dest_yaw = (kind == TriggerType::Teleport).then(|| {
            let angle = js_string_to_number(dest_entity.property("angle").unwrap_or("0"));
            q3_angle_to_game_yaw(if angle.is_nan() || angle == 0.0 { 0.0 } else { angle })
        });
        out.push(TriggerVolume {
            id: out.len(),
            kind,
            min: Vec3::new(mins[0], mins[2], mins[1]),
            max: Vec3::new(maxs[0], maxs[2], maxs[1]),
            dest,
            dest_yaw,
        });
    }
    out
}
