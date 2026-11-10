// Player model golden vectors: Babylon's own retargeting and SHAG's own fit
// (`src/engine/animation/PlayerModelFit.ts`) on Vanguard, loaded as a
// packaged body whose clips come from another root. The SHAG checkout must
// have its dependencies installed (`npm ci`): Babylon, its glTF loader and
// its Draco decoder come from there.
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs \
//     player_model.mts <shag> <washoi_player_model.pk3 unpacked> <out dir>
//
// Writes `player_model_retarget.json` (Vanguard as it is) and
// `player_model_retarget_skew.json` (Vanguard with four joints' rest turned
// and the right hand scaled, so the clips' rig and the body's disagree), which
// shagger's `tests/player_model_golden.rs` reads.
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [shagArg, dataArg, outArg] = process.argv.slice(2);
const shag = resolve(shagArg);
const require = createRequire(resolve(shag, "package.json"));
const fromShag = (spec: string) => import(pathToFileURL(require.resolve(spec)).href);
const load = (path: string) => import(pathToFileURL(resolve(shag, path)).href);

const { NullEngine } = await fromShag("@babylonjs/core/Engines/nullEngine.js");
const { Scene } = await fromShag("@babylonjs/core/scene.js");
const { LoadAssetContainerAsync } = await fromShag("@babylonjs/core/Loading/sceneLoader.js");
const { AnimatorAvatar } = await fromShag("@babylonjs/core/Animations/animatorAvatar.js");
const { AnimationGroup } = await fromShag("@babylonjs/core/Animations/animationGroup.js");
const { Quaternion, Vector3 } = await fromShag("@babylonjs/core/Maths/math.vector.js");
const { DracoDecoder } = await fromShag("@babylonjs/core/Meshes/Compression/dracoDecoder.js");
await fromShag("@babylonjs/loaders/glTF/2.0/index.js");
const { measureBladeStance, measureGunSupport, measurePlayerModelFit } = await load(
  "src/engine/animation/PlayerModelFit.ts",
);

// The decoder SHAG serves (PlayerModelDecoder.ts), run in this process as
// the CommonJS script it becomes under Node.
const wrapperPath = require.resolve("@babylonjs/core/assets/Draco/draco_wasm_wrapper_gltf.js");
const DracoDecoderModule = new Function(
  "require",
  "__dirname",
  "__filename",
  "module",
  "exports",
  `${readFileSync(wrapperPath, "utf8")}\nreturn DracoDecoderModule;`,
)(require, dirname(wrapperPath), wrapperPath, { exports: {} }, {});
const wasm = readFileSync(require.resolve("@babylonjs/core/assets/Draco/draco_decoder_gltf.wasm"));
DracoDecoder.DefaultConfiguration = {
  jsModule: DracoDecoderModule,
  wasmBinary: wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength),
  wasmUrl: "unused",
  wasmBinaryUrl: "unused",
  numWorkers: 0,
};

const clipDir = resolve(dataArg, "models/players/vanguard");
const CLIPS = [
  ["idle", "rifle aiming idle.glb"],
  ["run", "rifle run.glb"],
  ["blade_slash", "blade_slash.glb"],
  // Vanguard's packages have no blade stance; the idle stands in for it.
  ["blade_idle", "rifle aiming idle.glb"],
];
const WATCH = [
  "mixamorig:Hips",
  "mixamorig:Spine",
  "mixamorig:RightHand",
  "mixamorig:LeftForeArm",
  "mixamorig:RightUpLeg",
  "mixamorig:Head",
];

/** Vanguard with some joints' rest turned and the right hand scaled. */
function skewed(glb: Buffer): Uint8Array {
  const jsonLength = glb.readUInt32LE(12);
  const doc = JSON.parse(glb.subarray(20, 20 + jsonLength).toString("utf8"));
  const turn = (axis: number[], angle: number) =>
    Quaternion.RotationAxis(new Vector3(axis[0], axis[1], axis[2]).normalize(), angle);
  const edits: Record<string, any> = {
    "mixamorig:Spine": turn([1, 0, 0], 0.4),
    "mixamorig:RightHand": turn([0, 1, 1], 0.9),
    "mixamorig:LeftArm": turn([0, 0, 1], -0.6),
    "mixamorig:RightUpLeg": turn([1, 1, 0], 0.3),
  };
  for (const node of doc.nodes) {
    const edit = edits[node.name];
    if (!edit) continue;
    const [x, y, z, w] = node.rotation ?? [0, 0, 0, 1];
    const q = new Quaternion(x, y, z, w).multiply(edit);
    node.rotation = [q.x, q.y, q.z, q.w];
    if (node.name === "mixamorig:RightHand") node.scale = [1.3, 1.3, 1.3];
  }
  let text = Buffer.from(JSON.stringify(doc), "utf8");
  if (text.length % 4) text = Buffer.concat([text, Buffer.alloc(4 - (text.length % 4), 0x20)]);
  const rest = glb.subarray(20 + jsonLength);
  const header = Buffer.alloc(20);
  glb.copy(header, 0, 0, 8);
  header.writeUInt32LE(20 + text.length + rest.length, 8);
  header.writeUInt32LE(text.length, 12);
  glb.copy(header, 16, 16, 20);
  return new Uint8Array(Buffer.concat([header, text, rest]));
}

async function measure(body: Uint8Array) {
  const engine = new NullEngine();
  const scene = new Scene(engine);
  const pluginOptions = { gltf: { animationStartMode: 0 } };
  const open = (bytes: Uint8Array) => LoadAssetContainerAsync(bytes, scene, { pluginExtension: ".glb", pluginOptions });
  const container = await open(body);
  const [skeleton] = container.skeletons;
  const boneMap = new Map<string, any>();
  for (const bone of skeleton.bones) {
    const node = bone.getTransformNode();
    if (node) boneMap.set(bone.name, node);
  }
  for (const group of container.animationGroups.splice(0)) group.dispose();
  const avatar = new AnimatorAvatar("player", undefined, false);
  avatar.skeletons.add(skeleton);
  avatar.showWarnings = false;
  // PlayerModelManager.loadAssets, the retargeting branch.
  let handBasis: any = null;
  const result: any = { clips: {} };
  for (const [name, file] of CLIPS) {
    const clip = await open(new Uint8Array(readFileSync(resolve(clipDir, file))));
    const source = clip.animationGroups[0];
    if (!handBasis) {
      const referenceHand = source.targetedAnimations.find((c: any) => c.target.name === "mixamorig:RightHand")?.target;
      const hand = boneMap.get("mixamorig:RightHand");
      skeleton.returnToRest();
      skeleton.prepare(true);
      referenceHand.computeWorldMatrix(true);
      hand.computeWorldMatrix(true);
      const correction = referenceHand.getWorldMatrix().multiply(hand.getWorldMatrix().clone().invert());
      const rotation = new Quaternion();
      const scale = new Vector3();
      correction.decompose(scale, rotation);
      handBasis = { rotation, scale };
    }
    const retargeted = avatar.retargetAnimationGroup(source, { fixRootPosition: false });
    const group = new AnimationGroup(name, scene);
    scene.removeAnimationGroup(group);
    const channels: any = {};
    for (const ta of retargeted.targetedAnimations) {
      const target = ta.target.name;
      const prop = ta.animation.targetProperty;
      if (prop === "position") continue;
      const ours = boneMap.get(target);
      if (!ours) throw new Error(`Unmapped source animation target: ${target}`);
      group.addTargetedAnimation(ta.animation.clone(), ours);
      if (WATCH.includes(target) && prop === "rotationQuaternion") {
        channels[target] = ta.animation
          .getKeys()
          .slice(0, 12)
          .map((k: any) => [k.frame, k.value.x, k.value.y, k.value.z, k.value.w]);
      }
    }
    container.animationGroups.push(group);
    result.clips[name] = { from: group.from, to: group.to, channels };
    retargeted.dispose();
    clip.dispose();
  }
  for (const node of [...container.meshes, ...container.transformNodes]) node.animations = [];
  const v = (p: any) => [p.x, p.y, p.z];
  result.handBasis = {
    rotation: [handBasis.rotation.x, handBasis.rotation.y, handBasis.rotation.z, handBasis.rotation.w],
    scale: v(handBasis.scale),
  };
  result.fit = measurePlayerModelFit(container, (64 * 1.1) / 36);
  const gun = measureGunSupport(container, handBasis);
  result.gunSupport = gun && { palm: v(gun.palm), forward: v(gun.forward) };
  const blade = measureBladeStance(container, handBasis, "blade_idle", (35 * Math.PI) / 180);
  result.bladeStance = blade && { axis: v(blade.axis), side: v(blade.side), fist: v(blade.fist) };
  scene.dispose();
  engine.dispose();
  return result;
}

const vanguard = readFileSync(resolve(clipDir, "vanguard.glb"));
const out = resolve(outArg);
writeFileSync(resolve(out, "player_model_retarget.json"), `${JSON.stringify(await measure(new Uint8Array(vanguard)), null, 1)}\n`);
writeFileSync(resolve(out, "player_model_retarget_skew.json"), `${JSON.stringify(await measure(skewed(vanguard)), null, 1)}\n`);
