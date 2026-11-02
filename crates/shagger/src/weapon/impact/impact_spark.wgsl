// SHAG src/engine/weapon/impact/ImpactSparkMaterial.ts, its GLSL in WGSL
// (MIT). Both impact tiers, chosen per instance: 0 an ember streak flying
// its own ballistic arc, 1 a camera-facing hit star. Instances:
//   i0: origin.xyz, seed   i1: velocity.xyz, kind
//   i2: color.rgb, spawn time   i3: size, duration, gravity, intensity
// Not yet: atmospheric fog.

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
    @location(1) life: f32,
    @location(2) kind: f32,
    @location(3) seed: f32,
    @location(4) intensity: f32,
    @location(5) color: vec3f,
};

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    let origin = in.i0.xyz;
    let velocity = in.i1.xyz;
    let kind = in.i1.w;
    let spawn = in.i2.w;
    let size = in.i3.x;
    let duration = in.i3.y;
    let gravity = in.i3.z;

    let age = max(0.0, globals.viewport_time.z - spawn);
    let life = 1.0 - clamp(age / max(duration, 0.001), 0.0, 1.0);

    let pos = origin + velocity * age - vec3f(0.0, 0.5 * gravity * age * age, 0.0);
    let vel = velocity - vec3f(0.0, gravity * age, 0.0);
    let pixel_scale = world_per_pixel(pos);
    let alive = step(0.0001, life);
    var offset: vec3f;
    if kind < 0.5 {
        let speed = length(vel);
        let streak = max(size * (0.55 + 0.85 * clamp(speed / 420.0, 0.0, 1.7)), 2.2 * pixel_scale);
        let width = max(size * 0.16, 0.85 * pixel_scale);
        let dir = safe_normalize(vel, vec3f(0.0, 1.0, 0.0));
        let side = safe_normalize(cross(dir, globals.camera.xyz - pos), vec3f(1.0, 0.0, 0.0));
        offset = (dir * (in.position.x * streak) + side * (in.position.y * width)) * alive;
    } else {
        let radius = max(size, 10.0 * pixel_scale) * alive;
        let right = safe_normalize(camera_right(), vec3f(1.0, 0.0, 0.0));
        let up = safe_normalize(camera_up(), vec3f(0.0, 1.0, 0.0));
        offset = (right * in.position.x + up * in.position.y) * radius;
    }

    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(pos + offset, 1.0);
    out.uv = in.uv;
    out.life = life;
    out.kind = kind;
    out.seed = in.i0.w;
    out.intensity = in.i3.w;
    out.color = in.i2.rgb;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let white = vec3f(1.0, 0.97, 0.89);
    var color: vec3f;
    var shape: f32;
    var alpha: f32;
    var hdr: f32;
    let r = length(in.uv);
    let aa = max(fwidth(r), 0.002);
    if in.kind < 0.5 {
        let along = clamp(in.uv.x * 0.5 + 0.5, 0.0, 1.0);
        let across = 1.0 - clamp(abs(in.uv.y), 0.0, 1.0);
        shape = pow(across, 1.6) * pow(along, 1.4);
        let heat = pow(clamp(in.life, 0.0, 1.0), 0.55) * along;
        color = mix(in.color * 0.55, white, heat);
        alpha = clamp(shape * in.life, 0.0, 1.0);
        hdr = 2.4;
    } else {
        let a = atan2(in.uv.y, in.uv.x) + hash11(in.seed) * 6.2831853;
        var spikes = pow(abs(cos(a * 2.5)), 18.0) + 0.55 * pow(abs(cos(a * 5.0 + 1.1)), 34.0);
        spikes *= smoothstep(0.04, 0.26, r) * (1.0 - smoothstep(0.34, 1.0 + aa, r));
        let nucleus = 1.0 - smoothstep(0.0, 0.22 + aa, r);
        shape = nucleus * 1.15 + spikes;
        color = mix(in.color, white, nucleus);
        let q = floor(clamp(in.life, 0.0, 0.999) * 4.0);
        let stepped = select(select(select(0.12, 0.3, q > 0.5), 0.55, q > 1.5), 0.85, q > 2.5);
        alpha = clamp(shape * stepped, 0.0, 0.95);
        hdr = 2.9;
    }
    let energy = max(0.7 + 0.45 * clamp(in.intensity / 25.0, 0.0, 2.0), 0.0);
    return vec4f(color * shape * energy * hdr * alpha, alpha);
}
