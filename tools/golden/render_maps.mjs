// Render-side golden vectors on the shipped maps: SHAG's own face builders
// (bsp/mesh/faceBuilders.ts) over every face of every map, and its lightmap
// encoding (bsp/mesh/lightmaps.ts) over every lightmap page (the array form,
// which is the pages end to end, needs a GL context), run through
// Babylon's NullEngine so the bytes come from the real texture path. Run by
// generate.mjs when WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs render_maps.mjs <shag> <out dir> <shag commit> <maps dir>
//
// Each face's canonical line holds its index, its type and every array (f64 as
// hex bits); the vectors keep the FNV-1a hash of each run of 256 lines, and
// DUMP=map,face prints a line. The IBSP transfer curve is tabulated for all
// 256 inputs, scaleBspVertexColors is checked on the maps' vertex colours, and
// bspLightingProfile (BspLightingProfile.ts) on each map's worldspawn.
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { fnv, num } from "./movement_run.mjs";

const [shagArg, outArg, shagCommit, mapsArg] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { BspLoader } = await load("src/engine/loader/bsp/BspLoader.ts");
const { buildPolygonFace, buildPatchFace, buildMeshFace } = await load("src/engine/loader/bsp/mesh/faceBuilders.ts");
const { createBspLightmapTexture, scaleBspVertexColors } = await load("src/engine/loader/bsp/mesh/lightmaps.ts");
const { bspLightingProfile } = await load("src/engine/loader/bsp/BspLightingProfile.ts");
const { NullEngine } = await load("node_modules/@babylonjs/core/Engines/nullEngine.js");
const { Scene } = await load("node_modules/@babylonjs/core/scene.js");
console.warn = () => {};
console.log = () => {};
const scene = new Scene(new NullEngine());
const bytesOf = (texture) => texture.getInternalTexture()._bufferView;
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const [dumpMap, dumpFace] = (process.env.DUMP ?? "").split(",");

const GROUP = 256;
const line = (i, type, g) =>
  [i, type, g.positions.map(num).join(","), g.normals.map(num).join(","), g.uvs.map(num).join(","), g.uv2s.map(num).join(","), g.colors.map(num).join(","), g.indices.join(",")].join(" ");

// The IBSP curve, one byte at a time.
const curve = [];
for (let c = 0; c < 256; c++) curve.push(bytesOf(createBspLightmapTexture(new Uint8Array([c, c, c]), 1, scene, "c"))[0]);

const files = readdirSync(mapsArg).filter((f) => f.endsWith(".bsp")).sort();
const maps = [];
for (const file of files) {
  const bytes = readFileSync(resolve(mapsArg, file));
  const bsp = BspLoader.load(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  const faces = [];
  let group = [];
  const counts = { 1: 0, 2: 0, 3: 0, other: 0 };
  bsp.faces.forEach((face, i) => {
    const build = { 1: buildPolygonFace, 2: buildPatchFace, 3: buildMeshFace }[face.faceType];
    const text = build ? line(i, face.faceType, build(bsp, face)) : `${i} ${face.faceType}`;
    counts[build ? face.faceType : "other"]++;
    if (file === `${dumpMap}.bsp` && String(i) === dumpFace) process.stderr.write(`${text}\n`);
    group.push(fnv(text));
    if (group.length === GROUP) {
      faces.push(fnv(group.join("")));
      group = [];
    }
  });
  if (group.length) faces.push(fnv(group.join("")));

  const size = bsp.lightmapSize;
  const pages = bsp.lightmaps.map((rgb, i) => ({
    ibsp: sha(bytesOf(createBspLightmapTexture(rgb, size, scene, `l${i}`, false))),
    raw: sha(bytesOf(createBspLightmapTexture(rgb, size, scene, `r${i}`, true))),
    direction: sha(bytesOf(createBspLightmapTexture(rgb, size, scene, `d${i}`, true, true))),
  }));

  // Vertex colours as the builder sees them, scaled by a few gains.
  const colors = [];
  for (let i = 0; i < Math.min(bsp.vertices.length, 4096); i++) {
    const v = bsp.vertices[i];
    const c = v.color ?? v.colors[0];
    colors.push(c[0] / 255, c[1] / 255, c[2] / 255, c[3] / 255);
  }
  const scaled = [0.5, 1, 1.7, 2, 4].map((gain) => fnv(scaleBspVertexColors(colors, gain).map(num).join(",")));

  const p = bspLightingProfile(bsp);
  const profile = { gain: num(p.gain), ambient: p.ambient.map(num), deluxe: p.deluxe };
  maps.push({ map: file.replace(/\.bsp$/, ""), sha256: sha(bytes), profile, faceCount: bsp.faces.length, counts, faces, lightmapSize: size, pages, scaled });
  process.stderr.write(`${file}: ${bsp.faces.length} faces, ${bsp.lightmaps.length} lightmap pages\n`);
}

const doc = { generator: "shagger tools/golden/render_maps.mjs", shag: shagCommit, group: GROUP, curve, maps };
writeFileSync(resolve(outArg, "render_maps.json"), `${JSON.stringify(doc).replaceAll('},{"map"', '},\n{"map"')}\n`);
