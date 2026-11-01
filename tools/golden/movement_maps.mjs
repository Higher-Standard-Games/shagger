// Movement golden vectors on the shipped maps: Washoi's own MovementProfile
// (wash-fps `simulation/`) stepping scripted players over the real BSPs
// through SHAG's collision stack, with movement.mjs's stepping loop
// (movement_run.mjs). Run by generate.mjs when WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs movement_maps.mjs <shag> <out dir> <shag commit> <maps dir>
//
// <maps dir> holds the shipped BSPs (washoi_dm2.bsp and so on), as
// wash-fps-rust's tools/fetch_maps.sh extracts them from the published pk3s.
// The vectors name each map by file name and SHA-256, not by its bytes, so the
// Rust test reads the same files.
//
// Runs start at every kind of place a player is: spawn points facing their
// authored yaw, dropped onto teleporters and jumppads, and random points in
// the map's bounds (some inside walls). Each step keeps only the FNV-1a hash of
// movement_run.mjs's canonical line; DUMP=map,run,step prints that line.
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { movementRunner, num, vec } from "./movement_run.mjs";

const [shagArg, outArg, shagCommit, mapsArg] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { BspLoader } = await load("src/engine/loader/bsp/BspLoader.ts");
const { SweepCollisionBackend } = await load("src/engine/physics/SweepCollisionBackend.ts");
const { SweepBackendAdapter } = await load("src/engine/physics/SweepBackendAdapter.ts");
const { SharedCollisionWorld } = await load("src/shared/collision/SharedCollisionWorld.ts");
const { extractTriggerVolumes } = await load("src/shared/bsp/TriggerTable.ts");
const { extractSpawnFacings } = await load("src/shared/bsp/SpawnPointTable.ts");
const { sanitizeMovementOrZero } = await load("src/shared/server/simulation/InputSanitizer.ts");
const { MovementProfile } = await load("src/games/fps/simulation/MovementProfile.ts");
const { applyKnockbackDrag } = await load("src/games/fps/simulation/Knockback.ts");
console.warn = () => {};
console.log = () => {};
console.info = () => {};
console.debug = () => {};
const print = (text) => process.stderr.write(`${text}\n`);

let seed = 0x2545f491;
function random() {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  return (seed >>> 0) / 0x1_0000_0000;
}
const int = (n) => Math.floor(random() * n);
const pick = (list) => list[int(list.length)];
const chance = (p) => random() < p;
const span = (lo, hi) => lo + random() * (hi - lo);

const cover = process.env.COVER ? {} : null;
const { simulateRun } = movementRunner({ rng: { random, int, pick, chance, span }, lib: { sanitizeMovementOrZero, applyKnockbackDrag }, cover, print });

const RUNS = { spawn: 14, trigger: 6, air: 4 };
const STEPS = 600;

const files = readdirSync(mapsArg).filter((f) => f.endsWith(".bsp")).sort();
const vectors = [];
for (const [m, file] of files.entries()) {
  const bytes = readFileSync(resolve(mapsArg, file));
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  const bsp = BspLoader.loadCollision(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  const triggers = extractTriggerVolumes(bsp.entities, bsp.models);
  const facings = extractSpawnFacings(bsp.entities);
  // World model 0 bounds, Q3 axes to Babylon axes.
  const w = bsp.models[0];
  const lo = { x: w.mins[0], y: w.mins[2], z: w.mins[1] };
  const hi = { x: w.maxs[0], y: w.maxs[2], z: w.maxs[1] };
  const spots = [];
  for (let i = 0; i < RUNS.spawn && facings.length; i++) {
    const f = pick(facings);
    spots.push({ kind: "spawn", at: { ...f.position }, yaw: f.yaw });
  }
  for (let i = 0; i < RUNS.trigger && triggers.length; i++) {
    const t = pick(triggers);
    const at = { x: span(t.min.x, t.max.x), y: t.max.y + span(8, 64), z: span(t.min.z, t.max.z) };
    spots.push({ kind: "trigger", at, yaw: Math.round(span(-Math.PI, Math.PI) * 1024) / 1024 });
  }
  for (let i = 0; i < RUNS.air; i++) {
    spots.push({ kind: "air", at: { x: span(lo.x, hi.x), y: span(lo.y, hi.y), z: span(lo.z, hi.z) } });
  }
  const runs = [];
  for (const [run, spot] of spots.entries()) {
    // Each run gets a fresh world, so lifts start where the map puts them.
    const backend = new SweepCollisionBackend();
    await backend.init(bsp);
    const world = new SharedCollisionWorld(new SweepBackendAdapter(backend));
    const profile = new MovementProfile();
    profile.setTriggerVolumes(triggers);
    runs.push(simulateRun({ backend, world, profile, spot, m, run, count: STEPS, record: (_p, _v, hash) => hash }));
  }
  print(`${file}: ${facings.length} spawns, ${triggers.length} triggers, ${runs.length} runs`);
  vectors.push({
    map: file,
    sha256,
    triggers: triggers.map((t) => [t.id, t.type, ...vec(t.min), ...vec(t.max), ...vec(t.dest), t.destYaw === undefined ? null : num(t.destYaw)]),
    runs,
  });
}

if (cover) print(JSON.stringify(cover));
const fingerprint = new MovementProfile().getDeterminismFingerprint();
const doc = { generator: "tools/golden/movement_maps.mjs", shag: shagCommit, node: process.version, fingerprint, vectors };
writeFileSync(resolve(outArg, "movement_maps.json"), `${JSON.stringify(doc).replaceAll('},{"map"', '},\n{"map"').replaceAll('},{"spot"', '},\n{"spot"')}\n`);
