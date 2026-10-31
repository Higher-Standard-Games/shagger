//! The wire protocol against bytes and decoded states from SHAG's own
//! `BinaryProtocol` and `MessageCodec` (`tools/golden/protocol.mjs`).

use shagger_shared::js::{to_uint8, to_uint16, to_uint32};
use shagger_shared::json::{self, Value};
use shagger_shared::net::binary_protocol::{BinaryDecoder, BinaryEncoder};
use shagger_shared::net::command::Command;
use shagger_shared::net::game_state::GameState;
use shagger_shared::net::message_codec as mc;
use shagger_shared::net::snapshot::{NoEvents, decode_snapshot};
use std::collections::HashMap;

fn vectors(text: &str) -> Vec<Value> {
    let doc = json::parse(text).expect("golden json");
    doc.get("vectors").and_then(Value::as_array).expect("vectors").to_vec()
}

fn bytes(v: &Value) -> Vec<u8> {
    let s = v.as_str().expect("hex string");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn f(v: &Value) -> f64 {
    match v.as_str().expect("float string") {
        "nan" => f64::NAN,
        hex => f64::from_bits(u64::from_str_radix(hex, 16).expect("hex f64")),
    }
}

fn s(v: &Value) -> &str {
    v.as_str().expect("string")
}

/// The canonical float form `protocol.mjs` writes.
fn num(x: f64) -> Value {
    Value::String(if x.is_nan() {
        "nan".to_owned()
    } else {
        format!("{:016x}", x.to_bits())
    })
}

fn obj(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn int(x: impl Into<f64>) -> Value {
    Value::Number(x.into())
}

fn text(x: &str) -> Value {
    Value::String(x.to_owned())
}

fn vec3(v: shagger_shared::net::game_state::Vec3) -> Value {
    Value::Array(vec![num(v.x), num(v.y), num(v.z)])
}

/// The shape of `canonicalState` in `protocol.mjs`.
fn canonical<E>(state: &GameState<E>) -> Value {
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

#[test]
fn strings_decode_like_text_decoder() {
    let all = vectors(include_str!("golden/strings.json"));
    for (i, v) in all.iter().enumerate() {
        let raw = bytes(v.get("bytes").unwrap());
        let mut e = BinaryEncoder::new();
        e.write_uint16(u16::try_from(raw.len()).unwrap());
        e.write_bytes(&raw);
        let decoded = BinaryDecoder::new(e.data()).read_string().unwrap();
        let units: Vec<f64> = decoded.encode_utf16().map(f64::from).collect();
        let expected: Vec<f64> = v
            .get("units")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|u| u.as_f64().unwrap())
            .collect();
        assert_eq!(units, expected, "vector {i}: bytes {raw:02x?}");
    }
    assert!(all.len() > 400);
}

#[test]
fn raw_snapshots_decode_like_shag() {
    let all = vectors(include_str!("golden/snapshots.json"));
    let mut decoded: HashMap<usize, GameState<std::convert::Infallible>> = HashMap::new();
    let (mut ok, mut errors) = (0, 0);
    for (i, v) in all.iter().enumerate() {
        let data = bytes(v.get("bytes").unwrap());
        let baseline = v
            .get("baseline")
            .and_then(Value::as_f64)
            .map(|b| &decoded[&(b as usize)]);
        let result = decode_snapshot(&data, baseline, &NoEvents);
        match (result, v.get("decoded")) {
            (Ok(state), Some(expected)) => {
                assert_eq!(&canonical(&state), expected, "vector {i}");
                decoded.insert(i, state);
                ok += 1;
            }
            (Err(_), None) => errors += 1,
            (Ok(_), None) => panic!("vector {i}: SHAG failed with {:?}, Rust decoded", v.get("error")),
            (Err(e), Some(_)) => panic!("vector {i}: SHAG decoded, Rust failed with {e}"),
        }
    }
    assert!(
        ok > 200 && errors > 10,
        "golden file looks thin: {ok} ok, {errors} errors"
    );
}

#[test]
fn snapshot_envelopes_decode_like_shag() {
    let all = vectors(include_str!("golden/snapshot_envelopes.json"));
    let mut retained: HashMap<u32, GameState<std::convert::Infallible>> = HashMap::new();
    for (i, v) in all.iter().enumerate() {
        if i % 20 == 0 && i < 120 {
            retained.clear();
        }
        let data = bytes(v.get("bytes").unwrap());
        let result = mc::decode_snapshot(&data, |tick| retained.get(&tick), &NoEvents);
        match (result, v.get("decoded")) {
            (Ok(state), Some(expected)) => {
                assert_eq!(&canonical(&state), expected, "vector {i}");
                if i < 120 {
                    retained.insert(state.tick, state);
                }
            }
            (Err(_), None) => {}
            (Ok(_), None) => panic!("vector {i}: SHAG failed with {:?}, Rust decoded", v.get("error")),
            (Err(e), Some(_)) => panic!("vector {i}: SHAG decoded, Rust failed with {e}"),
        }
    }
}

#[test]
fn commands_encode_and_decode_like_shag() {
    let all = vectors(include_str!("golden/commands.json"));
    for (i, v) in all.iter().enumerate() {
        let dir = v.get("direction").and_then(Value::as_array).unwrap();
        // What JavaScript's DataView does to a number it writes into an integer field.
        let command = Command {
            direction: [f(&dir[0]), f(&dir[1]), f(&dir[2])],
            pitch: f(v.get("pitch").unwrap()),
            yaw: f(v.get("yaw").unwrap()),
            buttons: to_uint16(f(v.get("buttons").unwrap())),
            tick: to_uint32(f(v.get("tick").unwrap())),
            weapon_slot: to_uint8(f(v.get("weaponSlot").unwrap())),
        };
        let player_id = s(v.get("playerId").unwrap());
        let sequence = to_uint32(f(v.get("sequence").unwrap()));
        let data = mc::encode_command(player_id, sequence, &command);
        assert_eq!(data, bytes(v.get("bytes").unwrap()), "vector {i}: encode");
        assert_eq!(
            mc::peek_command_seq(&data).map(f64::from),
            v.get("peek").and_then(Value::as_f64),
            "vector {i}"
        );

        let d = v.get("decoded").unwrap();
        let (id, seq, back) = mc::decode_command(&data).unwrap();
        assert_eq!(id, s(d.get("playerId").unwrap()));
        assert_eq!(f64::from(seq), d.get("sequence").unwrap().as_f64().unwrap());
        let dd = d.get("direction").and_then(Value::as_array).unwrap();
        for (k, (got, want)) in back.direction.iter().zip(dd).enumerate() {
            assert_eq!(num(*got), *want, "vector {i}: direction {k}");
        }
        assert_eq!(num(back.pitch), *d.get("pitch").unwrap(), "vector {i}");
        assert_eq!(num(back.yaw), *d.get("yaw").unwrap(), "vector {i}");
        assert_eq!(f64::from(back.buttons), d.get("buttons").unwrap().as_f64().unwrap());
        assert_eq!(f64::from(back.tick), d.get("tick").unwrap().as_f64().unwrap());
        assert_eq!(
            f64::from(back.weapon_slot),
            d.get("weaponSlot").unwrap().as_f64().unwrap()
        );
        let sent = command.as_sent();
        let bits = |c: &Command| [c.direction[0], c.direction[1], c.direction[2], c.pitch, c.yaw].map(num);
        assert_eq!(bits(&back), bits(&sent), "vector {i}: as_sent is what the server reads");
        assert_eq!(
            (back.buttons, back.tick, back.weapon_slot),
            (sent.buttons, sent.tick, sent.weapon_slot)
        );
    }
}

fn opt(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_owned)
}

fn u8_of(v: &Value) -> u8 {
    to_uint8(v.as_f64().unwrap())
}

#[test]
#[allow(clippy::too_many_lines)]
fn messages_encode_and_decode_like_shag() {
    let all = vectors(include_str!("golden/messages.json"));
    let mut names = std::collections::BTreeSet::new();
    for (i, v) in all.iter().enumerate() {
        let name = s(v.get("name").unwrap());
        names.insert(name.to_owned());
        let a = v.get("args").and_then(Value::as_array).unwrap();
        let data = bytes(v.get("bytes").unwrap());
        let d = v.get("decoded");
        let field = |k: &str| d.and_then(|d| d.get(k)).unwrap();
        let encoded = match name {
            "welcome" => {
                let w = mc::Welcome {
                    server_id: s(&a[0]).into(),
                    server_name: s(&a[1]).into(),
                    map: s(&a[2]).into(),
                    tick_rate: u8_of(&a[3]),
                    max_players: u8_of(&a[4]),
                    current_players: u8_of(&a[5]),
                };
                let back = mc::decode_welcome(&data).unwrap();
                assert_eq!(back.server_id, s(field("serverId")));
                assert_eq!(back.server_name, s(field("serverName")));
                assert_eq!(back.map, s(field("map")));
                assert_eq!(f64::from(back.tick_rate), field("tickRate").as_f64().unwrap());
                assert_eq!(f64::from(back.max_players), field("maxPlayers").as_f64().unwrap());
                assert_eq!(
                    f64::from(back.current_players),
                    field("currentPlayers").as_f64().unwrap()
                );
                Some(mc::encode_welcome(&w))
            }
            "join" | "joinShort" => {
                let back = mc::decode_join(&data).unwrap();
                assert_eq!(back.player_name, s(field("playerName")));
                assert_eq!(back.is_bot, field("isBot").as_bool().unwrap());
                assert_eq!(back.admission_ticket, opt(d.unwrap().get("admissionTicket")));
                (name == "join").then(|| {
                    mc::encode_join(&mc::Join {
                        player_name: s(&a[0]).into(),
                        is_bot: a[1].as_bool().unwrap(),
                        admission_ticket: opt(a.get(2)),
                    })
                })
            }
            "joinAccept" => {
                let back = mc::decode_join_accept(&data).unwrap();
                assert_eq!(back.client_id, s(field("clientId")));
                assert_eq!(back.player_id, s(field("playerId")));
                assert_eq!(back.entity_id, s(field("entityId")));
                Some(mc::encode_join_accept(&mc::JoinAccept {
                    client_id: s(&a[0]).into(),
                    player_id: s(&a[1]).into(),
                    entity_id: s(&a[2]).into(),
                }))
            }
            "resumeToken" => {
                let back = mc::decode_resume_token(&data).unwrap();
                assert_eq!(back.token, s(field("token")));
                assert_eq!(f64::from(back.grace_ms), field("graceMs").as_f64().unwrap());
                Some(mc::encode_resume_token(&mc::ResumeToken {
                    token: s(&a[0]).into(),
                    grace_ms: to_uint32(a[1].as_f64().unwrap()),
                }))
            }
            "resume" => {
                assert_eq!(mc::decode_resume(&data).unwrap(), s(field("token")));
                Some(mc::encode_resume(s(&a[0])))
            }
            "ack" => {
                assert_eq!(
                    f64::from(mc::decode_ack(&data).unwrap()),
                    field("tick").as_f64().unwrap()
                );
                Some(mc::encode_ack(to_uint32(a[0].as_f64().unwrap())))
            }
            "joinGame" => {
                let back = mc::decode_join_game(&data).unwrap();
                assert_eq!(back.team_id, opt(d.unwrap().get("teamId")));
                assert_eq!(back.manual_pick, field("manualPick").as_bool().unwrap());
                assert_eq!(back.admission_ticket, opt(d.unwrap().get("admissionTicket")));
                Some(mc::encode_join_game(&mc::JoinGame {
                    team_id: opt(a.first()),
                    manual_pick: a[1].as_bool().unwrap(),
                    admission_ticket: opt(a.get(2)),
                }))
            }
            "setLoadout" => {
                let (p, q) = mc::decode_set_loadout(&data).unwrap();
                assert_eq!((p.as_str(), q.as_str()), (s(field("primary")), s(field("secondary"))));
                Some(mc::encode_set_loadout(s(&a[0]), s(&a[1])))
            }
            "clientPrefs" | "clientPrefsShort" => {
                let back = mc::decode_client_prefs(&data).unwrap();
                assert_eq!(back.auto_switch, field("autoSwitch").as_bool().unwrap());
                (name == "clientPrefs").then(|| {
                    mc::encode_client_prefs(mc::ClientPrefs {
                        auto_switch: a[0].get("autoSwitch").unwrap().as_bool().unwrap(),
                    })
                })
            }
            "chat" => {
                assert_eq!(mc::decode_chat(&data).unwrap(), s(field("text")));
                Some(mc::encode_chat(s(&a[0])))
            }
            "serverMessage" => {
                let back = mc::decode_server_message(&data).unwrap();
                assert_eq!(back.kind, s(field("type")));
                assert_eq!(back.message, s(field("message")));
                Some(mc::encode_server_message(&mc::ServerMessage {
                    message: s(&a[0]).into(),
                    kind: s(&a[1]).into(),
                }))
            }
            "ping" => {
                assert_eq!(num(mc::decode_ping(&data).unwrap()), *field("clientSendTime"));
                Some(mc::encode_ping(f(&a[0])))
            }
            "pong" => {
                let back = mc::decode_pong(&data).unwrap();
                assert_eq!(num(back.client_send_time), *field("clientSendTime"));
                assert_eq!(num(back.server_time), *field("serverTime"));
                Some(mc::encode_pong(mc::Pong {
                    client_send_time: f(&a[0]),
                    server_time: f(&a[1]),
                }))
            }
            "ready" => Some(mc::encode_ready()),
            "spectate" => Some(mc::encode_spectate()),
            "requestSpawn" => Some(mc::encode_request_spawn()),
            other => panic!("vector {i}: unknown message {other}"),
        };
        if let Some(encoded) = encoded {
            assert_eq!(encoded, data, "vector {i}: encode {name}");
        }
    }
    assert!(names.len() >= 18, "messages missing: {names:?}");
}
