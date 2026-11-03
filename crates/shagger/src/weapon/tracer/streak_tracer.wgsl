// SHAG src/engine/weapon/tracer/StreakTracerMaterial.ts, its GLSL in WGSL
// (MIT). A bullet streak: a white-hot core in a weapon-coloured glow, laid
// along the shot and turned to face the camera about its own axis; the vertex
// shader slides it from the muzzle to the hit. Instances:
//   i0: start.xyz, spawn time   i1: end.xyz, speed
//   i2: color.rgb, length       i3: width, intensity


struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec3f,
    @location(2) intensity: f32,
    // vAtmWorld, for world fog.
    @location(3) atm_world: vec3f,
};

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    let start = in.i0.xyz;
    let end = in.i1.xyz;
    let spawn = in.i0.w;
    let speed = in.i1.w;
    let streak_length = in.i2.w;
    let width = in.i3.x;

    let path = end - start;
    let total = length(path);
    let dir = safe_normalize(path, vec3f(0.0, 0.0, 1.0));

    let travelled = max(0.0, globals.viewport_time.z - spawn) * speed;
    let head = min(travelled, total);
    let tail = clamp(travelled - streak_length, 0.0, total);
    let alive = step(tail + 0.01, head);

    let along = mix(tail, head, in.position.x * 0.5 + 0.5);
    let center = start + dir * along;

    let pixel_scale = world_per_pixel(center);
    let half_width = max(width, 1.4 * pixel_scale) * alive;
    let side = safe_normalize(cross(dir, globals.camera.xyz - center), vec3f(1.0, 0.0, 0.0));
    let world_position = center + side * (in.position.y * half_width);

    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(world_position, 1.0);
    out.atm_world = world_position;
    out.uv = in.uv;
    out.color = in.i2.rgb;
    out.intensity = in.i3.y;
    return out;
}

// The effect faded by the world fog in front of it (atmFogFadeGlsl), as
// Warfork's additive passes sink into the fog.
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    return shade(in) * (1.0 - atm_fog_evaluate(globals.fog, in.atm_world, globals.camera.xyz, false).a);
}

fn shade(in: VertexOut) -> vec4f {
    let white = vec3f(1.0, 0.97, 0.9);
    let along = clamp(in.uv.x * 0.5 + 0.5, 0.0, 1.0);
    let across = 1.0 - clamp(abs(in.uv.y), 0.0, 1.0);

    let core = pow(across, 6.0);
    let glow = pow(across, 1.6);
    let length_fade = pow(along, 1.3) * (1.0 - smoothstep(0.92, 1.0, along));
    let shape = (core * 1.2 + glow * 0.55) * length_fade;
    let color = mix(in.color, white, clamp(core * 1.4 + along * 0.25, 0.0, 1.0));

    let energy = 0.8 + 0.4 * clamp(in.intensity / 20.0, 0.0, 2.0);
    let alpha = clamp(shape, 0.0, 1.0);
    return vec4f(color * shape * energy * 2.2 * alpha, alpha);
}
