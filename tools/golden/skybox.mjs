// Sky box golden vectors: SHAG's own SkyboxLoader. transformFace and
// scaleFaceTo over synthetic faces, then loadSkybox for each shipped map's
// far box (the first sky shader with skyparms, as BspMeshBuilder.applySkybox
// takes it), on a Babylon NullEngine whose createRawCubeTexture captures the
// six faces it is handed. JPEG, PNG and WebP, which SHAG leaves to the
// browser, are decoded with sharp (libwebp, as the browser has it). Run by
// generate.mjs when WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs skybox.mjs <shag> <out dir> <shag commit> <maps dir>
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [shagArg, outArg, shagCommit, mapsArg] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const sharp = createRequire(resolve(shag, "package.json"))("sharp");
const { ingestPk3Dir } = await load("src/engine/loader/pk3/NodePk3.ts");
const { Q3ShaderRegistry } = await load("src/engine/loader/shader/Q3ShaderRegistry.ts");
const { BspLoader } = await load("src/engine/loader/bsp/BspLoader.ts");
const { SkyboxLoader, scaleFaceTo } = await load("src/engine/loader/texture/SkyboxLoader.ts");
const { NullEngine } = await load("node_modules/@babylonjs/core/Engines/nullEngine.js");
const { InternalTexture } = await load("node_modules/@babylonjs/core/Materials/Textures/internalTexture.js");
const { Scene } = await load("node_modules/@babylonjs/core/scene.js");
console.warn = () => {};
console.log = () => {};

const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const loader = SkyboxLoader.getInstance();

// Synthetic faces: transformFace for every transform, scaleFaceTo up and down.
const pattern = (size) => Uint8Array.from({ length: size * size * 4 }, (_, i) => (i * 7 + 3) & 255);
const TRANSFORMS = ["none", "flipH", "flipV", "rotate180", "rotate90CW", "rotate90CCW", "rotate90CW_flipH", "rotate90CCW_flipH"];
const transforms = [];
for (let size = 1; size <= 5; size++)
  for (const t of TRANSFORMS) transforms.push([size, t, sha(loader.transformFace(pattern(size), size, t))]);
const scales = [];
for (const [size, target] of [[1, 4], [2, 5], [3, 7], [4, 4], [5, 3], [8, 13]])
  scales.push([size, target, sha(scaleFaceTo(pattern(size), size, target))]);

// The browser's decode, by sharp.
loader.parseStandardImageToRGBA = async (data, ext) => {
  if (![".jpg", ".jpeg", ".png", ".webp"].includes(ext)) return null;
  try {
    const { data: raw, info } = await sharp(Buffer.from(data)).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
    return { data: new Uint8Array(raw.buffer, raw.byteOffset, raw.byteLength), size: info.width };
  } catch {
    return null;
  }
};
const engine = new NullEngine();
let uploaded = null;
engine.createRawCubeTexture = (data, size) => {
  uploaded = { data, size };
  return new InternalTexture(engine, 0);
};
const scene = new Scene(engine);
loader.init(scene);

await ingestPk3Dir(resolve(mapsArg, "pk3"));
const registry = Q3ShaderRegistry.getInstance();
await registry.loadShaders();
const maps = [];
for (const file of readdirSync(mapsArg).filter((f) => f.endsWith(".bsp")).sort()) {
  const bytes = readFileSync(resolve(mapsArg, file));
  const bsp = BspLoader.load(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  let farbox = null;
  for (const face of bsp.faces) {
    const texture = bsp.textures[face.textureIndex];
    if (!texture) continue;
    const props = registry.resolveMaterialProps(texture.name, { worldSurface: true });
    if (!(props.isSky || (texture.surfaceFlags & 4) !== 0)) continue;
    if (props.skyParms?.farbox) {
      farbox = props.skyParms.farbox;
      break;
    }
  }
  if (!farbox) {
    maps.push({ map: file, farbox });
    continue;
  }
  // Each Quake face as loadSkyboxFace finds it.
  const sources = [];
  for (const suffix of ["_rt", "_bk", "_lf", "_ft", "_up", "_dn"]) {
    const face = (await loader.loadSkyboxFace(farbox, suffix)) ?? (await loader.loadSkyboxFace(farbox, ""));
    sources.push(face && { path: face.path, size: face.size, sha: sha(face.data) });
  }
  uploaded = null;
  const result = await loader.loadSkybox(farbox);
  maps.push({
    map: file,
    farbox,
    success: result.success,
    loadedFaces: result.loadedFaces ?? null,
    sources,
    size: uploaded?.size ?? null,
    slots: uploaded ? uploaded.data.map((face) => sha(face)) : null,
  });
  result.skybox?.dispose();
}
writeFileSync(
  resolve(outArg, "skybox.json"),
  `${JSON.stringify({ generator: "shagger tools/golden/skybox.mjs", shag: shagCommit, transforms, scales, maps }, null, 1)}\n`,
);
process.stderr.write(`${transforms.length} transforms, ${scales.length} scales, ${maps.length} maps\n`);
