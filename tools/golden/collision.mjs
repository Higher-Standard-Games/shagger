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

class Writer {
  bytes = [];
  u8(v) { this.bytes.push(v & 0xff); }
  i32(v) { for (let k = 0; k < 4; k++) this.u8(v >> (8 * k)); }
  f32(v) { view.setFloat32(0, v, true); for (let k = 0; k < 4; k++) this.u8(view.getUint8(k)); }
  raw(list) { for (const b of list) this.u8(b); }
  name(text) { const b = Buffer.from(text, "utf8").subarray(0, 64); this.raw(b); for (let k = b.length; k < 64; k++) this.u8(0); }
}

const WALL = 0;
function textureTable() {
  return [
    ["textures/base/wall", 0, 1],
    ["textures/common/clip", 0, pick([0x10000, 1, 0x10001, 0])],
    ["textures/common/playerclip", 0, pick([0x10000, 0])],
    ["textures/common/FullClip", 0, pick([0x10000, 0, 1])],
    ["textures/liquids/water", pick([0, 0x4000]), 0x20],
    ["textures/common/ladder", 0x8 | pick([0, 0x4000, 0x100]), pick([0x1, 0x10000, 0, 0x20])],
    ["textures/common/hint", 0x100, pick([0, 1])],
    ["textures/liquids/lava", 0, 0x8],
    ["textures/sfx/ghost", 0x4000, 1],
    ["textures/liquids/slime", 0, 0x10 | pick([0, 1])],
    ["textures/common/trigger", 0, 0x40000000],
    ["textures/base/detail_CLIPPED", pick([0, 0x8]), 0x8000001 | pick([0, 0x20000])],
    ["textures/common/monsterclip", 0, 0x20000],
  ];
}
const brushTexture = () => (chance(0.55) ? WALL : int(13));

/** Axial box sides, Q3 axes, and optionally a slanted bevel cutting a corner. */
function boxSides(min, max, bevel) {
  const sides = [
    [[1, 0, 0], max[0]], [[-1, 0, 0], -min[0]],
    [[0, 1, 0], max[1]], [[0, -1, 0], -min[1]],
    [[0, 0, 1], max[2]], [[0, 0, -1], -min[2]],
  ];
  if (bevel) {
    let n;
    do n = [int(3) - 1, int(3) - 1, int(3) - 1];
    while (n.filter((c) => c !== 0).length < 2);
    const len = Math.hypot(...n);
    n = n.map((c) => c / len);
    const corner = n.map((c, k) => (c > 0 ? max[k] : min[k]));
    sides.push([n, n[0] * corner[0] + n[1] * corner[1] + n[2] * corner[2] - span(2, 24)]);
  }
  if (chance(0.15)) sides.sort(() => random() - 0.5);
  return sides;
}

function randomBox(lo, hi, size) {
  const min = lo.map((l, k) => Math.round(span(l, hi[k])));
  const max = min.map((m) => m + Math.round(span(size[0], size[1])));
  return { min, max };
}

function buildMap() {
  const textures = textureTable();
  const world = [];
  const brush = (min, max, texture, opts = {}) => {
    world.push({ min, max, texture, sides: boxSides(min, max, opts.bevel ?? chance(0.25)), ...opts });
    return world.at(-1);
  };
  brush([-1024, -1024, -64], [1024, 1024, 0], WALL, { bevel: false, everywhere: true });
  for (let i = 2 + int(5); i > 0; i--) {
    const b = randomBox([-600, -600, 0], [600, 600, 0], [32, 128]);
    b.max[2] = pick([8, 16, 18, 19, 20, 24, 40]);
    brush(b.min, b.max, chance(0.8) ? WALL : brushTexture(), { bevel: chance(0.15), stair: true });
  }
  for (let i = 8 + int(12); i > 0; i--) {
    const b = randomBox([-700, -700, -40], [700, 700, 300], [8, 256]);
    brush(b.min, b.max, brushTexture());
  }
  // Ramps: a slanted top over a box.
  for (let i = int(3); i > 0; i--) {
    const b = randomBox([-500, -500, 0], [500, 500, 0], [64, 192]);
    b.max[2] = Math.round(span(16, 96));
    const rise = b.max[2] - 0;
    const run = b.max[0] - b.min[0];
    const len = Math.hypot(rise, run);
    const n = [rise / len, 0, run / len];
    const sides = boxSides(b.min, b.max, false);
    sides.push([n, n[0] * b.max[0] + n[2] * 0]);
    world.push({ min: b.min, max: b.max, texture: WALL, sides });
  }
  // Nested copies: strictly inside an earlier brush, same texture, same leaves.
  for (let i = int(4); i > 0; i--) {
    const outer = pick(world.slice(1));
    const min = outer.min.map((m, k) => m + Math.min(1 + int(8), (outer.max[k] - m) / 4));
    const max = outer.max.map((m, k) => m - Math.min(1 + int(8), (m - outer.min[k]) / 4));
    if (min.every((m, k) => m < max[k])) world.push({ min, max, texture: outer.texture, sides: boxSides(min, max, false), nestedIn: outer });
  }
  // A three-sided brush, and a brush with a bad shader falling back to its first side.
  if (chance(0.3)) {
    const b = randomBox([-500, -500, 0], [500, 500, 100], [16, 64]);
    world.push({ min: b.min, max: b.max, texture: WALL, sides: boxSides(b.min, b.max, false).slice(0, 3) });
  }
  if (chance(0.4)) {
    const b = randomBox([-500, -500, 0], [500, 500, 100], [16, 64]);
    brush(b.min, b.max, pick([-1, 999]), { firstSideTexture: brushTexture() });
  }

  // Inline models: lifts, doors, explosives, and the trigger volumes that call lifts.
  const entities = ['{\n"classname" "worldspawn"\n}'];
  const inline = [];
  const model = (brushes, fields) => {
    inline.push(brushes);
    const index = inline.length;
    entities.push(`{\n${Object.entries({ ...fields, model: `*${index}` }).map(([k, v]) => `"${k}" "${v}"`).join("\n")}\n}`);
    return index;
  };
  const liftBox = (localOrigin) => {
    const b = randomBox([-400, -400, 0], [400, 400, 0], [48, 128]);
    b.max[2] = Math.round(span(8, 64));
    if (localOrigin) {
      for (let k = 0; k < 3; k++) { b.max[k] -= b.min[k]; b.min[k] = 0; }
    }
    return { min: b.min, max: b.max, texture: chance(0.9) ? WALL : brushTexture(), sides: boxSides(b.min, b.max, chance(0.2)) };
  };
  for (let i = int(4); i > 0; i--) {
    const local = chance(0.4);
    const b = liftBox(local);
    const fields = {};
    if (local) fields.origin = `${int(800) - 400} ${int(800) - 400} ${int(64)}`;
    const kind = pick(["func_plat", "func_plat", "func_door"]);
    fields.classname = kind;
    if (kind === "func_door") fields.angle = pick(["-1", "-2"]);
    if (chance(0.5)) fields.height = String(pick([32, 64, 128, 200, 0]));
    if (chance(0.3)) fields.lip = String(pick([8, 0, 16]));
    if (chance(0.3)) fields.spawnflags = String(int(4));
    if (chance(0.3)) fields.speed = String(pick([100, 200, 400]));
    if (chance(0.3)) fields.dmg = String(pick([2, 10]));
    if (chance(0.5)) fields.targetname = `lift${i}`;
    const extra = chance(0.2) ? [liftBox(local)] : [];
    model([b, ...extra], fields);
    if (fields.targetname && chance(0.8)) {
      const t = randomBox([-500, -500, 0], [500, 500, 0], [32, 96]);
      t.max[2] = 64;
      const triggerClass = pick(["trigger_multiple", "trigger_once", "func_button"]);
      model([{ min: t.min, max: t.max, texture: 10, sides: boxSides(t.min, t.max, false) }], {
        classname: triggerClass,
        target: fields.targetname,
        ...(chance(0.4) ? { wait: String(pick([0, 1, -1, 3])) } : {}),
      });
    }
  }
  for (let i = int(3); i > 0; i--) {
    const local = chance(0.4);
    const b = liftBox(local);
    b.max[2] = Math.round(span(32, 128));
    b.sides = boxSides(b.min, b.max, chance(0.2));
    const fields = { classname: "func_explosive" };
    if (local) fields.origin = `${int(800) - 400} ${int(800) - 400} ${int(64)}`;
    if (chance(0.3)) fields.spawnflags = "1";
    model([b], fields);
  }

  // Patches: curved control grids, a few with sizes that cannot tessellate.
  const vertices = [];
  const faces = [];
  for (let i = int(5); i > 0; i--) {
    const [w, h] = chance(0.85) ? pick([[3, 3], [5, 3], [3, 5], [5, 5]]) : pick([[2, 2], [4, 3], [3, 1], [1, 1]]);
    const origin = [span(-500, 400), span(-500, 400), span(0, 120)];
    const size = [span(48, 256), span(48, 256)];
    const lift = span(-64, 96);
    const vertical = chance(0.3);
    const first = vertices.length;
    for (let r = 0; r < h; r++) {
      for (let c = 0; c < w; c++) {
        const u = w > 1 ? c / (w - 1) : 0;
        const v = h > 1 ? r / (h - 1) : 0;
        const bump = lift * Math.sin(Math.PI * u) * (vertical ? 1 : Math.sin(Math.PI * v));
        vertices.push(vertical
          ? [origin[0] + u * size[0], origin[1] + bump, origin[2] + v * size[1]]
          : [origin[0] + u * size[0], origin[1] + v * size[1], origin[2] + bump]);
      }
    }
    faces.push({ texture: pick([WALL, WALL, 2, 4, 1]), type: 2, first, count: w * h - (chance(0.05) ? 1 : 0), size: [w, h] });
  }
  if (chance(0.5)) faces.push({ texture: WALL, type: 1, first: 0, count: 0, size: [0, 0] });
  const worldFaces = faces.length;
  // One patch in a lift model, which the world must skip.
  let liftFace = -1;
  if (inline.length && chance(0.4)) {
    liftFace = faces.length;
    const first = vertices.length;
    for (let r = 0; r < 3; r++) for (let c = 0; c < 3; c++) vertices.push([c * 32, r * 32, c === 1 ? 16 : 0]);
    faces.push({ texture: WALL, type: 2, first, count: 9, size: [3, 3] });
  }

  // Lumps.
  const planes = [];
  const sides = [];
  const brushes = [];
  const addBrush = (b) => {
    const first = sides.length;
    b.sides.forEach(([n, d], k) => {
      planes.push([n, d]);
      const texture = k === 0 && b.firstSideTexture !== undefined ? b.firstSideTexture : chance(0.08) ? brushTexture() : b.texture;
      sides.push([planes.length - 1, texture]);
    });
    brushes.push([first, b.sides.length, b.texture]);
  };
  world.forEach(addBrush);
  const models = [];
  const bounds = (list) => {
    const min = [Infinity, Infinity, Infinity];
    const max = [-Infinity, -Infinity, -Infinity];
    for (const b of list) for (let k = 0; k < 3; k++) { min[k] = Math.min(min[k], b.min[k]); max[k] = Math.max(max[k], b.max[k]); }
    return { min, max };
  };
  models.push({ ...bounds(world), firstFace: 0, numFaces: worldFaces, firstBrush: 0, numBrushes: world.length });
  inline.forEach((list, i) => {
    const firstBrush = brushes.length;
    list.forEach(addBrush);
    const face = i === 0 && liftFace >= 0;
    models.push({ ...bounds(list), firstFace: face ? liftFace : 0, numFaces: face ? 1 : 0, firstBrush, numBrushes: list.length });
  });

  // The tree: random split planes, leaves holding random subsets.
  const nodes = [];
  const leafs = [];
  const leafBrushes = [];
  const leafFaces = [];
  const nodeCount = 1 + int(7);
  const leaf = () => {
    const brushList = [];
    world.forEach((b, i) => {
      if (b.everywhere || b.nestedIn || chance(0.75)) brushList.push(i);
    });
    // A nested brush must share its parent's leaves; keep the parent with it.
    for (const i of brushList) {
      const outer = world[i].nestedIn;
      if (outer && !brushList.includes(world.indexOf(outer))) brushList.push(world.indexOf(outer));
    }
    if (inline.length && chance(0.2)) brushList.push(world.length + int(brushes.length - world.length));
    if (chance(0.1)) brushList.push(pick([-1, 99999]));
    const faceList = [];
    for (let f = 0; f < faces.length; f++) if (f !== liftFace ? chance(0.8) : chance(0.5)) faceList.push(f);
    if (chance(0.1)) faceList.push(pick([-1, 99999]));
    leafs.push([leafBrushes.length, brushList.length, leafFaces.length, faceList.length]);
    leafBrushes.push(...brushList);
    leafFaces.push(...faceList);
    return -leafs.length;
  };
  for (let i = 0; i < nodeCount; i++) {
    let n;
    let d;
    if (chance(0.7)) {
      const axis = int(3);
      n = [0, 0, 0];
      n[axis] = chance(0.8) ? 1 : -1;
      d = Math.round(span(-400, 400)) * n[axis];
      if (axis === 2) d = Math.round(span(-32, 200)) * n[axis];
    } else {
      n = [span(-1, 1), span(-1, 1), span(-0.5, 0.5)];
      const len = Math.hypot(...n);
      n = n.map((c) => c / len);
      d = span(-300, 300);
    }
    planes.push([n, d]);
    nodes.push([planes.length - 1]);
  }
  for (let i = 0; i < nodeCount; i++) {
    const a = 2 * i + 1;
    const b = 2 * i + 2;
    nodes[i].push(a < nodeCount ? a : leaf(), b < nodeCount ? b : leaf());
  }

  const lumps = [];
  let w;
  const lump = (fill) => { w = new Writer(); fill(); lumps.push(w.bytes); };
  lump(() => w.raw(Buffer.from(entities.join("\n"), "utf8")));
  lump(() => textures.forEach(([name, surface, content]) => { w.name(name); w.i32(surface); w.i32(content); }));
  lump(() => planes.forEach(([n, d]) => { n.forEach((c) => w.f32(c)); w.f32(d); }));
  lump(() => nodes.forEach(([p, a, b]) => { w.i32(p); w.i32(a); w.i32(b); for (let k = 0; k < 3; k++) w.i32(-2048); for (let k = 0; k < 3; k++) w.i32(2048); }));
  lump(() => leafs.forEach(([fb, nb, ff, nf]) => { w.i32(0); w.i32(0); for (let k = 0; k < 3; k++) w.i32(-2048); for (let k = 0; k < 3; k++) w.i32(2048); w.i32(ff); w.i32(nf); w.i32(fb); w.i32(nb); }));
  lump(() => leafFaces.forEach((f) => w.i32(f)));
  lump(() => leafBrushes.forEach((b) => w.i32(b)));
  lump(() => models.forEach((m) => { m.min.forEach((c) => w.f32(c)); m.max.forEach((c) => w.f32(c)); w.i32(m.firstFace); w.i32(m.numFaces); w.i32(m.firstBrush); w.i32(m.numBrushes); }));
  lump(() => brushes.forEach(([first, count, texture]) => { w.i32(first); w.i32(count); w.i32(texture); }));
  lump(() => sides.forEach(([plane, texture]) => { w.i32(plane); w.i32(texture); }));
  lump(() => vertices.forEach((p) => { p.forEach((c) => w.f32(c)); for (let k = 0; k < 7; k++) w.f32(0); w.raw([255, 255, 255, 255]); }));
  lump(() => {});
  lump(() => {});
  lump(() => faces.forEach((f) => {
    w.i32(f.texture); w.i32(-1); w.i32(f.type); w.i32(f.first); w.i32(f.count); w.i32(0); w.i32(0); w.i32(-1);
    for (let k = 0; k < 4; k++) w.i32(0);
    for (let k = 0; k < 12; k++) w.f32(k === 11 ? 1 : 0);
    w.i32(f.size[0]); w.i32(f.size[1]);
  }));
  lump(() => {});
  lump(() => {});
  lump(() => {});

  const header = 8 + 8 * lumps.length;
  const out = new Writer();
  out.raw([0x49, 0x42, 0x53, 0x50]);
  out.i32(46);
  let offset = header;
  for (const l of lumps) { out.i32(offset); out.i32(l.length); offset += l.length; }
  for (const l of lumps) out.raw(l);
  return { bytes: Uint8Array.from(out.bytes), world, inline };
}

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
