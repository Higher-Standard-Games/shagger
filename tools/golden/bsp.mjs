// BSP golden vectors: synthetic IBSP and FBSP files, and what SHAG's own
// BspLoader makes of them (load and loadCollision). Run by generate.mjs:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs bsp.mjs <shag> <out dir> <shag commit>
//
// No real maps are needed: the files are built record by record, with the
// damage real files can carry (lumps outside the file, partial records, bad
// patch ranges, odd worldspawn keys). Floats are f64 bit patterns in hex.
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [shagArg, outArg, shagCommit] = process.argv.slice(2);
const shag = resolve(shagArg);
const { BspLoader, findLeafForPoint, isClusterVisible, getBspUsedTextures } = await import(
  pathToFileURL(resolve(shag, "src/engine/loader/bsp/BspLoader.ts")).href
);
console.warn = () => {}; // version and light grid warnings
console.log = () => {};

let seed = 0x1b873593;
function random() {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  return (seed >>> 0) / 0x1_0000_0000;
}
const int = (n) => Math.floor(random() * n);
const pick = (list) => list[int(list.length)];
const chance = (p) => random() < p;

const view = new DataView(new ArrayBuffer(8));
function num(x) {
  if (Number.isNaN(x)) return "nan";
  view.setFloat64(0, x, true);
  return view.getBigUint64(0, true).toString(16).padStart(16, "0");
}
function canon(value) {
  if (typeof value === "number") return num(value);
  if (value instanceof Uint8Array) return digest(value);
  if (Array.isArray(value)) return Array.from(value, (v) => (v === undefined ? null : canon(v)));
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, canon(v)]));
  return value;
}
/** Length and a 31-polynomial hash, so big pages stay small in the file. */
function digest(bytes) {
  let h = 0;
  for (const b of bytes) h = (Math.imul(h, 31) + b) >>> 0;
  return `${bytes.length}:${h.toString(16)}`;
}

class Writer {
  bytes = [];
  u8(v) { this.bytes.push(v & 0xff); }
  u16(v) { this.u8(v); this.u8(v >> 8); }
  i32(v) { for (let k = 0; k < 4; k++) this.u8(v >> (8 * k)); }
  f32(v) { view.setFloat32(0, v, true); for (let k = 0; k < 4; k++) this.u8(view.getUint8(k)); }
  raw(list) { for (const b of list) this.u8(b); }
  name(text) { const b = Buffer.from(text, "utf8").subarray(0, 64); this.raw(b); for (let k = b.length; k < 64; k++) this.u8(chance(0.9) || k === b.length ? 0 : 65); }
}

const floats = [0, -0, 1, -1, 0.5, 1e-6, -1e-6, 9e-6, 1.1e-5, 1e-5, -1e-5, 1.00000005e-5, 0.9999999e-5, 64, -64, 128.25, 3e38, Infinity, NaN, 1e-40];
const f = () => (chance(0.3) ? pick(floats) : (random() - 0.5) * 2 ** int(14));
const small = () => (chance(0.1) ? pick([-1, -2, 0x7fffffff, -0x80000000]) : int(8));
const names = ["textures/base/wall", "textures/common/clip", "Textures/COMMON/caulk", "", "é/ü", "a".repeat(64), "﻿bom"];

function entityText(fbsp) {
  const lines = [];
  const world = ['"classname" "worldspawn"'];
  if (chance(0.5)) world.push(`"gridsize" "${pick(["64 64 128", "32 32 64", " 64 64 128", "64 64", "0 64 64", "16\t16\t16", "64 64 128 1", "1e1 2e1 3e1", "abc", "", "1.5x 2 3"])}"`);
  if (fbsp && chance(0.5)) world.push(`"${pick(["_lightmapImageSize", "_LIGHTMAPIMAGESIZE", "_lightmapimagesize"])}" "${pick(["4", "8", "0x8", " 4 ", "16", "3", "0", "-4", "", "4.0", "1e1", "2"])}"`);
  if (chance(0.3)) world.push('"message" "Arena \\"one\\""');
  lines.push("{", ...world, "}");
  for (let i = int(4); i > 0; i--) {
    lines.push(pick(["{", " { ", "{\r"]));
    lines.push(`"classname" "${pick(["info_player_deathmatch", "worldspawn", "item_armor", ""])}"`);
    if (chance(0.5)) lines.push(`"origin" "${int(1000)} ${int(1000)} ${int(100)}"`);
    if (chance(0.3)) lines.push(pick(['"" "empty key"', '"k"  "v" "w"', '  "spaced"\t"tab"  ', '"__proto__" "x"', '"2" "two"', '"1" "one"', '"origin" "dup"', 'junk "a" "b"', '"a""b"', '"x" y "z" "q"']));
    lines.push(pick(["}", " }", "}\r"]));
  }
  if (chance(0.2)) lines.push("{", '"classname" "unterminated"');
  let text = lines.join(pick(["\n", "\r\n"]));
  if (chance(0.7)) text += "\0";
  return Buffer.from((chance(0.1) ? "﻿" : "") + text, "utf8");
}

function buildBsp() {
  const fbsp = chance(0.5);
  const lumps = [];
  let w;
  const lump = (fill) => { w = new Writer(); fill(); lumps.push(w.bytes); };
  const n = () => int(6);
  lump(() => w.raw(entityText(fbsp)));
  lump(() => { for (let i = n(); i > 0; i--) { w.name(pick(names)); w.i32(small()); w.i32(small()); } });
  lump(() => { for (let i = n() + 2; i > 0; i--) { w.f32(f()); w.f32(f()); w.f32(f()); w.f32(f()); } });
  const nodes = int(5);
  lump(() => { for (let i = 0; i < nodes; i++) { w.i32(int(4)); w.i32(chance(0.5) && i + 1 < nodes ? i + 1 : -1 - int(3)); w.i32(-1 - int(3)); for (let k = 0; k < 6; k++) w.i32(int(2000) - 1000); } });
  lump(() => { for (let i = n() + 1; i > 0; i--) for (let k = 0; k < 12; k++) w.i32(k < 2 ? int(6) - 1 : int(200)); });
  lump(() => { for (let i = n(); i > 0; i--) w.i32(small()); });
  lump(() => { for (let i = n(); i > 0; i--) w.i32(small()); });
  lump(() => { for (let i = chance(0.9) ? 1 + int(3) : 0; i > 0; i--) { for (let k = 0; k < 6; k++) w.f32(chance(0.1) ? pick(floats) : (random() - 0.5) * 8000); for (let k = 0; k < 4; k++) w.i32(small()); } });
  lump(() => { for (let i = n(); i > 0; i--) { w.i32(small()); w.i32(small()); w.i32(small()); } });
  lump(() => { for (let i = n(); i > 0; i--) { w.i32(small()); w.i32(small()); if (fbsp) w.i32(small()); } });
  const vertexCount = int(12);
  lump(() => { for (let i = 0; i < vertexCount; i++) { const floatsPer = fbsp ? 16 : 10; for (let k = 0; k < floatsPer; k++) w.f32(f()); for (let k = 0; k < (fbsp ? 16 : 4); k++) w.u8(int(256)); } });
  lump(() => { for (let i = n(); i > 0; i--) w.i32(small()); });
  lump(() => { for (let i = n(); i > 0; i--) { w.name(pick(names)); w.i32(small()); w.i32(small()); } });
  lump(() => {
    for (let i = n(); i > 0; i--) {
      const type = pick([1, 2, 2, 3, 4, 7]);
      const ints = fbsp ? 7 + 1 + 1 + 4 + 4 + 4 + 2 : 12;
      const vals = Array.from({ length: fbsp ? 37 : 26 }, () => f());
      for (let k = 0; k < (fbsp ? 37 : 26); k++) {
        if (k === 2) w.i32(type);
        else if (k === 3) w.i32(chance(0.1) ? -1 : int(vertexCount + 2));
        else if (k >= (fbsp ? 35 : 24)) w.i32(chance(0.1) ? pick([-1, 100, 0x10000]) : 1 + int(3));
        else if (fbsp && (k === 7 || k === 8)) w.raw([int(256), int(256), 255, 255]);
        else if (k < ints && !(fbsp && k >= 23 && k < 35)) w.i32(small());
        else w.f32(vals[k]);
      }
    }
  });
  lump(() => {
    // IBSP pages are 128x128; FBSP pages follow _lightmapImageSize, default 512.
    const text = Buffer.from(lumps[0]).toString("utf8");
    const m = /"_lightmapimagesize" "([^"]*)"/i.exec(text);
    const side = fbsp ? (m ? Number(m[1]) : 512) : 128;
    const page = Number.isSafeInteger(side) && side > 0 && side <= 16 ? side * side * 3 : 0;
    const pages = side === 128 ? (chance(0.04) ? 1 : 0) : page ? int(3) : 0;
    for (let i = 0; i < pages * (side === 128 ? 49152 : page); i++) w.u8(int(256));
    if (chance(0.05)) w.u8(1);
  });
  lump(() => {
    if (fbsp) { for (let i = chance(0.05) ? 1 : 0; i > 0; i--) w.u8(0); for (let i = int(4); i > 0; i--) for (let k = 0; k < 30; k++) w.u8(k >= 24 && k < 28 && chance(0.4) ? 255 : int(256)); }
    else for (let i = n() * 8 + (chance(0.05) ? 3 : 0); i > 0; i--) w.u8(int(256));
  });
  lump(() => {
    if (chance(0.3)) return;
    const clusters = chance(0.1) ? -1 : int(4);
    const bpc = chance(0.1) ? 100 : 1;
    w.i32(clusters); w.i32(bpc);
    for (let i = Math.max(0, clusters * bpc) + (chance(0.1) ? 2 : 0); i > 0; i--) w.u8(int(256));
    if (chance(0.05)) w.bytes.length = int(8);
  });
  if (fbsp) lump(() => { for (let i = int(6); i > 0; i--) w.u16(chance(0.2) ? 0xffff : int(3)); if (chance(0.05)) w.u8(0); });

  // Assemble with the directory.
  const header = 8 + 8 * lumps.length;
  const out = new Writer();
  out.u8(0x49 - (fbsp ? 3 : 0)); out.raw([0x42, 0x53, 0x50]);
  out.i32(fbsp ? (chance(0.9) ? 1 : 2) : chance(0.9) ? 46 : 47);
  let offset = header;
  const directory = lumps.map((l) => { const d = [offset, l.length]; offset += l.length; return d; });
  if (chance(0.08)) directory[int(directory.length)][0] = pick([-4, offset + 10, 0x7fffffff]);
  if (chance(0.05)) directory[int(directory.length)][1] = pick([-1, 1 << 30]);
  for (const [o, l] of directory) { out.i32(o); out.i32(l); }
  for (const l of lumps) out.raw(l);
  let bytes = Uint8Array.from(out.bytes);
  if (chance(0.03)) bytes = bytes.subarray(0, int(Math.min(bytes.length, header + 4)));
  if (chance(0.02)) bytes[0] = 0x58;
  return bytes;
}

function run(fn) {
  try {
    return { ok: canon(fn()) };
  } catch (error) {
    return { error: String(error.message) };
  }
}

const vectors = [];
for (let i = 0; i < 250; i++) {
  const bytes = buildBsp();
  const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const vector = { bytes: Buffer.from(bytes).toString("hex") };
  vector.load = run(() => BspLoader.load(buffer));
  vector.collision = run(() => BspLoader.loadCollision(buffer));
  if (vector.load.ok) {
    const bsp = BspLoader.load(buffer);
    vector.queries = [];
    for (let q = 0; q < 6; q++) {
      const point = [f(), f(), f()];
      const a = int(6) - 1;
      const b = int(40) - 1;
      let leaf;
      try {
        const found = findLeafForPoint(point, bsp.nodes, bsp.leafs, bsp.planes);
        leaf = found ? bsp.leafs.indexOf(found) : null;
      } catch {
        leaf = "throws";
      }
      vector.queries.push({ point: point.map(num), leaf, a, b, visible: isClusterVisible(bsp.visData, a, b) });
    }
    vector.usedTextures = getBspUsedTextures(bsp);
  }
  vectors.push(vector);
}
const doc = { generator: "tools/golden/bsp.mjs", shag: shagCommit, node: process.version, vectors };
writeFileSync(resolve(outArg, "bsp_loader.json"), `${JSON.stringify(doc).replaceAll('},{"bytes"', '},\n{"bytes"')}\n`);
