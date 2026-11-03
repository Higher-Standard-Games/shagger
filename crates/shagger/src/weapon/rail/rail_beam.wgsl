// SHAG src/engine/weapon/rail/RailShaderMaterial.ts (BEAM_VERTEX_SHADER,
// BEAM_FRAGMENT_SHADER) in WGSL, after effect_quad.wgsl. Instance: i0
// iStartSeed, i1 iEndImpact, i2 iColorLife, i3 iParams.

const PI: f32 = 3.141592653589793;
const TURNS: f32 = 7.0;
// How fast the helix turns about the axis while it fades (radians/second).
const SPIN: f32 = 5.0;

struct BeamOut {
    @builtin(position) position: vec4f,
    @location(0) side: f32,
    @location(1) part: f32,
    @location(2) energy: f32,
    @location(3) life: f32,
    @location(4) color: vec3f,
    // vAtmWorld, for world fog.
    @location(5) atm_world: vec3f,
};

fn hybrid_half_width(world_position: vec3f, world_width: f32, min_pixels: f32, max_pixels: f32) -> f32 {
    let pixel_scale = world_per_pixel(world_position);
    return clamp(world_width, min_pixels * pixel_scale, max_pixels * pixel_scale);
}

@vertex
fn vs_main(in: QuadIn) -> BeamOut {
    let start = in.i0.xyz;
    let end = in.i1.xyz;
    let base_color = in.i2.rgb;
    let spawn_time = in.i2.w;
    let intensity = in.i3.x;
    let helix_radius = in.i3.y;
    let duration = in.i3.w;
    let time = globals.viewport_time.z;

    let age = max(0.0, time - spawn_time);
    let life = 1.0 - glsl_smoothstep(duration * 0.35, max(duration, 0.001), age);

    let t = in.position.x;
    let side = in.position.y;
    let part = in.position.z;

    let axis = end - start;
    let view = globals.view;
    let view_forward = safe_normalize(vec3f(view[0][2], view[1][2], view[2][2]), vec3f(0.0, 0.0, 1.0));
    let direction = safe_normalize(axis, view_forward);

    // A frame perpendicular to the axis carries the helix as a true spiral.
    let reference = select(vec3f(1.0, 0.0, 0.0), vec3f(0.0, 1.0, 0.0), abs(direction.y) < 0.95);
    let perp_a = safe_normalize(cross(direction, reference), vec3f(1.0, 0.0, 0.0));
    let perp_b = safe_normalize(cross(direction, perp_a), vec3f(0.0, 1.0, 0.0));

    let straight_center = mix(start, end, t);
    var center = straight_center;
    if part > 1.5 {
        // Fat in the middle, converging at muzzle and impact; tightening
        // toward the core as the shot fades.
        let envelope = pow(sin(clamp(t, 0.0, 1.0) * PI), 0.55);
        let phase_offset = select(0.0, PI, part > 2.5);
        let phase = t * TURNS * 2.0 * PI + time * SPIN + phase_offset;
        let radius = helix_radius * envelope * mix(0.35, 1.0, life);
        center = straight_center + perp_a * cos(phase) * radius + perp_b * sin(phase) * radius;
    }

    // Billboard the ribbon around the beam tangent, toward the camera.
    let to_camera = safe_normalize(globals.camera.xyz - center, -view_forward);
    let ribbon_side = safe_normalize(cross(direction, to_camera), perp_a);

    var half_width: f32;
    if part < 0.5 {
        half_width = hybrid_half_width(center, 3.8, 13.0, 74.0);
    } else if part < 1.5 {
        half_width = hybrid_half_width(center, 0.80, 4.0, 17.0);
    } else {
        half_width = hybrid_half_width(center, 0.55, 3.2, 12.0);
    }

    let world_position = center + ribbon_side * side * half_width;
    var out: BeamOut;
    out.position = globals.view_proj * vec4f(world_position, 1.0);
    out.atm_world = world_position;
    out.side = side;
    out.part = part;
    out.energy = intensity;
    out.life = life;
    out.color = base_color;
    return out;
}

// The effect faded by the world fog in front of it (atmFogFadeGlsl), as
// Warfork's additive passes sink into the fog.
@fragment
fn fs_main(in: BeamOut) -> @location(0) vec4f {
    return shade(in) * (1.0 - atm_fog_evaluate(globals.fog, in.atm_world, globals.camera.xyz, false).a);
}

fn shade(in: BeamOut) -> vec4f {
    let d = abs(in.side);
    let aa = max(fwidth(d), 0.001);
    var hot = 1.0 - glsl_smoothstep(0.10 - aa, 0.30 + aa, d);
    var body = 1.0 - glsl_smoothstep(0.45 - aa, 0.80 + aa, d);
    let fringe = 1.0 - glsl_smoothstep(0.80 - aa, 1.0 + aa, d);

    var color: vec3f;
    var alpha_scale: f32;
    var hdr_scale: f32;
    if in.part < 0.5 {
        // Wide cold sheath.
        color = mix(vec3f(0.10, 0.34, 1.0), in.color, 0.30);
        hot = 0.0;
        body *= 0.72;
        alpha_scale = 0.16;
        hdr_scale = 0.85;
    } else if in.part < 1.5 {
        // Bright white-blue core.
        color = mix(vec3f(0.55, 0.80, 1.0), vec3f(1.0, 1.0, 1.0), hot);
        alpha_scale = 0.98;
        hdr_scale = 3.0;
    } else {
        // Electric-blue helix, whitening at its own centreline.
        color = mix(mix(vec3f(0.20, 0.52, 1.0), in.color, 0.25), vec3f(0.85, 0.95, 1.0), hot);
        alpha_scale = 0.72;
        hdr_scale = 2.1;
    }

    let shape = hot + body * 0.60 + fringe * 0.20;
    let energy = max(in.energy * in.life, 0.0);
    let alpha = clamp(shape * energy * alpha_scale, 0.0, 0.96);
    let rgb = color * shape * energy * hdr_scale * alpha;
    return vec4f(rgb, alpha);
}
