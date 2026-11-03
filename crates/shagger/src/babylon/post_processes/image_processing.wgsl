// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, Copyright Babylon.js contributors, licensed
// under the Apache License 2.0 (see ../LICENSE): packages/dev/core/src/Shaders/
// imageProcessing.fragment.fx and ShadersInclude/imageProcessingFunctions.fx
// (applyImageProcessing) and helperFunctions.fx (toGammaSpace), in WGSL.
//
// Changes: FROMLINEARSPACE with IMAGEPROCESSING, as DefaultRenderingPipeline
// runs it on an HDR target: exposure, the vignette (multiply or opaque), to
// gamma, saturate, contrast. No white balance, tone mapping, colour grading,
// colour curves or dithering. The target is read texel for texel.

struct Settings {
    // x: exposureLinear, y: contrast, z: 1 vignette, w: 1 vignette opaque
    // (else multiply)
    params: vec4f,
    vignette_settings1: vec4f,
    vignette_settings2: vec4f,
    // xy: vInverseScreenSize
    inverse_screen: vec4f,
    // SHAG's colour correction (ColorCorrectionPostProcess): the matrix's
    // rows; color_rows[0].w is 1 when set.
    color_rows: array<vec4f, 3>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> settings: Settings;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let ndc = vec2f(f32((index << 1u) & 2u) * 2.0 - 1.0, f32(index & 2u) * 2.0 - 1.0);
    return vec4f(ndc, 0.0, 1.0);
}

const GAMMA_ENCODE_POWER_APPROX: f32 = 1.0 / 2.2;

@fragment
fn fs_main(@builtin(position) frag: vec4f) -> @location(0) vec4f {
    var result = textureLoad(source, vec2i(frag.xy), 0);
    result = vec4f(max(result.rgb, vec3f(0.0)), result.a);
    result = vec4f(result.rgb * settings.params.x, result.a);
    if settings.params.z > 0.5 {
        // gl_FragCoord runs from the bottom left in WebGL.
        let size = 1.0 / settings.inverse_screen.xy;
        let frag_gl = vec2f(frag.x, size.y - frag.y);
        var viewport_xy = frag_gl * settings.inverse_screen.xy;
        viewport_xy = viewport_xy * 2.0 - 1.0;
        let vignette_xy1 = vec3f(viewport_xy * settings.vignette_settings1.xy + settings.vignette_settings1.zw, 1.0);
        let vignette_term = dot(vignette_xy1, vignette_xy1);
        let vignette = pow(vignette_term, settings.vignette_settings2.w);
        let vignette_color = settings.vignette_settings2.rgb;
        if settings.params.w > 0.5 {
            result = vec4f(mix(vignette_color, result.rgb, vignette), result.a);
        } else {
            let multiplier = mix(vignette_color, vec3f(1.0), vignette);
            result = vec4f(result.rgb * multiplier, result.a);
        }
    }
    var rgb = pow(result.rgb, vec3f(GAMMA_ENCODE_POWER_APPROX));
    rgb = clamp(rgb, vec3f(0.0), vec3f(1.0));
    let contrast = settings.params.y;
    let high = rgb * rgb * (3.0 - 2.0 * rgb);
    if contrast < 1.0 {
        rgb = mix(vec3f(0.5), rgb, contrast);
    } else {
        rgb = mix(rgb, high, contrast - 1.0);
    }
    rgb = max(rgb, vec3f(0.0));
    if settings.color_rows[0].w > 0.5 {
        // The matrices hold for linear light; the frame here is display-encoded.
        let linear = pow(clamp(rgb, vec3f(0.0), vec3f(1.0)), vec3f(2.2));
        let mapped = clamp(
            vec3f(
                dot(settings.color_rows[0].xyz, linear),
                dot(settings.color_rows[1].xyz, linear),
                dot(settings.color_rows[2].xyz, linear),
            ),
            vec3f(0.0),
            vec3f(1.0),
        );
        rgb = pow(mapped, vec3f(1.0 / 2.2));
    }
    return vec4f(rgb, result.a);
}
