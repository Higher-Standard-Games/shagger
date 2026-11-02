// SHAG src/engine/weapon/explosion/ExplosionBatchMaterial.ts, its GLSL
// rewritten in WGSL (MIT). Each layer is one instanced mesh whose vertex
// shader animates every live explosion from its spawn time. Five vec4s per
// instance, read per layer:
//
//   layer      a                b                  c                d                   e
//   fireball   origin, spawn    hit normal, speed  core, scale      outer, -            smoke, -
//   ray        origin, spawn    direction, speed   core, scale      outer, width        length, -
//   spark      origin, spawn    velocity, life     color, size      dead color, -       -
//
// Not yet: atmospheric fog, and linear output for image processing as a
// post-process.

struct Globals {
    view_proj: mat4x4f,
    view: mat4x4f,
    // x: seconds
    time: vec4f,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct VertexIn {
    @location(0) position: vec3f,
    @location(1) uv: vec2f,
    @location(2) a: vec4f,
    @location(3) b: vec4f,
    @location(4) c: vec4f,
    @location(5) d: vec4f,
    @location(6) e: vec4f,
};

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) color: vec3f,
    @location(1) alpha: f32,
    @location(2) uv: vec2f,
};

// EXPLOSION_FIRE_TIME, SPARK_AGE_PER_SECOND, SPARK_GRAVITY
const FIRE_TIME: f32 = 0.6;
const SPARK_AGE_PER_SECOND: f32 = 0.6;
const SPARK_GRAVITY: f32 = -30.0;

fn ease_out(t: f32) -> f32 {
    return t * (2.0 - t);
}

fn finish(in: VertexIn, world_in: vec3f, color: vec3f, alpha: f32) -> VertexOut {
    var out: VertexOut;
    // A finished layer collapses rather than drawing invisible triangles.
    let world = select(in.a.xyz, world_in, alpha > 0.0);
    out.clip = globals.view_proj * vec4f(world, 1.0);
    out.color = color;
    out.alpha = alpha;
    out.uv = in.uv;
    return out;
}

@vertex
fn vs_fireball(in: VertexIn) -> VertexOut {
    let origin = in.a.xyz;
    let seconds = max(0.0, globals.time.x - in.a.w);
    let speed = in.b.w;
    let scale = in.c.w;
    let t = clamp(seconds * speed / FIRE_TIME, 0.0, 1.0);
    let grow = ease_out(min(t / 0.7, 1.0));
    let base_r = 2.5 * scale;
    let unit = normalize(in.position);
    let angle1 = atan2(unit.y, unit.x);
    let angle2 = acos(clamp(unit.z / base_r, -1.0, 1.0));
    let noise = 0.5 + 0.5 * sin(angle1 * 7.3 + angle2 * 5.1) * cos(angle1 * 3.7 - angle2 * 8.9);
    let o = unit * base_r * (1.0 + noise * 0.8);
    let dist = length(o);
    var v = o;
    if dist > 0.0 {
        let new_dist = base_r + (dist - base_r) * (1.0 + grow * 1.5);
        v = o * (new_dist / dist);
        let spread = ease_out(select(0.0, (t - 0.4) / 0.6, t > 0.4)) * 1.2;
        if spread > 0.0 {
            let n = in.b.xyz;
            let along = dot(v, n) * n;
            let across = v - along;
            v = along * max(0.3, 1.0 - spread * 0.4) + across * (1.0 + spread * 0.5);
        }
    }
    let world = origin + v * grow;
    let core = in.c.rgb;
    let outer = in.d.rgb;
    let fade = outer * 0.4;
    var color: vec3f;
    if t < 0.3 {
        color = mix(core, outer, t / 0.3);
    } else if t < 0.6 {
        color = mix(outer, fade, (t - 0.3) / 0.3);
    } else {
        color = mix(fade, in.e.rgb, (t - 0.6) / 0.4);
    }
    let alpha = select(0.9 * (1.0 - (t - 0.4) / 0.6), 0.9, t < 0.4);
    return finish(in, world, color, alpha);
}

@vertex
fn vs_ray(in: VertexIn) -> VertexOut {
    let origin = in.a.xyz;
    let seconds = max(0.0, globals.time.x - in.a.w);
    let speed = in.b.w;
    let scale = in.c.w;
    let t = clamp(seconds * speed / FIRE_TIME, 0.0, 1.0);
    let grow = ease_out(min(t / 0.15, 1.0));
    let dir = in.b.xyz;
    let center = origin + dir * (ease_out(min(t / 0.3, 1.0)) * 4.0 * scale);
    // The cone's +Y runs along the ray; its base lies across it.
    let helper = select(vec3f(1.0, 0.0, 0.0), vec3f(0.0, 1.0, 0.0), abs(dir.y) < 0.999);
    let side = normalize(cross(helper, dir));
    let up = cross(side, dir);
    let local = side * (in.position.x * in.d.w) + dir * (in.position.y * in.e.x) + up * (in.position.z * in.d.w);
    let world = center + local * grow;
    let color = mix(in.c.rgb, in.d.rgb, min(t * 3.0, 1.0));
    let alpha = select(0.8 * max(0.0, 1.0 - (t - 0.25) / 0.35), 0.8, t < 0.25);
    return finish(in, world, color, alpha);
}

@vertex
fn vs_spark(in: VertexIn) -> VertexOut {
    let origin = in.a.xyz;
    let seconds = max(0.0, globals.time.x - in.a.w);
    let age = seconds * SPARK_AGE_PER_SECOND;
    let life = max(in.b.w, 0.0001);
    let frac = clamp(age / life, 0.0, 1.0);
    let center = origin + in.b.xyz * age + vec3f(0.0, 0.5 * SPARK_GRAVITY * age * age, 0.0);
    let view = globals.view;
    let cam_right = vec3f(view[0][0], view[1][0], view[2][0]);
    let cam_up = vec3f(view[0][1], view[1][1], view[2][1]);
    let world = center + (cam_right * in.position.x + cam_up * in.position.y) * in.c.w;
    return finish(in, world, mix(in.c.rgb, in.d.rgb, frac), 1.0 - frac);
}

@fragment
fn fs_fire(in: VertexOut) -> @location(0) vec4f {
    if in.alpha <= 0.0 {
        discard;
    }
    // The batch shader's `uLinearOutput` (set under an HDR pipeline).
    return vec4f(pow(clamp(in.color, vec3f(0.0), vec3f(1.0)), vec3f(2.2)), in.alpha);
}

@fragment
fn fs_spark(in: VertexOut) -> @location(0) vec4f {
    if in.alpha <= 0.0 {
        discard;
    }
    // The soft spark sprite: white centre through amber to a clear rim.
    let r = length(in.uv - 0.5) * 2.0;
    let white = vec4f(1.0, 1.0, 1.0, 1.0);
    let amber = vec4f(1.0, 0.902, 0.706, 0.8);
    let orange = vec4f(1.0, 0.588, 0.196, 0.3);
    let rim = vec4f(1.0, 0.392, 0.078, 0.0);
    var sprite: vec4f;
    if r < 0.3 {
        sprite = mix(white, amber, r / 0.3);
    } else if r < 0.7 {
        sprite = mix(amber, orange, (r - 0.3) / 0.4);
    } else {
        sprite = mix(orange, rim, clamp((r - 0.7) / 0.3, 0.0, 1.0));
    }
    return vec4f(pow(max(sprite.rgb * in.color, vec3f(0.0)), vec3f(2.2)), sprite.a * in.alpha);
}
