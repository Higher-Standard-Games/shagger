// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, Copyright Babylon.js contributors, licensed
// under the Apache License 2.0 (see ../LICENSE):
// packages/dev/core/src/Shaders/default.fragment.fx and default.vertex.fx
// (StandardMaterial), with ShadersInclude/lightsFragmentFunctions.fx
// (computeLighting for directional lights, computeHemisphericLighting),
// fresnelFunction.fx (computeFresnelTerm) and defaultFragmentAlpha.fx.
//
// Changes: rewritten in WGSL for the terms SHAG's procedural pickups use:
// diffuse and specular from hemispheric and directional lights, emissive
// colour and texture, emissive and opacity Fresnel, a diffuse texture with
// its alpha, an RGB opacity texture, the ambient colour, and the
// linear output image processing as a post-process asks for. No bump, reflection, refraction, lightmap, fog or
// shadows. The draw's emissive and diffuse overrides, texture matrix and
// texture level stand in for the per-frame material writes SHAG makes.

struct Light {
    // x: 0 none, 1 hemispheric, 2 directional
    kind: vec4f,
    direction: vec4f,
    diffuse: vec4f,
    ground: vec4f,
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
    // unused (image processing runs as a post-process)
    image: vec4f,
    lights: array<Light, 4>,
    dynamic: DynamicLights,
};

struct Draw {
    world: mat4x4f,
    tint: vec4f,
    outline: vec4f,
    skin: vec4u,
    // rgb replaces the material's emissive colour when w is 1.
    emissive: vec4f,
    // The texture matrix (getTextureMatrix): u row in xy, v row in zw.
    uv_rows: vec4f,
    // xy: the matrix origin; z: the texture's level.
    uv_origin: vec4f,
};

struct Material {
    diffuse: vec4f,
    // rgb: specular colour, a: specular power (glossiness)
    specular: vec4f,
    emissive: vec4f,
    // rgb: material ambient times the scene's
    ambient: vec4f,
    // x: alpha, y: lighting on, z: texture use bits, w: emissive as
    // illumination
    params: vec4f,
    // Fresnel parameters: left rgb + power, right rgb + bias.
    emissive_left: vec4f,
    emissive_right: vec4f,
    // x: left luminance, y: right luminance, z: bias, w: power
    opacity_parts: vec4f,
    // x: emissive Fresnel on, y: opacity Fresnel on
    fresnel: vec4f,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> draw: Draw;
@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var tex_sampler: sampler;
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

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    let world = draw.world * vec4f(in.position, 1.0);
    out.clip = globals.view_proj * world;
    out.world_position = world.xyz;
    out.normal = (draw.world * vec4f(in.normal, 0.0)).xyz;
    out.uv = in.uv;
    return out;
}

// Texture use bits (params.z).
const USE_DIFFUSE: u32 = 1u;
const USE_EMISSIVE: u32 = 2u;
const USE_OPACITY_RGB: u32 = 4u;
const ALPHA_FROM_DIFFUSE: u32 = 8u;

fn compute_fresnel_term(view_direction: vec3f, world_normal: vec3f, bias: f32, power: f32) -> f32 {
    let term = pow(bias + abs(dot(view_direction, world_normal)), power);
    return clamp(term, 0.0, 1.0);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let view_direction = normalize(globals.eye.xyz - in.world_position);
    let normal = normalize(in.normal);
    let uses = u32(material.params.z);
    let uv = in.uv.x * draw.uv_rows.xy + in.uv.y * draw.uv_rows.zw + draw.uv_origin.xy;
    let sampled = textureSample(tex, tex_sampler, uv);
    // Texture.level scales what the texture gives (diffuse, emissive and
    // opacity read the same texture here).
    let texel = vec4f(sampled.rgb * draw.uv_origin.z, sampled.a);

    var base_color = vec4f(1.0);
    var alpha = material.params.x;
    if (uses & USE_DIFFUSE) != 0u {
        base_color = texel;
        if (uses & ALPHA_FROM_DIFFUSE) != 0u {
            alpha *= base_color.a;
        }
    }

    let glossiness = material.specular.a;
    var diffuse_base = vec3f(0.0);
    var specular_base = vec3f(0.0);
    if material.params.y > 0.5 {
        for (var i = 0; i < 4; i++) {
            let light = globals.lights[i];
            if light.kind.x == 1.0 {
                // computeHemisphericLighting
                let ndl = dot(normal, light.direction.xyz) * 0.5 + 0.5;
                diffuse_base += mix(light.ground.rgb, light.diffuse.rgb, ndl);
                let angle = normalize(view_direction + light.direction.xyz);
                let spec = pow(max(0.0, dot(normal, angle)), max(1.0, glossiness));
                specular_base += spec * light.specular.rgb;
            } else if light.kind.x == 2.0 {
                // computeLighting, directional
                let l = normalize(-light.direction.xyz);
                let ndl = max(0.0, dot(normal, l));
                diffuse_base += ndl * light.diffuse.rgb;
                let angle = normalize(view_direction + l);
                let spec = pow(max(0.0, dot(normal, angle)), max(1.0, glossiness));
                specular_base += spec * light.specular.rgb;
            }
        }
        // The cluster's point lights: computeLighting with a linear falloff
        // to each light's range.
        for (var i = 0u; i < globals.dynamic.count.x; i++) {
            let data = globals.dynamic.lights[3u * i];
            let direction = data.xyz - in.world_position;
            let attenuation = max(0.0, 1.0 - length(direction) / data.w);
            let l = normalize(direction);
            let ndl = max(0.0, dot(normal, l));
            diffuse_base += ndl * globals.dynamic.lights[3u * i + 1u].rgb * attenuation;
            let angle = normalize(view_direction + l);
            let spec = pow(max(0.0, dot(normal, angle)), max(1.0, glossiness));
            specular_base += spec * globals.dynamic.lights[3u * i + 2u].rgb * attenuation;
        }
    }

    // defaultFragmentAlpha
    if (uses & USE_OPACITY_RGB) != 0u {
        let o = texel.rgb * vec3f(0.3, 0.59, 0.11);
        alpha *= o.x + o.y + o.z;
    }
    if material.fresnel.y > 0.5 {
        let p = material.opacity_parts;
        let term = compute_fresnel_term(view_direction, normal, p.z, p.w);
        alpha += p.x * (1.0 - term) + term * p.y;
    }

    var emissive = select(material.emissive.rgb, draw.emissive.rgb, draw.emissive.w > 0.5);
    if (uses & USE_EMISSIVE) != 0u {
        emissive += texel.rgb;
    }
    if material.fresnel.x > 0.5 {
        let term = compute_fresnel_term(view_direction, normal, material.emissive_right.a, material.emissive_left.a);
        emissive *= material.emissive_left.rgb * (1.0 - term) + term * material.emissive_right.rgb;
    }

    // A tint replaces the diffuse colour (an rgbGen on a diffuse stage).
    let diffuse_color = select(material.diffuse.rgb, draw.tint.rgb, draw.tint.w > 0.5);
    let final_specular = specular_base * material.specular.rgb;
    var color: vec3f;
    if material.params.w > 0.5 {
        // EMISSIVEASILLUMINATION
        let final_diffuse = clamp(diffuse_base * diffuse_color + material.ambient.rgb, vec3f(0.0), vec3f(1.0)) * base_color.rgb;
        color = clamp(final_diffuse + final_specular + emissive, vec3f(0.0), vec3f(1.0));
    } else {
        let final_diffuse = clamp(diffuse_base * diffuse_color + emissive + material.ambient.rgb, vec3f(0.0), vec3f(1.0)) * base_color.rgb;
        color = final_diffuse + final_specular;
    }

    // Image processing runs as a post-process (`applyByPostProcess`), so
    // default.fragment only takes the colour to linear space for the HDR
    // target.
    color = pow(max(color, vec3f(0.0)), vec3f(2.2));
    return vec4f(color, clamp(alpha, 0.0, 1.0));
}
