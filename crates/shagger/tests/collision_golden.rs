//! The collision backend against SHAG's own `SweepCollisionBackend`,
//! `SweepBackendAdapter` and `SharedCollisionWorld` on synthetic maps
//! (`tools/golden/collision.mjs`). Every query runs in the recorded order on
//! one backend per map, so lift motion and brush model toggles carry over.

use shagger::babylon::maths::math_vector::Vector3;
use shagger::loader::bsp::bsp_loader::load_collision;
use shagger::physics::collision_backend::{TraceOptions, TraceResult};
use shagger::physics::sweep_backend_adapter::SweepBackendAdapter;
use shagger::physics::sweep_collision_backend::SweepCollisionBackend;
use shagger::shared::bsp::lift_system::LiftActor;
use shagger::shared::collision::collision_world::{Aabb, CollisionWorld};
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::shared::json::{self, Value};
use shagger::shared::server::simulation::types::Vec3;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn num(x: f64) -> Value {
    Value::String(if x.is_nan() {
        "nan".into()
    } else {
        format!("{:016x}", x.to_bits())
    })
}

fn int(x: impl Into<f64>) -> Value {
    Value::Number(x.into())
}

fn vec3(x: f64, y: f64, z: f64) -> Value {
    Value::Array(vec![num(x), num(y), num(z)])
}

fn bv(v: Vector3) -> Value {
    vec3(v.x, v.y, v.z)
}

fn sv(v: Vec3) -> Value {
    vec3(v.x, v.y, v.z)
}

fn opt<T>(v: Option<T>, f: impl FnOnce(T) -> Value) -> Value {
    v.map_or(Value::Null, f)
}

fn read_num(v: &Value) -> f64 {
    match v {
        Value::String(s) if s == "nan" => f64::NAN,
        Value::String(s) => f64::from_bits(u64::from_str_radix(s, 16).unwrap()),
        Value::Number(n) => *n,
        other => panic!("not a number: {other:?}"),
    }
}

fn read_vec(v: &Value) -> Vec3 {
    let a = v.as_array().unwrap();
    Vec3::new(read_num(&a[0]), read_num(&a[1]), read_num(&a[2]))
}

fn read_babylon(v: &Value) -> Vector3 {
    let p = read_vec(v);
    Vector3::new(p.x, p.y, p.z)
}

fn field<'v>(op: &'v Value, key: &str) -> &'v Value {
    op.get(key).unwrap_or(&Value::Null)
}

fn read_i32(v: &Value) -> Option<i32> {
    v.as_f64().map(|n| n as i32)
}

fn read_options(v: &Value) -> TraceOptions {
    if *v == Value::Null {
        return TraceOptions::default();
    }
    TraceOptions {
        radius: v.get("radius").map(read_num),
        height: v.get("height").map(read_num),
        content_mask: v.get("contentMask").and_then(read_i32),
    }
}

fn trace_result(r: &TraceResult) -> Value {
    Value::Array(vec![
        num(r.fraction),
        bv(r.end_pos),
        opt(r.normal, bv),
        Value::Bool(r.start_solid),
        Value::Bool(r.all_solid),
        int(r.contents),
        int(r.hit_brush_index as f64),
        Value::String(r.hit_texture_name.clone()),
        int(r.hit_surface_flags),
    ])
}

fn aabb(v: &Value) -> Aabb {
    let a = v.as_array().unwrap();
    Aabb {
        min: read_vec(&a[0]),
        max: read_vec(&a[1]),
    }
}

type World = SharedCollisionWorld<SweepBackendAdapter>;

#[allow(clippy::too_many_lines)]
fn run(world: &mut World, op: &Value) -> Value {
    let kind = op.get("kind").and_then(Value::as_str).unwrap();
    let backend = &mut world.backend.backend;
    match kind {
        "trace" => {
            let r = backend.trace_models(
                read_babylon(field(op, "start")),
                read_babylon(field(op, "end")),
                read_options(field(op, "options")),
                field(op, "exclude").as_f64().unwrap() as i64,
                field(op, "only").as_f64().unwrap() as i64,
            );
            trace_result(&r)
        }
        "contents" => int(backend.point_contents(read_babylon(field(op, "point")), read_i32(field(op, "mask")))),
        "ladder" => Value::Bool(backend.is_in_ladder(read_babylon(field(op, "min")), read_babylon(field(op, "max")))),
        "worldLadder" => Value::Bool(world.is_in_ladder(read_vec(field(op, "min")), read_vec(field(op, "max")))),
        "slide" | "step" => {
            let (start, velocity, options) = (
                read_babylon(field(op, "start")),
                read_babylon(field(op, "velocity")),
                read_options(field(op, "options")),
            );
            let r = if kind == "slide" {
                backend.slide_move(start, velocity, options)
            } else {
                backend.step_slide_move(start, velocity, options)
            };
            let stepped = if kind == "slide" {
                Value::Null
            } else {
                Value::Bool(r.stepped)
            };
            Value::Array(vec![bv(r.end_pos), bv(r.velocity), int(r.hit_count), stepped])
        }
        "ground" => {
            let g = backend.find_ground(
                read_babylon(field(op, "point")),
                field(op, "distance").as_f64().unwrap(),
                TraceOptions::default(),
            );
            opt(g, |(p, n)| Value::Array(vec![bv(p), bv(n)]))
        }
        "onGround" => Value::Bool(backend.is_on_ground(read_babylon(field(op, "point")))),
        "lifts" => {
            let dt = read_num(field(op, "dt"));
            let mut actors: Vec<LiftActor> = field(op, "actors")
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    let a = a.as_array().unwrap();
                    LiftActor {
                        position: read_vec(&a[0]),
                        velocity: read_vec(&a[1]),
                        id: a[2].as_f64().map(|n| n as i64),
                        passive: a[3].as_bool().unwrap_or(false),
                    }
                })
                .collect();
            let crushes = backend.step_lifts(dt, &mut actors);
            let lifts = backend.lifts.as_ref();
            let offsets = lifts.map(|l| l.offsets()).unwrap_or_default();
            let phases = lifts.map_or_else(Vec::new, |l| {
                l.definitions
                    .iter()
                    .map(|d| opt(l.phase(d.model_index), |p| Value::String(p.name().into())))
                    .collect()
            });
            Value::Object(vec![
                (
                    "crushes".into(),
                    Value::Array(
                        crushes
                            .iter()
                            .map(|c| {
                                Value::Array(vec![
                                    opt(c.id, |id| int(id as f64)),
                                    int(c.model_index as f64),
                                    num(c.damage),
                                ])
                            })
                            .collect(),
                    ),
                ),
                (
                    "actors".into(),
                    Value::Array(
                        actors
                            .iter()
                            .map(|a| Value::Array(vec![sv(a.position), sv(a.velocity)]))
                            .collect(),
                    ),
                ),
                ("offsets".into(), Value::Array(offsets.into_iter().map(num).collect())),
                ("phases".into(), Value::Array(phases)),
            ])
        }
        "toggle" => {
            let model = field(op, "model").as_f64().unwrap() as usize;
            let enabled = field(op, "enabled").as_bool().unwrap();
            if let Some(models) = backend.brush_models.as_mut() {
                models.set_enabled(model, enabled);
            }
            Value::Null
        }
        "worldTrace" => {
            let bounds = match field(op, "bounds") {
                Value::Null => None,
                b => Some(aabb(b)),
            };
            let r = world.trace(
                read_vec(field(op, "start")),
                read_vec(field(op, "end")),
                bounds,
                read_i32(field(op, "mask")),
            );
            Value::Array(vec![
                Value::Bool(r.hit),
                num(r.fraction),
                opt(r.hit_point, sv),
                opt(r.normal, sv),
                int(r.contents),
            ])
        }
        "worldStep" => {
            let r = world.step_slide_move(
                read_vec(field(op, "start")),
                read_vec(field(op, "velocity")),
                read_num(field(op, "dt")),
                aabb(field(op, "bounds")),
            );
            Value::Array(vec![sv(r.end_pos), sv(r.velocity), Value::Bool(r.grounded)])
        }
        "worldContents" => int(world.point_contents(read_vec(field(op, "point")))),
        other => panic!("unknown op {other}"),
    }
}

#[test]
fn collision_matches_shag() {
    let doc = json::parse(include_str!("golden/collision.json")).unwrap();
    let vectors = doc.get("vectors").and_then(Value::as_array).unwrap();
    assert!(vectors.len() >= 100);
    let mut ops = 0;
    for (i, v) in vectors.iter().enumerate() {
        let bsp = load_collision(&hex(v.get("bytes").and_then(Value::as_str).unwrap())).unwrap();
        let mut world = SharedCollisionWorld::new(SweepBackendAdapter {
            backend: SweepCollisionBackend::new(bsp),
        });
        for (q, op) in v.get("ops").and_then(Value::as_array).unwrap().iter().enumerate() {
            let got = run(&mut world, op);
            assert_eq!(&got, field(op, "out"), "map {i} op {q}: {op:?}");
            ops += 1;
        }
        let s = world.backend.backend.stats();
        let expected = v.get("stats").unwrap();
        let stats = [
            ("traceCount", s.trace_count as f64),
            ("hitCount", s.hit_count as f64),
            ("brushCount", s.brush_count as f64),
            ("boundedBrushCount", s.bounded_brush_count as f64),
            ("clipBrushCount", s.clip_brush_count as f64),
            ("collisionTriangleCount", s.collision_triangle_count as f64),
            ("boundedTriangleCount", s.bounded_triangle_count as f64),
            (
                "partiallyBoundedTriangleCount",
                s.partially_bounded_triangle_count as f64,
            ),
            ("redundantBrushCount", s.redundant_brush_count as f64),
            ("prunedBrushTests", s.pruned_brush_tests as f64),
        ];
        for (key, value) in stats {
            assert_eq!(
                expected.get(key).and_then(Value::as_f64),
                Some(value),
                "map {i}: stats {key}"
            );
        }
    }
    assert!(ops > 5000, "{ops} ops");
}
