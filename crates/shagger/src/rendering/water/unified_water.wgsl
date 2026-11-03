// SHAG src/engine/rendering/water/UnifiedWaterMaterial.ts (VERTEX_SHADER,
// FRAGMENT_SHADER) in WGSL (MIT): one water material for every liquid
// surface. Follows rendering/fog.wgsl, which it is prepended with.
//
// The scene behind the water comes from copies of the HDR target taken
// after the opaque pass: its depth (hardware depth, `uDepthProjection.w` 1)
// and its linear colour (`uRefractionIsLinear` 1).

const MAX_WATER_LIGHTS: u32 = 4u;
const MAX_WATER_IMPULSES: u32 = 12u;

struct Water {
    view_proj: mat4x4f,
    view: mat4x4f,
    // xyz uCameraPosition, w uTime
    camera: vec4f,
    // xyz uCameraForward, w uCameraFar
    forward: vec4f,
    // xy uScreenSize, z uDebug, w uOutputLinear
    screen: vec4f,
    // rgb uShallowColor, w uAlpha
    shallow: vec4f,
    // rgb uDeepColor, w uMinAlpha
    deep: vec4f,
    // rgb uSkyColor, w uEmissive
    sky: vec4f,
    // xyz uSunDirection, w uSpecular
    sun: vec4f,
    // rgb uAbsorbTint, w uAbsorb
    absorb: vec4f,
    // uScale, uScroll, uWaveStrength, uReflectivity
    wave: vec4f,
    // uHasDepth, uHasRefraction, uRefractionIsLinear, uCalmWater
    flags: vec4f,
    // uHasEnvironment, uLocalProbe, uEnvironmentYSign, uEnvironmentIsLinear
    env: vec4f,
    // uFoam, uFoamWidth, uCaustics, uHighlightStrength
    foam: vec4f,
    // uRoughness, uReflectionStrength, uReflectionBlur, uDetailStrength
    tuning: vec4f,
    // uContactStrength, uHasActorReflection, uActorReflectionLinear, -
    actor_flags: vec4f,
    depth_projection: vec4f,
    probe_position: vec4f,
    probe_min: vec4f,
    probe_max: vec4f,
    light_mask_min: vec4f,
    light_mask_max: vec4f,
    actor_matrix: mat4x4f,
    // xy uActorReflectionTexel
    actor_texel: vec4f,
    lights: array<vec4f, 4>,
    light_colours: array<vec4f, 4>,
    impulses: array<vec4f, 24>,
    fog: Fog,
};

@group(0) @binding(0) var<uniform> u: Water;
@group(0) @binding(1) var noise_texture: texture_2d<f32>;
@group(0) @binding(2) var noise_sampler: sampler;
@group(0) @binding(3) var environment: texture_cube<f32>;
@group(0) @binding(4) var environment_sampler: sampler;
@group(0) @binding(5) var scene_depth: texture_depth_2d;
@group(0) @binding(6) var scene_colour: texture_2d<f32>;
@group(0) @binding(7) var capture_sampler: sampler;
@group(0) @binding(8) var light_visibility: texture_2d<f32>;
@group(0) @binding(9) var actor_reflection: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
};

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) view_depth: f32,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    // BSP world meshes are baked in world space: `position` IS the world
    // position, so adjacent meshes share one wave field.
    var out: VertexOut;
    out.world = v.position;
    out.normal = normalize(v.normal);
    // Babylon is left-handed: view-space Z forward, not negated.
    out.view_depth = (u.view * vec4f(v.position, 1.0)).z;
    out.clip = u.view_proj * vec4f(v.position, 1.0);
    return out;
}

fn capture(uv: vec2f) -> vec4f {
    return textureSampleLevel(scene_colour, capture_sampler, uv, 0.0);
}

// `sceneViewDepth`: view-space Z behind this pixel, 0 where nothing was drawn.
fn scene_view_depth(uv: vec2f) -> f32 {
    let size = vec2i(textureDimensions(scene_depth));
    let texel = clamp(vec2i(uv * vec2f(size)), vec2i(0), size - vec2i(1));
    let d = textureLoad(scene_depth, texel, 0);
    if (u.depth_projection.w < 0.5) { return d; }
    if (d >= 1.0) { return 0.0; }
    let n = u.depth_projection.x;
    let f = u.depth_projection.y;
    var z = d;
    if (u.depth_projection.z <= 0.5) { z = d * 2.0 - 1.0; }
    if (f <= 0.0) {
        if (u.depth_projection.z > 0.5) { return n / (1.0 - z); }
        return 2.0 * n / (1.0 - z);
    }
    if (u.depth_projection.z > 0.5) { return n * f / (f - z * (f - n)); }
    return 2.0 * n * f / (f + n - z * (f - n));
}

// `waterOutput`: world fog mixed in display space, then linear for the HDR
// target when image processing runs as a post-process.
fn water_output(world: vec3f, color_in: vec3f, alpha: f32) -> vec4f {
    let fog = atm_fog_evaluate(u.fog, world, u.camera.xyz, false);
    var color = mix(color_in, fog.rgb, fog.a);
    if (u.screen.w > 0.5) { color = pow(max(color, vec3f(0.0)), vec3f(2.2)); }
    return vec4f(color, alpha);
}

// `localReflection`: ray/box parallax for a room probe.
fn local_reflection(world: vec3f, direction: vec3f) -> vec3f {
    if (u.env.y < 0.5 || any(world < u.probe_min.xyz) || any(world > u.probe_max.xyz)) { return direction; }
    let safe_dir = mix(vec3f(-1.0), vec3f(1.0), step(vec3f(0.0), direction)) * max(abs(direction), vec3f(0.0001));
    let to_box = max((u.probe_min.xyz - world) / safe_dir, (u.probe_max.xyz - world) / safe_dir);
    let along = min(to_box.x, min(to_box.y, to_box.z));
    return world + direction * along - u.probe_position.xyz;
}

fn environment_linear(direction: vec3f) -> vec3f {
    let colour = textureSampleLevel(environment, environment_sampler, direction, 0.0).rgb;
    if (u.env.w > 0.5) { return colour; }
    return pow(max(colour, vec3f(0.0)), vec3f(2.2));
}

fn soft_environment(direction: vec3f) -> vec3f {
    let ray = normalize(direction);
    var colour = environment_linear(ray);
    if (u.flags.w > 0.5 && u.tuning.z > 0.0) {
        var axis = vec3f(1.0, 0.0, 0.0);
        if (abs(ray.y) < 0.95) { axis = vec3f(0.0, 1.0, 0.0); }
        let across = normalize(cross(ray, axis));
        let up = cross(across, ray);
        let spread = u.tuning.z * u.tuning.z * 0.12;
        colour = colour * 0.4 + 0.15 * (
            environment_linear(ray + across * spread) + environment_linear(ray - across * spread) +
            environment_linear(ray + up * spread) + environment_linear(ray - up * spread));
    }
    return pow(max(colour, vec3f(0.0)), vec3f(1.0 / 2.2));
}

fn noise(uv: vec2f) -> vec4f {
    return textureSample(noise_texture, noise_sampler, uv);
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let world = in.world;
    let time = u.camera.w;
    let scale = u.wave.x;
    let scroll = u.wave.y;
    let calm = u.flags.w > 0.5;
    let has_depth = u.flags.x > 0.5;
    let base = world.xz * scale;

    // Two layers, the second rotated so the pair never shares an axis.
    // GLSL's mat2(0.5403, -0.8415, 0.8415, 0.5403) is column-major.
    let swirl = mat2x2f(vec2f(0.5403, -0.8415), vec2f(0.8415, 0.5403));
    let uv1 = base + vec2f(time * scroll, time * scroll * 0.55);
    let uv2 = swirl * base * 2.3 - vec2f(time * scroll * 0.75, -time * scroll * 1.15);
    let n1 = noise(uv1).rgb * 2.0 - 1.0;
    let n2 = noise(uv2).rgb * 2.0 - 1.0;

    let second_slope = vec2f(0.5403 * n2.x - 0.8415 * n2.y, 0.8415 * n2.x + 0.5403 * n2.y);
    var slope = (n1.xy + second_slope * 0.6) * u.wave.z;
    if (calm) {
        let p = world.xz;
        let distance_to_eye = length(u.camera.xyz - world);
        let geometric_facing = abs(dot(normalize(u.camera.xyz - world), normalize(in.normal)));
        let detail = u.tuning.w * (1.0 - smoothstep(192.0, 768.0, distance_to_eye)) *
            smoothstep(0.03, 0.3, geometric_facing);
        var broad = vec2f(0.94, 0.34) * (0.11 * cos(dot(p, vec2f(0.94, 0.34)) * 0.039 - time * 0.28));
        broad += vec2f(-0.38, 0.925) * (0.075 * cos(dot(p, vec2f(-0.38, 0.925)) * 0.063 - time * 0.35));
        broad += vec2f(0.6, -0.8) * (0.045 * cos(dot(p, vec2f(0.6, -0.8)) * 0.09 - time * 0.41));
        broad += vec2f(-0.8, -0.6) * (0.1 * detail * cos(dot(p, vec2f(-0.8, -0.6)) * 0.18 - time * 0.6));
        slope = broad + slope * detail;
    }
    var reactive_slope = vec2f(0.0);
    for (var i = 0u; i < MAX_WATER_IMPULSES; i++) {
        let origin = u.impulses[i * 2u];
        let wave = u.impulses[i * 2u + 1u];
        if (wave.x <= 0.0) { break; }
        let delta = world.xz - origin.xz;
        let distance_from_origin = length(delta);
        let phase = distance_from_origin - origin.w * (38.0 + wave.x * 25.0);
        let decay = (1.0 - smoothstep(0.25, 2.0, origin.w)) / (1.0 + origin.w * 0.8);
        let envelope = exp(-phase * phase / 140.0) * decay;
        let boundary = 1.0 - smoothstep(wave.y * 0.7, max(wave.y, 0.001), distance_from_origin);
        let same_plane = 1.0 - step(3.0, abs(world.y - origin.y));
        if (dot(wave.zw, wave.zw) > 0.01) {
            let direction = normalize(wave.zw);
            let side = vec2f(-direction.y, direction.x);
            let trail = -dot(delta, direction);
            let across = dot(delta, side);
            let wake_phase = abs(across) - max(0.0, trail) * 0.45 - origin.w * 18.0;
            let wake_envelope = exp(-wake_phase * wake_phase / 60.0) *
                smoothstep(-5.0, 8.0, trail) * max(0.0, 1.0 - origin.w / 2.0);
            let gradient = side * sign(across) + direction * 0.45;
            reactive_slope += gradient * cos(wake_phase * 0.55) * wake_envelope * boundary * wave.x * same_plane * 0.32;
        } else {
            let trailing_phase = phase + 16.0;
            let trailing = cos(trailing_phase * 0.38) * exp(-trailing_phase * trailing_phase / 220.0) *
                decay * smoothstep(0.12, 0.4, origin.w) * 0.3;
            reactive_slope += delta / max(distance_from_origin, 1.0) *
                (cos(phase * 0.42) * envelope + trailing) * boundary * wave.x * same_plane * 0.28;
        }
    }
    reactive_slope *= 0.65 / (0.65 + length(reactive_slope));
    slope += reactive_slope;
    let geometric_normal = normalize(in.normal);
    var tangent_axis = cross(vec3f(0.0, 1.0, 0.0), geometric_normal);
    if (abs(geometric_normal.y) > 0.9) { tangent_axis = vec3f(1.0, 0.0, 0.0); }
    let tangent = normalize(tangent_axis);
    let bitangent = normalize(cross(tangent, geometric_normal));
    var n = normalize(geometric_normal + tangent * slope.x + bitangent * slope.y);
    if (!front) { n = -n; }

    let view_dir = normalize(u.camera.xyz - world);
    let facing = clamp(dot(view_dir, n), 0.0, 1.0);
    let flat_fresnel = pow(1.0 - facing, 4.0) * u.wave.w;

    let reflection_direction = reflect(-view_dir, n);
    var environment_sample = u.sky.rgb;
    if (u.env.x > 0.5) {
        var cube_direction = local_reflection(world, reflection_direction);
        cube_direction.y *= u.env.z;
        environment_sample = soft_environment(cube_direction);
    }

    var reflected = environment_sample;
    var actor_colour = vec3f(0.0);
    var actor_coverage = 0.0;
    if (u.actor_flags.y > 0.5 && front) {
        let projected = u.actor_matrix * vec4f(world, 1.0);
        let uv = projected.xy / max(projected.w, 0.0001) * 0.5 + 0.5;
        let distance_fade = 1.0 - smoothstep(140.0, 240.0, distance(u.camera.xyz, world));
        let texel = u.actor_texel.xy;
        let margin = texel * 6.0;
        if (projected.w > 0.0 && all(uv > margin) && all(uv < vec2f(1.0) - margin)) {
            let coverage = textureSampleLevel(actor_reflection, capture_sampler, uv, 0.0).a;
            let softness = mix(0.65, 4.0, smoothstep(32.0, 200.0, distance(u.camera.xyz, world)));
            let step_uv = texel * softness;
            let warped = uv + clamp(slope, vec2f(-0.6), vec2f(0.6)) * texel * 2.0;
            var actor = textureSampleLevel(actor_reflection, capture_sampler, warped, 0.0) * 0.3;
            actor += textureSampleLevel(actor_reflection, capture_sampler, warped + vec2f(step_uv.x, 0.0), 0.0) * 0.175;
            actor += textureSampleLevel(actor_reflection, capture_sampler, warped - vec2f(step_uv.x, 0.0), 0.0) * 0.175;
            actor += textureSampleLevel(actor_reflection, capture_sampler, warped + vec2f(0.0, step_uv.y), 0.0) * 0.175;
            actor += textureSampleLevel(actor_reflection, capture_sampler, warped - vec2f(0.0, step_uv.y), 0.0) * 0.175;
            actor_colour = actor.rgb / max(actor.a, 0.0001);
            if (u.actor_flags.z > 0.5) { actor_colour = pow(max(actor_colour, vec3f(0.0)), vec3f(1.0 / 2.2)); }
            actor_coverage = min(coverage, actor.a) * distance_fade * 0.8;
        }
    }
    let environment_fresnel = min(0.95, 0.04 + 0.96 * pow(1.0 - facing, 5.0));
    var fresnel = mix(flat_fresnel, environment_fresnel, u.env.x);
    if (calm) { fresnel = min(0.95, fresnel * u.tuning.y); }
    if (actor_coverage > 0.0) {
        let actor_fresnel = max(fresnel, min(0.95, 0.18 * u.tuning.y));
        let room_weight = fresnel * (1.0 - actor_coverage);
        let actor_weight = actor_fresnel * actor_coverage;
        fresnel = room_weight + actor_weight;
        reflected = (environment_sample * room_weight + actor_colour * actor_weight) / max(fresnel, 0.0001);
    }

    // How much liquid the view ray crosses, per channel.
    var thickness = 0.0;
    var valid_contact_depth = 0.0;
    var absorbed_rgb = clamp(u.absorb.rgb * 0.35, vec3f(0.0), vec3f(1.0));
    var transmittance = vec3f(1.0) - absorbed_rgb;
    let screen_uv = in.clip.xy / u.screen.xy;
    if (has_depth) {
        let scene_depth_here = scene_view_depth(screen_uv);
        let ray_axis = max(0.05, dot(normalize(world - u.camera.xyz), normalize(u.forward.xyz)));
        if (scene_depth_here > 0.0) { thickness = max(0.0, scene_depth_here - in.view_depth) / ray_axis; }
        if (scene_depth_here > in.view_depth + 0.05) { valid_contact_depth = 1.0; }
        transmittance = exp(-thickness * u.absorb.w * u.absorb.rgb);
        absorbed_rgb = vec3f(1.0) - transmittance;
    }
    let absorbed = dot(absorbed_rgb, vec3f(0.2126, 0.7152, 0.0722));

    var contact = 0.0;
    if (calm && front && has_depth && u.actor_flags.x > 0.0) {
        let band = smoothstep(0.1, 0.6, thickness) * (1.0 - smoothstep(0.6, 4.0, thickness));
        contact = band * u.actor_flags.x * valid_contact_depth;
    }

    var foam = 0.0;
    if (has_depth && u.foam.x > 0.0) {
        let band = 1.0 - smoothstep(0.0, u.foam.y, thickness);
        let churn = 0.65 + 0.35 * (n1.x + n2.y);
        foam = clamp(band * churn, 0.0, 1.0) * u.foam.x;
    }

    var caustics = 0.0;
    if (has_depth && u.foam.z > 0.0) {
        let ray_dir = normalize(world - u.camera.xyz);
        let floor_pos = world + ray_dir * thickness;
        let cuv = floor_pos.xz * scale * 0.55;
        let c1 = textureSampleLevel(noise_texture, noise_sampler, cuv + vec2f(time * scroll * 0.35, time * scroll * 0.2), 0.0).b;
        let c2 = textureSampleLevel(noise_texture, noise_sampler, cuv * 1.45 - vec2f(time * scroll * 0.25, -time * scroll * 0.4), 0.0).b;
        let ridge = max(0.0, 1.0 - abs(c1 - c2) * 5.0);
        let reach = transmittance.g * min(1.0, thickness / 12.0);
        caustics = pow(ridge, 7.0) * reach * u.foam.z;
    }

    let debug = u.screen.z;
    if (debug > 2.5) {
        if (debug > 5.5) {
            var colour = capture(screen_uv).rgb;
            if (u.flags.z > 0.5) { colour = pow(max(colour, vec3f(0.0)), vec3f(1.0 / 2.2)); }
            return water_output(world, select(vec3f(1.0, 0.0, 0.0), colour, u.flags.y > 0.5), 1.0);
        }
        if (debug > 4.5) {
            return water_output(world, vec3f(0.5) + vec3f(reactive_slope.x, 0.0, reactive_slope.y) * 2.0, 1.0);
        }
        if (debug > 3.5) {
            return water_output(world, select(vec3f(1.0, 0.0, 0.0), environment_sample, u.env.y > 0.5), 1.0);
        }
        return water_output(world, select(vec3f(1.0, 0.0, 0.0), environment_sample, u.env.x > 0.5), 1.0);
    }
    if (debug > 0.5) {
        if (!has_depth) { return vec4f(1.0, 0.0, 0.0, 1.0); }
        if (debug < 1.5) { return water_output(world, absorbed_rgb, 1.0); }
        var band: vec3f;
        if (thickness <= 0.0) { band = vec3f(1.0, 0.0, 1.0); }
        else if (thickness < 8.0) { band = vec3f(1.0, 0.0, 0.0); }
        else if (thickness < 32.0) { band = vec3f(1.0, 1.0, 0.0); }
        else if (thickness < 128.0) { band = vec3f(0.0, 1.0, 0.0); }
        else if (thickness < 512.0) { band = vec3f(0.0, 1.0, 1.0); }
        else { band = vec3f(1.0, 1.0, 1.0); }
        return water_output(world, band, 1.0);
    }

    let body = mix(u.shallow.rgb, u.deep.rgb, absorbed_rgb);
    let contact_shade = 1.0 - contact * 0.25;
    var surface_sheen = vec3f(0.0);
    if (calm) { surface_sheen = u.sky.rgb * (0.07 * clamp(dot(slope, slope) * 8.0, 0.0, 1.0)); }

    var light_highlights = vec3f(0.0);
    if (calm && front && u.foam.w > 0.0) {
        let mask_uv = (world.xz - u.light_mask_min.xz) / max(u.light_mask_max.xz - u.light_mask_min.xz, vec2f(1.0));
        let visibility = textureSampleLevel(light_visibility, capture_sampler, clamp(mask_uv, vec2f(0.0), vec2f(1.0)), 0.0);
        let roughness = u.tuning.x;
        let exponent = max(2.0, 2.0 / (roughness * roughness) - 2.0);
        for (var i = 0u; i < MAX_WATER_LIGHTS; i++) {
            let source = u.lights[i];
            if (source.w <= 0.0) { break; }
            let delta_light = source.xyz - world;
            let distance_light = length(delta_light);
            let to_light = delta_light / max(distance_light, 0.001);
            let half_vector = normalize(to_light + view_dir);
            let attenuation = pow(max(0.0, 1.0 - distance_light / source.w), 2.0);
            let light_fresnel = 0.04 + 0.96 * pow(1.0 - max(0.0, dot(view_dir, half_vector)), 5.0);
            let lobe = (exponent + 2.0) / 8.0 * pow(max(0.0, dot(n, half_vector)), exponent);
            var light_colour = u.light_colours[i].rgb;
            if (u.light_colours[i].w > 0.5) {
                var panel_ray = source.xyz - u.probe_position.xyz;
                panel_ray.y *= u.env.z;
                var panel_colour = textureSampleLevel(environment, environment_sampler, panel_ray, 0.0).rgb;
                if (u.env.w > 0.5) { panel_colour = pow(max(panel_colour, vec3f(0.0)), vec3f(1.0 / 2.2)); }
                light_colour *= panel_colour * u.env.y;
            }
            light_highlights += light_colour * visibility[i] * attenuation *
                max(0.0, dot(n, to_light)) * light_fresnel * lobe * u.foam.w;
        }
    }

    if (u.flags.y > 0.5 && has_depth && calm && front) {
        let view_slope = (u.view * vec4f(n - geometric_normal, 0.0)).xy;
        var offset = view_slope * vec2f(u.screen.y / u.screen.x, 1.0) * 0.035;
        offset *= smoothstep(0.0, 32.0, thickness);
        // Screen UV runs down in wgpu where Babylon's GL coordinates run up.
        var refracted_uv = screen_uv + vec2f(offset.x, -offset.y);
        let margin = vec2f(2.0) / u.screen.xy;
        let displaced_depth = scene_view_depth(clamp(refracted_uv, margin, vec2f(1.0) - margin));
        if (any(refracted_uv < margin) || any(refracted_uv > vec2f(1.0) - margin) ||
            displaced_depth <= in.view_depth + 0.5) {
            refracted_uv = screen_uv;
        }
        var transmitted_scene = capture(refracted_uv).rgb;
        if (u.flags.z > 0.5) { transmitted_scene = pow(max(transmitted_scene, vec3f(0.0)), vec3f(1.0 / 2.2)); }
        let through_water = transmitted_scene * transmittance + body * (vec3f(1.0) - transmittance);
        let surface_colour = reflected * fresnel + through_water * (1.0 - fresnel) + surface_sheen + light_highlights;
        return water_output(world, surface_colour * contact_shade, 1.0);
    }

    // Sun glint: a broad lobe that sells "wet" when the camera stands still.
    let halfway = normalize(u.sun.xyz + view_dir);
    let spec = pow(clamp(dot(n, halfway), 0.0, 1.0), 24.0) * u.sun.w;

    let body_alpha = clamp(absorbed, u.deep.w, u.shallow.w);
    var alpha = fresnel + (1.0 - fresnel) * body_alpha;
    var contribution = reflected * fresnel + body * body_alpha * (1.0 - fresnel);
    contribution += surface_sheen + light_highlights;
    contribution += (vec3f(spec) + body * u.sky.w + u.shallow.rgb * caustics) * alpha;
    contribution = mix(contribution, vec3f(1.0), foam);
    alpha = mix(alpha, 1.0, foam);
    contribution *= contact_shade;
    return water_output(world, contribution / max(alpha, 0.0001), alpha);
}
