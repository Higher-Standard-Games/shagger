//! The demo reader against SHAG's own `DemoReader` on synthetic `.shagdemo`
//! files of every version, raw and gzip, many of them damaged
//! (`tools/golden/protocol.mjs`). Header, index, every frame by index,
//! `readAllFrames` and `findFrameByTick` must agree, including which reads
//! fail.

mod common;

use common::{bytes, canonical, num, vectors};
use shagger_shared::demo::demo_inputs::RecordedInput;
use shagger_shared::demo::demo_reader::{DemoError, DemoReader};
use shagger_shared::json::Value;
use shagger_shared::net::snapshot::NoEvents;

fn err(e: &DemoError) -> Value {
    let message = match e {
        // zlib's own wording, which the golden file records.
        DemoError::Gzip(_) => "gzip".to_owned(),
        other => other.to_string(),
    };
    Value::Object(vec![("error".into(), Value::String(message))])
}

/// Gzip failures keep Node's zlib message; Rust's differs, so compare only that it failed.
fn same_error(got: &Value, want: &Value) -> bool {
    match (got.get("error"), want.get("error")) {
        (Some(Value::String(g)), Some(_)) if g == "gzip" => true,
        _ => got == want,
    }
}

fn opt(x: Option<Value>) -> Value {
    x.unwrap_or(Value::Null)
}

fn inputs(list: &[RecordedInput]) -> Value {
    Value::Array(
        list.iter()
            .map(|r| {
                Value::Array(vec![
                    Value::String(r.player_id.clone()),
                    Value::Number(f64::from(r.buttons)),
                    num(r.yaw),
                    num(r.pitch),
                    num(r.forward),
                    num(r.right),
                    Value::Number(f64::from(r.weapon_slot)),
                    Value::Number(f64::from(r.commands)),
                ])
            })
            .collect(),
    )
}

fn obj(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

#[test]
fn demos_read_like_shag() {
    let mut frames_read = 0;
    for (i, v) in vectors(include_str!("golden/demos.json")).iter().enumerate() {
        let data = bytes(v.get("bytes").unwrap());
        let mut reader = match DemoReader::new(data) {
            Ok(r) => r,
            Err(e) => {
                assert!(v.get("error").is_some(), "vector {i}: {e}, but SHAG read it");
                continue;
            }
        };
        assert!(
            v.get("error").is_none(),
            "vector {i}: SHAG failed to inflate, Rust did not"
        );

        let header = match reader.read_header() {
            Ok(h) => obj(vec![
                ("version", Value::Number(f64::from(h.version))),
                ("map", Value::String(h.map.clone())),
                ("gametype", Value::String(h.gametype.clone())),
                ("tickRate", Value::Number(f64::from(h.tick_rate))),
                ("snapshotRate", Value::Number(f64::from(h.snapshot_rate))),
                ("startTime", num(h.start_time)),
                ("matchId", opt(h.match_id.clone().map(Value::String))),
                ("private", opt(h.private.map(Value::Bool))),
                (
                    "players",
                    Value::Array(
                        h.players
                            .iter()
                            .map(|p| {
                                Value::Array(vec![
                                    Value::String(p.player_id.clone()),
                                    Value::String(p.name.clone()),
                                    Value::String(p.team.clone()),
                                    opt(p.account_id.map(num)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ]),
            Err(e) => err(&e),
        };
        assert_eq!(&header, v.get("header").unwrap(), "vector {i}: header");

        let index = match reader.read_index() {
            Ok(x) => obj(vec![
                ("frameCount", Value::Number(f64::from(x.frame_count))),
                (
                    "entries",
                    Value::Array(
                        x.entries
                            .iter()
                            .map(|e| {
                                Value::Array(vec![
                                    Value::Number(f64::from(e.tick)),
                                    Value::Number(f64::from(e.offset)),
                                ])
                            })
                            .collect(),
                    ),
                ),
                ("durationMs", Value::Number(f64::from(x.duration_ms))),
                ("indexOffset", Value::Number(f64::from(x.index_offset))),
            ]),
            Err(e) => err(&e),
        };
        assert_eq!(&index, v.get("index").unwrap(), "vector {i}: index");
        if index.get("error").is_some() {
            continue;
        }

        let want_frames = v.get("frames").and_then(Value::as_array).unwrap();
        for (k, want) in want_frames.iter().enumerate() {
            let got = match reader.read_frame(k, &NoEvents) {
                Ok(Some(f)) => obj(vec![
                    ("tick", Value::Number(f64::from(f.tick))),
                    ("state", canonical(&f.state)),
                    ("inputs", inputs(&f.inputs)),
                ]),
                Ok(None) => Value::Null,
                Err(e) => err(&e),
            };
            assert!(
                same_error(&got, want),
                "vector {i} frame {k}: got {got:?}\nwant {want:?}"
            );
            frames_read += 1;
        }

        let all = match reader.read_all_frames(&NoEvents) {
            Ok(frames) => Value::Array(frames.iter().map(|f| Value::Number(f64::from(f.tick))).collect()),
            Err(e) => err(&e),
        };
        assert_eq!(&all, v.get("all").unwrap(), "vector {i}: readAllFrames");

        let ticks = [-1.0, 0.0, 1000.0, 1003.0, 1005.0, 1011.0, 99999.0];
        let found: Vec<Value> = ticks
            .iter()
            .map(|&t| Value::Number(reader.find_frame_by_tick(t).unwrap() as f64))
            .collect();
        assert_eq!(
            &Value::Array(found),
            v.get("find").unwrap(),
            "vector {i}: findFrameByTick"
        );
    }
    assert!(frames_read > 500, "{frames_read} frames");
}
