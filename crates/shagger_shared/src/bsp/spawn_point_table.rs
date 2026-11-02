//! SHAG `src/shared/bsp/SpawnPointTable.ts`: the map's spawn points and the
//! way a player spawned at each faces.
//!
//! Q3 coordinates (x right, y forward, z up) become the engine's (x right,
//! y up, z forward), raised by [`PLAYER_ORIGIN_HEIGHT`], since spawn
//! entities sit at feet level. The team-tagged lists (`extractTeamSpawnPoints`,
//! `hasTeamSpawns`) are not ported yet; the client needs only the facings.

use crate::bsp::map_entities::{EntityLike, split_numbers};
use crate::bsp::trigger_table::q3_angle_to_game_yaw;
use crate::js::js_string_to_number;
use crate::server::simulation::types::Vec3;

/// BSP spawn entities sit at feet level; the player origin is 24 units above.
pub const PLAYER_ORIGIN_HEIGHT: f64 = 24.0;

/// Untagged spawn classnames, usable by any player in any gametype.
pub const SPAWN_CLASSNAMES: [&str; 4] = [
    "info_player_deathmatch",
    "info_player_start",
    "info_player_team1",
    "info_player_team2",
];

/// Warsow's team spawn classnames (alpha is udon, beta is soba).
pub const TEAM_SPAWN_CLASSNAMES: [(&str, &str); 2] = [("team_CTF_alphaspawn", "udon"), ("team_CTF_betaspawn", "soba")];

/// Horizontal reach of a spawn's facing: the server moves a player up to
/// 24 units along each axis off the spawn point, about 34 diagonally.
pub const SPAWN_FACING_RADIUS: f64 = 40.0;

/// A spawn point's position and the yaw a player spawned there faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnFacing {
    pub position: Vec3,
    /// Game yaw in radians, as `q3_angle_to_game_yaw` returns it.
    pub yaw: f64,
}

fn is_spawn(classname: &str) -> bool {
    SPAWN_CLASSNAMES.contains(&classname) || TEAM_SPAWN_CLASSNAMES.iter().any(|(c, _)| *c == classname)
}

/// `parseSpawnOrigin`.
fn parse_spawn_origin(origin: Option<&str>) -> Option<Vec3> {
    let origin = origin.filter(|o| !o.is_empty())?;
    let parts = split_numbers(origin);
    if parts.len() < 3 || parts.iter().any(|n| !n.is_finite()) {
        return None;
    }
    Some(Vec3::new(parts[0], parts[2] + PLAYER_ORIGIN_HEIGHT, parts[1]))
}

/// Q3 yaw in degrees from "angles" ("pitch yaw roll") or "angle"; 0 when absent.
fn spawn_angle_degrees<E: EntityLike>(entity: &E) -> f64 {
    if let Some(angles) = entity.property("angles") {
        let angles = split_numbers(angles);
        if angles.len() >= 2 && angles[1].is_finite() {
            return angles[1];
        }
    }
    let angle = entity.property("angle").map_or(f64::NAN, js_string_to_number);
    if angle.is_finite() { angle } else { 0.0 }
}

/// `extractSpawnPoints`: every spawn point, generic and team-tagged, in BSP order.
#[must_use]
pub fn extract_spawn_points<E: EntityLike>(entities: &[E]) -> Vec<Vec3> {
    entities
        .iter()
        .filter(|e| is_spawn(e.classname()))
        .filter_map(|e| parse_spawn_origin(e.property("origin")))
        .collect()
}

/// `extractSpawnFacings`: every spawn point with its authored facing.
#[must_use]
pub fn extract_spawn_facings<E: EntityLike>(entities: &[E]) -> Vec<SpawnFacing> {
    entities
        .iter()
        .filter(|e| is_spawn(e.classname()))
        .filter_map(|e| {
            parse_spawn_origin(e.property("origin")).map(|position| SpawnFacing {
                position,
                yaw: q3_angle_to_game_yaw(spawn_angle_degrees(e)),
            })
        })
        .collect()
}

/// `spawnFacingNear`: the facing of the spawn point nearest `position`, or
/// `None` when none is within reach (40 units across, 32 up or down).
#[must_use]
pub fn spawn_facing_near(facings: &[SpawnFacing], position: Vec3) -> Option<f64> {
    let mut best = None;
    let mut best_distance = SPAWN_FACING_RADIUS * SPAWN_FACING_RADIUS;
    for facing in facings {
        let dx = facing.position.x - position.x;
        let dz = facing.position.z - position.z;
        let distance = dx * dx + dz * dz;
        if distance <= best_distance && (facing.position.y - position.y).abs() <= 32.0 {
            best = Some(facing.yaw);
            best_distance = distance;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    struct E(&'static str, Vec<(&'static str, &'static str)>);

    impl EntityLike for E {
        fn classname(&self) -> &str {
            self.0
        }
        fn property(&self, key: &str) -> Option<&str> {
            self.1.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
        }
    }

    #[test]
    fn facings_and_nearest() {
        let entities = [
            E(
                "info_player_deathmatch",
                vec![("origin", " 10 20 30 "), ("angle", "90")],
            ),
            E("team_CTF_betaspawn", vec![("origin", "100 0 0"), ("angles", "0 180 0")]),
            E("info_player_start", vec![("origin", "1 2")]),
            E("light", vec![("origin", "0 0 0")]),
        ];
        let facings = extract_spawn_facings(&entities);
        assert_eq!(facings.len(), 2);
        assert_eq!(facings[0].position, Vec3::new(10.0, 54.0, 20.0));
        assert_eq!(facings[0].yaw, q3_angle_to_game_yaw(90.0));
        assert_eq!(facings[1].yaw, q3_angle_to_game_yaw(180.0));
        assert_eq!(extract_spawn_points(&entities).len(), 2);
        assert_eq!(
            spawn_facing_near(&facings, Vec3::new(30.0, 60.0, 40.0)),
            Some(facings[0].yaw)
        );
        assert_eq!(spawn_facing_near(&facings, Vec3::new(10.0, 100.0, 20.0)), None);
        assert_eq!(spawn_facing_near(&facings, Vec3::new(60.0, 54.0, 20.0)), None);
    }
}
