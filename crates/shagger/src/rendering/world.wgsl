// The world surfaces: base texture times baked light. The lighting follows
// SHAG's BspBakedLightingPlugin (StandardMaterial path, MIT snippet bodies):
// a layered lightmap page scaled by the map's gain, and with deluxe pages
// light * N.L + ambient * (1 - N.L). Vertex-lit faces use their scaled
// vertex colour. Colours stay in display space, as Babylon writes them.
// Each material pass adds its alpha test, image alpha and lighting mode
// (full bright for surfaces without a lightmap stage, as Q3's rgbGen
// identity draws them, and unlit for additive passes).
//
// Companion surfaces (BspMeshBuilder's loadTextures): a normal map bends
// the surface normal through the mesh's tangent frame (Babylon's bump
// include), and a gloss map with deluxe pages adds Qfusion's baked
// Blinn-Phong gloss (bspSpec). Surfaces with both maps draw as SHAG's
// generated specular-gloss PBR material, whose dynamic lights are GGX.
// A glow map adds as the emissive; with an animated glow stage it draws
// instead as an additive shine pass over the surface.
//
// loadTextures' other materials: the glow overlay ShaderMaterial (base x
// lightmap + glow), the filter blend ShaderMaterial (the image multiplied
// in), both writing display colour as Babylon's ShaderMaterials do; the
// vertex lighting plugin (vertex colour plus game lights); and tcGen
// environment, the StandardMaterial's spherical reflection map.

// The dynamic point lights (rendering::dynamic_lights): per light, position
// and range, diffuse, specular.
struct DynamicLights {
    count: vec4u,
    lights: array<vec4f, 48>,
};

struct Globals {
    view_proj: mat4x4f,
    // x: gain, y: 1 when deluxe pages exist
    lighting: vec4f,
    ambient: vec4f,
    eye: vec4f,
    fog: Fog,
    dynamic: DynamicLights,
    // The camera's view, for soft particles' view depth.
    view: mat4x4f,
    // Soft particles: near, far, fade rate, 1 when the scene depth is bound.
    soft: vec4f,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var lightmap: texture_2d_array<f32>;
@group(0) @binding(2) var lightmap_direction: texture_2d_array<f32>;
@group(0) @binding(3) var lightmap_sampler: sampler;
// The scene depth behind soft particles, copied after the opaque pass.
@group(2) @binding(0) var soft_depth: texture_depth_2d;
@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var base_sampler: sampler;

struct Material {
    // x: alpha cutoff (< 0: none), y: alpha from the image, z: lighting
    // (0 baked, 1 full bright, 2 vertex colour, 3 vertex colour and game
    // lights (BspVertexLightingPlugin), 4 glow overlay, 5 baked with the
    // base tinted by the vertex colour, as a lightmapped rgbGen vertex
    // StandardMaterial draws), w: times vertex alpha
    params: vec4f,
    // The texture matrix (Babylon's getTextureMatrix): U row, V row, origin.
    uv_rows: vec4f,
    uv_origin: vec4f,
    // rgbGen/alphaGen const and waves.
    color: vec4f,
    // Distance ramps: from, to, near, far (Q3DistanceRampPlugin).
    ramp_rgb: vec4f,
    ramp_alpha: vec4f,
    // x: normal map, y: gloss map, z: generated PBR material, w: glow map
    // as the emissive
    surface: vec4f,
    // x: baked gloss strength, y: gloss exponent, z: PBR roughness,
    // w: PBR reflectivity colour. A glow overlay: x 1 to mix its glow by
    // the glow's alpha (glowBlendMode).
    gloss: vec4f,
    // x: how world fog meets this pass (0 mix, 1 additive, 2 filter),
    // y: 1 for a ShaderMaterial, which writes display colour
    fog: vec4f,
    // x: 1 for a soft particle (Q3SoftParticlePlugin), y: 1 to fade its
    // colour instead of its alpha
    soft: vec4f,
    // x: the authored cube (Q3EnvironmentPlugin): 0 none, 1 its reflection
    // (cubemap), 2 along the view ray (surroundmap), 3 the base image as a
    // spherical reflection map (tcGen environment); y: 1 to turn the normal
    // to the side seen (TWOSIDEDLIGHTING); z: 1 when the material uses its
    // emissive as illumination
    environment: vec4f,
};
@group(1) @binding(2) var<uniform> material: Material;
@group(1) @binding(3) var normal_texture: texture_2d<f32>;
@group(1) @binding(4) var gloss_texture: texture_2d<f32>;
@group(1) @binding(5) var glow_texture: texture_2d<f32>;
@group(1) @binding(6) var environment_texture: texture_cube<f32>;
@group(1) @binding(7) var environment_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) uv2: vec2f,
    @location(4) color: vec4f,
    @location(5) lightmapped: f32,
    @location(6) sprite_center: vec4f,
    @location(7) sprite_frame: vec4f,
    @location(8) sprite_right: vec4f,
    @location(9) tangent: vec4f,
};

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) normal: vec3f,
    @location(1) uv: vec2f,
    @location(2) uv2: vec2f,
    @location(3) color: vec4f,
    @location(4) lightmapped: f32,
    @location(5) fade: vec2f,
    @location(6) world_position: vec3f,
    // bumpVertex's TBN columns (the world matrix is the identity)
    @location(7) tbn0: vec3f,
    @location(8) tbn1: vec3f,
    // The untransformed UV the companion maps read (their own texture
    // matrices are the identity).
    @location(9) uv0: vec2f,
};

// computeLighting's diffuse for every dynamic light: N.L times a linear
// falloff to the light's range.
fn dynamic_diffuse(position: vec3f, n: vec3f) -> vec3f {
    var sum = vec3f(0.0);
    for (var i = 0u; i < globals.dynamic.count.x; i++) {
        let data = globals.dynamic.lights[3u * i];
        let direction = data.xyz - position;
        let attenuation = max(0.0, 1.0 - length(direction) / data.w);
        let ndl = max(0.0, dot(n, normalize(direction)));
        sum += ndl * globals.dynamic.lights[3u * i + 1u].rgb * attenuation;
    }
    return sum;
}

// Q3DistanceRampPlugin: the eye's distance in front of the surface, eased.
fn ramp(r: vec4f, depth: f32) -> f32 {
    let t = clamp((depth - r.z) / max(r.w - r.z, 0.0001), 0.0, 1.0);
    return mix(r.x, r.y, t * t * (3.0 - 2.0 * t));
}

// Q3AutospritePlugin's vertex code (SHAG, MIT): right is horizontal,
// up = forward x right, and an axis sprite takes a negative cross product
// for the Q3-to-Babylon handedness. A sprite that turned also gives the
// direction it faces, which becomes its normal.
struct Billboard {
    position: vec3f,
    facing: vec3f,
};

fn autosprite(v: VertexIn) -> Billboard {
    var out = Billboard(v.position, vec3f(0.0));
    if (v.sprite_center.w <= 0.5) { return out; }
    let center = v.sprite_center.xyz;
    let to_eye = globals.eye.xyz - center;
    if (v.sprite_center.w < 1.5) {
        let distance = length(to_eye);
        if (distance >= 0.001) {
            let forward = to_eye / distance;
            var right = vec3f(forward.z, 0.0, -forward.x);
            let right_length = length(right);
            var up: vec3f;
            if (right_length < 0.001) {
                right = vec3f(1.0, 0.0, 0.0);
                up = vec3f(0.0, 0.0, -1.0);
            } else {
                right = right / right_length;
                up = vec3f(forward.y * right.z, forward.z * right.x - forward.x * right.z, -forward.y * right.x);
            }
            out.position = center + right * v.sprite_frame.x + up * v.sprite_frame.y;
            out.facing = forward;
        }
    } else {
        let axis = v.sprite_frame.xyz;
        var across = to_eye - axis * dot(to_eye, axis);
        let across_length = length(across);
        if (across_length >= 0.0001) {
            across = across / across_length;
            let right = vec3f(
                axis.z * across.y - axis.y * across.z,
                axis.x * across.z - axis.z * across.x,
                axis.y * across.x - axis.x * across.y);
            out.position = center + axis * v.sprite_frame.w + right * v.sprite_right.w;
            out.facing = across;
        }
    }
    return out;
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var o: VertexOut;
    let billboard = autosprite(v);
    var normal = v.normal;
    if (dot(billboard.facing, billboard.facing) > 0.0) { normal = billboard.facing; }
    o.clip = globals.view_proj * vec4f(billboard.position, 1.0);
    o.normal = normal;
    o.uv = v.uv.x * material.uv_rows.xy + v.uv.y * material.uv_rows.zw + material.uv_origin.xy;
    let depth = dot(globals.eye.xyz - billboard.position, normalize(normal));
    o.fade = vec2f(ramp(material.ramp_rgb, depth), ramp(material.ramp_alpha, depth));
    o.uv0 = v.uv;
    o.uv2 = v.uv2;
    o.color = v.color;
    o.lightmapped = v.lightmapped;
    o.world_position = billboard.position;
    let tbn_normal = normalize(normal);
    let tbn_tangent = normalize(v.tangent.xyz);
    o.tbn0 = tbn_tangent;
    o.tbn1 = cross(tbn_normal, tbn_tangent) * v.tangent.w;
    return o;
}

const PI: f32 = 3.141592653589793;
const EPSILON: f32 = 0.0000001;

// The generated PBR material's dynamic lights (pbrClusteredLightingFunctions
// with Babylon's defaults: Lambert diffuse at zero diffuse roughness,
// height-correlated Smith GGX, legacy specular energy conservation, the
// standard linear falloff since usePhysicalLightFalloff is off).
struct PbrLighting {
    diffuse: vec3f,
    specular: vec3f,
};

fn pbr_dynamic(position: vec3f, n: vec3f, v: vec3f, reflectivity: vec3f, roughness: f32) -> PbrLighting {
    var out = PbrLighting(vec3f(0.0), vec3f(0.0));
    let n_dot_v = abs(dot(n, v)) + EPSILON;
    let alpha_g = roughness * roughness + 0.0005;
    let a2 = alpha_g * alpha_g;
    for (var i = 0u; i < globals.dynamic.count.x; i++) {
        let data = globals.dynamic.lights[3u * i];
        let offset = data.xyz - position;
        let distance = length(offset);
        let attenuation = max(0.0, 1.0 - distance / data.w);
        let l = offset / max(distance, EPSILON);
        let h = normalize(v + l);
        let n_dot_l = clamp(dot(n, l), EPSILON, 1.0);
        let v_dot_h = clamp(dot(v, h), 0.0, 1.0);
        let n_dot_h = clamp(dot(n, h), EPSILON, 1.0);
        let color = globals.dynamic.lights[3u * i + 1u].rgb;
        out.diffuse += (1.0 / PI) * attenuation * n_dot_l * color;
        let fresnel = reflectivity + (vec3f(1.0) - reflectivity) * pow(1.0 - v_dot_h, 5.0);
        let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
        let distribution = a2 / (PI * d * d);
        let ggx_v = n_dot_l * sqrt(n_dot_v * (n_dot_v - a2 * n_dot_v) + a2);
        let ggx_l = n_dot_v * sqrt(n_dot_l * (n_dot_l - a2 * n_dot_l) + a2);
        let visibility = 0.5 / (ggx_v + ggx_l);
        out.specular += fresnel * distribution * visibility * attenuation * n_dot_l * color;
    }
    return out;
}

// StandardMaterial's computeLighting specular for every dynamic light
// (Blinn-Phong at the gloss exponent, the light's specular colour).
fn dynamic_specular(position: vec3f, n: vec3f, v: vec3f, glossiness: f32) -> vec3f {
    var sum = vec3f(0.0);
    for (var i = 0u; i < globals.dynamic.count.x; i++) {
        let data = globals.dynamic.lights[3u * i];
        let direction = data.xyz - position;
        let attenuation = max(0.0, 1.0 - length(direction) / data.w);
        let angle = normalize(v + normalize(direction));
        let spec = pow(max(0.0, dot(n, angle)), max(1.0, glossiness));
        sum += spec * globals.dynamic.lights[3u * i + 2u].rgb * attenuation;
    }
    return sum;
}

// World fog over the shaded surface (AtmosphericFogPlugin).
@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let color = soft_particle(in, environment(in, front, shade(in, front)));
    let fogged = atm_fog_apply(globals.fog, color.rgb, in.world_position, globals.eye.xyz, material.fog.x, false);
    if (material.fog.y > 0.5) {
        // A ShaderMaterial's display colour, fogged in display space and
        // written as it is.
        return vec4f(pow(max(fogged, vec3f(0.0)), vec3f(1.0 / 2.2)), color.a);
    }
    return vec4f(fogged, color.a);
}

// The spherical reflection coordinates Babylon computes (computeSphericalCoords), with the
// fragment's flip of v: the reflection of the view ray in view space.
fn sphere_map_uv(in: VertexOut, front: bool) -> vec2f {
    var n = normalize(in.normal);
    if (!front && material.environment.y > 0.5) { n = -n; }
    let view_dir = normalize((globals.view * vec4f(in.world_position, 1.0)).xyz);
    let view_normal = normalize((globals.view * vec4f(n, 0.0)).xyz);
    var r = reflect(view_dir, view_normal);
    r.z = r.z - 1.0;
    let m = max(2.0 * length(r), 0.000001);
    return vec2f(r.x / m + 0.5, r.y / m + 0.5);
}

// Q3EnvironmentPlugin (SHAG, MIT): an authored cube replaces the colour
// before fog, sampled along -viewDirectionW for surroundmap or its
// reflection about normalW for cubemap. The cube holds display colours;
// the HDR target takes linear ones.
fn environment(in: VertexOut, front: bool, color: vec4f) -> vec4f {
    if (material.environment.x < 0.5 || material.environment.x > 2.5) { return color; }
    let ray = normalize(in.world_position - globals.eye.xyz);
    var direction = ray;
    if (material.environment.x < 1.5) {
        var n = normalize(in.normal);
        if (!front && material.environment.y > 0.5) { n = -n; }
        direction = reflect(ray, n);
    }
    let sample = textureSampleLevel(environment_texture, environment_sampler, direction, 0.0).rgb;
    return vec4f(pow(max(sample, vec3f(0.0)), vec3f(2.2)), color.a);
}

// Q3SoftParticlePlugin: fade a soft surface where it nears the opaque scene
// behind it, over 1 / rate units of view depth.
fn soft_particle(in: VertexOut, color: vec4f) -> vec4f {
    let size = textureDimensions(soft_depth);
    let pixel = min(vec2u(in.clip.xy), size - vec2u(1u));
    let d = textureLoad(soft_depth, pixel, 0);
    if (material.soft.x < 0.5 || globals.soft.w < 0.5 || d >= 1.0) {
        return color;
    }
    let near = globals.soft.x;
    let far = globals.soft.y;
    let scene_z = near * far / (far - d * (far - near));
    let frag_z = (globals.view * vec4f(in.world_position, 1.0)).z;
    let fade = clamp((scene_z - frag_z) * globals.soft.z, 0.0, 1.0);
    let softness = 1.0 - pow(1.0 - fade, 4.0);
    if (material.soft.y > 0.5) {
        // Babylon fades the gamma-space colour; ours is linear.
        return vec4f(color.rgb * pow(softness, 2.2), color.a);
    }
    return vec4f(color.rgb, color.a * softness);
}

fn shade(in: VertexOut, front: bool) -> vec4f {
    let base = textureSample(base_texture, base_sampler, in.uv);
    // Sampled in uniform control flow; the masks below choose what counts.
    let bump = textureSample(normal_texture, base_sampler, in.uv0).xyz;
    let gloss_map = textureSample(gloss_texture, base_sampler, in.uv).rgb;
    let specular_map = textureSample(gloss_texture, base_sampler, in.uv0).rgb;
    let glow_sample = textureSample(glow_texture, base_sampler, in.uv0);
    let glow = glow_sample.rgb * material.surface.w;
    let reflection = textureSample(base_texture, base_sampler, sphere_map_uv(in, front)).rgb;
    let sphere = material.environment.x > 2.5;
    var alpha = material.color.a;
    if (material.params.y > 0.5) { alpha = alpha * base.a; }
    if (material.params.w > 0.5) { alpha = alpha * in.color.a; }
    if (material.params.x >= 0.0 && alpha < material.params.x) { discard; }
    let vertex_tinted = material.params.z > 4.5;
    var tint = material.color.rgb * in.fade.x;
    if (vertex_tinted && in.lightmapped > 0.5) { tint = tint * in.color.rgb; }
    if (material.params.z > 3.5 && !vertex_tinted) {
        // GlowOverlayShaderMaterial: the base lit by the raw lightmap page,
        // then the glow added (or mixed by its alpha).
        var lit = base.rgb;
        if (in.lightmapped > 0.5) {
            let layer = floor(in.uv2.x * 0.5);
            let uv = vec2f(in.uv2.x - layer * 2.0, in.uv2.y);
            lit = base.rgb * textureSampleLevel(lightmap, lightmap_sampler, uv, i32(layer), 0.0).rgb * globals.lighting.x;
        }
        var color = lit + glow_sample.rgb;
        if (material.gloss.x > 0.5) { color = mix(lit, glow_sample.rgb, glow_sample.a); }
        return vec4f(pow(max(color, vec3f(0.0)), vec3f(2.2)), base.a);
    }
    if (material.params.z > 2.5 && material.params.z < 3.5) {
        // BspVertexLightingPlugin: the vertex-lit image, plus the image lit
        // by the game lights, their specular and the emissive colour.
        var n = normalize(in.normal);
        if (material.surface.x > 0.5) {
            let tbn = mat3x3f(in.tbn0, in.tbn1, in.normal);
            n = normalize(tbn * (bump * 2.0 - 1.0));
        }
        if (!front) { n = -n; }
        let view = normalize(globals.eye.xyz - in.world_position);
        let image = base.rgb * tint;
        var specular = vec3f(0.0);
        if (material.surface.y > 0.5) {
            specular = dynamic_specular(in.world_position, n, view, material.gloss.y) * specular_map;
        }
        var emissive = vec3f(0.0);
        if (material.surface.w > 0.5) { emissive = vec3f(1.0) + glow; }
        let color = image * in.color.rgb + image * dynamic_diffuse(in.world_position, n) + specular + emissive;
        return vec4f(pow(max(color, vec3f(0.0)), vec3f(2.2)), alpha * in.fade.y);
    }
    if ((material.params.z > 0.5 && !vertex_tinted) || in.lightmapped < 0.5) {
        var light = in.color.rgb;
        var image = base.rgb;
        if (material.params.z < 1.5 && material.params.z > 0.5) {
            light = vec3f(1.0);
            // A fullbright surface's emissive is its glow image when it has
            // one, and its diffuse adds nothing with lighting disabled.
            if (material.surface.w > 0.5) { image = glow; }
        }
        var color = max(image * light * tint, vec3f(0.0));
        if (sphere) {
            // No diffuse image: an emissive-lit material shows its image
            // and the reflection, clamped; otherwise its fullbright base
            // (the vertex colour) and the reflection.
            if (material.environment.z > 0.5) {
                color = clamp(color + reflection, vec3f(0.0), vec3f(1.0));
            } else {
                color = light + reflection;
            }
        }
        return vec4f(pow(color, vec3f(2.2)), alpha * in.fade.y);
    }

    // normalW, bent by the normal map (perturbNormal), then turned to the
    // side seen.
    var n = normalize(in.normal);
    if (material.surface.x > 0.5) {
        let tbn = mat3x3f(in.tbn0, in.tbn1, in.normal);
        n = normalize(tbn * (bump * 2.0 - 1.0));
    }
    if (!front) { n = -n; }
    let view = normalize(globals.eye.xyz - in.world_position);

    // A layered page: the layer in lightmap U above 1, the page's U below it.
    let layer = floor(in.uv2.x * 0.5);
    let uv = vec2f(in.uv2.x - layer * 2.0, in.uv2.y);
    var light = textureSampleLevel(lightmap, lightmap_sampler, uv, i32(layer), 0.0).rgb * globals.lighting.x;
    var spec = vec3f(0.0);
    if (globals.lighting.y > 0.5) {
        let encoded = textureSampleLevel(lightmap_direction, lightmap_sampler, uv, i32(layer), 0.0).rgb - vec3f(0.5);
        let dir = encoded / max(length(encoded), 0.0001);
        let n_dot_l = max(dot(n, dir), 0.0);
        light = light * n_dot_l + globals.ambient.rgb * (1.0 - n_dot_l);
        if (material.surface.y > 0.5) {
            let half_vector = normalize(dir + view);
            spec = gloss_map * material.gloss.x * pow(max(dot(n, half_vector), 0.0), material.gloss.y);
        }
    }

    if (material.surface.z > 0.5) {
        // The generated PBR material: linear albedo and dynamic terms, the
        // baked terms multiplied in display space.
        let albedo = pow(base.rgb, vec3f(2.2)) * tint;
        let lit = pbr_dynamic(in.world_position, n, view, specular_map * material.gloss.w, material.gloss.z);
        let baked = pow(max(pow(max(albedo, vec3f(0.0)), vec3f(1.0 / 2.2)) * (light + spec), vec3f(0.0)), vec3f(2.2));
        let emissive = pow(glow, vec3f(2.2));
        let color = baked + max(lit.diffuse * albedo, vec3f(0.0)) + emissive + max(lit.specular, vec3f(0.0));
        return vec4f(clamp(color, vec3f(0.0), vec3f(30.0)), alpha * in.fade.y);
    }

    // StandardMaterial: game lights stay additive in diffuseBase, and a
    // gloss map is the specular colour (specularTexture replaces it).
    let diffuse_base = dynamic_diffuse(in.world_position, n);
    var final_specular = vec3f(0.0);
    if (material.surface.y > 0.5) {
        final_specular = dynamic_specular(in.world_position, n, view, material.gloss.y) * specular_map;
    }
    // With image processing as a post-process, default.fragment hands the
    // HDR target linear colour.
    var color = max(base.rgb * tint * (light + spec + diffuse_base) + final_specular + glow, vec3f(0.0));
    if (sphere) { color += reflection; }
    return vec4f(pow(color, vec3f(2.2)), alpha * in.fade.y);
}
