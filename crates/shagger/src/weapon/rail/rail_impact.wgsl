// SHAG src/engine/weapon/rail/RailShaderMaterial.ts (IMPACT_VERTEX_SHADER,
// IMPACT_FRAGMENT_SHADER) in WGSL, after effect_quad.wgsl. Instance: i0
// iStartSeed, i1 iEndImpact, i2 iColorLife, i3 iParams, i4 iNormal. No
// atmospheric fog.

const PI: f32 = 3.141592653589793;

struct ImpactOut {
    @builtin(position) position: vec4f,
    @location(0) impact_uv: vec2f,
    @location(1) oriented_part: f32,
    @location(2) kind: f32,
    @location(3) energy: f32,
    @location(4) muzzle: f32,
    @location(5) color: vec3f,
    @location(6) state_seed: vec2f,
};

@vertex
fn vs_main(in: QuadIn) -> ImpactOut {
    let start = in.i0.xyz;
    let seed = in.i0.w;
    let end = in.i1.xyz;
    let impact_kind = in.i1.w;
    let base_color = in.i2.rgb;
    let spawn_time = in.i2.w;
    let intensity = in.i3.x;
    let duration = in.i3.w;
    let time = globals.viewport_time.z;

    let age = max(0.0, time - spawn_time);
    let life = 1.0 - glsl_smoothstep(duration * 0.35, max(duration, 0.001), age);

    let part = in.position.z;
    // Parts 2 and 3: the muzzle crack at the start.
    let muzzle = step(1.5, part);
    // 0: camera billboard, 1: surface disc.
    let oriented_part = glsl_mod(part, 2.0);

    // The muzzle crack always fires; the terminal only on a real contact.
    let terminal_life = life * step(0.5, impact_kind);
    let this_life = mix(terminal_life, life, muzzle);

    let center = mix(end, start, muzzle);
    let pixel_scale = world_per_pixel(center);
    let radius = clamp(
        mix(3.2, 5.4, muzzle),
        mix(8.0, 12.0, muzzle) * pixel_scale,
        mix(26.0, 40.0, muzzle) * pixel_scale,
    ) * step(0.0001, this_life);

    let cam_right = safe_normalize(camera_right(), vec3f(1.0, 0.0, 0.0));
    let cam_up = safe_normalize(camera_up(), vec3f(0.0, 1.0, 0.0));
    var normal = mix(in.i4.xyz, start - end, muzzle);
    if length(normal) < 0.1 {
        normal = safe_normalize(globals.camera.xyz - center, vec3f(0.0, 1.0, 0.0));
    } else {
        normal = safe_normalize(normal, vec3f(0.0, 1.0, 0.0));
    }
    let reference = select(vec3f(1.0, 0.0, 0.0), vec3f(0.0, 1.0, 0.0), abs(normal.y) < 0.95);
    let surface_right = safe_normalize(cross(reference, normal), cam_right);
    let surface_up = safe_normalize(cross(normal, surface_right), cam_up);

    let billboard = oriented_part < 0.5;
    let right = select(surface_right, cam_right, billboard);
    let up = select(surface_up, cam_up, billboard);
    let offset_center = center + normal * select(0.12, 0.08, billboard);
    let world_position = offset_center + (right * in.position.x + up * in.position.y) * radius;

    var out: ImpactOut;
    out.position = globals.view_proj * vec4f(world_position, 1.0);
    out.impact_uv = in.uv;
    out.oriented_part = oriented_part;
    out.kind = impact_kind;
    out.energy = intensity * this_life;
    out.muzzle = muzzle;
    out.color = base_color;
    out.state_seed = vec2f(floor(time * 20.0 + seed * 0.013), seed);
    return out;
}

@fragment
fn fs_main(in: ImpactOut) -> @location(0) vec4f {
    let r = length(in.impact_uv);
    let angle = atan2(in.impact_uv.y, in.impact_uv.x);
    let aa = max(fwidth(r), 0.002);
    let state = in.state_seed.x;
    let seed = in.state_seed.y;
    let energy = max(in.energy, 0.0);

    let nucleus = 1.0 - glsl_smoothstep(0.04, 0.30 + aa, r);
    let halo = (1.0 - glsl_smoothstep(0.16, 1.0, r)) * 0.25;
    let crown = 1.0 - glsl_smoothstep(0.03, 0.08 + aa, abs(r - 0.48));
    let segment = floor(((angle + PI) / (2.0 * PI)) * 13.0);
    let broken = step(0.27, hash11(segment + state * 7.0 + seed));
    let spikes = pow(abs(cos(angle * 7.0 + state * 0.7 + seed)), 40.0)
        * glsl_smoothstep(0.20, 0.36, r) * (1.0 - glsl_smoothstep(0.50, 1.0, r));

    // Cold white-blue everywhere.
    let cold_edge = mix(vec3f(0.18, 0.45, 1.0), in.color, 0.30);
    let white = vec3f(0.90, 0.96, 1.0);

    var shape: f32;
    var color: vec3f;
    var hdr_scale: f32;
    var alpha_scale: f32;
    if in.oriented_part < 0.5 {
        // Camera-facing billboard: bright nucleus and a soft halo.
        shape = nucleus + halo;
        color = mix(cold_edge, white, nucleus);
        hdr_scale = 3.0;
        alpha_scale = 0.92;
    } else {
        // Surface disc: a broken shock ring with radial spikes.
        shape = crown * broken + spikes;
        color = mix(cold_edge, white, crown);
        hdr_scale = 2.2;
        alpha_scale = 0.80;
    }

    let alpha = clamp(shape * energy * alpha_scale, 0.0, 0.97);
    let rgb = color * shape * energy * hdr_scale * alpha;
    return vec4f(rgb, alpha);
}
