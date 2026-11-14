//! The BSP loader against SHAG's own `BspLoader` on synthetic IBSP and FBSP
//! files (`tools/golden/bsp.mjs`).

use shagger::loader::bsp::bsp_loader::{
    self as bsp, BspCommon, BspFace, BspLightVol, BspVertices, bsp_used_textures, find_leaf_for_point,
    is_cluster_visible,
};
use shagger::shared::json::{self, Value};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn num(x: impl Into<f64>) -> Value {
    let x = x.into();
    Value::String(if x.is_nan() {
        "nan".into()
    } else {
        format!("{:016x}", x.to_bits())
    })
}

fn nums<T: Copy + Into<f64>>(xs: &[T]) -> Value {
    Value::Array(xs.iter().map(|&x| num(x)).collect())
}

fn text(s: &str) -> Value {
    Value::String(s.into())
}

fn obj(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn digest(bytes: &[u8]) -> Value {
    let h = bytes
        .iter()
        .fold(0u32, |h, &b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    Value::String(format!("{}:{h:x}", bytes.len()))
}

/// Equal as JSON, with object keys in any order.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.iter().any(|(k2, v2)| k == k2 && same(v, v2)))
        }
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        _ => a == b,
    }
}

fn face(f: &BspFace) -> Value {
    let mut fields = vec![
        ("textureIndex", num(f.texture_index)),
        ("effectIndex", num(f.effect_index)),
        ("faceType", num(f.face_type)),
        ("firstVertex", num(f.first_vertex)),
        ("numVertices", num(f.num_vertices)),
        ("firstMeshVert", num(f.first_mesh_vert)),
        ("numMeshVerts", num(f.num_mesh_verts)),
        ("lightmapIndex", num(f.lightmap_index)),
        ("lightmapStart", nums(&f.lightmap_start)),
        ("lightmapSize", nums(&f.lightmap_size)),
        ("lightmapOrigin", nums(&f.lightmap_origin)),
        (
            "lightmapVecs",
            Value::Array(f.lightmap_vecs.iter().map(|v| nums(v)).collect()),
        ),
        ("normal", nums(&f.normal)),
        ("patchSize", nums(&f.patch_size)),
    ];
    if let Some(i) = f.lightmap_indices {
        fields.push(("lightmapIndices", nums(&i)));
    }
    obj(fields)
}

fn common(c: &BspCommon) -> Vec<(&'static str, Value)> {
    vec![
        (
            "header",
            obj(vec![
                ("magic", num(c.header.magic)),
                ("version", num(c.header.version)),
                ("isFBSP", Value::Bool(c.header.is_fbsp)),
            ]),
        ),
        (
            "lumps",
            Value::Array(
                c.lumps
                    .iter()
                    .map(|l| obj(vec![("offset", num(l.offset)), ("length", num(l.length))]))
                    .collect(),
            ),
        ),
        (
            "entities",
            Value::Array(
                c.entities
                    .iter()
                    .map(|e| {
                        let props = Value::Object(e.properties.iter().map(|(k, v)| (k.to_owned(), text(v))).collect());
                        obj(vec![("classname", text(&e.classname)), ("properties", props)])
                    })
                    .collect(),
            ),
        ),
        (
            "textures",
            Value::Array(
                c.textures
                    .iter()
                    .map(|t| {
                        obj(vec![
                            ("name", text(&t.name)),
                            ("surfaceFlags", num(t.surface_flags)),
                            ("contentFlags", num(t.content_flags)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "planes",
            Value::Array(
                c.planes
                    .iter()
                    .map(|p| obj(vec![("normal", nums(&p.normal)), ("dist", num(p.dist))]))
                    .collect(),
            ),
        ),
        (
            "nodes",
            Value::Array(
                c.nodes
                    .iter()
                    .map(|n| {
                        obj(vec![
                            ("planeIndex", num(n.plane_index)),
                            ("children", nums(&n.children)),
                            ("mins", nums(&n.mins)),
                            ("maxs", nums(&n.maxs)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "leafs",
            Value::Array(
                c.leafs
                    .iter()
                    .map(|l| {
                        obj(vec![
                            ("cluster", num(l.cluster)),
                            ("area", num(l.area)),
                            ("mins", nums(&l.mins)),
                            ("maxs", nums(&l.maxs)),
                            ("firstLeafFace", num(l.first_leaf_face)),
                            ("numLeafFaces", num(l.num_leaf_faces)),
                            ("firstLeafBrush", num(l.first_leaf_brush)),
                            ("numLeafBrushes", num(l.num_leaf_brushes)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("leafFaces", nums(&c.leaf_faces)),
        ("leafBrushes", nums(&c.leaf_brushes)),
        (
            "models",
            Value::Array(
                c.models
                    .iter()
                    .map(|m| {
                        obj(vec![
                            ("mins", nums(&m.mins)),
                            ("maxs", nums(&m.maxs)),
                            ("firstFace", num(m.first_face)),
                            ("numFaces", num(m.num_faces)),
                            ("firstBrush", num(m.first_brush)),
                            ("numBrushes", num(m.num_brushes)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "brushes",
            Value::Array(
                c.brushes
                    .iter()
                    .map(|b| {
                        obj(vec![
                            ("firstSide", num(b.first_side)),
                            ("numSides", num(b.num_sides)),
                            ("shaderIndex", num(b.shader_index)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "brushSides",
            Value::Array(
                c.brush_sides
                    .iter()
                    .map(|s| {
                        let mut f = vec![("planeIndex", num(s.plane_index)), ("shaderIndex", num(s.shader_index))];
                        if let Some(n) = s.surface_num {
                            f.push(("surfaceNum", num(n)));
                        }
                        obj(f)
                    })
                    .collect(),
            ),
        ),
        ("faces", Value::Array(c.faces.iter().map(face).collect())),
        (
            "visData",
            c.vis_data.as_ref().map_or(Value::Null, |v| {
                obj(vec![
                    ("numClusters", num(v.num_clusters)),
                    ("bytesPerCluster", num(v.bytes_per_cluster)),
                    ("bitVectors", digest(&v.bit_vectors)),
                ])
            }),
        ),
    ]
}

fn light_vol(v: &BspLightVol) -> Value {
    let mut f = vec![
        ("ambient", nums(&v.ambient)),
        ("directional", nums(&v.directional)),
        ("direction", nums(&v.direction)),
    ];
    if !v.valid {
        f.push(("valid", Value::Bool(false)));
    }
    obj(f)
}

fn full(file: &bsp::BspFile) -> Value {
    let mut f = common(file);
    let vertices = match &file.vertices {
        BspVertices::Ibsp(v) => v
            .iter()
            .map(|v| {
                obj(vec![
                    ("position", nums(&v.position)),
                    ("texCoord", nums(&v.tex_coord)),
                    ("lightmapCoord", nums(&v.lightmap_coord)),
                    ("normal", nums(&v.normal)),
                    ("color", nums(&v.color)),
                ])
            })
            .collect(),
        BspVertices::Fbsp(v) => v
            .iter()
            .map(|v| {
                obj(vec![
                    ("position", nums(&v.position)),
                    ("texCoord", nums(&v.tex_coord)),
                    (
                        "lightmapCoords",
                        Value::Array(v.lightmap_coords.iter().map(|c| nums(c)).collect()),
                    ),
                    ("normal", nums(&v.normal)),
                    ("colors", Value::Array(v.colors.iter().map(|c| nums(c)).collect())),
                ])
            })
            .collect(),
    };
    f.push(("vertices", Value::Array(vertices)));
    f.push(("meshVerts", nums(&file.mesh_verts)));
    f.push((
        "effects",
        Value::Array(
            file.effects
                .iter()
                .map(|e| {
                    obj(vec![
                        ("name", text(&e.name)),
                        ("brushIndex", num(e.brush_index)),
                        ("visibleSide", num(e.visible_side)),
                    ])
                })
                .collect(),
        ),
    ));
    f.push((
        "lightmaps",
        Value::Array(file.lightmaps.iter().map(|l| digest(l)).collect()),
    ));
    f.push((
        "lightVols",
        Value::Array(file.light_vols.iter().map(light_vol).collect()),
    ));
    f.push(("lightmapSize", num(file.lightmap_size)));
    if let Some(g) = file.light_grid {
        f.push(("lightGridOrigin", nums(&g.origin)));
        f.push(("lightGridSize", nums(&g.size)));
        f.push(("lightGridStep", nums(&g.step)));
    }
    obj(f)
}

fn collision(c: &bsp::BspCollisionData) -> Value {
    let mut f = common(c);
    f.push((
        "vertices",
        Value::Array(
            c.vertices
                .iter()
                .map(|v| v.map_or(Value::Null, |p| obj(vec![("position", nums(&p))])))
                .collect(),
        ),
    ));
    obj(f)
}

fn check(i: usize, what: &str, got: Result<Value, bsp::BspError>, expected: &Value) {
    match (got, expected.get("ok")) {
        (Ok(value), Some(want)) => assert!(same(&value, want), "vector {i} {what}: differs from SHAG"),
        (Err(_), None) => {}
        (Ok(_), None) => panic!("vector {i} {what}: SHAG threw {:?}, Rust loaded", expected.get("error")),
        (Err(e), Some(_)) => panic!("vector {i} {what}: SHAG loaded, Rust failed with {e}"),
    }
}

#[test]
fn loads_like_shag() {
    let doc = json::parse(include_str!("golden/bsp_loader.json")).unwrap();
    let vectors = doc.get("vectors").and_then(Value::as_array).unwrap();
    let mut loaded = 0;
    for (i, v) in vectors.iter().enumerate() {
        let data = hex(v.get("bytes").and_then(Value::as_str).unwrap());
        let file = bsp::load(&data);
        check(
            i,
            "load",
            file.as_ref().map(full).map_err(Clone::clone),
            v.get("load").unwrap(),
        );
        check(
            i,
            "loadCollision",
            bsp::load_collision(&data).map(|c| collision(&c)),
            v.get("collision").unwrap(),
        );
        let Ok(file) = file else { continue };
        loaded += 1;
        for q in v.get("queries").and_then(Value::as_array).unwrap() {
            let p: Vec<f64> = q
                .get("point")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(|x| f64::from_bits(u64::from_str_radix(x.as_str().unwrap(), 16).unwrap_or(0x7ff8_0000_0000_0000)))
                .collect();
            let leaf = find_leaf_for_point([p[0], p[1], p[2]], &file.nodes, &file.leafs, &file.planes)
                .map(|l| std::ptr::from_ref(l) as usize - file.leafs.as_ptr() as usize)
                .map(|bytes| bytes / std::mem::size_of_val(&file.leafs[0]));
            match q.get("leaf").unwrap() {
                Value::String(s) if s == "throws" => {}
                Value::Null => assert_eq!(leaf, None, "vector {i}: leaf"),
                n => assert_eq!(leaf.map(|l| l as f64), n.as_f64(), "vector {i}: leaf"),
            }
            #[allow(clippy::cast_possible_truncation)]
            let (a, b) = (
                q.get("a").unwrap().as_f64().unwrap() as i32,
                q.get("b").unwrap().as_f64().unwrap() as i32,
            );
            assert_eq!(
                is_cluster_visible(file.vis_data.as_ref(), a, b),
                q.get("visible").unwrap().as_bool().unwrap(),
                "vector {i}: visible({a}, {b})"
            );
        }
        let used: Vec<Value> = bsp_used_textures(&file).into_iter().map(text).collect();
        assert_eq!(
            &Value::Array(used),
            v.get("usedTextures").unwrap(),
            "vector {i}: used textures"
        );
    }
    assert!(loaded > 100, "only {loaded} files loaded");
}

/// A damaged count names only records that exist once clamped: a model
/// naming two billion brushes stalled the collision build for a minute
/// (fuzzing).
#[test]
fn a_range_past_its_records_is_clamped_to_them() {
    assert_eq!(bsp::record_range(2, 3, 10), 2..5);
    assert_eq!(bsp::record_range(8, 0x7fff_ffff, 10), 8..10);
    assert_eq!(bsp::record_range(-3, 5, 10), 0..2);
    assert_eq!(bsp::record_range(12, 4, 10), 10..10);
    assert_eq!(bsp::record_range(4, -2, 10), 4..4);
    assert_eq!(bsp::record_range(i32::MAX, i32::MAX, 10), 10..10);
    let doc = json::parse(include_str!("golden/bsp_loader.json")).unwrap();
    let vectors = doc.get("vectors").and_then(Value::as_array).unwrap();
    for v in vectors {
        let data = hex(v.get("bytes").and_then(Value::as_str).unwrap());
        let Ok(mut c) = bsp::load_collision(&data) else {
            continue;
        };
        if let Some(m) = c.common.models.first_mut() {
            m.num_brushes = i32::MAX;
        }
        bsp::clamp_ranges(&mut c.common);
        for m in &c.common.models {
            assert!((m.first_brush + m.num_brushes) as usize <= c.common.brushes.len());
            assert!((m.first_face + m.num_faces) as usize <= c.common.faces.len());
        }
        for b in &c.common.brushes {
            assert!((b.first_side + b.num_sides) as usize <= c.common.brush_sides.len());
        }
    }
}

/// A node whose child points back up the tree is a loop: the walk stops
/// instead of spinning forever (fuzzing found one in a damaged map).
#[test]
fn a_loop_in_the_tree_ends_the_walk() {
    let node = |children| bsp::BspNode {
        plane_index: 0,
        children,
        mins: [0; 3],
        maxs: [0; 3],
    };
    let planes = [bsp::BspPlane {
        normal: [1.0, 0.0, 0.0],
        dist: 0.0,
    }];
    let leafs = [bsp::BspLeaf {
        cluster: 7,
        area: 0,
        mins: [0; 3],
        maxs: [0; 3],
        first_leaf_face: 0,
        num_leaf_faces: 0,
        first_leaf_brush: 0,
        num_leaf_brushes: 0,
    }];
    let looped = [node([1, -1]), node([0, -1])];
    assert!(find_leaf_for_point([1.0, 0.0, 0.0], &looped, &leafs, &planes).is_none());
    let sound = [node([1, -1]), node([-1, -1])];
    assert_eq!(
        find_leaf_for_point([1.0, 0.0, 0.0], &sound, &leafs, &planes).map(|l| l.cluster),
        Some(7)
    );
}
