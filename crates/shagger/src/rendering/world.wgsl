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
};
@group(1) @binding(2) var<uniform> material: Material;

struct VertexIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) uv2: vec2f,
    @location(4) color: vec4f,
    @location(5) lightmapped: f32,
};

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) normal: vec3f,
    @location(1) uv: vec2f,
    @location(2) uv2: vec2f,
    @location(3) color: vec4f,
    @location(4) lightmapped: f32,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var o: VertexOut;
    o.clip = globals.view_proj * vec4f(v.position, 1.0);
    o.normal = v.normal;
    o.uv = v.uv;
    o.uv2 = v.uv2;
    o.color = v.color;
    o.lightmapped = v.lightmapped;
    return o;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let base = textureSample(base_texture, base_sampler, in.uv);
    var alpha = 1.0;
    if (material.params.y > 0.5) { alpha = base.a; }
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
    return vec4f(base.rgb * light, alpha);
}
