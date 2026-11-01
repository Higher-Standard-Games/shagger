// Shader material golden vectors: SHAG's own Q3ShaderRegistry over the
// shipped packages. Every package is mounted (NodePk3.ingestPk3Dir), the
// registry loads every scripts/*.shader, and each name (every loaded shader
// and every texture of every map, lower case, sorted) is resolved with
// resolveMaterialPasses under three option sets: a world surface without and
// with deluxe maps, and a model surface without cube maps. Run by
// generate.mjs when WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs shader_materials.mjs <shag> <out dir> <shag commit> <maps dir>
//
// Each name keeps the FNV-1a hash of its canonical JSON (as shaders.mjs
// makes it); DUMP=<name> prints one.
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
console.warn = () => {};
console.log = () => {};

// As shaders.mjs has it (which runs on import).
function canonical(value) {
  if (value instanceof Set) return canonical([...value]);
  if (Array.isArray(value)) return value.map(canonical);
  if (typeof value === "number") return Number.isFinite(value) ? num(value) : null;
  if (value && typeof value === "object") {
    const out = {};
    for (const key of Object.keys(value).sort()) if (value[key] !== undefined) out[key] = canonical(value[key]);
    return out;
  }
  return value;
}

const packages = await ingestPk3Dir(resolve(mapsArg, "pk3"));
const registry = Q3ShaderRegistry.getInstance();
const shaderCount = await registry.loadShaders();

const names = new Set(registry.getShaderNames());
for (const file of readdirSync(mapsArg).filter((f) => f.endsWith(".bsp")).sort()) {
  const bytes = readFileSync(resolve(mapsArg, file));
  const bsp = BspLoader.load(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  for (const texture of bsp.textures) names.add(texture.name.toLowerCase());
}
const sorted = [...names].sort();

const OPTION_SETS = [
  { worldSurface: true, deluxe: false },
  { worldSurface: true, deluxe: true },
  { deluxe: false, textureCubeMap: false },
];
const dump = process.env.DUMP;
const hashes = sorted.map((name) => {
  const text = JSON.stringify(
    canonical({
      shader: registry.getShader(name)?.name ?? null,
      sets: OPTION_SETS.map((options) => ({
        cubemap: registry.hasGeneratedCubemap(name, options),
        passes: registry.resolveMaterialPasses(name, options),
      })),
    }),
  );
  if (dump === name) process.stderr.write(`${text}\n`);
  return [name, fnv(text)];
});
writeFileSync(
  resolve(outArg, "shader_materials.json"),
  `${JSON.stringify({ generator: "shagger tools/golden/shader_materials.mjs", shag: shagCommit, packages, shaders: shaderCount, materials: hashes }).replaceAll('],["', '],\n["')}\n`,
);
process.stderr.write(`${packages} packages, ${shaderCount} shaders, ${sorted.length} names\n`);
