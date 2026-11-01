// Autosprite golden vectors: SHAG's own autospriteRecord and
// autospriteVertexData (bsp/mesh/autospriteRecords.ts, autosprite2.ts) for
// every world face whose shader is an autosprite or autosprite2, on every
// shipped map, each as a batch of its own. Run by generate.mjs when
// WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs autosprites.mjs <shag> <out dir> <shag commit> <maps dir>
//
// Each face's line holds its index, the record's sizes and the three
// attribute arrays (f64 bits of the f32 values); each map keeps the FNV-1a
// hash of its lines. DUMP=map,face prints one.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { fnv, num } from "./movement_run.mjs";

const [shagArg, outArg, shagCommit, mapsArg] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { ingestPk3Dir } = await load("src/engine/loader/pk3/NodePk3.ts");
const { Q3ShaderRegistry } = await load("src/engine/loader/shader/Q3ShaderRegistry.ts");
const { BspLoader } = await load("src/engine/loader/bsp/BspLoader.ts");
const { buildPolygonFace, buildPatchFace, buildMeshFace } = await load("src/engine/loader/bsp/mesh/faceBuilders.ts");
const { autospriteRecord, autospriteVertexData } = await load("src/engine/loader/bsp/mesh/autospriteRecords.ts");
console.warn = () => {};
console.log = () => {};

await ingestPk3Dir(resolve(mapsArg, "pk3"));
const registry = Q3ShaderRegistry.getInstance();
await registry.loadShaders();
const [dumpMap, dumpFace] = (process.env.DUMP ?? "").split(",");

const maps = [];
for (const file of readdirSync(mapsArg).filter((f) => f.endsWith(".bsp")).sort()) {
  const bytes = readFileSync(resolve(mapsArg, file));
  const bsp = BspLoader.load(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  const world = bsp.models[0];
  const lines = [];
  for (let i = world.firstFace; i < world.firstFace + world.numFaces; i++) {
    const face = bsp.faces[i];
    const name = bsp.textures[face.textureIndex]?.name ?? "";
    const props = registry.resolveMaterialProps(name, { worldSurface: true });
    if (!props.autosprite && !props.autosprite2) continue;
    const build = { 1: buildPolygonFace, 2: buildPatchFace, 3: buildMeshFace }[face.faceType];
    const g = build?.(bsp, face);
    if (!g || g.positions.length === 0) continue;
    const n = g.positions.length / 3;
    let cx = 0, cy = 0, cz = 0;
    for (let v = 0; v < n; v++) {
      cx += g.positions[v * 3];
      cy += g.positions[v * 3 + 1];
      cz += g.positions[v * 3 + 2];
    }
    const center = [cx / n, cy / n, cz / n];
    const record = autospriteRecord({ positions: g.positions, center, shaderProps: props }, null, 0, g.positions, g.indices);
    const data = autospriteVertexData([record], g.positions);
    const text = [i, num(record.halfWidth), num(record.halfHeight), record.isAutosprite2, [...data.center].map(num).join(","), [...data.frame].map(num).join(","), [...data.right].map(num).join(",")].join(" ");
    if (file === `${dumpMap}.bsp` && String(i) === dumpFace) process.stderr.write(`${text}\n`);
    lines.push(text);
  }
  maps.push({ map: file.replace(/\.bsp$/, ""), sprites: lines.length, hash: fnv(lines.join("\n")) });
}
writeFileSync(resolve(outArg, "autosprites.json"), `${JSON.stringify({ generator: "shagger tools/golden/autosprites.mjs", shag: shagCommit, maps }).replaceAll('},{"map"', '},\n{"map"')}\n`);
process.stderr.write(`${maps.reduce((n, m) => n + m.sprites, 0)} autosprite faces on ${maps.length} maps\n`);
