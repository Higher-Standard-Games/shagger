// SHAG src/engine/weapon/voltaic/VoltaicShaderMaterial.ts
// (IMPACT_VERTEX_SHADER, IMPACT_FRAGMENT_SHADER, WORLD_BATCH) in WGSL, after
// effect_quad.wgsl. Instance: i0 iStartSeed, i1 iEndImpact, i2 iColorLife,
// i3 iParams, i4 iNormal. No atmospheric fog.

const PI: f32 = 3.141592653589793;
const REMOTE_FADE_START: f32 = 0.15;
const REMOTE_REMOVE_TIME: f32 = 0.21;

struct ImpactOut {
    @builtin(position) position: vec4f,
    @location(0) impact_uv: vec2f,
    @location(1) impact_part: f32,
    @location(2) impact_kind: f32,
    @location(3) impact_energy: f32,
    @location(4) impact_color: vec3f,
    @location(5) state_seed: vec2f,
    @location(6) is_muzzle: f32,
    @location(7) is_spark: f32,
    @location(8) spark_life: f32,
};

fn sample_life(mode: f32, color_life_w: f32, duration: f32) -> f32 {
    if mode > 1.5 {
        return color_life_w;
    }
    let age = max(0.0, globals.viewport_time.z - color_life_w);
    if mode > 0.5 {
        return 1.0 - glsl_smoothstep(duration * 0.35, max(duration, 0.001), age);
    }
    return 1.0 - glsl_smoothstep(REMOTE_FADE_START, REMOTE_REMOVE_TIME, age);
}

@vertex
fn vs_main(in: QuadIn) -> ImpactOut {
    let start_seed = in.i0;
    let end_impact = in.i1;
    let color_life = in.i2;
    let params = in.i3;
    let time = globals.viewport_time.z;

    let mode = params.z;
    let hero = step(1.5, mode);
    let impact_kind = end_impact.w;
    let part = in.position.z;
    let spark = step(3.5, part);
    let muzzle = step(1.5, part) * (1.0 - spark);
    let sample_energy = sample_life(mode, color_life.w, params.w);
    let impact_life = sample_energy * step(0.5, impact_kind);
    let muzzle_life = sample_energy * hero;
    let life = mix(impact_life, muzzle_life, muzzle);
    let center = mix(end_impact.xyz, start_seed.xyz, muzzle);
    let pixel_scale = world_per_pixel(center);
    let endpoint_radius = clamp(
        mix(2.6, 9.0, hero),
        mix(6.0, 22.0, hero) * pixel_scale,
        mix(19.0, 72.0, hero) * pixel_scale,
    );
    // The corona reaches past the view model's silhouette.
    let muzzle_radius = clamp(8.4, 30.0 * pixel_scale, 84.0 * pixel_scale);
    let radius = mix(endpoint_radius, muzzle_radius, muzzle) * step(0.0001, life);

    let cam_right = safe_normalize(camera_right(), vec3f(1.0, 0.0, 0.0));
    let cam_up = safe_normalize(camera_up(), vec3f(0.0, 1.0, 0.0));
    var normal = mix(in.i4.xyz, start_seed.xyz - end_impact.xyz, muzzle);
    if length(normal) < 0.1 {
        normal = safe_normalize(globals.camera.xyz - center, vec3f(0.0, 1.0, 0.0));
    } else {
        normal = safe_normalize(normal, vec3f(0.0, 1.0, 0.0));
    }
    let reference = select(vec3f(1.0, 0.0, 0.0), vec3f(0.0, 1.0, 0.0), abs(normal.y) < 0.95);
    let surface_right = safe_normalize(cross(reference, normal), cam_right);
    let surface_up = safe_normalize(cross(normal, surface_right), cam_up);

    let oriented_part = glsl_mod(part, 2.0);
    var world_position: vec3f;
    var spark_life = 1.0;

    if spark > 0.5 {
        let spark_id = part - 4.0;
        let spark_seed = hash11(start_seed.w * 0.017 + spark_id * 19.19);
        // A continuous spray rather than stationary needles.
        let cycle = fract(time * mix(3.5, 4.6, hero) + spark_seed * 7.31);
        let flight_time = cycle * mix(0.26, 0.34, hash11(spark_seed * 31.7));
        let azimuth = hash11(spark_seed * 73.1 + 4.2) * 6.28318530718;
        let tangent = surface_right * cos(azimuth) + surface_up * sin(azimuth);
        let outward_speed = mix(32.0, 58.0, hash11(spark_seed * 11.3));
        let tangent_speed = mix(42.0, 78.0, hash11(spark_seed * 43.7));
        let velocity = normal * outward_speed + tangent * tangent_speed;
        let gravity = vec3f(0.0, -64.0, 0.0);
        let spark_origin = end_impact.xyz + normal * 0.18;
        let head = spark_origin + velocity * flight_time + gravity * (0.5 * flight_time * flight_time);
        let tail_time = max(0.0, flight_time - mix(0.085, 0.145, hash11(spark_seed * 97.3)));
        let tail = spark_origin + velocity * tail_time + gravity * (0.5 * tail_time * tail_time);
        let along = in.uv.x * 0.5 + 0.5;
        let streak_center = mix(tail, head, along);
        let streak_direction = safe_normalize(head - tail, tangent);
        let to_camera = safe_normalize(globals.camera.xyz - streak_center, cam_up);
        let streak_side = safe_normalize(cross(streak_direction, to_camera), cam_right);
        let spark_pixel_scale = world_per_pixel(streak_center);
        let spark_half_width = clamp(1.12, 6.00 * spark_pixel_scale, mix(10.4, 16.8, hero) * spark_pixel_scale);
        world_position = streak_center + streak_side * in.position.y * spark_half_width;

        // The hero shows all eight streaks, the remote profile four; misses
        // stay clean, and gated quads collapse before rasterisation.
        let profile_gate = mix(1.0 - step(3.5, spark_id), 1.0, hero);
        let contact_gate = step(0.5, impact_kind) * step(0.0001, sample_energy);
        let draw_gate = profile_gate * contact_gate;
        world_position = mix(end_impact.xyz, world_position, draw_gate);
        let fade = 1.0 - glsl_smoothstep(0.76, 0.98, cycle);
        let flicker = mix(0.62, 1.0, hash11(floor(time * 24.0) + spark_id * 13.0 + start_seed.w));
        spark_life = draw_gate * fade * flicker;
    } else {
        let billboard = oriented_part < 0.5;
        let right = select(surface_right, cam_right, billboard);
        let up = select(surface_up, cam_up, billboard);
        let offset_center = center + normal * select(0.12, 0.08, billboard);
        world_position = offset_center + (right * in.position.x + up * in.position.y) * radius;
    }

    var out: ImpactOut;
    out.position = globals.view_proj * vec4f(world_position, 1.0);
    out.impact_uv = in.uv;
    out.impact_part = oriented_part;
    out.impact_kind = impact_kind;
    out.impact_energy = params.x * life;
    out.impact_color = color_life.rgb;
    out.state_seed = vec2f(floor(time * mix(17.0, 22.0, hero) + start_seed.w * 0.013), start_seed.w);
    out.is_muzzle = muzzle;
    out.is_spark = spark;
    out.spark_life = spark_life;
    return out;
}

@fragment
fn fs_main(in: ImpactOut) -> @location(0) vec4f {
    let r = length(in.impact_uv);
    let angle = atan2(in.impact_uv.y, in.impact_uv.x);
    let aa = max(fwidth(r), 0.002);
    let state = in.state_seed.x;
    let seed = in.state_seed.y;
    let energy = max(in.impact_energy, 0.0);

    if in.is_spark > 0.5 {
        let across = abs(in.impact_uv.y);
        let along = in.impact_uv.x * 0.5 + 0.5;
        let filament = 1.0 - glsl_smoothstep(0.08, 1.0, across);
        let hot_head = glsl_smoothstep(0.52, 1.0, along);
        let tail_fade = glsl_smoothstep(0.0, 0.18, along);
        let shape = filament * tail_fade * mix(0.48, 1.35, hot_head);
        let ember = mix(vec3f(1.0, 0.16, 0.015), in.impact_color, 0.28);
        let spark_color = mix(ember, vec3f(1.0, 0.98, 0.76), hot_head);
        let alpha = clamp(shape * energy * in.spark_life * 0.96, 0.0, 0.98);
        return vec4f(spark_color * shape * energy * in.spark_life * 4.1 * alpha, alpha);
    }

    if in.is_muzzle > 0.5 {
        let muzzle_segment = floor(((angle + PI) / (2.0 * PI)) * 15.0);
        let muzzle_broken = step(0.28, hash11(muzzle_segment + state * 5.0 + seed * 0.31));
        let muzzle_nucleus = 1.0 - glsl_smoothstep(0.03, 0.23 + aa, r);
        let muzzle_halo = (1.0 - glsl_smoothstep(0.12, 0.94, r)) * 0.35;
        let muzzle_ring = 1.0 - glsl_smoothstep(0.025, 0.075 + aa, abs(r - 0.43));
        let muzzle_outer_ring = 1.0 - glsl_smoothstep(0.025, 0.065 + aa, abs(r - 0.69));
        let muzzle_needles = pow(abs(cos(angle * 7.0 + state * 0.81 + seed)), 44.0);
        let needle_envelope = glsl_smoothstep(0.18, 0.34, r) * (1.0 - glsl_smoothstep(0.62, 1.02, r));
        let muzzle_ticks = pow(abs(cos(angle * 13.0 - state * 0.37 + seed)), 70.0);
        let tick_envelope = glsl_smoothstep(0.46, 0.58, r) * (1.0 - glsl_smoothstep(0.76, 0.98, r));
        let camera_layer = 1.0 - step(0.5, in.impact_part);
        let muzzle_shape = mix(
            (muzzle_ring * 0.9 + muzzle_outer_ring * 0.55) * muzzle_broken
                + muzzle_needles * needle_envelope * 1.35 + muzzle_ticks * tick_envelope,
            muzzle_nucleus + muzzle_halo + muzzle_ring * muzzle_broken * 0.58
                + muzzle_outer_ring * muzzle_broken * 0.25 + muzzle_needles * needle_envelope * 0.35,
            camera_layer,
        );
        let muzzle_edge = mix(vec3f(0.22, 0.07, 1.0), in.impact_color, 0.72);
        let muzzle_color = mix(muzzle_edge, vec3f(1.0, 0.985, 0.88), max(muzzle_nucleus, muzzle_needles * 0.58));
        let muzzle_alpha = clamp(muzzle_shape * energy * 0.86, 0.0, 0.97);
        return vec4f(muzzle_color * muzzle_shape * energy * 2.9 * muzzle_alpha, muzzle_alpha);
    }

    let nucleus = 1.0 - glsl_smoothstep(0.04, 0.32 + aa, r);
    let halo = (1.0 - glsl_smoothstep(0.18, 1.0, r)) * 0.25;
    let crown = 1.0 - glsl_smoothstep(0.035, 0.085 + aa, abs(r - 0.50));
    let outer_crown = 1.0 - glsl_smoothstep(0.028, 0.065 + aa, abs(r - 0.73));
    let segment = floor(((angle + PI) / (2.0 * PI)) * 13.0);
    let broken = step(0.27, hash11(segment + state * 7.0 + seed));
    let rays = pow(abs(cos(angle * 7.0 + state * 0.71 + seed)), 38.0);
    let spike_envelope = glsl_smoothstep(0.20, 0.36, r) * (1.0 - glsl_smoothstep(0.48, 1.0, r));
    let spikes = rays * spike_envelope;
    let spark_rays = pow(abs(cos(angle * 11.0 - state * 0.43 + seed)), 64.0);
    let spark_envelope = glsl_smoothstep(0.42, 0.58, r) * (1.0 - glsl_smoothstep(0.72, 1.02, r));
    let sparks = spark_rays * spark_envelope * step(0.22, hash11(segment + state * 11.0 + seed));

    var color: vec3f;
    var shape: f32;
    var alpha_scale: f32;
    var hdr_scale: f32;
    if in.impact_part < 0.5 {
        shape = nucleus + halo;
        color = mix(vec3f(0.32, 0.10, 1.0), vec3f(1.0, 0.99, 0.90), nucleus);
        alpha_scale = 0.92;
        hdr_scale = 3.2;
    } else {
        let player = step(1.5, in.impact_kind);
        let contact_pulse = mix(1.0, 0.84 + 0.16 * sin(state * 2.31 + seed), player);
        shape = ((crown + outer_crown * 0.58) * broken * mix(1.0, 1.30, player)
            + spikes * mix(1.0, 1.55, player) + sparks * mix(0.85, 1.35, player)) * contact_pulse;
        let world_color = mix(in.impact_color, vec3f(1.0, 0.91, 0.42), crown);
        let player_color = mix(vec3f(0.20, 0.08, 1.0), vec3f(1.0, 0.98, 0.82), max(crown, spikes));
        color = mix(world_color, player_color, player);
        alpha_scale = mix(0.72, 0.88, player);
        hdr_scale = mix(1.9, 2.7, player);
    }

    let alpha = clamp(shape * energy * alpha_scale, 0.0, 0.97);
    let rgb = color * shape * energy * hdr_scale * alpha;
    return vec4f(rgb, alpha);
}
