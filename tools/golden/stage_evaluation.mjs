// Shader animation golden vectors: SHAG's own Q3StageEvaluation over every
// stage of every shader in the shipped packages. Each stage's tcMods are
// stepped 600 frames from createTcState (frame lengths cycling through
// 16 to 22 ms) and its rgbGen, alphaGen and stretch waves are sampled at
// the same times; `noise` draws from a Park-Miller generator both sides
// share. Run by generate.mjs when WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs stage_evaluation.mjs <shag> <out dir> <shag commit> <maps dir>
//
// Each shader with an animated stage keeps the FNV-1a hash of its lines
// (one per stage and frame, f64 as hex bits). DUMP=<shader> prints them.
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { fnv, num } from "./movement_run.mjs";

const [shagArg, outArg, shagCommit, mapsArg] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { ingestPk3Dir } = await load("src/engine/loader/pk3/NodePk3.ts");
const { Q3ShaderRegistry } = await load("src/engine/loader/shader/Q3ShaderRegistry.ts");
const { createTcState, evaluateWaveFunc, stepTcModsInto } = await load("src/engine/loader/shader/Q3StageEvaluation.ts");
console.warn = () => {};
console.log = () => {};

await ingestPk3Dir(resolve(mapsArg, "pk3"));
const registry = Q3ShaderRegistry.getInstance();
await registry.loadShaders();

export const FRAMES = 600;
const frameSeconds = (i) => 0.016 + (i % 7) * 0.001;
const parkMiller = () => {
  let x = 1;
  return () => {
    x = (x * 16807) % 2147483647;
    return x / 2147483647;
  };
};

const dump = process.env.DUMP;
const shaders = [];
for (const name of registry.getShaderNames().sort()) {
  const shader = registry.getShader(name);
  const lines = [];
  shader.stages.forEach((stage, index) => {
    const tcMods = stage.tcMod ?? [];
    const waves = [stage.rgbGen?.wave, stage.alphaGen?.wave].filter(Boolean);
    if (tcMods.length === 0 && waves.length === 0) return;
    const rng = parkMiller();
    const state = createTcState(tcMods);
    let elapsed = 0;
    for (let i = 0; i < FRAMES; i++) {
      const dt = frameSeconds(i);
      elapsed += dt;
      stepTcModsInto(state, tcMods, dt, elapsed, rng);
      const sampled = waves.map((w) => num(evaluateWaveFunc(w, elapsed, rng)));
      lines.push(`${index} ${i} ${[state.uOffset, state.vOffset, state.rotation, state.uScale, state.vScale].map(num).join(",")} ${sampled.join(",")}`);
    }
  });
  if (lines.length === 0) continue;
  if (dump === name) process.stderr.write(`${lines.slice(0, 20).join("\n")}\n`);
  shaders.push([name, fnv(lines.join("\n"))]);
}
writeFileSync(
  resolve(outArg, "stage_evaluation.json"),
  `${JSON.stringify({ generator: "shagger tools/golden/stage_evaluation.mjs", shag: shagCommit, frames: FRAMES, shaders }).replaceAll('],["', '],\n["')}\n`,
);
process.stderr.write(`${shaders.length} animated shaders\n`);
