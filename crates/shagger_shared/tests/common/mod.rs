//! Helpers shared by the golden tests that read `protocol.mjs` output: hex
//! bytes, f64 bit strings and `canonicalState`.
#![allow(dead_code)]

use shagger_shared::json::{self, Value};
use shagger_shared::net::game_state::GameState;

pub fn vectors(text: &str) -> Vec<Value> {
    let doc = json::parse(text).expect("golden json");
    doc.get("vectors").and_then(Value::as_array).expect("vectors").to_vec()
}

pub fn bytes(v: &Value) -> Vec<u8> {
    let s = v.as_str().expect("hex string");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

pub fn f(v: &Value) -> f64 {
    match v.as_str().expect("float string") {
        "nan" => f64::NAN,
        hex => f64::from_bits(u64::from_str_radix(hex, 16).expect("hex f64")),
    }
}

pub fn s(v: &Value) -> &str {
    v.as_str().expect("string")
}

/// The canonical float form `protocol.mjs` writes.
pub fn num(x: f64) -> Value {
    Value::String(if x.is_nan() {
        "nan".to_owned()
    } else {
        format!("{:016x}", x.to_bits())
    })
}

pub fn obj(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

pub fn int(x: impl Into<f64>) -> Value {
    Value::Number(x.into())
}

pub fn text(x: &str) -> Value {
    Value::String(x.to_owned())
}

pub fn vec3(v: shagger_shared::net::game_state::Vec3) -> Value {
    Value::Array(vec![num(v.x), num(v.y), num(v.z)])
}

/// The shape of `canonicalState` in `protocol.mjs`.
pub fn canonical<E>(state: &GameState<E>) -> Value {
    let entities = state
        .entities
        .iter()
        .map(|(id, e)| {
            let mut c = Vec::new();
            if let Some(p) = e.position {
                c.push(("position", vec3(p)));
            }
            if let Some(t) = e.transform {
                let rotation = t.rotation.iter().map(|&r| num(r)).collect();
                c.push((
                    "transform",
                    obj(vec![
                        ("position", vec3(t.position)),
                        ("rotation", Value::Array(rotation)),
                    ]),
                ));
            }
            if let Some(v) = e.velocity {
                c.push((
                    "velocity",
                    obj(vec![
                        ("linear", vec3(v.linear)),
                        ("grounded", Value::Bool(v.grounded)),
                        ("isCrouching", Value::Bool(v.is_crouching)),
                        ("isSliding", Value::Bool(v.is_sliding)),
                    ]),
                ));
            }
            if let Some(m) = e.movement_state {
                let mut fields = vec![
                    ("isCrouching", Value::Bool(m.is_crouching)),
                    ("isSliding", Value::Bool(m.is_sliding)),
                    (
                        "wallJumpState",
                        text(if m.is_wall_jumping { "charging" } else { "none" }),
                    ),
                    ("isAiming", Value::Bool(m.is_aiming)),
                ];
                if let Some(view) = m.view {
                    fields.push((
                        "view",
                        Value::Array(vec![
                            int(view.strafe_input),
                            int(view.forward_input),
                            num(view.slide_phase),
                            num(view.ledge_grab_phase),
                            num(view.crouch_view_offset),
                        ]),
                    ));
                }
                c.push(("movementState", obj(fields)));
            }
            if let Some(w) = &e.weapon_meta {
                c.push((
                    "weaponMeta",
                    obj(vec![
                        ("weaponId", text(&w.weapon_id)),
                        ("settled", Value::Bool(w.settled)),
                    ]),
                ));
            }
            if let Some(p) = e.powerups {
                c.push(("powerups", int(p)));
            }
            Value::Array(vec![text(id), obj(c)])
        })
        .collect();
    let players = state
        .players
        .iter()
        .map(|(id, p)| {
            Value::Array(vec![
                text(id),
                obj(vec![
                    ("entityId", text(&p.entity_id)),
                    ("name", text(&p.name)),
                    ("team", text(&p.team)),
                    ("score", int(p.score)),
                    ("deaths", int(p.deaths)),
                    ("ping", int(p.ping)),
                ]),
            ])
        })
        .collect();
    let mask: String = state
        .pickup_active_mask
        .iter()
        .map(|&b| if b { '1' } else { '0' })
        .collect();
    obj(vec![
        ("tick", int(state.tick)),
        ("serverTimestamp", num(state.server_timestamp)),
        ("ackedInputSeq", int(state.acked_input_seq)),
        ("entities", Value::Array(entities)),
        ("players", Value::Array(players)),
        ("events", int(u32::try_from(state.events.len()).unwrap())),
        ("pickupActiveMask", Value::String(mask)),
    ])
}
