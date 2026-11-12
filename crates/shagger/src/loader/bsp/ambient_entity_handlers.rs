//! SHAG `src/engine/loader/bsp/AmbientEntityHandlers.ts`: how a
//! `func_rotating` brush spins (`rotationPlan`) and where it turns
//! (`handleFuncRotating`'s pivot). Neither touches gameplay, so neither is
//! networked: every client spins its own from the map.
//!
//! Not yet: `target_speaker` (`speakerPlan`, `handleTargetSpeaker`).

use crate::loader::bsp::bsp_loader::{BspEntity, BspModel};

const ROTATING_START_ON: i32 = 1;
const ROTATING_REVERSE: i32 = 2;
const ROTATING_X_AXIS: i32 = 4;
const ROTATING_Y_AXIS: i32 = 8;

/// `parseQ3Vector`: three finite numbers split on whitespace.
#[must_use]
pub fn parse_q3_vector(value: Option<&str>) -> Option<[f64; 3]> {
    let parts: Vec<f64> = value?
        .split_whitespace()
        .map(|p| p.parse::<f64>().unwrap_or(f64::NAN))
        .collect();
    if parts.len() != 3 || parts.iter().any(|n| !n.is_finite()) {
        return None;
    }
    Some([parts[0], parts[1], parts[2]])
}

/// `q3ToEngine`.
fn q3_to_engine([x, y, z]: [f64; 3]) -> [f64; 3] {
    [x, z, y]
}

/// `flags`: `parseInt(spawnflags) || 0`.
fn flags(entity: &BspEntity) -> i32 {
    let text = entity.properties.get("spawnflags").unwrap_or("0").trim_start();
    let sign = text.starts_with('-');
    let digits: String = text
        .trim_start_matches(['-', '+'])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let value: i32 = digits.parse().unwrap_or(0);
    if sign { -value } else { value }
}

/// `number`: `parseFloat`, or the fallback when it is not finite.
fn number(value: Option<&str>, fallback: f64) -> f64 {
    let Some(text) = value else { return fallback };
    let text = text.trim_start();
    let end = text
        .char_indices()
        .take_while(|(i, c)| c.is_ascii_digit() || *c == '.' || (*i == 0 && (*c == '-' || *c == '+')))
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    text[..end]
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .unwrap_or(fallback)
}

/// `RotationPlan`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationPlan {
    /// Engine space.
    pub axis: [f64; 3],
    pub degrees_per_second: f64,
    /// False for a rotator that waits for a trigger.
    pub spinning: bool,
}

/// `rotationPlan`: Quake turns about its vertical axis unless spawnflags
/// pick X (4) or Y (8); 2 reverses. The Quake-to-engine axis swap mirrors
/// space, which turns every rotation the other way.
#[must_use]
pub fn rotation_plan(entity: &BspEntity) -> RotationPlan {
    let spawnflags = flags(entity);
    let q3_axis = if spawnflags & ROTATING_X_AXIS != 0 {
        [1.0, 0.0, 0.0]
    } else if spawnflags & ROTATING_Y_AXIS != 0 {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let reverse = spawnflags & ROTATING_REVERSE != 0;
    let parsed = number(entity.properties.get("speed"), 0.0);
    let speed = if parsed == 0.0 { 100.0 } else { parsed };
    RotationPlan {
        axis: q3_to_engine(q3_axis),
        degrees_per_second: if reverse { speed } else { -speed },
        spinning: entity.properties.get("targetname").is_none_or(str::is_empty) || spawnflags & ROTATING_START_ON != 0,
    }
}

/// `handleFuncRotating`'s placement, in engine space: the model turns about
/// `pivot` (its own coordinates) and that point stands at `position`. An
/// origin brush stores the model relative to its origin; without one the
/// model sits where it is and turns about the origin key, if any.
#[must_use]
pub fn rotation_pivot(entity: &BspEntity, model: &BspModel) -> ([f64; 3], [f64; 3]) {
    let Some(origin) = parse_q3_vector(entity.properties.get("origin")) else {
        return ([0.0; 3], [0.0; 3]);
    };
    let inside =
        (0..3).all(|i| origin[i] >= f64::from(model.mins[i]) - 1.0 && origin[i] <= f64::from(model.maxs[i]) + 1.0);
    let at = q3_to_engine(origin);
    if inside { (at, at) } else { ([0.0; 3], at) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::bsp::bsp_loader::Properties;

    fn entity(pairs: &[(&str, &str)]) -> BspEntity {
        let mut properties = Properties::default();
        for (k, v) in pairs {
            properties.set(k, v);
        }
        BspEntity {
            classname: "func_rotating".to_owned(),
            properties,
        }
    }

    fn model(mins: [f32; 3], maxs: [f32; 3]) -> BspModel {
        BspModel {
            mins,
            maxs,
            first_face: 0,
            num_faces: 0,
            first_brush: 0,
            num_brushes: 0,
        }
    }

    #[test]
    fn spins_about_quake_z_backwards_by_default() {
        let plan = rotation_plan(&entity(&[("speed", "550"), ("spawnflags", "16")]));
        assert_eq!(plan.axis, [0.0, 1.0, 0.0]);
        assert_eq!(plan.degrees_per_second, -550.0);
        assert!(plan.spinning);
    }

    #[test]
    fn spawnflags_pick_the_axis_and_reverse() {
        let x = rotation_plan(&entity(&[("spawnflags", "20")]));
        assert_eq!(x.axis, [1.0, 0.0, 0.0]);
        assert_eq!(x.degrees_per_second, -100.0);
        let y = rotation_plan(&entity(&[("spawnflags", "10"), ("speed", "700")]));
        assert_eq!(y.axis, [0.0, 0.0, 1.0]);
        assert_eq!(y.degrees_per_second, 700.0);
    }

    #[test]
    fn a_named_rotator_waits_unless_started() {
        assert!(!rotation_plan(&entity(&[("targetname", "fan")])).spinning);
        assert!(rotation_plan(&entity(&[("targetname", "fan"), ("spawnflags", "1")])).spinning);
    }

    #[test]
    fn an_origin_inside_the_model_is_its_pivot() {
        let fan = entity(&[("origin", "271 136 -85")]);
        let inside = model([200.0, 100.0, -100.0], [300.0, 200.0, -50.0]);
        assert_eq!(
            rotation_pivot(&fan, &inside),
            ([271.0, -85.0, 136.0], [271.0, -85.0, 136.0])
        );
        let origin_brush = model([-40.0, -40.0, -10.0], [40.0, 40.0, 10.0]);
        assert_eq!(rotation_pivot(&fan, &origin_brush), ([0.0; 3], [271.0, -85.0, 136.0]));
        assert_eq!(rotation_pivot(&entity(&[]), &inside), ([0.0; 3], [0.0; 3]));
    }
}
