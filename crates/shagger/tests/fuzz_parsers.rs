//! Mutation fuzzing, on stable Rust, of every parser that reads bytes from a
//! downloaded package or from the network: real files are mutated (bit
//! flips, boundary values, truncation, splices) and each parser must return
//! an error rather than panic. A panic is a crash in the game.
//!
//! Ignored by default. `FUZZ_DATA` names a directory of `.pk3` packages
//! (the synced game data) whose files are the seeds:
//!
//! ```text
//! FUZZ_DATA=<dir> FUZZ_ITERS=2000 cargo test --release -p shagger \
//!     --test fuzz_parsers -- --ignored --nocapture
//! ```
//!
//! Each failing input is written to `FUZZ_OUT` (default `target/fuzz`) and
//! reported by target and seed file. Not a port: the browser had no fuzzer.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use shagger::audio::decode::decode_sound;
use shagger::audio::music::Playlist;
use shagger::babylon::maths::math_vector::Vector3;
use shagger::console::command_registry::parse_input;
use shagger::core::cvar::{CvarOptions, Cvars};
use shagger::lighting::map_emissive_surfaces::{SurfaceEmission, extract_emissive_surfaces};
use shagger::loader::bsp::bsp_loader::{load, load_collision};
use shagger::loader::bsp::mesh::pvs::build_face_clusters;
use shagger::loader::bsp::pvs_culler::PvsCuller;
use shagger::loader::model::glb::Glb;
use shagger::loader::model::gltf_animation::AnimationClip;
use shagger::loader::model::gltf_model::GltfModel;
use shagger::loader::model::md3_loader::parse_md3;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::pk3::published_index::{parse_published_index, pk3_index_hash};
use shagger::loader::shader::q3_shader_parser::Q3ShaderParser;
use shagger::loader::texture::decode_image;
use shagger::loader::texture::ktx1_loader::{ktx1_rgba, parse_ktx1, parse_ktx1_cube_faces};
use shagger::loader::texture::skybox_loader::parse_image_to_rgba;
use shagger::lobby::types::LobbyIn;
use shagger::physics::collision_backend::TraceOptions;
use shagger::physics::sweep_collision_backend::SweepCollisionBackend;
use shagger::rendering::world_geometry::build_world_geometry;
use shagger::shared::demo::demo_inputs::decode_inputs;
use shagger::shared::json;
use shagger::shared::net::message_codec as codec;
use shagger::shared::net::snapshot::NoEvents;

/// xorshift64*: reproducible from the seed printed with a failure.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

const INTERESTING: [u32; 12] = [
    0,
    1,
    0x7f,
    0x80,
    0xff,
    0x7fff,
    0xffff,
    0x7fff_ffff,
    0x8000_0000,
    0xffff_ffff,
    0x0010_0000,
    0xffff_fffe,
];

/// One to eight mutations; half of them in the first kilobyte, where
/// headers and directories (lump tables, chunk lengths) live.
fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut data = seed.to_vec();
    for _ in 0..=rng.below(8) {
        if data.is_empty() {
            data.push(rng.next() as u8);
            continue;
        }
        let span = if rng.below(2) == 0 {
            data.len().min(1024)
        } else {
            data.len()
        };
        let at = rng.below(span);
        match rng.below(7) {
            0 => data[at] ^= 1 << rng.below(8),
            1 => data[at] = rng.next() as u8,
            2 => {
                let value = INTERESTING[rng.below(INTERESTING.len())];
                let at = at & !3;
                if at + 4 <= data.len() {
                    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
                }
            }
            3 => {
                let value = (INTERESTING[rng.below(INTERESTING.len())] as f32).to_le_bytes();
                let floats = [f32::NAN, f32::INFINITY, -1e30, 1e30, -0.0];
                let value = if rng.below(2) == 0 {
                    floats[rng.below(floats.len())].to_le_bytes()
                } else {
                    value
                };
                let at = at & !3;
                if at + 4 <= data.len() {
                    data[at..at + 4].copy_from_slice(&value);
                }
            }
            4 => data.truncate(at),
            5 => {
                let len = rng.below(64).min(data.len() - at);
                let chunk = data[at..at + len].to_vec();
                let to = rng.below(data.len());
                data.splice(to..to, chunk);
            }
            _ => {
                let len = rng.below(64).min(data.len() - at);
                data.drain(at..at + len);
            }
        }
    }
    data
}

type Target = fn(&[u8]);

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn fuzz_bsp(bytes: &[u8]) {
    if let Ok(bsp) = load(bytes) {
        let _ = build_world_geometry(&bsp, |_, _| Vec::new());
        let _ = build_face_clusters(&bsp);
        let _ = extract_emissive_surfaces(&bsp, |_| SurfaceEmission {
            emissive: true,
            surface_light: 100.0,
        });
        let culler = PvsCuller::new(&bsp);
        let _ = culler.find_cluster_at_position(glam::Vec3::ZERO);
    }
    if let Ok(collision) = load_collision(bytes) {
        let mut backend = SweepCollisionBackend::new(collision);
        let options = TraceOptions {
            radius: Some(16.0),
            height: Some(56.0),
            content_mask: None,
        };
        for (a, b) in [
            ((0.0, 0.0, 0.0), (512.0, 64.0, -128.0)),
            ((-1e6, 0.0, 0.0), (1e6, 0.0, 0.0)),
            ((100.0, 100.0, 100.0), (100.0, 100.0, 100.0)),
        ] {
            let _ = backend.trace(Vector3::new(a.0, a.1, a.2), Vector3::new(b.0, b.1, b.2), options);
        }
    }
}

fn fuzz_glb(bytes: &[u8]) {
    let _ = Glb::parse(bytes);
    let _ = GltfModel::from_glb(bytes);
    let _ = GltfModel::from_glb_rigged(bytes);
    let _ = AnimationClip::from_glb(bytes);
}

fn fuzz_md3(bytes: &[u8]) {
    let _ = parse_md3(bytes);
}

fn fuzz_image(bytes: &[u8]) {
    for name in ["a.jpg", "a.tga", "a.png", "a.ktx"] {
        let _ = decode_image(name, bytes);
    }
    for ext in ["jpg", "tga", "png"] {
        let _ = parse_image_to_rgba(bytes, ext);
    }
}

fn fuzz_ktx(bytes: &[u8]) {
    if let Ok(ktx) = parse_ktx1(bytes) {
        let _ = ktx1_rgba(&ktx);
    }
    let _ = parse_ktx1_cube_faces(bytes);
    let _ = decode_image("a.ktx", bytes);
}

fn fuzz_shader(bytes: &[u8]) {
    let _ = Q3ShaderParser::new().parse_shader_file(&text(bytes));
}

fn fuzz_sound(bytes: &[u8]) {
    let _ = decode_sound(bytes, 48_000);
    let _ = decode_sound(bytes, 44_100);
}

fn fuzz_json(bytes: &[u8]) {
    let source = text(bytes);
    let _ = json::parse(&source);
    let _ = Playlist::parse(&source);
    let _ = LobbyIn::parse(&source);
}

/// The package index, with a manifest that names its packages and matches
/// its hash, so the parse gets past the hash check to the records.
fn fuzz_index(bytes: &[u8]) {
    let text = text(bytes);
    let names: Vec<serde_json::Value> = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("packages")?
                .as_array()
                .map(|p| p.iter().filter_map(|e| e.get("pk3").cloned()).collect::<Vec<_>>())
        })
        .unwrap_or_default();
    let manifest = serde_json::json!({
        "files": names,
        "indexHash": pk3_index_hash(&text),
        "indexFile": format!("pk3_index.{}.json", pk3_index_hash(&text)),
    });
    let _ = parse_published_index(&text, &manifest, "/data/");
    let _ = json::parse(&text);
}

/// A config file and console lines.
fn fuzz_config(bytes: &[u8]) {
    let text = text(bytes);
    let mut cvars = Cvars::new(None, false);
    cvars.register("cl_fov", 90, CvarOptions::default().range(60.0, 130.0));
    cvars.register("s_volume", 0.8, CvarOptions::default().range(0.0, 1.0));
    cvars.register("name", "Player", CvarOptions::default());
    cvars.register("r_bloom", true, CvarOptions::default());
    let _ = cvars.exec_config(&text);
    let _ = cvars.export_config();
    for line in text.lines() {
        let _ = parse_input(line);
    }
}

fn config_seeds() -> Vec<Vec<u8>> {
    [
        "// settings\ncl_fov 105\ns_volume \"0.5\"\nname \"Ramen \\\"the\\\" Cat\"\nr_bloom 0\nbind w +forward; say \"hi there\"",
        "seta cl_fov 1e400\nset s_volume -0\nunknown_cvar 12\n# comment\nname",
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect()
}

fn fuzz_pk3(bytes: &[u8]) {
    let mut pk3 = Pk3Loader::new();
    if pk3.add_package("fuzz.pk3", bytes.to_vec()).is_ok() {
        let files: Vec<String> = pk3.files().take(64).map(str::to_owned).collect();
        for file in files {
            let _ = pk3.get_file(&file);
        }
    }
}

fn fuzz_net(bytes: &[u8]) {
    let _ = codec::peek_message_type(bytes);
    let _ = codec::read_snapshot_header(bytes);
    let _ = codec::decode_snapshot(bytes, |_| None, &NoEvents);
    let _ = codec::decode_command(bytes);
    let _ = codec::peek_command_seq(bytes);
    let _ = codec::decode_welcome(bytes);
    let _ = codec::decode_join(bytes);
    let _ = codec::decode_join_accept(bytes);
    let _ = codec::decode_resume_token(bytes);
    let _ = codec::decode_resume(bytes);
    let _ = codec::decode_ack(bytes);
    let _ = codec::decode_join_game(bytes);
    let _ = codec::decode_set_loadout(bytes);
    let _ = codec::decode_client_prefs(bytes);
    let _ = codec::decode_chat(bytes);
    let _ = codec::decode_server_message(bytes);
    let _ = codec::decode_ping(bytes);
    let _ = codec::decode_pong(bytes);
    let _ = decode_inputs(bytes);
}

/// Which target a packaged file feeds.
fn target_for(path: &str) -> Option<(&'static str, Target)> {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next()?;
    Some(match ext {
        "bsp" => ("bsp", fuzz_bsp as Target),
        "glb" => ("glb", fuzz_glb),
        "md3" => ("md3", fuzz_md3),
        "jpg" | "jpeg" | "tga" | "png" => ("image", fuzz_image),
        "ktx" => ("ktx", fuzz_ktx),
        "shader" => ("shader", fuzz_shader),
        "wav" | "ogg" | "mp3" => ("sound", fuzz_sound),
        "json" => ("json", fuzz_json),
        _ => return None,
    })
}

/// Network seeds: one of each message the codec writes.
fn net_seeds() -> Vec<Vec<u8>> {
    vec![
        codec::encode_resume("token-123"),
        codec::encode_ack(4242),
        codec::encode_ready(),
        codec::encode_spectate(),
        codec::encode_request_spawn(),
        codec::encode_set_loadout("rifle", "pistol"),
        codec::encode_chat("hello there"),
        codec::encode_server_message(&codec::ServerMessage {
            kind: "info".into(),
            message: "welcome".into(),
        }),
        codec::encode_ping(1234.5),
    ]
}

struct Seed {
    target: &'static str,
    run: Target,
    name: String,
    bytes: Vec<u8>,
}

fn seeds(data: &Path) -> Vec<Seed> {
    let mut out = Vec::new();
    // At most this many files of a kind: a few of each kind, not every texture.
    let mut per_kind: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut packages: Vec<PathBuf> = std::fs::read_dir(data)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    packages.sort();
    for path in packages {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if name.ends_with(".json") {
            let index = name == "index.json";
            out.push(Seed {
                target: if index { "index" } else { "json" },
                run: if index { fuzz_index } else { fuzz_json },
                name,
                bytes,
            });
            continue;
        }
        if !name.ends_with(".pk3") {
            continue;
        }
        if bytes.len() < 4 << 20 {
            out.push(Seed {
                target: "pk3",
                run: fuzz_pk3,
                name: name.clone(),
                bytes: bytes.clone(),
            });
        }
        let mut pk3 = Pk3Loader::new();
        if pk3.add_package(&name, bytes).is_err() {
            continue;
        }
        let files: Vec<String> = pk3.files().map(str::to_owned).collect();
        for file in files {
            let Some((target, run)) = target_for(&file) else {
                continue;
            };
            // Every model (the player's mesh is the one Draco file); a
            // sample of the rest. `FUZZ_SEEDS` raises the sample.
            let sample: usize = std::env::var("FUZZ_SEEDS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(12);
            let limit = match target {
                "bsp" => 3,
                "glb" | "md3" => usize::MAX,
                _ => sample,
            };
            let count = per_kind.entry(target).or_default();
            if *count >= limit {
                continue;
            }
            if let Some(bytes) = pk3.get_file(&file) {
                *count += 1;
                out.push(Seed {
                    target,
                    run,
                    name: format!("{name}:{file}"),
                    bytes,
                });
            }
        }
    }
    for (i, bytes) in config_seeds().into_iter().enumerate() {
        out.push(Seed {
            target: "config",
            run: fuzz_config,
            name: format!("config#{i}"),
            bytes,
        });
    }
    for (i, bytes) in net_seeds().into_iter().enumerate() {
        out.push(Seed {
            target: "net",
            run: fuzz_net,
            name: format!("net#{i}"),
            bytes,
        });
    }
    out
}

#[test]
#[ignore = "fuzzing: run by hand with FUZZ_DATA"]
fn fuzz_parsers() {
    let Some(data) = std::env::var_os("FUZZ_DATA") else {
        eprintln!("FUZZ_DATA is not set; nothing to fuzz");
        return;
    };
    let iterations: u64 = std::env::var("FUZZ_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let only = std::env::var("FUZZ_ONLY").ok();
    let out = PathBuf::from(std::env::var_os("FUZZ_OUT").unwrap_or_else(|| "target/fuzz".into()));
    std::fs::create_dir_all(&out).unwrap();
    let seeds: Vec<Seed> = seeds(Path::new(&data))
        .into_iter()
        .filter(|s| only.as_deref().is_none_or(|o| o.split(',').any(|t| t == s.target)))
        .collect();
    assert!(!seeds.is_empty(), "no seeds in {}", Path::new(&data).display());

    // A watchdog: an input that hangs a parser is a bug too.
    let progress = Arc::new(AtomicU64::new(0));
    let current = Arc::new(Mutex::new(String::new()));
    let current_input = Arc::new(Mutex::new(Vec::new()));
    {
        let (progress, current, current_input, out) =
            (progress.clone(), current.clone(), current_input.clone(), out.clone());
        std::thread::spawn(move || {
            let mut last = (0, Instant::now());
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let now = progress.load(Ordering::Relaxed);
                if now != last.0 {
                    last = (now, Instant::now());
                } else if last.1.elapsed() > Duration::from_secs(60) {
                    let file = out.join("hang.bin");
                    let _ = std::fs::write(&file, &*current_input.lock().unwrap());
                    eprintln!("fuzz: HANG on {} -> {}", current.lock().unwrap(), file.display());
                    std::process::abort();
                }
            }
        });
    }
    // Panics are expected and counted; keep their messages short.
    std::panic::set_hook(Box::new(|info| {
        let at = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        LAST_PANIC.with(|p| *p.borrow_mut() = format!("{at}: {message}"));
    }));

    let mut failures: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut runs = 0u64;
    let started = Instant::now();
    for (index, seed) in seeds.iter().enumerate() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (index as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
        // Large inputs are slow to parse: fewer runs on them.
        let budget = if seed.bytes.len() > 1 << 20 {
            iterations / 10
        } else {
            iterations
        }
        .max(1);
        let seed_started = Instant::now();
        // Named before it runs: an input that exhausts memory ends the
        // process without a report.
        eprintln!("fuzz: start {} {}", seed.target, seed.name);
        for i in 0..budget {
            let state = rng.0;
            let input = if i == 0 {
                seed.bytes.clone()
            } else {
                mutate(&seed.bytes, &mut rng)
            };
            current_input.lock().unwrap().clone_from(&input);
            *current.lock().unwrap() = format!("{} {} iteration {i} rng {state:#x}", seed.target, seed.name);
            if catch_unwind(AssertUnwindSafe(|| (seed.run)(&input))).is_err() {
                let message = LAST_PANIC.with(|p| p.borrow().clone());
                let entry = failures.entry(message.clone()).or_insert((0, String::new()));
                entry.0 += 1;
                if entry.1.is_empty() {
                    let file = out.join(format!("{}-{index}-{i}.bin", seed.target));
                    std::fs::write(&file, &input).unwrap();
                    entry.1 = format!("{} from {} -> {}", seed.target, seed.name, file.display());
                    eprintln!("fuzz: PANIC {message}\n      {}", entry.1);
                }
            }
            runs += 1;
            progress.store(runs, Ordering::Relaxed);
        }
        eprintln!(
            "fuzz: {:<6} {:>6} runs in {:>6.1}s  {}",
            seed.target,
            budget,
            seed_started.elapsed().as_secs_f64(),
            seed.name
        );
    }
    let _ = std::panic::take_hook();
    eprintln!(
        "fuzz: {runs} runs over {} seeds in {:.0}s, {} distinct panics",
        seeds.len(),
        started.elapsed().as_secs_f64(),
        failures.len()
    );
    for (message, (count, first)) in &failures {
        eprintln!("  {count:>5}x {message}\n         first: {first}");
    }
    assert!(failures.is_empty(), "{} distinct panics", failures.len());
}

thread_local! {
    static LAST_PANIC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Runs one saved input: `FUZZ_REPLAY=<file> FUZZ_TARGET=<target>`.
#[test]
#[ignore = "fuzzing: run by hand with FUZZ_REPLAY"]
fn fuzz_replay() {
    let (Some(file), Ok(target)) = (std::env::var_os("FUZZ_REPLAY"), std::env::var("FUZZ_TARGET")) else {
        return;
    };
    let bytes = std::fs::read(file).unwrap();
    let run: Target = match target.as_str() {
        "pk3" => fuzz_pk3,
        "net" => fuzz_net,
        "index" => fuzz_index,
        "config" => fuzz_config,
        other => target_for(&format!("x.{other}")).expect("a target").1,
    };
    let started = Instant::now();
    run(&bytes);
    eprintln!("fuzz: {target} ran in {:.2}s", started.elapsed().as_secs_f64());
}
