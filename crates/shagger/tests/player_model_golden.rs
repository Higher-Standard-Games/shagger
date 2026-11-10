//! The player model's retargeting and fit against Babylon itself
//! (`tools/golden/player_model.mjs`): Vanguard loaded as a packaged body
//! whose clips come from another root, so every clip goes through
//! `AnimatorAvatar.retargetAnimationGroup`, then measured as SHAG measures
//! it (`PlayerModelFit.ts`).
//!
//! The model is not in this repository: the test reads
//! `washoi_player_model.pk3` from the directory `SHAGGER_PLAYER_DATA` names
//! and passes without it.

use shagger::animation::player_model_assets::AnimFileConfig;
use shagger::animation::player_model_manager::PlayerModel;
use shagger::loader::model::gltf_animation::ChannelPath;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::shared::json::{self, Value};

const CLIPS: [AnimFileConfig; 4] = [
    AnimFileConfig {
        name: "idle",
        file: "rifle aiming idle.glb",
        optional: false,
    },
    AnimFileConfig {
        name: "run",
        file: "rifle run.glb",
        optional: false,
    },
    AnimFileConfig {
        name: "blade_slash",
        file: "blade_slash.glb",
        optional: false,
    },
    // Vanguard's packages have no blade stance; the idle stands in for it.
    AnimFileConfig {
        name: "blade_idle",
        file: "rifle aiming idle.glb",
        optional: false,
    },
];

fn f(v: &Value) -> f64 {
    v.as_f64().expect("a number")
}

fn vec3(v: &Value) -> [f64; 3] {
    let a = v.as_array().expect("an array");
    [f(&a[0]), f(&a[1]), f(&a[2])]
}

fn close(what: &str, got: f64, want: f64, tolerance: f64) {
    assert!(
        (got - want).abs() <= tolerance,
        "{what}: {got} against Babylon's {want}"
    );
}

/// The player package, when `SHAGGER_PLAYER_DATA` names its directory.
fn package() -> Option<Pk3Loader> {
    let Ok(dir) = std::env::var("SHAGGER_PLAYER_DATA") else {
        eprintln!("SHAGGER_PLAYER_DATA is not set; skipped");
        return None;
    };
    let path = std::path::Path::new(&dir).join("washoi_player_model.pk3");
    let bytes = std::fs::read(&path).expect("washoi_player_model.pk3");
    let mut pk3 = Pk3Loader::new();
    pk3.add_package("washoi_player_model.pk3", bytes).expect("a package");
    Some(pk3)
}

const ROOT: &str = "models/players/vanguard/";

/// Vanguard with its clips read from another root, so they retarget.
fn load(pk3: &mut Pk3Loader, body: Option<Vec<u8>>) -> PlayerModel {
    PlayerModel::load(
        |p| {
            if p == "body/vanguard.glb" {
                return body.clone();
            }
            pk3.get_file(&p.replace("retarget/", ROOT))
        },
        if body.is_some() { "body/" } else { ROOT },
        "vanguard.glb",
        &CLIPS,
        "retarget/",
    )
    .expect("Vanguard loads")
}

/// Vanguard with some joints' rest turned and the right hand scaled, so
/// the clips' rig and the body's disagree (`player_model.mjs`'s skew).
fn skewed(glb: &[u8]) -> Vec<u8> {
    use glam::{DQuat, DVec3};
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let mut doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    let turn = |axis: [f64; 3], angle: f64| DQuat::from_axis_angle(DVec3::from(axis).normalize(), angle);
    let edits = [
        ("mixamorig:Spine", turn([1.0, 0.0, 0.0], 0.4)),
        ("mixamorig:RightHand", turn([0.0, 1.0, 1.0], 0.9)),
        ("mixamorig:LeftArm", turn([0.0, 0.0, 1.0], -0.6)),
        ("mixamorig:RightUpLeg", turn([1.0, 1.0, 0.0], 0.3)),
    ];
    for node in doc["nodes"].as_array_mut().unwrap() {
        let Some(name) = node["name"].as_str().map(str::to_owned) else {
            continue;
        };
        let Some((_, r)) = edits.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        let q = node
            .get("rotation")
            .and_then(|v| v.as_array())
            .map_or(DQuat::IDENTITY, |a| {
                let c = |i: usize| a[i].as_f64().unwrap();
                DQuat::from_xyzw(c(0), c(1), c(2), c(3))
            });
        let out = q * *r;
        node["rotation"] = serde_json::json!([out.x, out.y, out.z, out.w]);
        if name == "mixamorig:RightHand" {
            node["scale"] = serde_json::json!([1.3, 1.3, 1.3]);
        }
    }
    let mut text = serde_json::to_vec(&doc).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let rest = &glb[20 + json_len..];
    let mut out = Vec::new();
    out.extend_from_slice(&glb[0..8]);
    out.extend_from_slice(&((12 + 8 + text.len() + rest.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(&glb[16..20]);
    out.extend_from_slice(&text);
    out.extend_from_slice(rest);
    out
}

#[test]
fn retargeting_and_fit_match_babylon() {
    let Some(mut pk3) = package() else { return };
    let model = load(&mut pk3, None);
    check(
        &model,
        &json::parse(include_str!("golden/player_model_retarget.json")).expect("the golden"),
    );
}

#[test]
fn retargeting_onto_another_rest_matches_babylon() {
    let Some(mut pk3) = package() else { return };
    let body = skewed(&pk3.get_file(&format!("{ROOT}vanguard.glb")).expect("vanguard.glb"));
    let model = load(&mut pk3, Some(body));
    check(
        &model,
        &json::parse(include_str!("golden/player_model_retarget_skew.json")).expect("the golden"),
    );
}

fn check(model: &PlayerModel, gold: &Value) {
    // The weapon hand's basis.
    let basis = model.hand_basis.expect("a hand basis");
    let want = gold
        .get("handBasis")
        .unwrap()
        .get("rotation")
        .unwrap()
        .as_array()
        .unwrap();
    let got = basis.rotation.to_array();
    for i in 0..4 {
        close("hand rotation", f64::from(got[i]), f(&want[i]), 1e-5);
    }
    let want = vec3(gold.get("handBasis").unwrap().get("scale").unwrap());
    for (i, want) in want.iter().enumerate() {
        close("hand scale", f64::from(basis.scale[i]), *want, 1e-5);
    }

    // Every watched channel's keys, as Babylon retargets them.
    for anim in &model.anims {
        let Some(clip) = gold.get("clips").unwrap().get(&anim.name) else {
            continue;
        };
        close(&format!("{} to", anim.name), anim.to, f(clip.get("to").unwrap()), 1e-3);
        let Value::Object(channels) = clip.get("channels").unwrap() else {
            panic!("channels")
        };
        for (bone, keys) in channels {
            let node = model.model.skeleton.find(bone).expect("the bone");
            let (_, channel) = anim
                .channels
                .iter()
                .find(|(n, c)| *n == node && c.path == ChannelPath::Rotation)
                .unwrap_or_else(|| panic!("{} has no {bone} rotation", anim.name));
            for (k, key) in keys.as_array().unwrap().iter().enumerate() {
                let key = key.as_array().unwrap();
                close("key frame", f64::from(channel.times[k]) * 60.0, f(&key[0]), 1e-3);
                let mut q = [0.0_f32; 4];
                q.copy_from_slice(&channel.values[k * 4..k * 4 + 4]);
                // A quaternion and its negation are the same turn.
                let sign = if (0..4).map(|i| f64::from(q[i]) * f(&key[i + 1])).sum::<f64>() < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                for i in 0..4 {
                    close(
                        &format!("{} {bone} key {k}", anim.name),
                        sign * f64::from(q[i]),
                        f(&key[i + 1]),
                        2e-5,
                    );
                }
            }
        }
    }

    // The standing fit.
    let fit = model.fit.expect("a fit");
    close(
        "fit scale",
        f64::from(fit.scale),
        f(gold.get("fit").unwrap().get("scale").unwrap()),
        1e-4,
    );
    close(
        "fit offset",
        f64::from(fit.offset_y),
        f(gold.get("fit").unwrap().get("offsetY").unwrap()),
        1e-4,
    );

    // What the hands hold.
    let gun = model.gun_support.expect("a gun support");
    for (got, want, what) in [
        (
            gun.palm,
            vec3(gold.get("gunSupport").unwrap().get("palm").unwrap()),
            "palm",
        ),
        (
            gun.forward,
            vec3(gold.get("gunSupport").unwrap().get("forward").unwrap()),
            "forward",
        ),
    ] {
        for i in 0..3 {
            close(what, f64::from(got[i]), want[i], 1e-4);
        }
    }
    let blade = model.blade_stance.expect("a blade stance");
    for (got, want, what) in [
        (
            blade.axis,
            vec3(gold.get("bladeStance").unwrap().get("axis").unwrap()),
            "axis",
        ),
        (
            blade.side,
            vec3(gold.get("bladeStance").unwrap().get("side").unwrap()),
            "side",
        ),
        (
            blade.fist,
            vec3(gold.get("bladeStance").unwrap().get("fist").unwrap()),
            "fist",
        ),
    ] {
        for i in 0..3 {
            close(what, f64::from(got[i]), want[i], 1e-4);
        }
    }
}

/// The shared clips' files, from the Vanguard clips that stand in for them
/// (`player_model_layers.mts`'s `STAND_INS`).
fn stand_in(file: &str) -> &str {
    match file {
        "backwards_run.glb" => "run backwards.glb",
        "blade_idle.glb" => "rifle aiming idle.glb",
        "death_flying_back.glb" | "death_from_back.glb" | "death_headshot.glb" => "hit reaction.glb",
        other => other,
    }
}

/// Staging's layered playback against `PlayerModelManager` itself
/// (`tools/golden/player_model_layers.mts`): Vanguard as the packaged
/// Police body with its clips as the shared ones, driven by the same
/// scenario at Babylon's constant 16 ms step.
#[test]
fn layered_playback_matches_babylon() {
    use shagger::animation::player_model_assets::{
        PLAYER_ANIMATION_ROOT, PLAYER_ANIMATIONS, PLAYER_MODEL_FILE, PLAYER_MODEL_ROOT,
    };
    let Some(mut pk3) = package() else { return };
    let model = PlayerModel::load(
        |p| {
            if p == format!("{PLAYER_MODEL_ROOT}{PLAYER_MODEL_FILE}") {
                return pk3.get_file(&format!("{ROOT}vanguard.glb"));
            }
            let file = p.strip_prefix(PLAYER_ANIMATION_ROOT)?;
            pk3.get_file(&format!("{ROOT}{}", stand_in(file)))
        },
        PLAYER_MODEL_ROOT,
        PLAYER_MODEL_FILE,
        PLAYER_ANIMATIONS,
        PLAYER_ANIMATION_ROOT,
    )
    .expect("the stand-in body loads");
    let gold = json::parse(include_str!("golden/player_model_layers.json")).expect("the golden");
    let watch: Vec<usize> = gold
        .get("watch")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|n| model.model.skeleton.find(n.as_str().unwrap()).expect("a watched bone"))
        .collect();
    let mut inst = model.create_instance();
    let run = |m: &PlayerModel, i: &mut shagger::animation::player_model_manager::PlayerModelInstance| {
        i.speed_ratio = 1.5 * 0.75;
        m.set_upper_body_share(i, 0.5 / 0.75);
    };
    let mut worst = (0.0_f64, 0, 0);
    for (frame, want) in gold.get("frames").unwrap().as_array().unwrap().iter().enumerate() {
        match frame {
            0 => {
                model.play_animation(&mut inst, "run");
                run(&model, &mut inst);
            }
            20 => model.set_upper_body_stance(&mut inst, Some("blade_idle"), 1.0),
            40 => {
                model.play_once(&mut inst, "blade_slash", 2.0);
            }
            45 => model.set_aim_pitch(&mut inst, 0.6),
            70 => {
                model.play_animation(&mut inst, "walk");
                inst.speed_ratio = -1.25;
                model.set_upper_body_share(&mut inst, 1.0);
            }
            90 => model.set_upper_body_stance(&mut inst, None, 1.0),
            100 => {
                model.play_once(&mut inst, "reload", 1.5);
            }
            110 => {
                model.play_animation(&mut inst, "run");
            }
            120 => model.set_aim_pitch(&mut inst, -0.3),
            _ => {}
        }
        if frame >= 110 {
            run(&model, &mut inst);
        }
        model.advance(&mut inst, 0.016);
        for (j, (&node, q)) in watch.iter().zip(want.as_array().unwrap()).enumerate() {
            let q = q.as_array().unwrap();
            let got = inst.locals[node].rotation.to_array();
            let dot: f64 = (0..4).map(|i| f64::from(got[i]) * f(&q[i])).sum();
            let error = 2.0 * dot.abs().min(1.0).acos();
            if std::env::var("SHAGGER_LAYERS_TRACE").is_ok() && error > 1e-3 {
                eprintln!("frame {frame} joint {j}: {error:.5} rad");
            }
            if error > worst.0 {
                worst = (error, frame, j);
            }
        }
    }
    assert!(
        worst.0 < 2e-3,
        "frame {} joint {} is {} rad from Babylon's pose",
        worst.1,
        worst.2,
        worst.0
    );
}
