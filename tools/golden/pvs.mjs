// PVS golden vectors: SHAG's own buildFaceClusters and PVSCuller on each
// shipped map. Every face is registered as a mesh of its own (its clusters
// as bspPvsClusters), then the camera walks 400 points in the world model's
// bounds, in Babylon space, with stationary frames and a missing camera in
// between. Each frame keeps the stats and the hash of which meshes show. Run
// by generate.mjs when WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs pvs.mjs <shag> <out dir> <shag commit> <maps dir>
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { fnv } from "./movement_run.mjs";

const [shagArg, outArg, shagCommit, mapsArg] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { BspLoader } = await load("src/engine/loader/bsp/BspLoader.ts");
const { buildFaceClusters } = await load("src/engine/loader/bsp/mesh/pvs.ts");
const { PVSCuller } = await load("src/engine/loader/bsp/PVSCuller.ts");
const { Vector3 } = await load("node_modules/@babylonjs/core/Maths/math.vector.js");
console.warn = () => {};
console.log = () => {};

let seed = 0x2545f491;
function random() {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  return (seed >>> 0) / 0x1_0000_0000;
}

const maps = [];
for (const file of readdirSync(mapsArg).filter((f) => f.endsWith(".bsp")).sort()) {
  const bytes = readFileSync(resolve(mapsArg, file));
  const bsp = BspLoader.load(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  const faceClusters = buildFaceClusters(bsp);
  const meshes = faceClusters.map((clusters) => ({
    isVisible: true,
    isDisposed: () => false,
    metadata: { bspPvsClusters: clusters },
  }));
  const culler = new PVSCuller(bsp);
  culler.registerFromWorldMesh({ getChildMeshes: () => meshes });
  const { mins, maxs } = bsp.models[0];
  const frames = [];
  let position = null;
  for (let i = 0; i < 400; i++) {
    const r = random();
    if (r < 0.05) position = null;
    else if (r < 0.15 && position) position = position.clone();
    else {
      // Q3 x, y, z inside the world bounds, as Babylon (x, z, y), in float32.
      const q = [0, 1, 2].map((axis) => Math.fround(mins[axis] + random() * (maxs[axis] - mins[axis])));
      position = new Vector3(q[0], q[2], q[1]);
    }
    const stats = culler.update(position);
    frames.push([
      position ? [position.x, position.y, position.z] : null,
      stats.enabled,
      stats.currentCluster,
      stats.visibleClusters,
      stats.visibleMeshes,
      stats.culledMeshes,
      fnv(meshes.map((m) => (m.isVisible ? "1" : "0")).join("")),
    ]);
  }
  maps.push({
    map: file,
    hasPVS: culler.hasPVS,
    faceClusters: fnv(JSON.stringify(faceClusters)),
    frames,
  });
}
writeFileSync(
  resolve(outArg, "pvs.json"),
  `${JSON.stringify({ generator: "shagger tools/golden/pvs.mjs", shag: shagCommit, maps }).replaceAll('],[', '],\n[')}\n`,
);
process.stderr.write(`${maps.length} maps\n`);
