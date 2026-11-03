// SHAG src/engine/weapon/muzzle/MuzzleFlashMaterial.ts, its GLSL in WGSL
// (MIT). One camera-facing billboard per shot, styled by a float: 0 burst
// (hot radial star), 1 plume (fiery puff), 2 arcs (electric). Instances:
//   i0: center.xyz, seed   i1: color.rgb, spawn time
//   i2: scale, intensity, duration, style   i3: unused


struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
    @location(1) life: f32,
    @location(2) intensity: f32,
    @location(3) style: f32,
    @location(4) seed: f32,
    @location(5) state: f32,
    @location(6) color: vec3f,
    // vAtmWorld, for world fog.
    @location(7) atm_world: vec3f,
};

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    let center = in.i0.xyz;
    let seed = in.i0.w;
    let spawn = in.i1.w;
    let scale = in.i2.x;
    let duration = in.i2.z;
    let time = globals.viewport_time.z;

    let age = max(0.0, time - spawn);
    let life = pow(1.0 - clamp(age / max(duration, 0.001), 0.0, 1.0), 1.35);

    let pixel_scale = world_per_pixel(center);
    let radius = clamp(scale, 30.0 * pixel_scale, 170.0 * pixel_scale) * step(0.0001, life);

    let right = safe_normalize(camera_right(), vec3f(1.0, 0.0, 0.0));
    let up = safe_normalize(camera_up(), vec3f(0.0, 1.0, 0.0));
    let world_position = center + (right * in.position.x + up * in.position.y) * radius;

    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(world_position, 1.0);
    out.atm_world = world_position;
    out.uv = in.uv;
    out.life = life;
    out.intensity = in.i2.y;
    out.style = in.i2.w;
    out.seed = seed;
    // Quantized time so the crackle holds for a frame or two.
    out.state = floor(time * 34.0 + seed * 0.017);
    out.color = in.i1.rgb;
    return out;
}

const PI: f32 = 3.141592653589793;

// The effect faded by the world fog in front of it (atmFogFadeGlsl), as
// Warfork's additive passes sink into the fog.
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    return shade(in) * (1.0 - atm_fog_evaluate(globals.fog, in.atm_world, globals.camera.xyz, false).a);
}

fn shade(in: VertexOut) -> vec4f {
    let r = length(in.uv);
    let angle = atan2(in.uv.y, in.uv.x);
    let aa = max(fwidth(r), 0.002);
    let seed = in.seed;
    let state = in.state;

    let spin = hash11(seed) * 6.2831853;
    let a = angle + spin;

    let nucleus = 1.0 - smoothstep(0.0, 0.46 + aa, r);
    let white = vec3f(1.0, 0.98, 0.92);

    var color: vec3f;
    var shape: f32;
    var hdr: f32;

    if in.style < 0.5 {
        var spikes = pow(abs(cos(a * 3.0)), 22.0) + 0.6 * pow(abs(cos(a * 6.0 + 1.7)), 40.0);
        spikes *= smoothstep(0.06, 0.30, r) * (1.0 - smoothstep(0.42, 1.0, r));
        shape = nucleus * 1.15 + spikes;
        color = mix(in.color, white, nucleus);
        hdr = 3.1;
    } else if in.style < 1.5 {
        let lobes = 0.62 + 0.38 * pow(abs(cos(a * 2.5 + state * 0.3)), 2.0);
        let rr = r / max(lobes, 0.35);
        let body = 1.0 - smoothstep(0.10, 1.0 + aa, rr);
        let turb = 0.5 + 0.5 * sin(a * 9.0 + seed * 6.0) * (1.0 - smoothstep(0.2, 0.95, rr));
        shape = nucleus * 1.1 + body * 0.9 * turb;
        let fire = mix(in.color, vec3f(1.0, 0.55, 0.12), 0.5);
        color = mix(mix(vec3f(0.35, 0.12, 0.02), fire, body), white, nucleus);
        hdr = 2.5;
    } else {
        var rods = 0.0;
        for (var i = 0; i < 5; i++) {
            let fi = f32(i);
            let base = hash11(fi * 12.7 + seed + floor(state)) * 6.2831853;
            let d = abs(glsl_mod(a - base + PI, 6.2831853) - PI);
            var bolt = glsl_smoothstep(0.16, 0.0, d) * (1.0 - smoothstep(0.12, 1.0, r));
            bolt *= step(r, mix(0.6, 1.0, hash11(fi * 3.1 + state)));
            rods = max(rods, bolt);
        }
        shape = nucleus * 1.2 + rods * 0.9;
        let electric = mix(in.color, vec3f(0.35, 0.75, 1.0), 0.4);
        color = mix(electric, white, max(nucleus, rods * 0.7));
        hdr = 3.0;
    }

    let energy = max(in.life * (0.85 + 0.4 * clamp(in.intensity / 25.0, 0.0, 2.0)), 0.0);
    let alpha = clamp(shape * energy, 0.0, 0.97);
    return vec4f(color * shape * energy * hdr * alpha, alpha);
}
