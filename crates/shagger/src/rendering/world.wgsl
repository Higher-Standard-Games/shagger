// The world surfaces: base texture times baked light. The lighting follows
// SHAG's BspBakedLightingPlugin (StandardMaterial path, MIT snippet bodies):
// a layered lightmap page scaled by the map's gain, and with deluxe pages
// light * N.L + ambient * (1 - N.L). Vertex-lit faces use their scaled
// vertex colour. Colours stay in display space, as Babylon writes them.
// Each material pass adds its alpha test, image alpha and lighting mode
// (full bright for surfaces without a lightmap stage, as Q3's rgbGen
// identity draws them, and unlit for additive passes).

struct Globals {
    view_proj: mat4x4f,
    // x: gain, y: 1 when deluxe pages exist
    lighting: vec4f,
    ambient: vec4f,
    eye: vec4f,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var lightmap: texture_2d_array<f32>;
@group(0) @binding(2) var lightmap_direction: texture_2d_array<f32>;
@group(0) @binding(3) var lightmap_sampler: sampler;
@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var base_sampler: sampler;

struct Material {
    // x: alpha cutoff (< 0: none), y: alpha from the image, z: lighting
    // (0 baked, 1 full bright, 2 vertex colour), w: times vertex alpha
    params: vec4f,
    // The texture matrix (Babylon's getTextureMatrix): U row, V row, origin.
    uv_rows: vec4f,
    uv_origin: vec4f,
    // rgbGen/alphaGen const and waves.
    color: vec4f,
    // Distance ramps: from, to, near, far (Q3DistanceRampPlugin).
    ramp_rgb: vec4f,
    ramp_alpha: vec4f,
};
@group(1) @binding(2) var<uniform> material: Material;

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
};

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) normal: vec3f,
    @location(1) uv: vec2f,
    @location(2) uv2: vec2f,
    @location(3) color: vec4f,
    @location(4) lightmapped: f32,
    @location(5) fade: vec2f,
};

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
    o.uv2 = v.uv2;
    o.color = v.color;
    o.lightmapped = v.lightmapped;
    return o;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let base = textureSample(base_texture, base_sampler, in.uv);
    var alpha = material.color.a;
    if (material.params.y > 0.5) { alpha = alpha * base.a; }
    if (material.params.w > 0.5) { alpha = alpha * in.color.a; }
    if (material.params.x >= 0.0 && alpha < material.params.x) { discard; }
    var light = in.color.rgb;
    if (material.params.z > 1.5) {
        light = in.color.rgb;
    } else if (material.params.z > 0.5) {
        light = vec3f(1.0);
    } else if (in.lightmapped > 0.5) {
        // A layered page: the layer in lightmap U above 1, the page's U below it.
        let layer = floor(in.uv2.x * 0.5);
        let uv = vec2f(in.uv2.x - layer * 2.0, in.uv2.y);
        light = textureSampleLevel(lightmap, lightmap_sampler, uv, i32(layer), 0.0).rgb * globals.lighting.x;
        if (globals.lighting.y > 0.5) {
            let encoded = textureSampleLevel(lightmap_direction, lightmap_sampler, uv, i32(layer), 0.0).rgb - vec3f(0.5);
            let dir = encoded / max(length(encoded), 0.0001);
            var n = normalize(in.normal);
            if (!front) { n = -n; }
            let n_dot_l = max(dot(n, dir), 0.0);
            light = light * n_dot_l + globals.ambient.rgb * (1.0 - n_dot_l);
        }
    }
    // SHAG's BSP materials are StandardMaterials; with image processing as a
    // post-process, default.fragment hands the HDR target linear colour.
    let color = max(base.rgb * light * material.color.rgb * in.fade.x, vec3f(0.0));
    return vec4f(pow(color, vec3f(2.2)), alpha * in.fade.y);
}
