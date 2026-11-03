// SHAG src/engine/weapon/voltaic/VoltaicShaderMaterial.ts (BEAM_VERTEX_SHADER,
// BEAM_FRAGMENT_SHADER, WORLD_BATCH) in WGSL, after effect_quad.wgsl.
// Instance: i0 iStartSeed, i1 iEndImpact, i2 iColorLife, i3 iParams. The hero
// beam draws as a batch of one, its uniforms as the instance.

const REMOTE_FADE_START: f32 = 0.15;
const REMOTE_REMOVE_TIME: f32 = 0.21;

struct BeamOut {
    @builtin(position) position: vec4f,
    @location(0) side: f32,
    @location(1) part: f32,
    @location(2) energy: f32,
    @location(3) life: f32,
    @location(4) volume_weight: f32,
    @location(5) color: vec3f,
    // vAtmWorld, for world fog.
    @location(6) atm_world: vec3f,
};

fn linear_noise(x: f32, seed: f32) -> f32 {
    let cell = floor(x);
    let f = fract(x);
    let a = hash11(cell + seed);
    let b = hash11(cell + 1.0 + seed);
    return mix(a, b, f) * 2.0 - 1.0;
}

fn bolt_field(distance_along: f32, state: f32, seed: f32) -> f32 {
    let coarse = linear_noise(distance_along / 92.0, seed + state * 17.0);
    let fine = linear_noise(distance_along / 27.0, seed * 3.1 + state * 41.0);
    return coarse + fine * 0.34;
}

// Welded to the muzzle, then the field peels away almost at once.
fn endpoint_envelope(t: f32) -> f32 {
    let enter = glsl_smoothstep(0.008, 0.14, t);
    let exit = 1.0 - glsl_smoothstep(0.92, 1.0, t);
    let final_third = mix(0.72, 1.0, glsl_smoothstep(0.14, 0.68, t));
    return enter * exit * final_third;
}

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

fn view_right() -> vec3f {
    return safe_normalize(camera_right(), vec3f(1.0, 0.0, 0.0));
}

fn view_up() -> vec3f {
    return safe_normalize(camera_up(), vec3f(0.0, 1.0, 0.0));
}

fn hybrid_half_width(world_position: vec3f, world_width: f32, min_pixels: f32, max_pixels: f32) -> f32 {
    let pixel_scale = world_per_pixel(world_position);
    return clamp(world_width, min_pixels * pixel_scale, max_pixels * pixel_scale);
}

@vertex
fn vs_main(in: QuadIn) -> BeamOut {
    let start = in.i0.xyz;
    let end = in.i1.xyz;
    let seed = in.i0.w;
    let intensity = in.i3.x;
    let amplitude = in.i3.y;
    let mode = in.i3.z;
    let life = sample_life(mode, in.i2.w, in.i3.w);
    let hero = step(1.5, mode);
    let time = globals.viewport_time.z;

    let axis = end - start;
    let beam_length = max(length(axis), 0.001);
    let v = globals.view;
    let view_forward = safe_normalize(vec3f(v[0][2], v[1][2], v[2][2]), vec3f(0.0, 0.0, 1.0));
    let direction = safe_normalize(axis, view_forward);
    // Parts above 15 are the hero's second sheet.
    let volume_layer = step(15.5, in.position.z);
    let part = in.position.z - volume_layer * 16.0;
    let local_t = in.position.x;
    var rate = mix(17.0, 22.0, hero);
    if mode > 0.5 && mode < 1.5 {
        rate = 20.0;
    }
    let electrical_time = time * rate + seed * 0.013;
    let state = floor(electrical_time);
    let state_phase = fract(electrical_time);

    var t = local_t;
    var stream_offset = 0.0;
    var depth_offset = 0.0;
    var energy = 1.0;
    var field_state = state;
    let part_seed = seed + part * 71.17;

    if part > 5.5 {
        let fork_selector = hash11(state + seed * 0.37 + part * 9.0);
        let fork_start = mix(0.42, 0.82, hash11(state * 2.3 + part_seed));
        let fork_span = mix(0.12, 0.24, hash11(state * 5.1 + part_seed));
        t = min(0.96, fork_start + local_t * fork_span);
        let anchor_distance = fork_start * beam_length;
        // Forks branch from the dominant stream's own field.
        let anchor = bolt_field(anchor_distance, state, seed + 2.0 * 71.17);
        let fork_sign = select(-1.0, 1.0, glsl_mod(floor(part), 2.0) < 0.5);
        let fork_noise = bolt_field(local_t * fork_span * beam_length, state, part_seed);
        stream_offset = anchor * endpoint_envelope(fork_start) * amplitude
            + fork_sign * pow(local_t, 0.72) * amplitude * mix(0.58, 1.12, fork_selector)
            + fork_noise * amplitude * 0.18 * local_t;
        depth_offset = fork_noise * amplitude * 0.12;
        let probability = mix(0.72, 0.95, hero);
        energy = step(1.0 - probability, fork_selector) * (1.0 - glsl_smoothstep(0.72, 1.0, local_t));
    } else {
        if part > 4.5 && part < 5.5 {
            field_state = state - 1.0;
        }
        let distance_along = t * beam_length;
        let field = bolt_field(distance_along, field_state, part_seed);
        let depth_field = bolt_field(distance_along + 31.0, field_state, part_seed + 211.0);
        let envelope = endpoint_envelope(t);

        if part < 0.5 {
            stream_offset = field * amplitude * envelope * 0.18;
            depth_offset = depth_field * amplitude * envelope * 0.08;
            energy = mix(0.30, 0.42, hero);
        } else if part < 1.5 {
            stream_offset = 0.0;
            depth_offset = 0.0;
            energy = 1.0;
        } else {
            var amplitude_scale = 1.0;
            if part > 2.5 && part < 3.5 {
                amplitude_scale = 0.72;
            }
            if part > 3.5 && part < 4.5 {
                amplitude_scale = 0.54;
            }
            if part > 4.5 && part < 5.5 {
                amplitude_scale = 0.88;
            }
            stream_offset = field * amplitude * envelope * amplitude_scale;
            depth_offset = depth_field * amplitude * envelope * amplitude_scale * 0.22;
            if part > 2.5 && part < 4.5 {
                let window = hash11(floor(t * 11.0) + state * 3.0 + part_seed);
                energy *= glsl_smoothstep(0.24, 0.48, window);
            }
            if part > 4.5 && part < 5.5 {
                energy *= 1.0 - glsl_smoothstep(0.0, 0.82, state_phase);
            }
        }
    }

    let straight_center = mix(start, end, t);
    let to_camera = safe_normalize(globals.camera.xyz - straight_center, -view_forward);
    var screen_side = cross(direction, to_camera);
    let side_length = length(screen_side);
    var fallback_side = cross(direction, vec3f(0.0, 1.0, 0.0));
    if dot(fallback_side, fallback_side) < 0.000001 {
        fallback_side = cross(direction, vec3f(1.0, 0.0, 0.0));
    }
    fallback_side = safe_normalize(fallback_side, view_right());
    let billboard_side = select(fallback_side, screen_side / side_length, side_length > 0.001);

    // The hero beam is nearly collinear with the camera ray: pin its field
    // to camera-right projected onto the beam plane.
    var hero_side = view_right() - direction * dot(view_right(), direction);
    if dot(hero_side, hero_side) < 0.000001 {
        hero_side = view_up() - direction * dot(view_up(), direction);
    }
    hero_side = safe_normalize(hero_side, fallback_side);
    screen_side = select(billboard_side, hero_side, hero > 0.5);
    let screen_depth = safe_normalize(cross(direction, screen_side), to_camera);

    let displacement_pixel_cap = mix(12.0, 52.0, hero) * world_per_pixel(straight_center);
    stream_offset = clamp(stream_offset, -displacement_pixel_cap, displacement_pixel_cap);
    depth_offset = clamp(depth_offset, -displacement_pixel_cap * 0.35, displacement_pixel_cap * 0.35);
    let center = straight_center + screen_side * stream_offset + screen_depth * depth_offset;

    var half_width: f32;
    if part < 0.5 {
        half_width = hybrid_half_width(center, mix(1.8, 8.0, hero), mix(4.0, 22.0, hero), mix(18.0, 78.0, hero));
    } else if part < 1.5 {
        half_width = hybrid_half_width(center, mix(0.48, 1.04, hero), mix(3.4, 7.2, hero), mix(7.2, 14.5, hero));
    } else {
        half_width = hybrid_half_width(center, mix(0.24, 0.76, hero), mix(1.6, 4.7, hero), mix(4.0, 15.0, hero));
    }

    // The hero's two sheets cross at ±45 degrees about the axis.
    let volume_sign = mix(-1.0, 1.0, volume_layer);
    let crossed_side = safe_normalize(
        screen_side * 0.70710678 + screen_depth * volume_sign * 0.70710678,
        screen_side,
    );
    let ribbon_side = safe_normalize(mix(screen_side, crossed_side, hero), screen_side);
    let world_position = center + ribbon_side * in.position.y * half_width;

    var out: BeamOut;
    out.position = globals.view_proj * vec4f(world_position, 1.0);
    out.atm_world = world_position;
    out.side = in.position.y;
    out.part = part;
    out.energy = energy * intensity;
    out.life = life;
    // The sheets overlap at the core; keep it hot without doubling it.
    out.volume_weight = mix(1.0, 0.72, hero);
    out.color = in.i2.rgb;
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
    var hot = 1.0 - glsl_smoothstep(0.12 - aa, 0.22 + aa, d);
    var body = 1.0 - glsl_smoothstep(0.52 - aa, 0.78 + aa, d);
    var fringe = 1.0 - glsl_smoothstep(0.78 - aa, 1.0 + aa, d);

    var color = in.color;
    var alpha_scale = 0.72;
    var hdr_scale = 1.0;
    if in.part < 0.5 {
        color = mix(vec3f(0.08, 0.18, 1.0), vec3f(0.46, 0.10, 1.0), glsl_smoothstep(0.0, 1.0, d));
        hot = 0.0;
        body *= 0.32;
        fringe *= 0.56;
        alpha_scale = 0.20;
        hdr_scale = 0.70;
    } else if in.part < 1.5 {
        color = mix(in.color, vec3f(1.0, 0.98, 0.86), 0.88);
        alpha_scale = 0.96;
        hdr_scale = 2.8;
    } else if in.part > 4.5 && in.part < 5.5 {
        color = vec3f(1.0, 0.24, 0.025);
        alpha_scale = 0.34;
        hdr_scale = 1.15;
    } else {
        color = mix(in.color, vec3f(1.0, 0.94, 0.55), hot * 0.42);
        alpha_scale = select(0.72, 0.48, in.part > 5.5);
        hdr_scale = select(1.7, 1.35, in.part > 5.5);
    }

    let shape = hot + body * 0.62 + fringe * 0.22;
    let energy = max(in.energy * in.life * in.volume_weight, 0.0);
    let alpha = clamp(shape * energy * alpha_scale, 0.0, 0.96);
    let rgb = color * shape * energy * hdr_scale * alpha;
    return vec4f(rgb, alpha);
}
