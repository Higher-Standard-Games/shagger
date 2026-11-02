// The far box (SHAG `SkyboxLoader.loadSkybox`): a cube texture sampled, as
// Babylon's skybox mode samples it, with the direction from the eye in
// Babylon space. One triangle covers the screen, so the box never clips.

struct Sky {
    // The inverse of the view-projection without the eye's position.
    inverse_view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var sky_texture: texture_cube<f32>;
@group(0) @binding(2) var sky_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let ndc = vec2<f32>(f32((index << 1u) & 2u) * 2.0 - 1.0, f32(index & 2u) * 2.0 - 1.0);
    var out: VertexOut;
    out.clip = vec4<f32>(ndc, 1.0, 1.0);
    out.ndc = ndc;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let far = sky.inverse_view_proj * vec4<f32>(in.ndc, 0.5, 1.0);
    let direction = far.xyz / far.w;
    // The skybox is a StandardMaterial: linear output for the HDR target.
    let color = textureSample(sky_texture, sky_sampler, direction).rgb;
    return vec4<f32>(pow(max(color, vec3<f32>(0.0)), vec3<f32>(2.2)), 1.0);
}
