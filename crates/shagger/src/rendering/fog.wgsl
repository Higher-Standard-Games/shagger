// Warfork's atmospheric fog (SHAG AtmosphericFogGlsl.ts, from Warfork's
// include/fog.glsl with Quake Z read as engine Y): the fog colour and its
// weight between the eye and a point. Prepended to every shader that fogs;
// the values come from rendering::world_fog_controller::FogUniforms.

struct Fog {
    // rgb, a: 1 when there is fog
    color: vec4f,
    // x: start distance, y: sky height, w: density
    dist: vec4f,
    // clear, full, 1 / (full - clear), w: 1 with a height band
    height: vec4f,
    // horizon bias, horizon scale, zenith falloff, w: 1 to tint the sky
    sky: vec4f,
    // rgb, w: exponent
    sun_color: vec4f,
    // direction, w: intensity
    sun_params: vec4f,
};

// How a pipeline's blend meets the fog (`fogPassKind`), for shaders that
// set it per pipeline: 0 mix, 1 additive, 2 filter.
override FOG_KIND: f32 = 0.0;

// `atmFogEvaluate`; `on_sky` is ATMFOG_SKY, the point on Warfork's sky box.
fn atm_fog_evaluate(fog: Fog, world_pos_in: vec3f, eye: vec3f, on_sky: bool) -> vec4f {
    if (fog.color.a <= 0.0001) { return vec4f(0.0); }
    var world_pos = world_pos_in;
    let dir = world_pos - eye;
    var dist = length(dir);
    if (dist <= 0.0001) { return vec4f(0.0); }
    let view_dir = dir / dist;
    if (on_sky) {
        dist = fog.dist.y / max(max(abs(view_dir.x), abs(view_dir.y)), abs(view_dir.z));
        world_pos = eye + view_dir * dist;
    }
    var factor = 1.0 - exp(-max(0.0, dist - fog.dist.x) * fog.dist.w);
    if (fog.height.w > 0.5) { factor *= clamp((world_pos.y - fog.height.x) * fog.height.z, 0.0, 1.0); }
    factor = clamp(factor, 0.0, 1.0);
    var fog_color = fog.color.rgb;
    if (fog.sun_params.w > 0.0) {
        let sun_dot = max(0.0, dot(view_dir, fog.sun_params.xyz));
        fog_color += fog.sun_color.rgb * pow(sun_dot, fog.sun_color.w) * fog.sun_params.w * factor;
    }
    if (fog.sky.w > 0.5) {
        let zenith = max(0.0, view_dir.y + fog.sky.x);
        fog_color = mix(fog_color, fog.color.rgb, clamp(pow(zenith, fog.sky.z) * fog.sky.y, 0.0, 1.0));
    }
    return vec4f(fog_color, factor * fog.color.a);
}

// The plugin's apply: linear colour (image processing as a post-process)
// moved to display space, fogged by the pass kind (0 mix, 1 additive: fade,
// 2 filter: toward white), and back.
fn atm_fog_apply(fog: Fog, linear: vec3f, world_pos: vec3f, eye: vec3f, kind: f32, on_sky: bool) -> vec3f {
    if (fog.color.a <= 0.0001) { return linear; }
    let f = atm_fog_evaluate(fog, world_pos, eye, on_sky);
    var c = pow(max(linear, vec3f(0.0)), vec3f(1.0 / 2.2));
    if (kind > 1.5) {
        c = mix(c, vec3f(1.0), f.a);
    } else if (kind > 0.5) {
        c *= 1.0 - f.a;
    } else {
        c = mix(c, f.rgb, f.a);
    }
    return pow(max(c, vec3f(0.0)), vec3f(2.2));
}
