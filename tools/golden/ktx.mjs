// KTX1 golden vectors: SHAG's own KTX1Loader over every .ktx in the shipped
// packages. parseKTX1, then createTextureFromKTX1 against a stand-in engine
// with no compressed-texture support, which captures the RGBA it uploads
// (the software ETC1 and DXT decode and the uncompressed conversions). Run
// by generate.mjs when WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs ktx.mjs <shag> <out dir> <shag commit> <pk3 dir>
//
// Each file keeps its size and the SHA-256 of the RGBA, or the parse error.
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [shagArg, outArg, shagCommit, pk3Arg] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { parseKTX1, createTextureFromKTX1 } = await load("src/engine/loader/texture/KTX1Loader.ts");
const JSZip = (await import(pathToFileURL(resolve(shag, "node_modules/jszip/lib/index.js")).href)).default;
console.warn = () => {};
console.log = () => {};

let uploaded = null;
const engine = {
  isWebGPU: false,
  getCaps: () => ({}),
  createRawTexture: (pixels, width, height) => {
    uploaded = { pixels: new Uint8Array(pixels.buffer, pixels.byteOffset, width * height * 4), width, height };
    return {};
  },
};

const files = [];
for (const file of readdirSync(pk3Arg).filter((f) => f.endsWith(".pk3")).sort((a, b) => a.localeCompare(b))) {
  const zip = await JSZip.loadAsync(readFileSync(resolve(pk3Arg, file)));
  const names = Object.keys(zip.files).filter((n) => /\.ktx$/i.test(n) && !zip.files[n].dir).sort();
  for (const name of names) {
    const bytes = await zip.files[name].async("uint8array");
    const source = `${file}:${name}`;
    let ktx;
    try {
      ktx = parseKTX1(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
    } catch (e) {
      files.push({ source, error: String(e.message) });
      continue;
    }
    uploaded = null;
    await createTextureFromKTX1(engine, ktx, name);
    files.push(
      uploaded
        ? { source, width: uploaded.width, height: uploaded.height, sha256: createHash("sha256").update(uploaded.pixels).digest("hex") }
        : { source, decoded: false },
    );
  }
}
writeFileSync(resolve(outArg, "ktx.json"), `${JSON.stringify({ generator: "shagger tools/golden/ktx.mjs", shag: shagCommit, files }).replaceAll('},{"source"', '},\n{"source"')}\n`);
process.stderr.write(`${files.length} KTX files, ${files.filter((f) => f.sha256).length} decoded\n`);
