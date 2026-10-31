// Protocol golden vectors: bytes, and what SHAG's own BinaryProtocol and
// MessageCodec decode them to. Run by generate.mjs, which passes the SHAG
// checkout and the output directory:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs protocol.mjs <shag> <out dir>
//
// Floats are written as their f64 bit pattern in hex ("nan" for any NaN, since
// V8 does not keep NaN payloads). Maps are arrays of [key, value] pairs in
// insertion order.
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [shagArg, outArg, shagCommit, eventsOutArg] = process.argv.slice(2);
const shag = resolve(shagArg);
const out = resolve(outArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { BinaryProtocol, BinaryEncoder, BinaryDecoder } = await load("src/shared/net/BinaryProtocol.ts");
const { MessageCodec } = await load("src/engine/network/protocol/MessageCodec.ts");
const { GameState } = await load("src/shared/net/GameState.ts");
const { Entity } = await load("src/shared/net/Entity.ts");

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

const view = new DataView(new ArrayBuffer(8));
function num(x) {
  if (Number.isNaN(x)) return "nan";
  view.setFloat64(0, x, true);
  return view.getBigUint64(0, true).toString(16).padStart(16, "0");
}
const hex = (bytes) => Buffer.from(bytes).toString("hex");
const vec = (v) => [num(v.x), num(v.y), num(v.z)];

// --- Values ------------------------------------------------------------------
const specialFloats = [0, -0, 1, -1, 0.1, 1e-45, 3.4028234663852886e38, 3.5e38, -3.5e38, 1e-46, Infinity,
  -Infinity, NaN, 2 ** -149, 2 ** -126, 16777217];
function float() {
  if (chance(0.15)) return pick(specialFloats);
  if (chance(0.1)) {
    view.setUint32(0, int(0x1_0000_0000), true);
    return view.getFloat32(0, true);
  }
  return (random() - 0.5) * 10 ** int(6);
}
const strings = ["", "p1", "p2", "bot_7", "item_12", "proj_3", "é", "日本", "🙂", "﻿bom", "a".repeat(300)];
const ids = ["p1", "p2", "bot_7", "item_12", "proj_3", "é", ""];

// --- Strings -----------------------------------------------------------------
function stringVectors() {
  const cases = [[], [0xef, 0xbb, 0xbf], [0xef, 0xbb, 0xbf, 0x41], [0xef, 0xbb, 0xbf, 0xef, 0xbb, 0xbf], [0xff],
    [0xc0, 0x80], [0xed, 0xa0, 0x80], [0xf4, 0x90, 0x80, 0x80], [0xe2, 0x82], [0xf0, 0x9f, 0x99], [0x80, 0x41],
    [0xe2, 0x28, 0xa1], [0xf8, 0x88, 0x80, 0x80, 0x80], [0x00]];
  for (const s of strings) cases.push([...Buffer.from(s, "utf8")]);
  for (let i = 0; i < 400; i++) {
    const n = int(12);
    const bytes = [];
    for (let j = 0; j < n; j++) {
      bytes.push(chance(0.5) ? 0x20 + int(0x60) : chance(0.5) ? 0x80 + int(0x40) : 0xc0 + int(0x40));
    }
    cases.push(bytes);
  }
  return cases.map((bytes) => {
    const e = new BinaryEncoder();
    e.writeUint16(bytes.length);
    for (const b of bytes) e.writeUint8(b);
    const s = new BinaryDecoder(e.getData()).readString();
    return { bytes: hex(bytes), units: Array.from({ length: s.length }, (_, i) => s.charCodeAt(i)) };
  });
}

// --- Raw snapshots written field by field ------------------------------------
// Covers every flag combination, repeated ids and truncation, which the
// server's encoder never produces but the decoder must handle like SHAG's.
function writeWireSnapshot(e) {
  e.writeUint32(int(0x1_0000_0000));
  e.writeFloat64(chance(0.9) ? 1.7e12 + int(1e9) : float());
  e.writeUint32(int(0x1_0000_0000));
  const entities = int(7);
  e.writeUint16(entities);
  for (let i = 0; i < entities; i++) {
    e.writeString(pick(ids));
    const flags = int(256);
    e.writeUint8(flags);
    if (flags & 0x01) for (let k = 0; k < 3; k++) e.writeFloat32(float());
    if (flags & 0x02) for (let k = 0; k < 3; k++) e.writeFloat32(float());
    if (flags & 0x04) e.writeUint8(chance(0.5) ? int(2) : int(256));
    if (flags & 0x08) for (let k = 0; k < 2; k++) e.writeFloat32(float());
    if (flags & 0x10) {
      const mov = int(256);
      e.writeUint8(mov);
      if (mov & 0x80) for (let k = 0; k < 4; k++) e.writeUint8(int(256));
    }
    if (flags & 0x20) {
      e.writeString(pick(strings));
      e.writeUint8(chance(0.5) ? int(2) : int(256));
    }
    if (flags & 0x40) e.writeUint8(int(256));
  }
  const players = int(5);
  e.writeUint16(players);
  for (let i = 0; i < players; i++) {
    e.writeString(pick(ids));
    const flags = int(256);
    e.writeUint8(flags);
    if (flags & 0x01) e.writeString(pick(ids));
    if (flags & 0x02) e.writeString(pick(strings));
    if (flags & 0x04) e.writeInt32(int(0x1_0000_0000) - 0x8000_0000);
    if (flags & 0x08) e.writeUint32(int(0x1_0000_0000));
    if (flags & 0x10) e.writeUint16(int(0x10000));
    if (flags & 0x20) e.writeString(pick(["", "red", "blue"]));
  }
  // Codes no game defines: SHAG's switch reads nothing for them.
  const events = int(3);
  e.writeUint16(events);
  for (let i = 0; i < events; i++) e.writeUint8(chance(0.3) ? 0 : 26 + int(230));
  if (chance(0.7)) {
    const count = pick([0, 1, 7, 8, 9, 16, 33, int(80)]);
    e.writeUint16(count);
    for (let b = 0; b < Math.ceil(count / 8); b++) e.writeUint8(int(256));
  }
}

function canonicalState(state) {
  const entities = [...state.entities].map(([id, entity]) => {
    const c = {};
    const pos = entity.getComponent("position");
    if (pos) c.position = vec(pos);
    const t = entity.getComponent("transform");
    if (t) c.transform = { position: vec(t.position), rotation: [...vec(t.rotation), num(t.rotation.w)] };
    const v = entity.getComponent("velocity");
    if (v) c.velocity = { linear: vec(v.linear), grounded: v.grounded, isCrouching: v.isCrouching, isSliding: v.isSliding };
    const m = entity.getComponent("movementState");
    if (m) {
      c.movementState = { isCrouching: m.isCrouching, isSliding: m.isSliding, wallJumpState: m.wallJumpState, isAiming: m.isAiming };
      if (m.strafeInput !== undefined) {
        c.movementState.view = [m.strafeInput, m.forwardInput, num(m.slidePhase), num(m.ledgeGrabPhase), num(m.crouchViewOffset)];
      }
    }
    const w = entity.getComponent("weaponMeta");
    if (w) c.weaponMeta = { weaponId: w.weaponId, settled: w.settled };
    const p = entity.getComponent("powerups");
    if (p) c.powerups = p.mask;
    return [id, c];
  });
  const players = [...state.players].map(([id, p]) => [id, {
    entityId: p.entityId, name: p.name, team: p.team, score: p.score, deaths: p.deaths, ping: p.ping,
  }]);
  return {
    tick: state.tick,
    serverTimestamp: num(state.serverTimestamp),
    ackedInputSeq: state.ackedInputSeq,
    entities,
    players,
    events: state.events.length,
    pickupActiveMask: state.pickupActiveMask.map((b) => (b ? "1" : "0")).join(""),
  };
}

function snapshotVectors() {
  const vectors = [];
  for (let chain = 0; chain < 60; chain++) {
    let baseline = null;
    let baselineIndex = null;
    const length = 1 + int(8);
    for (let k = 0; k < length; k++) {
      const e = new BinaryEncoder();
      writeWireSnapshot(e);
      let bytes = e.getData();
      if (chance(0.1)) bytes = bytes.subarray(0, int(bytes.length));
      if (chance(0.05)) bytes = Uint8Array.of(...bytes, ...Array.from({ length: 1 + int(4) }, () => int(256)));
      const vector = { bytes: hex(bytes), baseline: baselineIndex };
      try {
        const state = BinaryProtocol.decodeSnapshot(bytes, baseline ?? undefined);
        vector.decoded = canonicalState(state);
        baseline = state;
        baselineIndex = vectors.length;
      } catch (error) {
        vector.error = String(error.message);
      }
      vectors.push(vector);
    }
  }
  return vectors;
}

// --- Snapshots encoded by the server's own encoder ---------------------------
function serverState(tick, previous) {
  const state = new GameState();
  state.tick = tick;
  state.ackedInputSeq = tick * 3;
  const entities = ["p1", "p2", "bot_7", "item_12"];
  for (const id of entities) {
    if (previous && chance(0.15)) continue;
    const entity = new Entity(id);
    const old = previous?.entities.get(id);
    const keep = (c) => old && chance(0.6) ? old.getComponent(c) : undefined;
    const position = keep("transform")?.position ?? { x: float(), y: float(), z: float() };
    entity.addComponent("transform", { position: { ...position }, rotation: keep("transform")?.rotation ?? { x: float(), y: float(), z: 0, w: 1 } });
    if (!id.startsWith("item_")) {
      entity.addComponent("velocity", keep("velocity") ?? { linear: { x: float(), y: float(), z: float() }, grounded: chance(0.5) });
      const movement = { isCrouching: chance(0.3), isSliding: chance(0.2), wallJumpState: pick(["none", "charging", "cooldown"]), isAiming: chance(0.3) };
      if (chance(0.5)) {
        Object.assign(movement, { strafeInput: pick([-1, 0, 1]), forwardInput: pick([-1, 0, 1]), slidePhase: random() * 1.2 - 0.1,
          ledgeGrabPhase: random(), crouchViewOffset: (random() - 0.5) * 80 });
      }
      entity.addComponent("movementState", movement);
      if (chance(0.5)) entity.addComponent("powerups", { mask: int(512) });
    } else {
      entity.addComponent("weaponMeta", keep("weaponMeta") ?? { weaponId: pick(["shotgun", "rail", ""]), settled: chance(0.5) });
    }
    state.entities.set(id, entity);
  }
  for (const id of ["p1", "p2", "bot_7"]) {
    const old = previous?.players.get(id);
    state.players.set(id, old && chance(0.5) ? { ...old } : {
      entityId: id, name: pick(strings), team: pick(["red", "blue", ""]), score: int(200) - 50, deaths: int(100), ping: int(400),
    });
  }
  state.pickupActiveMask = Array.from({ length: int(20) }, () => chance(0.5));
  return state;
}

function envelopeVectors() {
  const vectors = [];
  let server = null;
  const decoded = new Map();
  for (let k = 0; k < 120; k++) {
    if (k % 20 === 0) {
      server = null;
      decoded.clear();
    }
    const state = serverState(1000 + k, server);
    Date.now = () => 1_761_900_000_000 + k * 50; // the encoder stamps the snapshot; keep reruns identical
    const bytes = MessageCodec.encodeSnapshot(state, server ?? undefined);
    const vector = { bytes: hex(bytes) };
    try {
      const result = MessageCodec.decodeSnapshot(bytes, decoded);
      vector.decoded = canonicalState(result);
      decoded.set(result.tick, result);
    } catch (error) {
      vector.error = String(error.message);
    }
    vectors.push(vector);
    server = state;
  }
  // Headers SHAG refuses or cannot resolve.
  const raw = hex(MessageCodec.encodeSnapshot(serverState(5, null))).slice(12);
  for (const head of ["0b0000000000", "0b0100000000", "0b0001000000", "0b0200000000", "0b01e8030000", "0b01ffffffff",
    "040000000000", "0a0000000000", "0b00", ""]) {
    const bytes = Buffer.from(head + raw, "hex");
    const vector = { bytes: hex(bytes) };
    try {
      vector.decoded = canonicalState(MessageCodec.decodeSnapshot(bytes, decoded));
    } catch (error) {
      vector.error = String(error.message);
    }
    vectors.push(vector);
  }
  return vectors;
}

// --- Commands ----------------------------------------------------------------
function commandVectors() {
  const odd = [0, -0, -1, 1.5, -1.5, 65535, 65536, 70000.7, 2 ** 32, 2 ** 32 + 5, -(2 ** 31), 2 ** 53, 1e300, NaN, Infinity, -Infinity, 255.9, 256, -255];
  const value = () => (chance(0.4) ? pick(odd) : int(70000));
  const vectors = [];
  for (let i = 0; i < 300; i++) {
    const command = {
      direction: { x: float(), y: float(), z: float() },
      pitch: float(),
      yaw: float(),
      buttons: value(),
      tick: value(),
      weaponSlot: value(),
    };
    const sequence = chance(0.3) ? pick(odd) : int(0x1_0000_0000);
    const playerId = pick(strings);
    const bytes = MessageCodec.encodeCommand(playerId, sequence, command);
    const back = MessageCodec.decodeCommand(bytes);
    vectors.push({
      playerId,
      sequence: num(sequence),
      direction: vec(command.direction),
      pitch: num(command.pitch),
      yaw: num(command.yaw),
      buttons: num(command.buttons),
      tick: num(command.tick),
      weaponSlot: num(command.weaponSlot),
      bytes: hex(bytes),
      peek: MessageCodec.peekCommandSeq(bytes),
      decoded: {
        playerId: back.playerId,
        sequence: back.sequence,
        direction: vec(back.command.direction),
        pitch: num(back.command.pitch),
        yaw: num(back.command.yaw),
        buttons: back.command.buttons,
        tick: back.command.tick,
        weaponSlot: back.command.weaponSlot,
      },
    });
  }
  return vectors;
}

// --- Other messages ----------------------------------------------------------
function messageVectors() {
  const vectors = [];
  const add = (name, args, bytes, decode) => {
    const vector = { name, args, bytes: hex(bytes) };
    if (decode) vector.decoded = decode(bytes);
    vectors.push(vector);
  };
  const s = () => pick(strings);
  const maybe = () => (chance(0.5) ? undefined : s());
  for (let i = 0; i < 30; i++) {
    const w = [s(), s(), s(), int(256), int(256), int(256)];
    add("welcome", w, MessageCodec.encodeWelcome(...w), MessageCodec.decodeWelcome);
    const j = [s(), chance(0.5), maybe()];
    add("join", j, MessageCodec.encodeJoin(...j), MessageCodec.decodeJoin);
    const a = [s(), s(), s()];
    add("joinAccept", a, MessageCodec.encodeJoinAccept(...a), MessageCodec.decodeJoinAccept);
    const r = [s(), int(0x1_0000_0000)];
    add("resumeToken", r, MessageCodec.encodeResumeToken(...r), MessageCodec.decodeResumeToken);
    const t = [s()];
    add("resume", t, MessageCodec.encodeResume(...t), MessageCodec.decodeResume);
    const k = [int(0x1_0000_0000)];
    add("ack", k, MessageCodec.encodeAck(...k), MessageCodec.decodeAck);
    const g = [maybe(), chance(0.5), maybe()];
    add("joinGame", g, MessageCodec.encodeJoinGame(...g), MessageCodec.decodeJoinGame);
    const l = [s(), s()];
    add("setLoadout", l, MessageCodec.encodeSetLoadout(...l), MessageCodec.decodeSetLoadout);
    const p = [{ autoSwitch: chance(0.5) }];
    add("clientPrefs", p, MessageCodec.encodeClientPrefs(...p), MessageCodec.decodeClientPrefs);
    add("chat", t, MessageCodec.encodeChat(...t), MessageCodec.decodeChat);
    const m = [s(), pick(["info", "queue", "reject", ""])];
    add("serverMessage", m, MessageCodec.encodeServerMessage(...m), MessageCodec.decodeServerMessage);
    const time = chance(0.2) ? pick(specialFloats) : 1.7e12 + random() * 1e9;
    add("ping", [num(time)], MessageCodec.encodePing(time), (b) => ({ clientSendTime: num(MessageCodec.decodePing(b).clientSendTime) }));
    const server = 1.7e12 + random() * 1e9;
    add("pong", [num(time), num(server)], MessageCodec.encodePong(time, server), (b) => {
      const d = MessageCodec.decodePong(b);
      return { clientSendTime: num(d.clientSendTime), serverTime: num(d.serverTime) };
    });
  }
  add("ready", [], MessageCodec.encodeReady());
  add("spectate", [], MessageCodec.encodeSpectate());
  add("requestSpawn", [], MessageCodec.encodeRequestSpawn());
  // Shorter forms older peers send.
  add("joinShort", [], Uint8Array.of(0x81, 2, 0, 0x68, 0x69), MessageCodec.decodeJoin);
  add("clientPrefsShort", [], Uint8Array.of(0x8c), MessageCodec.decodeClientPrefs);
  return vectors;
}

// --- Washoi's snapshot events (written to wash-fps-rust) -----------------------
const jsonTexts = ['{"phase":"warmup","timeLeft":30}', '{"type":"override","b":[1,2.5,-0],"a":{"x":null}}', '{"2":"two","1":"one","z":0}',
  "[1,2]", "null", "7", "{}", '{"a":1,"a":2}', '{"s":"\\ud800"}', '{"bad":', "", "nope", '["blaster","shotgun"]',
  '{"blaster":50,"shotgun":-1}', '[{"kind":"move","to":[1,2,3]}]', "1e400", '{"deep":[[[[[]]]]]}'];
function writeEvent(e, code) {
  const str = () => e.writeString(pick(strings));
  const json = () => e.writeString(pick(jsonTexts));
  const f32 = (n) => { for (let i = 0; i < n; i++) e.writeFloat32(float()); };
  const u8 = () => e.writeUint8(int(256));
  const u16 = () => e.writeUint16(int(0x10000));
  switch (code) {
    case 1: str(); u16(); u16(); u8(); break;
    case 2: json(); break;
    case 3: str(); str(); json(); json(); u8(); e.writeUint32(int(0x1_0000_0000)); break;
    case 4: str(); str(); f32(6); break;
    case 24: str(); str(); f32(6); e.writeInt32(int(0x1_0000_0000) - 0x8000_0000); f32(1); break;
    case 5: str(); str(); u8(); break;
    case 6: str(); str(); break;
    case 7: str(); f32(6); break;
    case 8: e.writeInt16(int(0x10000) - 0x8000); str(); str(); str(); f32(3); break;
    case 9: str(); str(); f32(6); break;
    case 10: str(); str(); u16(); str(); f32(3); break;
    case 23: str(); str(); u16(); str(); f32(3); e.writeUint8(int(5)); break;
    case 25: str(); str(); u16(); str(); f32(3); e.writeUint8(int(5)); e.writeUint8(int(8)); break;
    case 11: str(); str(); str(); str(); u16(); f32(9); break;
    case 12: str(); str(); str(); str(); u16(); break;
    case 13: { const n = int(5); str(); e.writeUint8(n); f32(3 * n); break; }
    case 14: str(); str(); str(); str(); break;
    case 15: str(); str(); break;
    case 16: case 17: str(); f32(12); u8(); break;
    case 18: str(); str(); u16(); break;
    case 19: str(); f32(3); break;
    case 20: str(); json(); break;
    case 21: {
      const lifts = int(4);
      e.writeUint8(lifts);
      for (let l = 0; l < lifts; l++) {
        e.writeUint16(int(0x10000)); f32(1); e.writeUint8(int(6)); f32(1);
        const c = int(4); e.writeUint8(c); f32(c);
      }
      break;
    }
    case 22: str(); e.writeUint8(chance(0.3) ? 255 : int(256)); break;
  }
}
function canon(value) {
  if (typeof value === "number") return num(value);
  if (Array.isArray(value)) return value.map(canon);
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, canon(v)]));
  return value;
}
function eventVectors() {
  const vectors = [];
  for (let i = 0; i < 1500; i++) {
    const e = new BinaryEncoder();
    e.writeUint32(i);
    e.writeFloat64(0);
    e.writeUint32(0);
    e.writeUint16(0);
    e.writeUint16(0);
    const count = 1 + int(3);
    e.writeUint16(count);
    for (let k = 0; k < count; k++) {
      const code = i < 26 * 4 ? i % 26 : chance(0.05) ? 26 + int(230) : int(26);
      e.writeUint8(code);
      writeEvent(e, code);
    }
    let bytes = e.getData();
    if (chance(0.05)) bytes = bytes.subarray(0, 19 + int(bytes.length - 19));
    const vector = { bytes: hex(bytes) };
    try {
      vector.events = canon(BinaryProtocol.decodeSnapshot(bytes).events);
    } catch (error) {
      vector.error = String(error.message);
    }
    vectors.push(vector);
  }
  return vectors;
}
const files = {
  "strings.json": stringVectors(),
  "snapshots.json": snapshotVectors(),
  "snapshot_envelopes.json": envelopeVectors(),
  "commands.json": commandVectors(),
  "messages.json": messageVectors(),
};
for (const [name, vectors] of Object.entries(files)) {
  const doc = { generator: "tools/golden/protocol.mjs", shag: shagCommit, node: process.version, vectors };
  writeFileSync(resolve(out, name), `${JSON.stringify(doc, null, 0).replaceAll('},{"', '},\n{"')}\n`);
}

// After the engine files, so adding Washoi vectors leaves those unchanged.
if (eventsOutArg) {
  const doc = { generator: "shagger tools/golden/protocol.mjs", shag: shagCommit, node: process.version, vectors: eventVectors() };
  writeFileSync(resolve(eventsOutArg, "snapshot_events.json"), `${JSON.stringify(doc).replaceAll('},{"bytes"', '},\n{"bytes"')}\n`);
}
