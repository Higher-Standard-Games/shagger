// Player model playback golden vectors: SHAG's own PlayerModelManager
// (staging's layered playback: upper-body share, blade stance, the blade
// swing over running legs, a full-body one-shot, the aim bend) driven frame
// by frame in Babylon's NullEngine at its constant 16 ms animation step, with
// the watched joints' local rotations written after every frame. Vanguard
// stands in for the packaged Police body and its clips for the shared ones
// (renamed as `config/player-animations.json` names them), so every clip is
// retargeted as a packaged body's are.
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs \
//     player_model_layers.mts <shag> <washoi_player_model.pk3 unpacked> <out dir>
//
// The SHAG checkout must have its dependencies installed (`npm ci`). Writes
// `player_model_layers.json`, which shagger's `tests/player_model_golden.rs`
// reads.
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
const { FreeCamera } = await fromShag("@babylonjs/core/Cameras/freeCamera.js");
const { Vector3 } = await fromShag("@babylonjs/core/Maths/math.vector.js");
const { DracoDecoder } = await fromShag("@babylonjs/core/Meshes/Compression/dracoDecoder.js");
await fromShag("@babylonjs/loaders/glTF/2.0/index.js");
const JSZip = (await fromShag("jszip")).default;
const { PlayerModelManager } = await load("src/engine/animation/PlayerModelManager.ts");
const { PK3Loader } = await load("src/engine/loader/pk3/PK3Loader.ts");

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

/** The shared clips' files, from the Vanguard clips that stand in for them. */
export const STAND_INS: Record<string, string> = {
  "backwards_run.glb": "run backwards.glb",
  "blade_idle.glb": "rifle aiming idle.glb",
  "death_flying_back.glb": "hit reaction.glb",
  "death_from_back.glb": "hit reaction.glb",
  "death_headshot.glb": "hit reaction.glb",
};
const vanguard = resolve(dataArg, "models/players/vanguard");
const config = JSON.parse(readFileSync(resolve(shag, "config/player-animations.json"), "utf8"));
const bodies = new JSZip();
bodies.file("models/players/police/police.glb", readFileSync(resolve(vanguard, "vanguard.glb")));
const clips = new JSZip();
for (const anim of config.animations) {
  if (anim.optional) continue;
  clips.file(`${config.root}${anim.file}`, readFileSync(resolve(vanguard, STAND_INS[anim.file] ?? anim.file)));
}
const pk3 = PK3Loader.getInstance();
for (const [name, zip] of [
  ["washoi_player_model.pk3", bodies],
  [config.package, clips],
] as const) {
  const bytes = await zip.generateAsync({ type: "uint8array" });
  await pk3.processPK3ArrayBuffer(name, bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
}

const engine = new NullEngine();
const scene = new Scene(engine);
scene.useConstantAnimationDeltaTime = true;
new FreeCamera("camera", new Vector3(0, 0, -10), scene);
const manager = new PlayerModelManager(scene);
await manager.loadModel();
if (!manager.isLoaded) throw new Error("the stand-in body did not load");
const instance = manager.createInstance("p", 36);

const WATCH = [
  "mixamorig:Hips",
  "mixamorig:Spine",
  "mixamorig:Spine2",
  "mixamorig:RightArm",
  "mixamorig:RightHand",
  "mixamorig:LeftUpLeg",
  "mixamorig:LeftLeg",
  "mixamorig:Head",
];
const nodes = WATCH.map((name) => instance.skeleton.bones.find((b: any) => b.name === name).getTransformNode());

/** What the renderer asks of the manager before frame `f` (the scenario). */
function step(f: number) {
  const run = () => {
    if (instance.masterGroup) instance.masterGroup.speedRatio = 1.5 * 0.75;
    manager.setUpperBodyShare(instance, 0.5 / 0.75);
  };
  if (f === 0) {
    manager.playAnimation(instance, "run");
    run();
  }
  if (f === 20) manager.setUpperBodyStance(instance, "blade_idle", 1);
  if (f === 40) manager.playOnce(instance, "blade_slash", 2.0);
  if (f === 45) manager.setAimPitch(instance, 0.6);
  if (f === 70) {
    manager.playAnimation(instance, "walk");
    if (instance.masterGroup) instance.masterGroup.speedRatio = -1.25;
    manager.setUpperBodyShare(instance, 1);
  }
  if (f === 90) manager.setUpperBodyStance(instance, null);
  if (f === 100) manager.playOnce(instance, "reload", 1.5);
  if (f === 110) manager.playAnimation(instance, "run");
  if (f === 120) manager.setAimPitch(instance, -0.3);
  if (f >= 110) run();
}

const FRAMES = 260;
const frames: number[][][] = [];
for (let f = 0; f < FRAMES; f++) {
  step(f);
  scene.render();
  frames.push(nodes.map((n: any) => {
    const q = n.rotationQuaternion;
    return [q.x, q.y, q.z, q.w];
  }));
}
writeFileSync(
  resolve(outArg, "player_model_layers.json"),
  `${JSON.stringify({ watch: WATCH, standIns: STAND_INS, frames })}\n`,
);
scene.dispose();
engine.dispose();
