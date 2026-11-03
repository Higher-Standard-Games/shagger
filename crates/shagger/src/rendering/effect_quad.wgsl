// The shared half of SHAG's instanced quad effects (MIT): the uniforms
// Babylon's ShaderMaterial binds for them, one quad with its uv in [-1, 1]
// (or another template), up to five vec4s per instance, and the helpers each shader repeats.

struct Globals {
    view_proj: mat4x4f,
    view: mat4x4f,
    projection: mat4x4f,
    // xyz: cameraPosition
    camera: vec4f,
    // xy: uViewport, z: uTime
    viewport_time: vec4f,
    // World fog (ATM_FOG_SHADER_UNIFORMS); the eye is `camera`.
    fog: Fog,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct QuadIn {
    @location(0) position: vec3f,
    @location(1) uv: vec2f,
    @location(2) i0: vec4f,
    @location(3) i1: vec4f,
    @location(4) i2: vec4f,
    @location(5) i3: vec4f,
    @location(6) i4: vec4f,
};

fn safe_normalize(value: vec3f, fallback: vec3f) -> vec3f {
    let ls = dot(value, value);
    return select(fallback, value * inverseSqrt(ls), ls > 0.0000001);
}

// World units covered by one pixel at this depth.
fn world_per_pixel(world_position: vec3f) -> f32 {
    let view_depth = max(abs((globals.view * vec4f(world_position, 1.0)).z), 0.05);
    return (2.0 * view_depth) / max(abs(globals.projection[1][1]) * globals.viewport_time.y, 1.0);
}

fn camera_right() -> vec3f {
    let v = globals.view;
    return vec3f(v[0][0], v[1][0], v[2][0]);
}

fn camera_up() -> vec3f {
    let v = globals.view;
    return vec3f(v[0][1], v[1][1], v[2][1]);
}

// GLSL's mod: x - y * floor(x / y).
fn glsl_mod(x: f32, y: f32) -> f32 {
    return x - y * floor(x / y);
}

// GLSL's smoothstep, which also takes its edges reversed.
fn glsl_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn hash11(p_in: f32) -> f32 {
    var p = fract(p_in * 0.1031);
    p *= p + 33.33;
    p *= p + p;
    return fract(p);
}
