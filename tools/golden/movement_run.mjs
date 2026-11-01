// The stepping loop shared by movement.mjs (synthetic maps) and
// movement_maps.mjs (the shipped maps): scripted input for one player, stepped
// the way V2NetworkClient predicts (sanitize, step the lifts with the player on
// them, step knockback drag, simulate), with a canonical line per step that
// holds the sanitized input, every result field and the whole saved state.
const view = new DataView(new ArrayBuffer(8));
export function num(x) {
  if (Number.isNaN(x)) return "nan";
  view.setFloat64(0, x, true);
  return view.getBigUint64(0, true).toString(16).padStart(16, "0");
}
export const vec = (v) => [num(v.x), num(v.y), num(v.z)];

export const B = { FORWARD: 1, BACK: 2, LEFT: 4, RIGHT: 8, JUMP: 16, CROUCH: 32, SPRINT: 64, WALK: 128, FIRE: 256, ALT_FIRE: 512 };
export const DT = 0.008;

export function fnv(text) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193) >>> 0;
  return h.toString(16).padStart(8, "0");
}

/**
 * `rng` holds the caller's random helpers, `lib` the SHAG and wash-fps
 * functions, `cover` an optional feature counter, `print` stderr output.
 */
export function movementRunner({ rng, lib, cover, print }) {
  const { random, int, pick, chance, span } = rng;
  const { sanitizeMovementOrZero, applyKnockbackDrag } = lib;
  const dump = process.env.DUMP?.split(",").map(Number);

  /** The harness's wish direction: forward (sin yaw, cos yaw), right (cos yaw, -sin yaw). */
  function wish(buttons, yaw, scale) {
    const f = (buttons & B.FORWARD ? 1 : 0) - (buttons & B.BACK ? 1 : 0);
    const s = (buttons & B.RIGHT ? 1 : 0) - (buttons & B.LEFT ? 1 : 0);
    let x = Math.sin(yaw) * f + Math.cos(yaw) * s;
    let z = Math.cos(yaw) * f - Math.sin(yaw) * s;
    const len = Math.sqrt(x * x + z * z);
    if (len > 0) {
      x = (x / len) * scale;
      z = (z / len) * scale;
    }
    return { x, y: 0, z };
  }

  const PATTERNS = [
    () => B.FORWARD,
    () => B.FORWARD | B.SPRINT,
    () => B.FORWARD | B.SPRINT | B.JUMP,
    () => B.FORWARD | B.JUMP,
    () => pick([B.LEFT, B.RIGHT]) | B.JUMP,
    () => B.FORWARD | pick([B.LEFT, B.RIGHT]),
    () => B.FORWARD | B.CROUCH,
    () => B.FORWARD | B.SPRINT | B.CROUCH,
    () => B.CROUCH,
    () => B.JUMP,
    () => 0,
    () => B.BACK | pick([0, B.JUMP]),
    () => B.FORWARD | B.FIRE | pick([0, B.ALT_FIRE]),
    () => int(0x400),
    () => int(0x10000) | (chance(0.1) ? 0x30000 : 0),
  ];

  const q = (x) => Math.round(x * 1024) / 1024;

  function canonical(input, r, s, liftOffsets, kbAge) {
    const v = (x) => (x === undefined ? "u" : x === null ? "n" : typeof x === "boolean" ? (x ? "1" : "0") : typeof x === "number" ? num(x) : typeof x === "string" ? x : vec(x).join(" "));
    const parts = [
      input.direction.x, input.direction.z, input.pitch, input.yaw, input.buttons,
      r.position, r.velocity, r.grounded, r.isSliding, r.isCrouching, r.wallJumpState, r.ledgeGrabState,
      r.actualHorizSpeed, r.crouchViewOffset, r.slidePhase, r.ledgeGrabPhase, r.jumppadLaunch, r.teleported,
      r.teleportYaw, r.teleportFrom, r.triggerId, r.waterLevel, r.liquidContents, r.liquidKind, r.onLadder,
      r.ladderClimbDelta,
      s.isGrounded, s.wasGroundedLastFrame, s.jumpConsumed, s.waterLevel, s.waterJumpUntilMs, s.proneInWater,
      s.inLadderVolume, s.climbing, s.ladderDetachUntilMs, s.simNowMs, s.actualHorizSpeed, s.smoothedY,
    ];
    const w = s.wallJump;
    parts.push(w.state, w.normal, w.position, w.preVelocity, w.keyReleased, w.chargeStartMs, w.graceUntilMs, w.groundGraceUntilMs, w.cooldowns.length);
    for (const c of w.cooldowns) parts.push(c.startMs, c.untilMs);
    const sl = s.slide;
    parts.push(sl.isSliding, sl.isCrouching, sl.crouchViewOffset, sl.slidePhase, sl.slideStartMs, sl.cooldownUntilMs, sl.slideEndMs, sl.slideEndPhase, sl.crouchAnchorMs, sl.crouchFromOffset, sl.crouchTargetOffset);
    const l = s.ledgeGrab;
    parts.push(l.state, l.phase, l.stateStartMs, l.cooldownUntilMs, l.targetPos, l.startPos);
    parts.push(liftOffsets.length, ...liftOffsets, kbAge);
    return parts.map(v).join(",");
  }

  /**
   * Steps `count` commands from `spot` and returns the run's record. `m` and
   * `run` only name the run for DUMP=map,run,step. `record` turns a step's
   * position, velocity and line hash into what the vectors keep.
   */
  function simulateRun({ backend, world, profile, spot, m, run, count, record }) {
    let position = { x: spot.at.x, y: spot.at.y, z: spot.at.z };
    let velocity = chance(0.6) ? { x: 0, y: 0, z: 0 } : { x: span(-300, 300), y: span(-200, 300), z: span(-300, 300) };
    let grounded = chance(0.5);
    profile.setGrounded(grounded);
    const startVelocity = vec(velocity);
    const startGrounded = grounded;
    let yaw = spot.yaw !== undefined && chance(0.7) ? spot.yaw : q(span(-Math.PI, Math.PI));
    let pitch = spot.kind === "pool" || spot.kind === "ladder" ? q(span(-1.2, 1.2)) : q(span(-0.3, 0.3));
    const clock = pick(["simStep", "simStep", "sequence", "none"]);
    const base = int(5000);
    let kbAge = null;
    const steps = [];
    const inputs = [];
    let pattern = spot.kind === "trigger" ? B.FORWARD | B.SPRINT : PATTERNS[0]();
    let left = spot.kind === "trigger" ? 60 : 0;
    let tap = 0;
    let slideAt = -1;
    let segment = 0;
    for (let step = 0; step < count; step++) {
      if (left-- <= 0) {
        pattern = pick(PATTERNS)();
        left = 8 + int(40);
        tap = chance(0.4) ? 2 + int(10) : 0;
        slideAt = chance(0.15) ? 30 + int(30) : -1;
        if (slideAt >= 0) {
          pattern = B.FORWARD | B.SPRINT;
          left = slideAt + 20 + int(60);
          tap = 0;
        }
        segment = 0;
        if (chance(0.3)) yaw = q(span(-Math.PI, Math.PI));
        if (chance(0.3)) pitch = q(span(-1.4, 1.4));
      }
      yaw = q(yaw + pick([0, 0, 0, 1, -1, 4, -4]) / 1024);
      let buttons = pattern;
      if (slideAt >= 0 && segment++ >= slideAt) buttons |= B.CROUCH;
      if (tap && step % tap >= tap / 2) buttons &= ~B.JUMP;
      let rawYaw = yaw;
      let rawPitch = pitch;
      let scale = 1;
      if (chance(0.01)) { rawYaw = pick([NaN, Infinity, 10.5, -7.25]); }
      if (chance(0.01)) { rawPitch = pick([3, -3, 1.6]); }
      if (chance(0.02)) scale = pick([0.5, 3, 0]);
      const op = chance(0.01) ? pick(["setGrounded", "reset", "bonus", "knock"]) : "";
      const opArg = chance(0.5);
      const raw = { direction: wish(buttons, rawYaw, scale), pitch: rawPitch, yaw: rawYaw, buttons };
      inputs.push([buttons, num(rawYaw), num(rawPitch), num(scale), op, opArg]);

      if (op === "setGrounded") profile.setGrounded(opArg);
      else if (op === "reset") profile.resetPhysicsState(opArg);
      else if (op === "bonus") profile.speedBonus = opArg ? 25 : 0;
      else if (op === "knock") {
        velocity.x += opArg ? 400 : -250;
        velocity.y += 300;
        kbAge = opArg ? 0 : 60;
      }

      const clean = sanitizeMovementOrZero(raw);
      const seq = base + step;
      const input = {
        direction: clean.direction,
        viewAngles: { pitch: clean.pitch, yaw: clean.yaw },
        buttons: clean.buttons,
        sequence: clock === "none" ? undefined : seq,
        simStep: clock === "simStep" ? seq : undefined,
        isPressed: (b) => (clean.buttons & b) !== 0,
      };
      const actor = { position, velocity, id: 1 };
      backend.lifts?.step(DT, [actor]);
      if (kbAge !== null) kbAge = applyKnockbackDrag(velocity, grounded, kbAge + 1, DT);
      const r = profile.simulate(position, velocity, input, DT, world);
      position = { ...r.position };
      velocity = { ...r.velocity };
      grounded = r.grounded;
      const line = canonical(clean, r, profile.saveState(), backend.lifts?.offsets() ?? [], kbAge);
      if (cover) {
        const c = (k) => (cover[k] = (cover[k] ?? 0) + 1);
        if (r.isSliding) c("slide");
        if (r.wallJumpState === "charging") c("wallCharge");
        if (r.wallJumpState === "available") c("wallAvail");
        if (r.ledgeGrabState !== "none") c(`ledge_${r.ledgeGrabState}`);
        if (r.waterLevel >= 2) c("swim");
        if (r.waterLevel === 1) c("wade");
        if (r.onLadder) c("ladder");
        if (r.teleported) c("teleport");
        if (r.jumppadLaunch) c("jumppad");
        if (profile.saveState().waterJumpUntilMs > 0) c("waterJump");
        if (profile.saveState().proneInWater) c("prone");
        if (kbAge !== null) c("knock");
        if (r.grounded) c("grounded");
        c("steps");
      }
      if (dump && dump[0] === m && dump[1] === run && dump[2] === step) print(line);
      steps.push(record(position, velocity, fnv(line)));
    }
    return { spot: spot.kind, start: [num(spot.at.x), num(spot.at.y), num(spot.at.z)], startVelocity, grounded: startGrounded, clock, base, inputs, steps };
  }

  return { simulateRun, PATTERNS, wish };
}
