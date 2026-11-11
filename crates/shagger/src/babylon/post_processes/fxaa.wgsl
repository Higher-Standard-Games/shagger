// SPDX-License-Identifier: Apache-2.0
//
// Ported from Babylon.js 9.31.2, Copyright Babylon.js contributors, licensed
// under the Apache License 2.0 (see ../LICENSE): packages/dev/core/src/
// ShadersWGSL/fxaa.vertex.fx and fxaa.fragment.fx.
//
// Changes: one full-screen triangle; the eight neighbour coordinates are
// worked out per fragment from the texel size instead of interpolated from
// the vertices (the same values on a full-screen pass). Texture
// coordinates run down the screen, as wgpu's do. The non-MALI path.

struct Uniforms {
    // xy: texelSize
    texel_size: vec4f,
};

@group(0) @binding(0) var texture_sampler: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler_sampler: sampler;
@group(0) @binding(2) var<uniform> uniforms: Uniforms;

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let p = vec2f(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: VertexOut;
    out.position = vec4f(p, 0.0, 1.0);
    out.uv = vec2f(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

const fxaaQualitySubpix: f32 = 1.0;
const fxaaQualityEdgeThreshold: f32 = 0.166;
const fxaaQualityEdgeThresholdMin: f32 = 0.0833;
const kLumaCoefficients: vec3f = vec3f(0.2126, 0.7152, 0.0722);

fn FxaaLuma(rgba: vec4f) -> f32 {
    return dot(rgba.rgb, kLumaCoefficients);
}

fn tap(uv: vec2f) -> vec4f {
    return textureSampleLevel(texture_sampler, texture_sampler_sampler, uv, 0.0);
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4f {
    let texelSize = uniforms.texel_size.xy;
    let vUV = input.uv;
    let sampleCoordS = vUV + vec2f(0.0, 1.0) * texelSize;
    let sampleCoordE = vUV + vec2f(1.0, 0.0) * texelSize;
    let sampleCoordN = vUV + vec2f(0.0, -1.0) * texelSize;
    let sampleCoordW = vUV + vec2f(-1.0, 0.0) * texelSize;
    let sampleCoordNW = vUV + vec2f(-1.0, -1.0) * texelSize;
    let sampleCoordSE = vUV + vec2f(1.0, 1.0) * texelSize;
    let sampleCoordNE = vUV + vec2f(1.0, -1.0) * texelSize;
    let sampleCoordSW = vUV + vec2f(-1.0, 1.0) * texelSize;

    var posM: vec2f = vUV;
    let rgbyM = tap(vUV);
    let lumaM = FxaaLuma(rgbyM);
    var lumaS = FxaaLuma(tap(sampleCoordS));
    let lumaE = FxaaLuma(tap(sampleCoordE));
    var lumaN = FxaaLuma(tap(sampleCoordN));
    let lumaW = FxaaLuma(tap(sampleCoordW));
    let maxSM = max(lumaS, lumaM);
    let minSM = min(lumaS, lumaM);
    let maxESM = max(lumaE, maxSM);
    let minESM = min(lumaE, minSM);
    let maxWN = max(lumaN, lumaW);
    let minWN = min(lumaN, lumaW);
    let rangeMax = max(maxWN, maxESM);
    let rangeMin = min(minWN, minESM);
    let rangeMaxScaled = rangeMax * fxaaQualityEdgeThreshold;
    let range = rangeMax - rangeMin;
    let rangeMaxClamped = max(fxaaQualityEdgeThresholdMin, rangeMaxScaled);
    if (range < rangeMaxClamped) {
        return rgbyM;
    }
    let lumaNW = FxaaLuma(tap(sampleCoordNW));
    let lumaSE = FxaaLuma(tap(sampleCoordSE));
    let lumaNE = FxaaLuma(tap(sampleCoordNE));
    let lumaSW = FxaaLuma(tap(sampleCoordSW));
    let lumaNS = lumaN + lumaS;
    let lumaWE = lumaW + lumaE;
    let subpixRcpRange = 1.0 / range;
    let subpixNSWE = lumaNS + lumaWE;
    let edgeHorz1 = (-2.0 * lumaM) + lumaNS;
    let edgeVert1 = (-2.0 * lumaM) + lumaWE;
    let lumaNESE = lumaNE + lumaSE;
    let lumaNWNE = lumaNW + lumaNE;
    let edgeHorz2 = (-2.0 * lumaE) + lumaNESE;
    let edgeVert2 = (-2.0 * lumaN) + lumaNWNE;
    let lumaNWSW = lumaNW + lumaSW;
    let lumaSWSE = lumaSW + lumaSE;
    let edgeHorz4 = (abs(edgeHorz1) * 2.0) + abs(edgeHorz2);
    let edgeVert4 = (abs(edgeVert1) * 2.0) + abs(edgeVert2);
    let edgeHorz3 = (-2.0 * lumaW) + lumaNWSW;
    let edgeVert3 = (-2.0 * lumaS) + lumaSWSE;
    let edgeHorz = abs(edgeHorz3) + edgeHorz4;
    let edgeVert = abs(edgeVert3) + edgeVert4;
    let subpixNWSWNESE = lumaNWSW + lumaNESE;
    var lengthSign = texelSize.x;
    let horzSpan = edgeHorz >= edgeVert;
    let subpixA = subpixNSWE * 2.0 + subpixNWSWNESE;
    if (!horzSpan) {
        lumaN = lumaW;
    }
    if (!horzSpan) {
        lumaS = lumaE;
    }
    if (horzSpan) {
        lengthSign = texelSize.y;
    }
    let subpixB = (subpixA * (1.0 / 12.0)) - lumaM;
    let gradientN = lumaN - lumaM;
    let gradientS = lumaS - lumaM;
    var lumaNN = lumaN + lumaM;
    let lumaSS = lumaS + lumaM;
    let pairN = abs(gradientN) >= abs(gradientS);
    let gradient = max(abs(gradientN), abs(gradientS));
    if (pairN) {
        lengthSign = -lengthSign;
    }
    let subpixC = clamp(abs(subpixB) * subpixRcpRange, 0.0, 1.0);
    var posB: vec2f = posM;
    var offNP: vec2f;
    offNP.x = select(texelSize.x, 0.0, !horzSpan);
    offNP.y = select(texelSize.y, 0.0, horzSpan);
    if (!horzSpan) {
        posB.x += lengthSign * 0.5;
    }
    if (horzSpan) {
        posB.y += lengthSign * 0.5;
    }
    var posN: vec2f = posB - offNP * 1.5;
    var posP: vec2f = posB + offNP * 1.5;
    let subpixD = ((-2.0) * subpixC) + 3.0;
    var lumaEndN = FxaaLuma(tap(posN));
    let subpixE = subpixC * subpixC;
    var lumaEndP = FxaaLuma(tap(posP));
    if (!pairN) {
        lumaNN = lumaSS;
    }
    let gradientScaled = gradient * 1.0 / 4.0;
    let lumaMM = lumaM - lumaNN * 0.5;
    let subpixF = subpixD * subpixE;
    let lumaMLTZero = lumaMM < 0.0;
    lumaEndN -= lumaNN * 0.5;
    lumaEndP -= lumaNN * 0.5;
    var doneN = abs(lumaEndN) >= gradientScaled;
    var doneP = abs(lumaEndP) >= gradientScaled;
    if (!doneN) {
        posN -= offNP * 3.0;
    }
    var doneNP = (!doneN) || (!doneP);
    if (!doneP) {
        posP += offNP * 3.0;
    }
    if (doneNP) {
        if (!doneN) {
            lumaEndN = FxaaLuma(tap(posN));
        }
        if (!doneP) {
            lumaEndP = FxaaLuma(tap(posP));
        }
        if (!doneN) {
            lumaEndN = lumaEndN - lumaNN * 0.5;
        }
        if (!doneP) {
            lumaEndP = lumaEndP - lumaNN * 0.5;
        }
        doneN = abs(lumaEndN) >= gradientScaled;
        doneP = abs(lumaEndP) >= gradientScaled;
        if (!doneN) {
            posN -= offNP * 12.0;
        }
        doneNP = (!doneN) || (!doneP);
        if (!doneP) {
            posP += offNP * 12.0;
        }
    }
    var dstN = posM.x - posN.x;
    var dstP = posP.x - posM.x;
    if (!horzSpan) {
        dstN = posM.y - posN.y;
    }
    if (!horzSpan) {
        dstP = posP.y - posM.y;
    }
    let goodSpanN = (lumaEndN < 0.0) != lumaMLTZero;
    let spanLength = dstP + dstN;
    let goodSpanP = (lumaEndP < 0.0) != lumaMLTZero;
    let spanLengthRcp = 1.0 / spanLength;
    let directionN = dstN < dstP;
    let dst = min(dstN, dstP);
    let goodSpan = select(goodSpanP, goodSpanN, directionN);
    let subpixG = subpixF * subpixF;
    let pixelOffset = (dst * (-spanLengthRcp)) + 0.5;
    let subpixH = subpixG * fxaaQualitySubpix;
    let pixelOffsetGood = select(0.0, pixelOffset, goodSpan);
    let pixelOffsetSubpix = max(pixelOffsetGood, subpixH);
    if (!horzSpan) {
        posM.x += pixelOffsetSubpix * lengthSign;
    }
    if (horzSpan) {
        posM.y += pixelOffsetSubpix * lengthSign;
    }
    return tap(posM);
}
