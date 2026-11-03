// Models: glTF meshes drawn as Babylon's PBRMaterial draws them with no
// environment texture (MIT; written from Babylon's pbr shader includes, not
// copied). Base colour is sampled in display space and taken to linear with
// toLinearSpace (pow 2.2); each scene light adds its diffuse share:
//   hemispheric: mix(ground, diffuse, dot(N, L) * 0.5 + 0.5)
//   directional: diffuse * NdotL * the diffuse BRDF
// and its GGX specular share (computeSpecularLighting: Schlick fresnel from
// F0 = mix(0.04, albedo, metallic) to 1, Trowbridge-Reitz distribution and
// height-correlated Smith visibility, times NdotL and the light colour;
// hemispheric lights use their half-Lambert NdotL there too).
// The cluster's dynamic point lights add theirs with an inverse-square
// falloff. The sum times the albedo stays linear, clamped to 30, for the HDR
// target and its image processing. Unlit materials skip the lights.
//
// A skinned draw (a player) moves each vertex by its four weighted joints
// from the frame's joint palette, as Babylon's bonesVertex include does. A
// draw's tint replaces the material's albedo colour (SHAG's
// `applyVisualStyle`). Outlines are Babylon's OutlineRenderer: the mesh
// pushed out along its normals in bind space by the outline width, in one
// flat colour.

struct Light {
    // x: 0 none, 1 hemispheric, 2 directional
    kind: vec4f,
    // Hemispheric: the normalized up direction. Directional: the direction
    // the light travels.
    direction: vec4f,
    // Colour times intensity.
    diffuse: vec4f,
    // Hemispheric ground colour times intensity.
    ground: vec4f,
    // Specular colour times intensity (StandardMaterial only).
    specular: vec4f,
};

// The dynamic point lights (rendering::dynamic_lights): per light, position
// and range, diffuse, specular.
struct DynamicLights {
    count: vec4u,
    lights: array<vec4f, 48>,
};

struct Globals {
    view_proj: mat4x4f,
    eye: vec4f,
    // z: diffuse model (0 Lambert, 1 Burley); x, y unused
    image: vec4f,
    lights: array<Light, 4>,
    fog: Fog,
    dynamic: DynamicLights,
};

struct Draw {
    world: mat4x4f,
    // rgb replaces the albedo colour when w is 1.
    tint: vec4f,
    // rgb: outline colour, w: outline width in bind space.
    outline: vec4f,
    // x: the draw's first joint in the palette.
    skin: vec4u,
    // Emissive: a StandardMaterial's override, added to a lit material's
    // light. Texture matrix and level: StandardMaterial only.
    emissive: vec4f,
    uv_rows: vec4f,
    uv_origin: vec4f,
};

struct Material {
    base_color: vec4f,
    // x: metallic, y: roughness, z: 1 unlit, w: alpha
    params: vec4f,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> draw: Draw;
@group(1) @binding(1) var<storage, read> joints: array<mat4x4f>;
@group(2) @binding(0) var base_texture: texture_2d<f32>;
@group(2) @binding(1) var base_sampler: sampler;
@group(2) @binding(2) var<uniform> material: Material;

struct VertexIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
};

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) world_position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
};

struct SkinIn {
    @location(3) joints: vec4u,
    @location(4) weights: vec4f,
};

fn skin_matrix(skin: SkinIn) -> mat4x4f {
    let base = draw.skin.x;
    return joints[base + skin.joints.x] * skin.weights.x
        + joints[base + skin.joints.y] * skin.weights.y
        + joints[base + skin.joints.z] * skin.weights.z
        + joints[base + skin.joints.w] * skin.weights.w;
}

fn shade_vertex(in: VertexIn, model: mat4x4f) -> VertexOut {
    var out: VertexOut;
    let world = model * vec4f(in.position, 1.0);
    out.clip = globals.view_proj * world;
    out.world_position = world.xyz;
    // The world matrices here scale uniformly or mirror an axis, and the
    // joints barely scale, so the upper 3x3 turns normals as the inverse
    // transpose would.
    out.normal = (model * vec4f(in.normal, 0.0)).xyz;
    out.uv = in.uv;
    return out;
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    return shade_vertex(in, draw.world);
}

@vertex
fn vs_skinned(in: VertexIn, skin: SkinIn) -> VertexOut {
    return shade_vertex(in, draw.world * skin_matrix(skin));
}

fn outline_vertex(in: VertexIn, model: mat4x4f) -> VertexOut {
    var pushed = in;
    pushed.position = in.position + in.normal * draw.outline.w;
    return shade_vertex(pushed, model);
}

@vertex
fn vs_outline(in: VertexIn) -> VertexOut {
    return outline_vertex(in, draw.world);
}

@vertex
fn vs_outline_skinned(in: VertexIn, skin: SkinIn) -> VertexOut {
    return outline_vertex(in, draw.world * skin_matrix(skin));
}

@fragment
fn fs_outline() -> @location(0) vec4f {
    return vec4f(draw.outline.rgb, material.base_color.a * material.params.w);
}

const PI: f32 = 3.141592653589793;

fn pow5(x: f32) -> f32 {
    let x2 = x * x;
    return x2 * x2 * x;
}

fn diffuse_brdf(n_dot_l: f32, n_dot_v: f32, v_dot_h: f32, roughness: f32) -> f32 {
    if globals.image.z < 0.5 {
        return 1.0 / PI;
    }
    // diffuseBRDF_Burley
    let fresnel90 = 0.5 + 2.0 * v_dot_h * v_dot_h * roughness;
    let fl = 1.0 + (fresnel90 - 1.0) * pow5(clamp(1.0 - n_dot_v, 1e-6, 1.0));
    let fv = 1.0 + (fresnel90 - 1.0) * pow5(clamp(1.0 - n_dot_l, 1e-6, 1.0));
    return fl * fv / PI;
}

// computeSpecularLighting, with convertRoughnessToAverageSlope's
// minimum variance.
fn specular_brdf(n: vec3f, v: vec3f, l: vec3f, n_dot_l: f32, n_dot_v: f32, roughness: f32, f0: vec3f) -> vec3f {
    let h = normalize(v + l);
    let n_dot_h = clamp(dot(n, h), 1e-6, 1.0);
    let v_dot_h = clamp(dot(v, h), 0.0, 1.0);
    let alpha_g = roughness * roughness + 0.0005;
    let a2 = alpha_g * alpha_g;
    let fresnel = f0 + (vec3f(1.0) - f0) * pow5(1.0 - v_dot_h);
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    let distribution = a2 / (PI * d * d);
    let ggx_v = n_dot_l * sqrt(n_dot_v * (n_dot_v - a2 * n_dot_v) + a2);
    let ggx_l = n_dot_v * sqrt(n_dot_l * (n_dot_l - a2 * n_dot_l) + a2);
    let visibility = 0.5 / (ggx_v + ggx_l);
    return fresnel * distribution * visibility * n_dot_l;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let texel = textureSample(base_texture, base_sampler, in.uv);
    let albedo_color = select(material.base_color.rgb, draw.tint.rgb, draw.tint.w > 0.5);
    let albedo = pow(texel.rgb, vec3f(2.2)) * albedo_color;
    let alpha = material.base_color.a * material.params.w;
    var color: vec3f;
    if material.params.z > 0.5 {
        color = albedo;
    } else {
        var n = normalize(in.normal);
        if !front {
            n = -n;
        }
        let v = normalize(globals.eye.xyz - in.world_position);
        let n_dot_v = clamp(abs(dot(n, v)), 1e-6, 1.0);
        var diffuse = vec3f(0.0);
        var specular = vec3f(0.0);
        let roughness = material.params.y;
        let f0 = mix(vec3f(0.04), albedo, material.params.x);
        for (var i = 0; i < 4; i++) {
            let light = globals.lights[i];
            if light.kind.x == 1.0 {
                let ndl = clamp(dot(n, light.direction.xyz) * 0.5 + 0.5, 1e-6, 1.0);
                diffuse += mix(light.ground.rgb, light.diffuse.rgb, ndl);
                let l = normalize(light.direction.xyz);
                specular += specular_brdf(n, v, l, ndl, n_dot_v, roughness, f0) * light.diffuse.rgb;
            } else if light.kind.x == 2.0 {
                let l = normalize(-light.direction.xyz);
                let h = normalize(v + l);
                let ndl = clamp(dot(n, l), 1e-6, 1.0);
                let vdh = clamp(dot(v, h), 0.0, 1.0);
                diffuse += light.diffuse.rgb * ndl * diffuse_brdf(ndl, n_dot_v, vdh, roughness);
                specular += specular_brdf(n, v, l, ndl, n_dot_v, roughness, f0) * light.diffuse.rgb;
            }
        }
        // The cluster's point lights: physical (inverse square) falloff,
        // inside the range the cluster culls them to.
        for (var i = 0u; i < globals.dynamic.count.x; i++) {
            let data = globals.dynamic.lights[3u * i];
            let offset = data.xyz - in.world_position;
            let distance_squared = dot(offset, offset);
            if distance_squared >= data.w * data.w {
                continue;
            }
            let l = normalize(offset);
            let h = normalize(v + l);
            let ndl = clamp(dot(n, l), 1e-6, 1.0);
            let vdh = clamp(dot(v, h), 0.0, 1.0);
            let attenuation = 1.0 / max(distance_squared, 1e-6);
            let light_color = globals.dynamic.lights[3u * i + 1u].rgb * attenuation;
            diffuse += light_color * ndl * diffuse_brdf(ndl, n_dot_v, vdh, roughness);
            specular += specular_brdf(n, v, l, ndl, n_dot_v, roughness, f0) * light_color;
        }
        color = diffuse * albedo * (1.0 - material.params.x) + specular;
    }
    // A draw's emissive (a powerup glow), added as PBR adds its own.
    if draw.emissive.w > 0.5 {
        color += draw.emissive.rgb;
    }
    // Image processing runs as a post-process (`applyByPostProcess`), so the
    // output stays linear: pbr.fragment's clamp to 30 for the HDR target.
    color = clamp(color, vec3f(0.0), vec3f(30.0));
    // World fog (AtmosphericFogPlugin), by the pipeline's blend.
    color = atm_fog_apply(globals.fog, color, in.world_position, globals.eye.xyz, FOG_KIND, false);
    return vec4f(color, alpha);
}
