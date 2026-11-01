// Map pickup golden vectors: Washoi's own pickup table and item landing
// (wash-fps `shared/bsp/PickupTable.ts` extractPickups, and
// `shared/items/ItemThrow.ts` landMapPickups) on each shipped map, through
// SHAG's collision stack, and the spawn toss of pickup ids 0 to 63. Run by
// generate.mjs when WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs pickups.mjs <shag> <out dir> <shag commit> <maps dir>
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { num, vec } from "./movement_run.mjs";

const [shagArg, outArg, shagCommit, mapsArg] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { BspLoader } = await load("src/engine/loader/bsp/BspLoader.ts");
const { SweepCollisionBackend } = await load("src/engine/physics/SweepCollisionBackend.ts");
const { SweepBackendAdapter } = await load("src/engine/physics/SweepBackendAdapter.ts");
const { SharedCollisionWorld } = await load("src/shared/collision/SharedCollisionWorld.ts");
const { extractPickups } = await load("src/games/fps/shared/bsp/PickupTable.ts");
const { landMapPickups, mapWeaponTossVelocity } = await load("src/games/fps/shared/items/ItemThrow.ts");
console.warn = () => {};
console.log = () => {};
console.info = () => {};
console.debug = () => {};

const tosses = Array.from({ length: 64 }, (_, id) => vec(mapWeaponTossVelocity(id)));
const maps = [];
for (const file of readdirSync(mapsArg).filter((f) => f.endsWith(".bsp")).sort()) {
  const bytes = readFileSync(resolve(mapsArg, file));
  const bsp = BspLoader.loadCollision(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  const table = extractPickups(bsp.entities);
  const backend = new SweepCollisionBackend();
  await backend.init(bsp);
  const landed = landMapPickups(table, new SharedCollisionWorld(new SweepBackendAdapter(backend)));
  maps.push({
    map: file,
    pickups: table.map((e, i) => [
      e.id,
      e.classname,
      e.type,
      e.kind,
      e.amount ?? null,
      e.suspended === true,
      ...vec(e.position),
      ...vec(landed[i].position),
    ]),
  });
  process.stderr.write(`${file}: ${table.length} pickups\n`);
}
writeFileSync(
  resolve(outArg, "pickups.json"),
  `${JSON.stringify({ generator: "shagger tools/golden/pickups.mjs", shag: shagCommit, tosses, maps }).replaceAll("],[", "],\n[")}\n`,
);
