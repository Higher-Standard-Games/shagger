// Collision golden vectors: synthetic IBSP maps run through SHAG's own
// SweepCollisionBackend, SweepBackendAdapter and SharedCollisionWorld. Run by
// generate.mjs:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs collision.mjs <shag> <out dir> <shag commit>
//
// Needs SHAG's node_modules (@babylonjs/core). Each map has a floor, stairs,
// random and nested brushes with every collision texture kind, slanted
// bevels, curved patches, a BSP tree whose leaves hold random subsets, lifts
// (func_plat, func_door) with trigger volumes, and func_explosive models.
// Then a random mix of queries runs against it in order, so lift motion and
// brush model toggles feed later traces. Floats are f64 bit patterns in hex.
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { mapBuilder } from "./maps.mjs";

const [shagArg, outArg, shagCommit] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { BspLoader } = await load("src/engine/loader/bsp/BspLoader.ts");
const { SweepCollisionBackend } = await load("src/engine/physics/SweepCollisionBackend.ts");
const { SweepBackendAdapter } = await load("src/engine/physics/SweepBackendAdapter.ts");
const { SharedCollisionWorld } = await load("src/shared/collision/SharedCollisionWorld.ts");
const { Vector3 } = await load("node_modules/@babylonjs/core/Maths/math.vector.js");
console.warn = () => {};
console.log = () => {};
console.info = () => {};
console.debug = () => {};

let seed = 0x2c1b3c6d;
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

const view = new DataView(new ArrayBuffer(8));
function num(x) {
  if (Number.isNaN(x)) return "nan";
  view.setFloat64(0, x, true);
  return view.getBigUint64(0, true).toString(16).padStart(16, "0");
}
const vec = (v) => (v ? [num(v.x), num(v.y), num(v.z)] : null);

const { buildMap } = mapBuilder({ random, int, pick, chance, span });

// Babylon-space points (y up) around the map's interesting places.
function makePoints(map) {
  const q3 = (p) => ({ x: p[0], y: p[2], z: p[1] });
  return () => {
    const r = random();
    if (r < 0.3) return q3([span(-800, 800), span(-800, 800), span(-80, 400)]);
    if (r < 0.55) return q3([span(-700, 700), span(-700, 700), pick([24, 24.03125, 25, 40, 24 + span(0, 60)])]);
    const list = r < 0.85 || !map.inline.length ? map.world : pick(map.inline);
    const b = pick(list);
    return q3(b.min.map((m, k) => span(m - 40, b.max[k] + 40)));
  };
}

const traceOptions = () => pick([
  undefined,
  undefined,
  { radius: 16, height: 64 },
  { radius: span(0, 20), height: span(0, 80) },
  { radius: 0, height: 0 },
  { contentMask: pick([1, 0x10000, 0x20, 0x10001, 0x38, -1, 0x40000000]) },
  { radius: 8, height: 16, contentMask: 1 },
]);

function traceResult(r) {
  return [
    num(r.fraction), vec(r.endPos), vec(r.normal), r.startSolid, r.allSolid, r.contents,
    r.hitBrushIndex, r.hitTextureName, r.hitSurfaceFlags,
  ];
}

const babylon = (p) => new Vector3(p.x, p.y, p.z);
const vectors = [];
for (let m = 0; m < 120; m++) {
  const map = buildMap();
  const buffer = map.bytes.buffer.slice(0);
  const bsp = BspLoader.loadCollision(buffer);
  const backend = new SweepCollisionBackend();
  await backend.init(bsp);
  const shared = new SharedCollisionWorld(new SweepBackendAdapter(backend));
  const point = makePoints(map);
  const near = (p, reach) => ({ x: p.x + span(-reach, reach), y: p.y + span(-reach, reach), z: p.z + span(-reach, reach) });
  const lifts = backend.lifts?.definitions ?? [];
  const explosives = backend.brushModels?.definitions ?? [];
  const modelChoice = () => (chance(0.7) ? -1 : pick([0, 99, ...lifts.map((d) => d.modelIndex), ...explosives.map((d) => d.modelIndex)]));
  const ops = [];
  for (let q = 0; q < 70; q++) {
    const kind = pick([
      "trace", "trace", "trace", "trace", "contents", "ladder", "slide", "step", "step", "ground", "onGround",
      "lifts", "lifts", "toggle", "worldTrace", "worldStep", "worldStep", "worldContents", "worldLadder",
    ]);
    const op = { kind };
    if (kind === "trace") {
      const start = point();
      const end = chance(0.15) ? start : chance(0.5) ? point() : near(start, 64);
      const options = traceOptions();
      const exclude = modelChoice();
      const only = chance(0.85) ? -1 : modelChoice();
      Object.assign(op, { start: vec(start), end: vec(end), options: options && { ...options, radius: options.radius === undefined ? undefined : num(options.radius), height: options.height === undefined ? undefined : num(options.height) }, exclude, only });
      op.out = traceResult(backend.trace(babylon(start), babylon(end), options, exclude, only));
    } else if (kind === "contents") {
      const p = point();
      const mask = chance(0.7) ? undefined : pick([1, 0x20, 0x10000, 0x38]);
      Object.assign(op, { point: vec(p), mask });
      op.out = backend.pointContents(babylon(p), mask);
    } else if (kind === "ladder" || kind === "worldLadder") {
      const min = point();
      const max = { x: min.x + span(0, 40), y: min.y + span(0, 70), z: min.z + span(0, 40) };
      Object.assign(op, { min: vec(min), max: vec(max) });
      op.out = kind === "ladder" ? backend.isInLadder(babylon(min), babylon(max)) : shared.isInLadder(min, max);
    } else if (kind === "slide" || kind === "step") {
      let start = point();
      let velocity = { x: span(-40, 40), y: chance(0.3) ? span(-40, 10) : chance(0.5) ? 0 : span(-4, 4), z: span(-40, 40) };
      if (chance(0.5)) {
        // Walk at a stair from the floor beside it.
        const stair = pick(map.world.filter((b) => b.stair));
        const centre = [(stair.min[0] + stair.max[0]) / 2, (stair.min[1] + stair.max[1]) / 2];
        const axis = int(2);
        const sign = chance(0.5) ? 1 : -1;
        const from = [...centre];
        from[axis] = (sign > 0 ? stair.max[axis] : stair.min[axis]) + sign * span(17, 30);
        start = { x: from[0], y: pick([24, 24.03125, 24.125]), z: from[1] };
        const speed = span(4, 40);
        velocity = { x: axis === 0 ? -sign * speed : span(-8, 8), y: pick([0, 0, -0.5, -6]), z: axis === 1 ? -sign * speed : span(-8, 8) };
      }
      const options = chance(0.6) ? undefined : traceOptions();
      Object.assign(op, { start: vec(start), velocity: vec(velocity), options: options && { ...options, radius: options.radius === undefined ? undefined : num(options.radius), height: options.height === undefined ? undefined : num(options.height) } });
      const r = kind === "slide" ? backend.slideMove(babylon(start), babylon(velocity), options) : backend.stepSlideMove(babylon(start), babylon(velocity), options);
      op.out = [vec(r.endPos), vec(r.velocity), r.hitCount, r.stepped ?? null];
    } else if (kind === "ground") {
      const p = point();
      const distance = pick([2, 100, 300, 0]);
      Object.assign(op, { point: vec(p), distance });
      const g = backend.findGround(babylon(p), distance);
      op.out = g && [vec(g.position), vec(g.normal)];
    } else if (kind === "onGround") {
      const p = point();
      op.point = vec(p);
      op.out = backend.isOnGround(babylon(p));
    } else if (kind === "lifts") {
      const dt = pick([0.008, 0.008, 0.05, 0.016, 0, -1, NaN, 0.2]);
      const actors = [];
      for (let a = int(4); a > 0; a--) {
        let position = point();
        if (lifts.length && chance(0.7)) {
          const li = int(lifts.length);
          const d = lifts[li];
          const model = bsp.models[d.modelIndex];
          const offset = backend.lifts.travelAt(li);
          position = {
            x: d.origin.x + span(model.mins[0], model.maxs[0]),
            y: d.origin.y + offset + model.maxs[2] + pick([24, 24.5, 30, span(-30, 60)]),
            z: d.origin.z + span(model.mins[1], model.maxs[1]),
          };
        }
        const actor = { position, velocity: { x: span(-50, 50), y: span(-200, 50), z: span(-50, 50) } };
        if (chance(0.7)) actor.id = int(100);
        if (chance(0.3)) actor.passive = chance(0.5);
        actors.push(actor);
      }
      op.dt = num(dt);
      op.actors = actors.map((a) => [vec(a.position), vec(a.velocity), a.id ?? null, a.passive ?? null]);
      const crushes = backend.lifts ? backend.lifts.step(dt, actors) : [];
      op.out = {
        crushes: crushes.map((c) => [c.id ?? null, c.modelIndex, num(c.damage)]),
        actors: actors.map((a) => [vec(a.position), vec(a.velocity)]),
        offsets: (backend.lifts?.offsets() ?? []).map(num),
        phases: lifts.map((d) => backend.lifts.phase(d.modelIndex)),
      };
    } else if (kind === "toggle") {
      const modelIndex = explosives.length && chance(0.8) ? pick(explosives).modelIndex : int(8);
      const enabled = chance(0.5);
      Object.assign(op, { model: modelIndex, enabled });
      backend.brushModels?.setEnabled(modelIndex, enabled);
    } else if (kind === "worldTrace") {
      const start = point();
      const end = chance(0.5) ? point() : near(start, 64);
      const half = pick([null, 16, 8, 2]);
      const bounds = half === null ? undefined : { min: { x: -half, y: -24, z: -half }, max: { x: half, y: pick([32, 40, 8]), z: half } };
      const mask = chance(0.6) ? undefined : pick([1, 0x10000, 0x10001]);
      Object.assign(op, { start: vec(start), end: vec(end), bounds: bounds && [vec(bounds.min), vec(bounds.max)], mask });
      const r = shared.trace(start, end, bounds, mask);
      op.out = [r.hit, num(r.fraction), vec(r.hitPoint), vec(r.normal), r.contents];
    } else if (kind === "worldStep") {
      const start = chance(0.6) ? { x: span(-700, 700), y: pick([24, 24.03125, 26, span(24, 80)]), z: span(-700, 700) } : point();
      const velocity = { x: span(-500, 500), y: pick([0, 0, span(-600, 0), span(0, 300), 0.05]), z: span(-500, 500) };
      const dt = pick([0.008, 0.016, 0.008, 0]);
      const small = chance(0.3);
      const bounds = small
        ? { min: { x: -8, y: -8, z: -8 }, max: { x: 8, y: 8, z: 8 } }
        : { min: { x: -16, y: -24, z: -16 }, max: { x: 16, y: 32, z: 16 } };
      Object.assign(op, { start: vec(start), velocity: vec(velocity), dt: num(dt), bounds: [vec(bounds.min), vec(bounds.max)] });
      const r = shared.stepSlideMove(start, velocity, dt, bounds);
      op.out = [vec(r.endPos), vec(r.velocity), r.grounded];
    } else if (kind === "worldContents") {
      const p = point();
      op.point = vec(p);
      op.out = shared.pointContents(p);
    }
    ops.push(op);
  }
  const s = backend.getStats();
  const stats = { traceCount: s.traceCount, hitCount: s.hitCount, ...s.backendStats };
  delete stats.isFBSP;
  vectors.push({ bytes: Buffer.from(map.bytes).toString("hex"), ops, stats });
}

const doc = { generator: "tools/golden/collision.mjs", shag: shagCommit, node: process.version, vectors };
writeFileSync(resolve(outArg, "collision.json"), `${JSON.stringify(doc).replaceAll('},{"bytes"', '},\n{"bytes"')}\n`);
