// Highlight reel golden vectors: synthetic matches, what SHAG's DemoAnalyzer
// finds in them, the reel SequenceBuilder cuts from that, and every shot's
// camera path from CinematicCameraPaths. Run by generate.mjs:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs highlights.mjs <shag> <out dir> <shag commit>
//
// Input numbers are plain JSON (multiples of 1/8, exact either side); outputs
// are f64 bit patterns in hex.
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [shagArg, outArg, shagCommit] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { DemoAnalyzer } = await load("src/shared/demo/DemoAnalyzer.ts");
const { SequenceBuilder } = await load("src/shared/demo/SequenceBuilder.ts");
const { generateCameraPath, interpolatePath } = await load("src/shared/demo/CinematicCameraPaths.ts");

let seed = 0x6a09e667;
function random() {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  return (seed >>> 0) / 0x1_0000_0000;
}
const int = (n) => Math.floor(random() * n);
// "|| 0": JSON has no -0, so the inputs never hold one.
const eighth = (x) => Math.round(x * 8) / 8 || 0;
const pick = (list) => list[int(list.length)];

const view = new DataView(new ArrayBuffer(8));
function bits(x) {
  if (Number.isNaN(x)) return "nan";
  view.setFloat64(0, x, true);
  return view.getBigUint64(0, true).toString(16).padStart(16, "0");
}
const vec = (v) => (v ? [bits(v.x), bits(v.y), bits(v.z)] : null);

const MODES = ["walk", "walk", "bhop", "slide", "walljump", "rocket", "still"];

/** One synthetic match: players running about in modes that make (or don't make) highlights. */
function scenario(index) {
  const tickRate = pick([20, 20, 30, 60, 0]);
  const rate = tickRate || 20;
  const ticks = 200 + int(400);
  const firstTick = int(3) === 0 ? 0 : 1 + int(5000);
  const count = 2 + int(3);
  const arena = pick([300, 1200, 2400]);
  const players = [];
  for (let i = 0; i < count; i++) {
    players.push({
      raw: String(i + 1),
      name: pick(["Ramen", "Noodlesoup", "Washiki!", "äö", "", "bot_7"]) + (i ? i : ""),
      inHeader: int(4) !== 0,
      pos: { x: eighth((random() - 0.5) * arena), y: eighth(random() * 200), z: eighth((random() - 0.5) * arena) },
      yaw: eighth((random() - 0.5) * 6),
      mode: "walk",
      modeLeft: 0,
      dead: 0,
      phase: 0,
    });
  }
  const header = { tickRate, players: players.filter((p) => p.inHeader).map((p) => ({ playerId: p.raw, name: p.name })) };
  const frames = [];
  let firstKill = true;
  for (let n = 0; n < ticks; n++) {
    const tick = firstTick + n;
    if (int(40) === 0) continue; // a dropped frame
    const entities = [];
    const events = [];
    for (const p of players) {
      if (p.dead > 0) {
        p.dead--;
        continue;
      }
      if (p.modeLeft-- <= 0) {
        p.mode = pick(MODES);
        p.modeLeft = Math.round(rate * (0.5 + random() * 3));
        p.phase = 0;
        p.yaw = eighth(p.yaw + (random() - 0.5) * 2);
      }
      p.phase++;
      let speed = 0;
      let vy = 0;
      let grounded = true;
      let sliding = false;
      let charging = false;
      switch (p.mode) {
        case "walk":
          speed = 300;
          break;
        case "bhop":
          speed = 480 + int(300);
          grounded = p.phase % Math.max(2, Math.round(rate / 2)) === 0;
          vy = grounded ? 0 : 200;
          break;
        case "slide":
          speed = 460 + int(100);
          sliding = p.phase > 2 && p.phase < rate;
          grounded = !(p.phase > rate && p.phase < rate * 1.5);
          break;
        case "walljump":
          speed = p.phase % rate < 3 ? 0 : 500;
          charging = p.phase % rate < 3;
          grounded = false;
          vy = charging ? 0 : 420;
          break;
        case "rocket":
          speed = 350 + int(400);
          grounded = p.phase < 3;
          vy = grounded ? 0 : 600 - p.phase * 20;
          break;
        default:
          break;
      }
      const vx = eighth(Math.sin(p.yaw) * speed);
      const vz = eighth(Math.cos(p.yaw) * speed);
      p.pos = {
        x: eighth(p.pos.x + vx / rate),
        y: eighth(Math.max(0, p.pos.y + (grounded ? -p.pos.y * 0.5 : vy / rate))),
        z: eighth(p.pos.z + vz / rate),
      };
      const shape = int(30);
      entities.push({
        id: `player_${p.raw}`,
        transform: shape === 0 ? null : { position: { ...p.pos }, pitch: eighth(random() - 0.5), yaw: p.yaw },
        velocity: shape === 1 ? null : { x: vx, y: vy, z: vz, grounded },
        movement: shape === 2 ? null : { isSliding: sliding, charging },
      });
    }
    if (int(25) === 0) entities.push({ id: `proj_${int(9)}`, transform: { position: { x: 1, y: 2, z: 3 }, pitch: 0, yaw: 0 }, velocity: null, movement: null });
    // Kills: someone alive kills someone else alive, sometimes from beyond the frame
    const alive = players.filter((p) => p.dead === 0);
    if (alive.length >= 2 && int(Math.max(4, Math.round(rate * 4))) === 0) {
      const flying = alive.filter((p) => p.mode === "rocket" && p.phase > 3);
      const killer = flying.length && int(2) === 0 ? pick(flying) : pick(alive);
      const victim = pick(alive.filter((p) => p !== killer));
      const killerId = int(20) === 0 ? "" : int(20) === 0 ? `player_${victim.raw}` : `player_${killer.raw}`;
      if (int(3) === 0) events.push({ type: "combat_state", entityId: killerId, health: pick([100, 25, 10, 0, 60]) });
      if (int(3) === 0) {
        const awardType = firstKill ? "first_blood" : pick(["on_fire", "raging", "unstoppable", "excellent"]);
        events.push({ type: "award", playerId: killer.raw, awardType });
        if (int(2) === 0) events.push({ type: "award", playerId: killer.raw, awardType });
      }
      firstKill = false;
      // Sometimes the victim saw it coming: their last frame faced the killer
      if (int(3) === 0) {
        const last = entities.find((e) => e.id === `player_${victim.raw}`);
        if (last?.transform) last.transform.yaw = Math.atan2(killer.pos.x - victim.pos.x, killer.pos.z - victim.pos.z) || 0;
      }
      events.push({
        type: "death",
        entityId: `player_${victim.raw}`,
        killerId,
        weaponId: pick(["rocket", "rail", "", "shotgun"]),
        x: victim.pos.x,
        y: victim.pos.y,
        z: victim.pos.z,
      });
      // The victim leaves the next frame (most of the time) and comes back later
      if (int(5) !== 0) victim.dead = 1 + Math.round(rate * random() * 3);
    }
    if (int(30) === 0) events.push({ type: "chat", text: "gg" });
    const playerStates = players.filter(() => int(2) === 0).map((p) => ({ raw: p.raw, name: p.name }));
    frames.push({ tick, entities, events, players: playerStates });
  }
  return { index, tickRate, header, frames, ground: int(2) === 0, minutes: pick([1, 2, 3, 0.5]) };
}

function toTsFrames(s) {
  return s.frames.map((f) => {
    const entities = new Map();
    for (const e of f.entities) {
      const c = {};
      if (e.transform) c.transform = { position: { ...e.transform.position }, rotation: { x: e.transform.pitch, y: e.transform.yaw, z: 0, w: 1 } };
      if (e.velocity) {
        const v = e.velocity;
        c.velocity = { x: v.x, y: v.y, z: v.z, linear: { x: v.x, y: v.y, z: v.z }, grounded: v.grounded };
      }
      if (e.movement) c.movementState = { isSliding: e.movement.isSliding, wallJumpState: e.movement.charging ? "charging" : "none" };
      entities.set(e.id, { getComponent: (name) => c[name] });
    }
    const players = new Map(f.players.map((p) => [p.raw, { name: p.name }]));
    return { tick: f.tick, state: { entities, players, events: f.events } };
  });
}

/** A frame as written: entities as [id, transform, velocity, movement], 0 for a missing one. */
function compact(f) {
  return {
    tick: f.tick,
    entities: f.entities.map((e) => [
      e.id,
      e.transform ? [e.transform.position.x, e.transform.position.y, e.transform.position.z, e.transform.pitch, e.transform.yaw] : 0,
      e.velocity ? [e.velocity.x, e.velocity.y, e.velocity.z, e.velocity.grounded ? 1 : 0] : 0,
      e.movement ? [e.movement.isSliding ? 1 : 0, e.movement.charging ? 1 : 0] : 0,
    ]),
    events: f.events,
    players: f.players.map((p) => [p.raw, p.name]),
  };
}

/** The clearance the Rust test gives back: height over y = 0, unknown west of x = 0. */
const ground = (origin) => (origin.x < 0 ? null : origin.y);

function highlight(h) {
  return {
    startTick: h.startTick,
    peakTick: h.peakTick,
    endTick: h.endTick,
    score: bits(h.score),
    type: h.type,
    primaryPlayer: h.primaryPlayer,
    primaryPlayerName: h.primaryPlayerName,
    victimPlayer: h.victimPlayer ?? null,
    victimPlayerName: h.victimPlayerName ?? null,
    weaponId: h.weaponId ?? null,
    killCount: h.killCount ?? null,
    tags: h.tags,
    killerPos: vec(h.killerPos),
    victimPos: vec(h.victimPos),
    killerVelocity: vec(h.killerVelocity),
    killerYaw: h.killerYaw === undefined ? null : bits(h.killerYaw),
    victimFacedKiller: h.victimFacedKiller ?? null,
    path: h.path ? h.path.map((p) => [p.tick, bits(p.x), bits(p.y), bits(p.z)]) : null,
  };
}

const vectors = [];
for (let i = 0; i < 32; i++) {
  const s = scenario(i);
  const frames = toTsFrames(s);
  const analysis = new DemoAnalyzer(s.ground ? ground : undefined).analyze(frames, s.header, 123456);
  const reel = new SequenceBuilder().build(analysis, s.minutes, s.tickRate, s.header);
  const rate = reel.tickRate;
  const segments = reel.segments.map((seg) => {
    const path = generateCameraPath(seg.cameraPreset, seg.highlight, seg.startTick, seg.endTick, rate);
    const samples = [];
    if (path.type === "freefly") {
      for (let k = -1; k <= 9; k++) {
        const tick = seg.startTick + ((seg.endTick - seg.startTick) * k) / 8 + (k === 3 ? 0.37 : 0);
        const pose = interpolatePath(path.keyframes, tick, path.interpolation);
        samples.push([bits(tick), vec(pose.position), vec(pose.lookAt), bits(pose.fov)]);
      }
    }
    return {
      startTick: seg.startTick,
      endTick: seg.endTick,
      durationMs: bits(seg.durationMs),
      cameraPreset: seg.cameraPreset,
      cameraTarget: seg.cameraTarget ?? null,
      playbackSpeed: bits(seg.playbackSpeed),
      speedKeyframes: (seg.speedKeyframes ?? []).map((k) => [k.tick, bits(k.speed)]),
      highlight: analysis.highlights.indexOf(seg.highlight),
      isReplay: seg.isReplay,
      segmentIndex: seg.segmentIndex,
      scene: seg.scene,
      role: seg.role,
      transitionIn: seg.transitionIn,
      transitionOut: seg.transitionOut,
      transitionDurationMs: bits(seg.transitionDurationMs),
      path:
        path.type === "chase"
          ? { chase: path.targetEntityId }
          : {
              interpolation: path.interpolation,
              keyframes: path.keyframes.map((k) => [bits(k.tick), vec(k.position), vec(k.lookAt), k.fov ?? null]),
              samples,
            },
    };
  });
  vectors.push({
    input: { tickRate: s.tickRate, header: s.header, frames: s.frames.map(compact), ground: s.ground, minutes: s.minutes },
    analysis: {
      highlights: analysis.highlights.map(highlight),
      totalTicks: analysis.totalTicks,
      playerNames: [...analysis.playerNames],
      frags: [...analysis.frags],
    },
    reel: {
      totalDurationMs: bits(reel.totalDurationMs),
      totalSegments: reel.totalSegments,
      totalScenes: reel.totalScenes,
      tickRate: reel.tickRate,
      segments,
    },
  });
}

const doc = { generator: "tools/golden/highlights.mjs", shag: shagCommit, node: process.version, vectors };
writeFileSync(resolve(outArg, "highlights.json"), `${JSON.stringify(doc)}\n`);
const kinds = new Map();
for (const v of vectors) for (const h of v.analysis.highlights) kinds.set(h.type, (kinds.get(h.type) ?? 0) + 1);
const tags = new Map();
for (const v of vectors) for (const h of v.analysis.highlights) for (const t of h.tags) tags.set(t, (tags.get(t) ?? 0) + 1);
const presets = new Map();
for (const v of vectors) for (const s of v.reel.segments) presets.set(s.cameraPreset, (presets.get(s.cameraPreset) ?? 0) + 1);
console.error("highlights:", [...kinds], [...tags], [...presets]);
