// Movement golden vectors: Washoi's own MovementProfile (wash-fps
// `simulation/`) stepping scripted players over synthetic maps, through SHAG's
// collision stack, the way V2NetworkClient predicts a step: sanitize the
// input, step the lifts with the player on them, step knockback drag, then
// simulate. Run by generate.mjs:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs movement.mjs <shag> <out dir> <shag commit>
//
// Needs wash-fps checked out at <shag>/src/games/fps (SHAG's submodule) and
// SHAG's node_modules. The maps add what movement needs to collision.mjs's:
// tall walls, ledges, a pool, ladders, teleporters and jumppads.
//
// Each step records the position, velocity and an FNV-1a hash of a canonical
// line holding the sanitized input, every result field and the whole saved
// profile state. Set DUMP=map,run,step to print that line for one step.
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { mapBuilder } from "./maps.mjs";
import { movementRunner, num, vec } from "./movement_run.mjs";

const [shagArg, outArg, shagCommit] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { BspLoader } = await load("src/engine/loader/bsp/BspLoader.ts");
const { SweepCollisionBackend } = await load("src/engine/physics/SweepCollisionBackend.ts");
const { SweepBackendAdapter } = await load("src/engine/physics/SweepBackendAdapter.ts");
const { SharedCollisionWorld } = await load("src/shared/collision/SharedCollisionWorld.ts");
const { extractTriggerVolumes } = await load("src/shared/bsp/TriggerTable.ts");
const { sanitizeMovementOrZero } = await load("src/shared/server/simulation/InputSanitizer.ts");
const { MovementProfile } = await load("src/games/fps/simulation/MovementProfile.ts");
const { applyKnockbackDrag } = await load("src/games/fps/simulation/Knockback.ts");
console.warn = () => {};
console.log = () => {};
console.info = () => {};
console.debug = () => {};
const print = (text) => process.stderr.write(`${text}\n`);

let seed = 0x5bd1e995;
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


const { buildMap } = mapBuilder({ random, int, pick, chance, span });

/** Movement features, in Q3 axes. Records spawn spots in Babylon axes. */
function movementExtras(spots) {
  const at = (x, y, z) => ({ x, y: z, z: y });
  return ({ brush, model, entities, randomBox }) => {
    for (let i = 2 + int(3); i > 0; i--) {
      const b = randomBox([-600, -600, 0], [600, 600, 0], [16, 32]);
      const long = Math.round(span(96, 320));
      if (chance(0.5)) b.max[0] = b.min[0] + long;
      else b.max[1] = b.min[1] + long;
      b.max[2] = Math.round(span(160, 320));
      brush(b.min, b.max, 0, { bevel: false });
      const side = chance(0.5) ? -1 : 1;
      const cx = (b.min[0] + b.max[0]) / 2;
      const cy = (b.min[1] + b.max[1]) / 2;
      spots.push({ kind: "wall", at: at(cx + side * span(20, 60), cy + side * span(20, 60), 24.03125 + span(0, 40)) });
    }
    for (let i = 2 + int(3); i > 0; i--) {
      const b = randomBox([-600, -600, 0], [600, 600, 0], [48, 160]);
      b.max[2] = Math.round(span(40, 96));
      brush(b.min, b.max, 0, { bevel: chance(0.1) });
      spots.push({ kind: "ledge", at: at(b.min[0] - span(18, 40), (b.min[1] + b.max[1]) / 2, 24.03125), yaw: Math.PI / 2 });
    }
    // A pool standing on the floor, with a lip a swimmer can climb out over.
    {
      const b = randomBox([-500, -500, 0], [300, 300, 0], [128, 256]);
      const depth = Math.round(span(40, 120));
      b.max[2] = depth;
      brush(b.min, b.max, 4, { bevel: false });
      const lip = [b.max[0], b.min[1], 0];
      brush(lip, [b.max[0] + 64, b.max[1], depth + pick([4, 6, 30])], 0, { bevel: false });
      const cx = (b.min[0] + b.max[0]) / 2;
      const cy = (b.min[1] + b.max[1]) / 2;
      spots.push({ kind: "pool", at: at(cx, cy, depth - span(0, 40)) });
      spots.push({ kind: "pool", at: at(b.max[0] - pick([20, 24, 40]), cy + span(-40, 40), depth - pick([8, 10, 20])), yaw: Math.PI / 2 });
    }
    // Ladders: thin climbable slabs against a tall backing wall.
    for (let i = 1 + int(2); i > 0; i--) {
      const x = Math.round(span(-500, 500));
      const y = Math.round(span(-500, 500));
      const top = Math.round(span(96, 256));
      brush([x, y, 0], [x + 16, y + 64, top + 64], 0, { bevel: false });
      brush([x - 8, y + 8, 0], [x, y + 56, top], 5, { bevel: false });
      spots.push({ kind: "ladder", at: at(x - 20, y + 32, 24.03125), yaw: Math.PI / 2 });
    }
    // Teleporters and jumppads, with a few the table must drop.
    for (let i = 1 + int(3); i > 0; i--) {
      const t = randomBox([-500, -500, 0], [500, 500, 0], [32, 96]);
      t.max[2] = Math.round(span(16, 80));
      const name = `dest${i}_${int(1000)}`;
      const push = chance(0.5);
      const bad = chance(0.1) ? pick(["model", "target", "origin"]) : null;
      const fields = { classname: push ? "trigger_push" : "trigger_teleport" };
      if (bad !== "target") fields.target = name;
      const index = model([{ min: t.min, max: t.max, texture: 10, sides: [
        [[1, 0, 0], t.max[0]], [[-1, 0, 0], -t.min[0]], [[0, 1, 0], t.max[1]],
        [[0, -1, 0], -t.min[1]], [[0, 0, 1], t.max[2]], [[0, 0, -1], -t.min[2]],
      ] }], fields);
      if (bad === "model") entities[entities.length - 1] = entities.at(-1).replace(`"*${index}"`, pick(['"*0"', '"*abc"', '"*999"', '"7"']));
      const dx = Math.round(span(-500, 500));
      const dy = Math.round(span(-500, 500));
      const dz = push ? Math.round(span(100, 300)) : 0;
      const origin = bad === "origin" ? pick(["1 2", "a b c", ""]) : pick([`${dx} ${dy} ${dz}`, ` ${dx} ${dy} ${dz}`, `${dx}  ${dy} ${dz} 9`]);
      const destClass = pick(["target_position", "misc_teleporter_dest", "info_teleport_destination", "target_location", "info_notnull", "info_player_start"]);
      const angle = pick(['"angle" "90"', '"angle" "-90"', '"angle" "0"', '"angle" "-1"', '"angle" "abc"', "", '"angle" "180"']);
      entities.push(`{\n"classname" "${destClass}"\n"targetname" "${name}"\n"origin" "${origin}"\n${angle}\n}`);
      const tx = (t.min[0] + t.max[0]) / 2;
      const ty = (t.min[1] + t.max[1]) / 2;
      const away = span(0, 2 * Math.PI);
      const sx = tx + Math.cos(away) * span(40, 120);
      const sy = ty + Math.sin(away) * span(40, 120);
      spots.push({ kind: "trigger", at: at(sx, sy, 24.03125), yaw: Math.atan2(tx - sx, ty - sy) });
    }
  };
}

const cover = process.env.COVER ? {} : null;
const { simulateRun } = movementRunner({ rng: { random, int, pick, chance, span }, lib: { sanitizeMovementOrZero, applyKnockbackDrag }, cover, print });

const vectors = [];
for (let m = 0; m < 40; m++) {
  const spots = [];
  const map = buildMap({ extra: movementExtras(spots) });
  const bsp = BspLoader.loadCollision(map.bytes.buffer.slice(0));
  const triggers = extractTriggerVolumes(bsp.entities, bsp.models);
  const runs = [];
  for (let run = 0; run < 4; run++) {
    // Each run gets a fresh world, so lifts start where the map puts them.
    const backend = new SweepCollisionBackend();
    await backend.init(bsp);
    const world = new SharedCollisionWorld(new SweepBackendAdapter(backend));
    const profile = new MovementProfile();
    profile.setTriggerVolumes(triggers);
    const triggerSpots = spots.filter((p) => p.kind === "trigger");
    const edgeSpots = spots.filter((p) => p.kind === "pool" && p.yaw !== undefined);
    const spot = run === 0 && triggerSpots.length ? pick(triggerSpots) : run === 1 ? pick(edgeSpots) : chance(0.85) ? pick(spots) : { kind: "air", at: { x: span(-600, 600), y: span(30, 300), z: span(-600, 600) } };
    runs.push(simulateRun({ backend, world, profile, spot, m, run, count: 180, record: (p, v, hash) => [...vec(p), ...vec(v), hash] }));
  }
  vectors.push({
    bytes: Buffer.from(map.bytes).toString("hex"),
    triggers: triggers.map((t) => [t.id, t.type, ...vec(t.min), ...vec(t.max), ...vec(t.dest), t.destYaw === undefined ? null : num(t.destYaw)]),
    runs,
  });
}

if (cover) print(JSON.stringify(cover));
const fingerprint = new MovementProfile().getDeterminismFingerprint();
const doc = { generator: "tools/golden/movement.mjs", shag: shagCommit, node: process.version, fingerprint, vectors };
writeFileSync(resolve(outArg, "movement.json"), `${JSON.stringify(doc).replaceAll('},{"bytes"', '},\n{"bytes"')}\n`);
