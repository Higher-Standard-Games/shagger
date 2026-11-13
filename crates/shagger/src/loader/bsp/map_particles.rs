//! SHAG `src/engine/loader/bsp/MapParticles.ts`: `misc_particles`, a point
//! that keeps throwing out sprites, such as smoke behind glass or bubbles on
//! slime. Presentation only, so the client owns it alone, and it stops with
//! the map.
//!
//! Each sprite follows the Warsow rules for these emitters: `count` sprites
//! a second, each living `delay` seconds; SPHERICAL throws them every way at
//! `speed`, otherwise along `angles`, spread by `radius`. They fade over
//! their life after a short fade-in, EXPAND_EFFECT grows them up to five
//! times and SHRINK_EFFECT shrinks them away. GRAVITY pulls them down.
//!
//! The browser's thin-instanced plane per shader is an [`EffectQuadBatch`]
//! per shader here, blending premultiplied: an additive stage writes no
//! alpha, which is `ALPHA_ADD`.

use glam::DVec3;

use crate::loader::bsp::bsp_loader::BspEntity;
use crate::loader::pk3::pk3_loader::Pk3Loader;
use crate::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use crate::loader::shader::q3_shader_types::{AlphaGenType, BlendFactor, Q3ShaderStage, RgbGenType};
use crate::loader::texture::resolve_and_load_texture;
use crate::rendering::effect_quad_batch::{EffectFrame, EffectInstance, EffectQuadBatch};
use crate::rendering::gpu::Gpu;
use crate::rendering::world_renderer::Rgba8Image;
use shagger_shared::js::{
    js_cos, js_hypot, js_max, js_min, js_number_to_string, js_parse_float, js_parse_int10, js_sin, js_string_to_number,
    js_trim, to_int32,
};
use shagger_shared::spread_utils::mulberry32;

const SPHERICAL: i32 = 1;
const GRAVITY: i32 = 4;
const EXPAND_EFFECT: i32 = 16;
const SHRINK_EFFECT: i32 = 32;
const START_OFF: i32 = 64;
/// Warsow ages local sprites in 100 ms frames and fades them in over two.
const FRAME_SECONDS: f64 = 0.1;
const FADE_IN_FRAMES: f64 = 2.0;
const GRAVITY_ACCEL: f64 = 175.0;
/// Sprites live this close to the camera only.
const RANGE: f64 = 2500.0;
/// The most sprites one scene draws, whatever the map asks for.
const CAPACITY: usize = 512;

/// `ParticlePlan`: what one emitter throws out.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticlePlan {
    pub shader: String,
    /// Engine-space emitter position.
    pub origin: DVec3,
    /// Engine-space unit direction of a non-spherical emitter.
    pub forward: DVec3,
    pub right: DVec3,
    pub up: DVec3,
    pub spherical: bool,
    pub speed: f64,
    /// Spread at 1024 units along `forward`, in units.
    pub spread: f64,
    /// Sprites a second.
    pub rate: f64,
    /// Seconds each sprite lives.
    pub life: f64,
    /// Sprite radius in units.
    pub size: f64,
    pub gravity: bool,
    pub expand: bool,
    pub shrink: bool,
}

/// `number(value, fallback)`: `parseFloat`, else the fallback.
fn number(value: Option<&str>, fallback: f64) -> f64 {
    let parsed = js_parse_float(value.unwrap_or(""));
    if parsed.is_finite() { parsed } else { fallback }
}

/// `Math.trunc(v) & 255`.
fn byte(v: f64) -> f64 {
    f64::from(to_int32(v.trunc()) & 255)
}

/// `value.trim().split(/\s+/).map(Number)`.
fn numbers(value: &str) -> Vec<f64> {
    js_trim(value).split_whitespace().map(js_string_to_number).collect()
}

/// `particlePlan`: what a `misc_particles` entity emits, or `None` when it
/// emits nothing on its own.
#[must_use]
pub fn particle_plan(entity: &BspEntity) -> Option<ParticlePlan> {
    let p = &entity.properties;
    let flags = {
        let f = js_parse_int10(p.get("spawnflags").unwrap_or("0"));
        if f.is_nan() { 0 } else { to_int32(f) }
    };
    // A START_OFF emitter waits for a trigger, which the client cannot fire.
    if flags & START_OFF != 0 {
        return None;
    }
    let origin = numbers(p.get("origin").unwrap_or(""));
    if origin.len() != 3 || !origin.iter().all(|v| v.is_finite()) {
        return None;
    }
    // The game truncates speed, count and size to bytes; a fractional speed
    // is still.
    let speed = byte(number(p.get("speed"), 0.0));
    let rate = byte(number(p.get("count"), 0.0));
    let size = match byte(number(p.get("size"), 16.0)) {
        0.0 => 16.0,
        s => s,
    };
    // Whole seconds, as the game stores them.
    let life = match byte(number(p.get("delay"), 0.0)) {
        0.0 => 4.0,
        l => l,
    };
    if rate <= 0.0 || life <= 0.0 {
        return None;
    }

    // Q3 angles: pitch, yaw, roll in degrees; negative pitch looks up.
    let angles = match p.get("angles") {
        Some(a) => numbers(a),
        None => numbers(&format!("0 {} 0", p.get("angle").unwrap_or("0"))),
    };
    let angle = |i: usize| {
        let a = angles.get(i).copied().filter(|a| a.is_finite()).unwrap_or(0.0);
        (a * std::f64::consts::PI) / 180.0
    };
    let (pitch, yaw, roll) = (angle(0), angle(1), angle(2));
    let (sp, cp) = (js_sin(pitch), js_cos(pitch));
    let (sy, cy) = (js_sin(yaw), js_cos(yaw));
    let (sr, cr) = (js_sin(roll), js_cos(roll));
    let q3_forward = [cp * cy, cp * sy, -sp];
    let q3_right = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let q3_up = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    let engine = |v: [f64; 3]| DVec3::new(v[0], v[2], v[1]);
    Some(ParticlePlan {
        shader: p
            .get("shadername")
            .or_else(|| p.get("shaderName"))
            .unwrap_or("particle")
            .to_owned(),
        origin: engine([origin[0], origin[1], origin[2]]),
        forward: engine(q3_forward),
        right: engine(q3_right),
        up: engine(q3_up),
        spherical: flags & SPHERICAL != 0,
        speed,
        spread: byte(number(p.get("radius"), 0.0)) * 25.0,
        rate,
        life,
        size,
        gravity: flags & GRAVITY != 0,
        expand: flags & EXPAND_EFFECT != 0 && flags & SHRINK_EFFECT == 0,
        shrink: flags & SHRINK_EFFECT != 0 && flags & EXPAND_EFFECT == 0,
    })
}

/// A sprite's `scale` and `alpha` ([`sprite_look`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteLook {
    pub scale: f64,
    pub alpha: f64,
}

/// `spriteLook`: scale and opacity of a sprite `age` seconds into a
/// `life`-second life, as Warsow's local entities draw them. `None` once it
/// is gone.
#[must_use]
pub fn sprite_look(life: f64, expand: bool, shrink: bool, age: f64) -> Option<SpriteLook> {
    let frames = (life * 10.0).trunc();
    let frac = age / FRAME_SECONDS;
    if frames <= 1.0 || frac.floor() >= frames - 1.0 {
        return None;
    }
    let left = js_min(1.0, js_max(0.0, 1.0 - frac / (frames - 1.0)));
    let fade_in = if frames > FADE_IN_FRAMES * 2.0 {
        js_min(1.0, frac / FADE_IN_FRAMES)
    } else {
        1.0
    };
    let alpha = js_min(left, fade_in);
    let scale = if expand {
        if left > 0.0 { js_min(5.0, 1.0 + 1.0 / left) } else { 5.0 }
    } else if shrink {
        js_min(1.0, js_max(0.1, left + 0.1))
    } else {
        1.0
    };
    Some(SpriteLook { scale, alpha })
}

/// `seedFrom(a, b)`: FNV-1a over the two numbers' 32 bits (and a zero).
fn seed_from(a: u32, b: u32) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for v in [a, b, 0] {
        h ^= v;
        h = h.wrapping_mul(16_777_619);
    }
    h
}

/// One sprite in flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    pub position: DVec3,
    pub velocity: DVec3,
    pub age: f64,
    pub rotation: f64,
}

/// `Emitter`: one entity's sprites, thrown out on the presentation clock.
pub struct Emitter {
    pub plan: ParticlePlan,
    pub sprites: Vec<Sprite>,
    next: f64,
    last: f64,
    random: Box<dyn FnMut() -> f64 + Send + Sync>,
}

impl Emitter {
    /// Starts at `now` ms, seeded by its shader and origin so every client
    /// throws the same sprites.
    #[must_use]
    pub fn new(plan: ParticlePlan, now: f64) -> Self {
        let text = format!(
            "{}|{}|{}|{}",
            plan.shader,
            js_number_to_string(plan.origin.x),
            js_number_to_string(plan.origin.y),
            js_number_to_string(plan.origin.z)
        );
        let mut seed = 0;
        let mut units = [0u16; 2];
        for letter in text.chars() {
            seed = seed_from(seed, u32::from(letter.encode_utf16(&mut units)[0]));
        }
        Self {
            plan,
            sprites: Vec::new(),
            next: now,
            last: now,
            random: Box::new(mulberry32(seed)),
        }
    }

    /// `advance`: ages and moves the sprites to `now` ms and, when `near`
    /// the camera, throws out the ones due.
    pub fn advance(&mut self, now: f64, near: bool) {
        let dt = js_min(0.25, js_max(0.0, (now - self.last) / 1000.0));
        self.last = now;
        let plan = &self.plan;
        for i in (0..self.sprites.len()).rev() {
            let s = &mut self.sprites[i];
            s.age += dt;
            if s.age >= plan.life {
                self.sprites.remove(i);
                continue;
            }
            if plan.gravity {
                s.velocity.y -= GRAVITY_ACCEL * dt;
            }
            s.position += s.velocity * dt;
        }
        let interval = 1000.0 / plan.rate;
        // Out of range it rests; coming back, it picks up from now rather
        // than catching up.
        if !near {
            self.next = now + interval;
            return;
        }
        let mut emitted = 0;
        while now >= self.next && emitted < 8 {
            let sprite = self.spawn((now - self.next) / 1000.0);
            self.sprites.push(sprite);
            self.next += interval;
            emitted += 1;
        }
        if now >= self.next {
            self.next = now + interval;
        }
    }

    fn spawn(&mut self, age: f64) -> Sprite {
        let plan = &self.plan;
        let random = &mut self.random;
        let (mut dx, mut dy, mut dz);
        if plan.spherical {
            // Every direction alike: a uniform point on the sphere.
            let u = random() * 2.0 - 1.0;
            let a = random() * std::f64::consts::PI * 2.0;
            let ring = (1.0 - u * u).sqrt();
            dx = js_cos(a) * ring;
            dy = u;
            dz = js_sin(a) * ring;
        } else {
            let a = std::f64::consts::PI * (random() * 2.0 - 1.0);
            let s = random();
            let r = s * js_cos(a) * plan.spread;
            let u = s * js_sin(a) * plan.spread;
            dx = plan.forward.x * 1024.0 + plan.right.x * r + plan.up.x * u;
            dy = plan.forward.y * 1024.0 + plan.right.y * r + plan.up.y * u;
            dz = plan.forward.z * 1024.0 + plan.right.z * r + plan.up.z * u;
            let length = match js_hypot(&[dx, dy, dz]) {
                l if l != 0.0 && !l.is_nan() => l,
                _ => 1.0,
            };
            dx /= length;
            dy /= length;
            dz /= length;
        }
        let direction = DVec3::new(dx, dy, dz);
        Sprite {
            position: plan.origin + direction * (plan.speed * age),
            velocity: direction * plan.speed,
            age,
            rotation: random() * std::f64::consts::PI * 2.0,
        }
    }
}

/// `stageTint`: shader stage colour, `rgbGen const` as authored (0-1 or
/// 0-255), else white.
#[must_use]
pub fn stage_tint(stage: Option<&Q3ShaderStage>) -> [f64; 3] {
    let color = stage
        .and_then(|s| s.rgb_gen)
        .filter(|g| g.kind == RgbGenType::Const)
        .and_then(|g| g.color);
    let Some(color) = color else {
        return [1.0, 1.0, 1.0];
    };
    let scale = if color.iter().any(|v| *v > 1.0) {
        1.0 / 255.0
    } else {
        1.0
    };
    [color[0] * scale, color[1] * scale, color[2] * scale]
}

/// The sprites' shader: a camera-facing quad turned by the sprite's roll,
/// the image times the stage tint, faded by the sprite's alpha.
const MAP_PARTICLE_WGSL: &str = r"
// SHAG MapParticles.ts in WGSL (MIT): a misc_particles sprite on a
// camera-facing plane turned by its own roll, StandardMaterial unlit with
// the image, the stage tint and the instance alpha. Instances:
//   i0: position.xyz, size (the plane's half width)
//   i1: cos, sin of the roll, alpha, additive (1) or not (0)
//   i2: tint rgb

@group(0) @binding(1) var sprite: texture_2d<f32>;
@group(0) @binding(2) var sprite_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
    @location(1) alpha_add: vec2f,
    @location(2) tint: vec3f,
    @location(3) atm_world: vec3f,
};

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    let size = in.i0.w;
    let c = in.i1.x * size;
    let n = in.i1.y * size;
    let right = camera_right();
    let up = camera_up();
    let axis_x = right * c + up * n;
    let axis_y = up * c - right * n;
    let world = in.i0.xyz + axis_x * in.uv.x + axis_y * in.uv.y;
    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(world, 1.0);
    out.uv = vec2f(in.uv.x * 0.5 + 0.5, 0.5 - in.uv.y * 0.5);
    out.alpha_add = in.i1.zw;
    out.tint = in.i2.xyz;
    out.atm_world = world;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let texel = textureSample(sprite, sprite_sampler, in.uv);
    let a = texel.a * in.alpha_add.x;
    if a < 0.004 { discard; }
    var colour = texel.rgb * in.tint;
    let fog = atm_fog_evaluate(globals.fog, in.atm_world, globals.camera.xyz, false);
    if in.alpha_add.y > 0.5 {
        // Fog takes an additive sprite away rather than tinting it.
        colour = pow(colour * (1.0 - fog.a), vec3f(2.2));
        return vec4f(colour * a, 0.0);
    }
    colour = pow(mix(colour, fog.rgb, fog.a), vec3f(2.2));
    return vec4f(colour * a, a);
}
";

/// `SpriteBatch`: the sprites of one shader in one draw call.
struct SpriteBatch {
    emitters: Vec<Emitter>,
    batch: EffectQuadBatch,
    additive: bool,
    tint: [f64; 3],
    fades: bool,
    instances: Vec<EffectInstance>,
}

/// `ParticleField`: the map's `misc_particles` emitters by shader, advanced
/// and drawn each frame.
#[derive(Default)]
pub struct MapParticles {
    batches: Vec<SpriteBatch>,
}

impl MapParticles {
    /// `startMapParticles` for every `misc_particles` in `entities`: an
    /// emitter whose shader has no image draws nothing (and says so).
    pub fn load(
        gpu: &Gpu,
        color_format: wgpu::TextureFormat,
        entities: &[BspEntity],
        shaders: &Q3ShaderRegistry,
        pk3: &mut Pk3Loader,
        now: f64,
    ) -> Self {
        let mut out = Self::default();
        let mut keys: Vec<(String, Option<usize>)> = Vec::new();
        for entity in entities.iter().filter(|e| e.classname == "misc_particles") {
            let Some(plan) = particle_plan(entity) else {
                continue;
            };
            let key = plan.shader.to_lowercase();
            let index = match keys.iter().find(|(k, _)| *k == key) {
                Some((_, index)) => *index,
                None => {
                    let index = Self::create(gpu, color_format, &plan.shader, shaders, pk3).map(|b| {
                        out.batches.push(b);
                        out.batches.len() - 1
                    });
                    keys.push((key, index));
                    index
                }
            };
            if let Some(index) = index {
                out.batches[index].emitters.push(Emitter::new(plan, now));
            }
        }
        out
    }

    fn create(
        gpu: &Gpu,
        color_format: wgpu::TextureFormat,
        shader: &str,
        shaders: &Q3ShaderRegistry,
        pk3: &mut Pk3Loader,
    ) -> Option<SpriteBatch> {
        let definition = shaders.get_shader(shader);
        let stage = definition.and_then(|d| d.stages.iter().find(|s| !s.map.is_empty() && !s.map.starts_with('$')));
        let path = stage.map_or(shader, |s| s.map.as_str());
        // Shaders name the source image; packages ship it as WebP or a KTX2
        // sidecar.
        let Some(texture) = resolve_and_load_texture(pk3, path) else {
            eprintln!("[MapParticles] No image for particle shader {shader}");
            return None;
        };
        let additive = stage
            .and_then(|s| s.blend_func)
            .is_some_and(|b| b == [BlendFactor::One, BlendFactor::One]);
        let fades = stage
            .and_then(|s| s.alpha_gen)
            .is_none_or(|g| g.kind == AlphaGenType::Vertex);
        let image = Rgba8Image {
            width: texture.width,
            height: texture.height,
            pixels: texture.pixels,
        };
        let label = format!("misc_particles:{shader}");
        Some(SpriteBatch {
            emitters: Vec::new(),
            batch: EffectQuadBatch::textured(gpu, color_format, &label, MAP_PARTICLE_WGSL, CAPACITY, &image, true),
            additive,
            tint: stage_tint(stage),
            fades,
            instances: Vec::with_capacity(CAPACITY),
        })
    }

    /// How many emitters are running.
    #[must_use]
    pub fn emitter_count(&self) -> usize {
        self.batches.iter().map(|b| b.emitters.len()).sum()
    }

    /// Every emitter advanced to `now` ms against the camera at `eye`
    /// (what [`Self::prepare`] does first; a still shot steps it ahead).
    pub fn advance(&mut self, eye: DVec3, now: f64) {
        for emitter in self.batches.iter_mut().flat_map(|b| &mut b.emitters) {
            let near = emitter.plan.origin.distance_squared(eye) < RANGE * RANGE;
            emitter.advance(now, near);
        }
    }

    /// `update`: every emitter advanced to `now` ms against the camera at
    /// `eye`, and the live sprites written.
    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame, now: f64) {
        self.advance(frame.camera_position.as_dvec3(), now);
        for b in &mut self.batches {
            b.instances.clear();
            for emitter in &b.emitters {
                for s in &emitter.sprites {
                    if b.instances.len() >= CAPACITY {
                        break;
                    }
                    let plan = &emitter.plan;
                    let Some(look) = sprite_look(plan.life, plan.expand, plan.shrink, s.age) else {
                        continue;
                    };
                    let size = plan.size * look.scale;
                    let alpha = if b.fades { look.alpha } else { 1.0 };
                    let mut instance = [0.0f32; 20];
                    instance[..4].copy_from_slice(&[
                        s.position.x as f32,
                        s.position.y as f32,
                        s.position.z as f32,
                        size as f32,
                    ]);
                    instance[4..8].copy_from_slice(&[
                        s.rotation.cos() as f32,
                        s.rotation.sin() as f32,
                        alpha as f32,
                        if b.additive { 1.0 } else { 0.0 },
                    ]);
                    instance[8..11].copy_from_slice(&b.tint.map(|v| v as f32));
                    b.instances.push(instance);
                }
            }
            b.batch.prepare(gpu, frame, &b.instances, true);
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        for b in &self.batches {
            b.batch.draw(pass);
        }
    }
}

#[cfg(test)]
mod tests {
    //! SHAG `src/engine/loader/bsp/__tests__/MapParticles.test.ts`.
    use super::*;
    use crate::loader::bsp::bsp_loader::Properties;

    fn entity(properties: &[(&str, &str)]) -> BspEntity {
        let mut p = Properties::default();
        for (k, v) in properties {
            p.set(k, v);
        }
        BspEntity {
            classname: "misc_particles".to_owned(),
            properties: p,
        }
    }

    #[test]
    fn reads_wda2s_smoke_spherical_expanding_6_a_second_for_3_s() {
        let plan = particle_plan(&entity(&[
            ("origin", "-450 -644 190"),
            ("shaderName", "gfx/misc/smokepuff"),
            ("speed", "8"),
            ("delay", "3"),
            ("count", "6"),
            ("size", "9"),
            ("spawnflags", "17"),
            ("angles", "-90 0 0"),
        ]))
        .unwrap();
        assert_eq!(plan.shader, "gfx/misc/smokepuff");
        assert_eq!(plan.origin, DVec3::new(-450.0, 190.0, -644.0));
        assert!(plan.spherical && plan.expand && !plan.shrink);
        assert_eq!((plan.rate, plan.life, plan.size, plan.speed), (6.0, 3.0, 9.0, 8.0));
    }

    #[test]
    fn truncates_wdm17s_bubble_speed_to_nothing_as_the_game_does_and_points_up() {
        let plan = particle_plan(&entity(&[
            ("origin", "0 0 0"),
            ("shaderName", "gfx/misc/toxicbubble"),
            ("speed", "0.25"),
            ("delay", "10"),
            ("count", "1"),
            ("size", "24"),
            ("spawnflags", "32"),
            ("angles", "-90 0 0"),
        ]))
        .unwrap();
        assert_eq!(plan.speed, 0.0);
        assert!(plan.shrink);
        assert!((plan.forward.y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn leaves_a_start_off_emitter_and_an_empty_one_alone() {
        assert_eq!(
            particle_plan(&entity(&[("origin", "0 0 0"), ("count", "4"), ("spawnflags", "64")])),
            None
        );
        assert_eq!(particle_plan(&entity(&[("origin", "0 0 0")])), None);
    }

    #[test]
    fn fades_in_quickly_out_over_its_life_and_grows_up_to_five_times() {
        let look = |age| sprite_look(3.0, true, false, age);
        assert_eq!(look(0.0).unwrap().alpha, 0.0);
        assert!((look(0.2).unwrap().alpha - (1.0 - 2.0 / 29.0)).abs() < 1e-9);
        assert_eq!(look(0.0).unwrap().scale, 2.0);
        assert_eq!(look(2.85).unwrap().scale, 5.0);
        assert_eq!(look(2.95), None);
        let shrink = sprite_look(10.0, false, true, 9.5).unwrap();
        assert!((shrink.scale - (0.1 + 1.0 - 95.0 / 99.0)).abs() < 1e-9);
    }

    #[test]
    fn throws_out_count_a_second_and_lets_them_go_after_their_life() {
        let plan = particle_plan(&entity(&[
            ("origin", "0 0 0"),
            ("shaderName", "gfx/misc/smokepuff"),
            ("speed", "8"),
            ("delay", "3"),
            ("count", "6"),
            ("spawnflags", "17"),
        ]))
        .unwrap();
        let mut emitter = Emitter::new(plan, 0.0);
        let mut now = 0.0;
        for _ in 0..600 {
            now += 1000.0 / 60.0;
            emitter.advance(now, true);
        }
        // Six a second, three seconds each.
        assert!((17..=19).contains(&emitter.sprites.len()), "{}", emitter.sprites.len());
        for s in &emitter.sprites {
            assert!((s.position.length() - 8.0 * s.age).abs() < 1e-6);
        }
        // Out of range it lets its sprites die and throws no more.
        for _ in 0..240 {
            now += 1000.0 / 60.0;
            emitter.advance(now, false);
        }
        assert!(emitter.sprites.is_empty());
    }

    #[test]
    fn seeds_from_shader_and_origin_as_v8_does() {
        // `seedFrom` over "gfx/misc/smokepuff|-450|190|-644" and the first
        // draws of `mulberry32`, in V8.
        let plan = particle_plan(&entity(&[
            ("origin", "-450 -644 190"),
            ("shaderName", "gfx/misc/smokepuff"),
            ("count", "6"),
        ]))
        .unwrap();
        let mut emitter = Emitter::new(plan, 0.0);
        let draws = [(emitter.random)(), (emitter.random)(), (emitter.random)()];
        assert_eq!(draws, [0.6966852627228945, 0.3928473119158298, 0.5154439362231642]);
    }
}
