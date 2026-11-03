//! The highlight reel against SHAG's own `DemoAnalyzer`, `SequenceBuilder`
//! and `CinematicCameraPaths` on synthetic matches
//! (`tools/golden/highlights.mjs`): every highlight, every shot and every
//! camera keyframe must agree bit for bit.

mod common;

use common::{f, num, s};
use shagger_shared::demo::cinematic_camera_paths::{CameraPath, Interpolation, generate_camera_path, interpolate_path};
use shagger_shared::demo::demo_analyzer::{AnalyzerEvent, DemoAnalyzer, HighlightCandidate, HighlightEvent};
use shagger_shared::demo::demo_reader::{DemoFrame, DemoHeader, DemoPlayerInfo};
use shagger_shared::demo::sequence_builder;
use shagger_shared::json::{self, Value};
use shagger_shared::net::game_state::{Entity, GameState, MovementState, PlayerState, Transform, Vec3, Velocity};

#[derive(Debug)]
enum Event {
    CombatState(String, f64),
    Award(String, String),
    Death(String, String, String, Vec3),
    Other,
}

impl HighlightEvent for Event {
    fn analyzer_event(&self) -> AnalyzerEvent<'_> {
        match self {
            Self::CombatState(id, health) => AnalyzerEvent::CombatState {
                entity_id: id,
                health: *health,
            },
            Self::Award(player, award) => AnalyzerEvent::Award {
                player_id: player,
                award_type: award,
            },
            Self::Death(victim, killer, weapon, at) => AnalyzerEvent::Death {
                entity_id: victim,
                killer_id: killer,
                weapon_id: weapon,
                at: *at,
            },
            Self::Other => AnalyzerEvent::Other,
        }
    }
}

fn n(v: &Value) -> f64 {
    v.as_f64().expect("number")
}

fn arr(v: &Value) -> &[Value] {
    v.as_array().expect("array")
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn frame(v: &Value) -> DemoFrame<Event> {
    let mut state = GameState::<Event>::default();
    for e in arr(v.get("entities").expect("entities")) {
        let e = arr(e);
        let mut entity = Entity::default();
        if let Some(t) = e[1].as_array() {
            entity.transform = Some(Transform {
                position: Vec3::new(n(&t[0]), n(&t[1]), n(&t[2])),
                rotation: [n(&t[3]), n(&t[4]), 0.0, 1.0],
            });
        }
        if let Some(t) = e[2].as_array() {
            entity.velocity = Some(Velocity {
                linear: Vec3::new(n(&t[0]), n(&t[1]), n(&t[2])),
                grounded: n(&t[3]) != 0.0,
                ..Velocity::default()
            });
        }
        if let Some(t) = e[3].as_array() {
            entity.movement_state = Some(MovementState {
                is_sliding: n(&t[0]) != 0.0,
                is_wall_jumping: n(&t[1]) != 0.0,
                ..MovementState::default()
            });
        }
        state.entities.insert(s(&e[0]), entity);
    }
    for p in arr(v.get("players").expect("players")) {
        let p = arr(p);
        state.players.insert(
            s(&p[0]),
            PlayerState {
                name: s(&p[1]).to_owned(),
                ..PlayerState::default()
            },
        );
    }
    for ev in arr(v.get("events").expect("events")) {
        state.events.push(match ev.get("type").and_then(Value::as_str) {
            Some("combat_state") => Event::CombatState(text(ev, "entityId"), n(ev.get("health").expect("health"))),
            Some("award") => Event::Award(text(ev, "playerId"), text(ev, "awardType")),
            Some("death") => Event::Death(
                text(ev, "entityId"),
                text(ev, "killerId"),
                text(ev, "weaponId"),
                Vec3::new(
                    n(ev.get("x").unwrap()),
                    n(ev.get("y").unwrap()),
                    n(ev.get("z").unwrap()),
                ),
            ),
            _ => Event::Other,
        });
    }
    DemoFrame {
        tick: n(v.get("tick").expect("tick")) as u32,
        state,
        inputs: Vec::new(),
    }
}

fn header(v: &Value) -> DemoHeader {
    DemoHeader {
        version: 6,
        map: String::new(),
        gametype: String::new(),
        tick_rate: n(v.get("tickRate").expect("tickRate")) as u8,
        snapshot_rate: 20,
        start_time: 0.0,
        match_id: None,
        private: None,
        players: arr(v.get("players").expect("players"))
            .iter()
            .map(|p| DemoPlayerInfo {
                player_id: text(p, "playerId"),
                name: text(p, "name"),
                team: String::new(),
                account_id: None,
            })
            .collect(),
    }
}

fn opt_vec(v: Option<Vec3>) -> Value {
    v.map_or(Value::Null, |v| Value::Array(vec![num(v.x), num(v.y), num(v.z)]))
}

fn opt_str(v: Option<&String>) -> Value {
    v.map_or(Value::Null, |s| Value::String(s.clone()))
}

fn int(x: i64) -> Value {
    Value::Number(x as f64)
}

fn highlight(h: &HighlightCandidate) -> Value {
    let fields: Vec<(&str, Value)> = vec![
        ("startTick", int(h.start_tick)),
        ("peakTick", int(h.peak_tick)),
        ("endTick", int(h.end_tick)),
        ("score", num(h.score)),
        ("type", Value::String(h.kind.as_str().into())),
        ("primaryPlayer", Value::String(h.primary_player.clone())),
        ("primaryPlayerName", Value::String(h.primary_player_name.clone())),
        ("victimPlayer", opt_str(h.victim_player.as_ref())),
        ("victimPlayerName", opt_str(h.victim_player_name.as_ref())),
        ("weaponId", opt_str(h.weapon_id.as_ref())),
        (
            "killCount",
            h.kill_count.map_or(Value::Null, |k| Value::Number(f64::from(k))),
        ),
        (
            "tags",
            Value::Array(h.tags.iter().map(|t| Value::String(t.as_str().into())).collect()),
        ),
        ("killerPos", opt_vec(h.killer_pos)),
        ("victimPos", opt_vec(h.victim_pos)),
        ("killerVelocity", opt_vec(h.killer_velocity)),
        ("killerYaw", h.killer_yaw.map_or(Value::Null, num)),
        (
            "victimFacedKiller",
            h.victim_faced_killer.map_or(Value::Null, Value::Bool),
        ),
        (
            "path",
            h.path.as_ref().map_or(Value::Null, |p| {
                Value::Array(
                    p.iter()
                        .map(|p| Value::Array(vec![int(p.tick), num(p.x), num(p.y), num(p.z)]))
                        .collect(),
                )
            }),
        ),
    ];
    Value::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn same_vec(got: Vec3, want: &Value, what: &str) {
    let w = arr(want);
    for (g, w) in [got.x, got.y, got.z].into_iter().zip(w) {
        let w = f(w);
        assert_eq!(g.to_bits(), w.to_bits(), "{what}: {g} vs {w}");
    }
}

#[test]
fn highlight_reels_match_shag() {
    let doc = json::parse(include_str!("golden/highlights.json")).expect("json");
    let vectors = arr(doc.get("vectors").expect("vectors"));
    assert!(vectors.len() >= 30);
    let ground = |origin: Vec3| (origin.x >= 0.0).then_some(origin.y);
    let mut shots = 0;
    for (i, v) in vectors.iter().enumerate() {
        let input = v.get("input").unwrap();
        let frames: Vec<_> = arr(input.get("frames").unwrap()).iter().map(frame).collect();
        let header = header(input.get("header").unwrap());
        let analyzer = if input.get("ground").and_then(Value::as_bool) == Some(true) {
            DemoAnalyzer::new(Some(&ground))
        } else {
            DemoAnalyzer::new(None)
        };
        let analysis = analyzer.analyze(&frames, &header, 123_456.0);
        let want = v.get("analysis").unwrap();

        let got: Vec<Value> = analysis.highlights.iter().map(highlight).collect();
        let want_highlights = arr(want.get("highlights").unwrap());
        assert_eq!(got.len(), want_highlights.len(), "vector {i}: highlight count");
        for (j, (g, w)) in got.iter().zip(want_highlights).enumerate() {
            assert_eq!(g, w, "vector {i}, highlight {j}");
        }
        assert_eq!(
            analysis.total_ticks as f64,
            n(want.get("totalTicks").unwrap()),
            "vector {i}"
        );
        let names: Vec<Value> = analysis
            .player_names
            .iter()
            .map(|(k, v)| Value::Array(vec![Value::String(k.into()), Value::String(v.clone())]))
            .collect();
        assert_eq!(&names[..], arr(want.get("playerNames").unwrap()), "vector {i}: names");
        let frags: Vec<Value> = analysis
            .frags
            .iter()
            .map(|(k, v)| Value::Array(vec![Value::String(k.clone()), Value::Number(f64::from(*v))]))
            .collect();
        assert_eq!(&frags[..], arr(want.get("frags").unwrap()), "vector {i}: frags");

        let minutes = n(input.get("minutes").unwrap());
        let reel = sequence_builder::build(&analysis, minutes, n(input.get("tickRate").unwrap()));
        let want = v.get("reel").unwrap();
        assert_eq!(
            num(reel.total_duration_ms),
            want.get("totalDurationMs").unwrap().clone(),
            "vector {i}"
        );
        assert_eq!(reel.total_scenes as f64, n(want.get("totalScenes").unwrap()));
        assert_eq!(reel.tick_rate, n(want.get("tickRate").unwrap()));
        let want_segments = arr(want.get("segments").unwrap());
        assert_eq!(reel.segments.len(), want_segments.len(), "vector {i}: segments");
        for (j, (seg, w)) in reel.segments.iter().zip(want_segments).enumerate() {
            let at = format!("vector {i}, segment {j}");
            assert_eq!(seg.start_tick as f64, n(w.get("startTick").unwrap()), "{at}");
            assert_eq!(seg.end_tick as f64, n(w.get("endTick").unwrap()), "{at}");
            assert_eq!(num(seg.duration_ms), *w.get("durationMs").unwrap(), "{at}");
            assert_eq!(seg.camera_preset.as_str(), s(w.get("cameraPreset").unwrap()), "{at}");
            assert_eq!(
                opt_str(seg.camera_target.as_ref()),
                *w.get("cameraTarget").unwrap(),
                "{at}"
            );
            assert_eq!(num(seg.playback_speed), *w.get("playbackSpeed").unwrap(), "{at}");
            let keys: Vec<Value> = seg
                .speed_keyframes
                .iter()
                .map(|k| Value::Array(vec![int(k.tick), num(k.speed)]))
                .collect();
            assert_eq!(&keys[..], arr(w.get("speedKeyframes").unwrap()), "{at}");
            let index = n(w.get("highlight").unwrap()) as usize;
            assert_eq!(seg.highlight, analysis.highlights[index], "{at}");
            assert_eq!(Some(seg.is_replay), w.get("isReplay").and_then(Value::as_bool), "{at}");
            assert_eq!(seg.segment_index as f64, n(w.get("segmentIndex").unwrap()), "{at}");
            assert_eq!(seg.scene as f64, n(w.get("scene").unwrap()), "{at}");
            assert_eq!(seg.role.as_str(), s(w.get("role").unwrap()), "{at}");
            assert_eq!(seg.transition_in.as_str(), s(w.get("transitionIn").unwrap()), "{at}");
            assert_eq!(seg.transition_out.as_str(), s(w.get("transitionOut").unwrap()), "{at}");
            assert_eq!(
                num(seg.transition_duration_ms),
                *w.get("transitionDurationMs").unwrap(),
                "{at}"
            );

            let path = generate_camera_path(
                seg.camera_preset,
                &seg.highlight,
                seg.start_tick as f64,
                seg.end_tick as f64,
                reel.tick_rate,
            );
            let wp = w.get("path").unwrap();
            match path {
                CameraPath::Chase { target_entity_id } => {
                    assert_eq!(target_entity_id, s(wp.get("chase").unwrap()), "{at}");
                }
                CameraPath::Freefly {
                    keyframes,
                    interpolation,
                } => {
                    let name = match interpolation {
                        Interpolation::Linear => "linear",
                        Interpolation::CatmullRom => "catmull_rom",
                    };
                    assert_eq!(name, s(wp.get("interpolation").unwrap()), "{at}");
                    let want_keys = arr(wp.get("keyframes").unwrap());
                    assert_eq!(keyframes.len(), want_keys.len(), "{at}");
                    for (k, wk) in keyframes.iter().zip(want_keys) {
                        let wk = arr(wk);
                        assert_eq!(num(k.tick), wk[0], "{at}");
                        same_vec(k.position, &wk[1], &at);
                        same_vec(k.look_at, &wk[2], &at);
                        assert_eq!(k.fov, wk[3].as_f64(), "{at}");
                    }
                    for sample in arr(wp.get("samples").unwrap()) {
                        let sample = arr(sample);
                        let pose = interpolate_path(&keyframes, f(&sample[0]), interpolation);
                        same_vec(pose.position, &sample[1], &at);
                        same_vec(pose.look_at, &sample[2], &at);
                        assert_eq!(num(pose.fov), sample[3], "{at}");
                    }
                    shots += 1;
                }
            }
        }
    }
    assert!(shots > 50, "freefly shots: {shots}");
}
