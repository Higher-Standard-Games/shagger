// Shader script golden vectors: SHAG's own Q3ShaderParser over every shader
// script in the shipped packages (scripts/*.shader, which the game loads,
// and the media-sources *.shader.txt the maps were built from), plus
// hand-written scripts for the edge cases. Run by generate.mjs when
// WASHOI_MAPS is set:
//
//   node --experimental-transform-types --no-warnings --import ./ts-register.mjs shaders.mjs <shag> <out dir> <shag commit> <pk3 dir>
//
// One parser parses everything in the order listed, since templates carry
// from one script to the next. Each script keeps the FNV-1a hash of its
// canonical JSON: keys sorted, numbers as f64 hex bits, Sets as arrays,
// NaN and infinities as null (as JSON has them). DUMP=<index> prints one.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { fnv, num } from "./movement_run.mjs";

const [shagArg, outArg, shagCommit, pk3Arg] = process.argv.slice(2);
const shag = resolve(shagArg);
const load = (path) => import(pathToFileURL(resolve(shag, path)).href);
const { parseShaderFile } = await load("src/engine/loader/shader/Q3ShaderParser.ts");
const JSZip = (await import(pathToFileURL(resolve(shag, "node_modules/jszip/lib/index.js")).href)).default;
console.warn = () => {};

export function canonical(value) {
  if (value instanceof Set) return canonical([...value]);
  if (Array.isArray(value)) return value.map(canonical);
  if (typeof value === "number") return Number.isFinite(value) ? num(value) : null;
  if (value && typeof value === "object") {
    const out = {};
    for (const key of Object.keys(value).sort()) if (value[key] !== undefined) out[key] = canonical(value[key]);
    return out;
  }
  return value;
}

const SYNTHETIC = [
  // Templates: defined, expanded with replacement patterns, unknown, case.
  `wsw_Template { qer_editorimage $1 { map $1.tga blendFunc $2 } { map $2/$3 rgbGen const ( $3 0 0 ) } }
   textures/a/one { template WSW_template textures/a/base add 0.5 }
   textures/a/two { template wsw_template "$$x" "$&" "$'" }
   textures/a/three { template missing_template x }
   textures/a/four { template }`,
  // Directives with numbers JavaScript reads oddly.
  `textures/n/one {
     sort 0x10 cull twosided glossIntensity 0 glossExponent 1e3 q3map_surfacelight -5 lightingIntensity 400 junk
     fogparms ( 0 .5 1e400 ) -0 skyparms - 0 - deformVertexes wave 100 sin 0 1 2 3 deformVertexes autosprite2
     { animMap 0 a b c rgbGen wave inversesawtooth 0 0 -0 x alphaGen const tcMod transform 0 0 0 0 0 0 tcMod turb 1 0 }
     { clampMap "" map "" blendFunc gl_dst_color nonsense alphaFunc ge128 depthFunc EQUAL tcGen vector ( 1 0 ) ( 0 1 0 ) }
   }`,
  // Conditionals, nesting, cubemaps, material and celshade.
  `textures/c/one {
     if deluxe { material a.tga b.tga - $blankbumpimage e f } else if ! deluxe { map x } endif
     if ! texturecubemap { cubemap env/sky } if deluxe { surroundmap env/x } endif endif
     if !deluxe { celshade $whiteimage ramp skin.tga } endif
     if foo { map y } endif
     skyparmssides env/s rt rt bk zz rt 1500 near
     skyparmssides a b c d e
     { map $lightmap rgbGen wave distanceRamp ( 0 1 ) 50 200 alphaGen wave distanceramp 1 tcMod stretch square 1 x }
   }`,
  // Comments, quotes, braces in odd places.
  `// comment { }
   /* block { */ textures/q/one{map a}{map "b c"}} "textures/q/two" { "map" "x" } textures/q/three { /* unterminated`,
];

const scripts = [];
SYNTHETIC.forEach((text, i) => scripts.push({ source: `synthetic ${i}`, text }));
for (const file of readdirSync(pk3Arg).filter((f) => f.endsWith(".pk3")).sort((a, b) => a.localeCompare(b))) {
  const zip = await JSZip.loadAsync(readFileSync(resolve(pk3Arg, file)));
  const names = Object.keys(zip.files).filter((n) => /\.shader(\.txt)?$/i.test(n) && !zip.files[n].dir).sort();
  for (const name of names) scripts.push({ source: `${file}:${name}`, package: file, path: name, text: new TextDecoder("utf-8").decode(await zip.files[name].async("uint8array")) });
}

const dump = process.env.DUMP;
const vectors = scripts.map((s, i) => {
  let parsed;
  try {
    parsed = { shaders: parseShaderFile(s.text) };
  } catch (e) {
    parsed = { error: true };
  }
  const text = JSON.stringify(canonical(parsed));
  if (dump === String(i)) process.stderr.write(`${text}\n`);
  return { source: s.source, ...(s.package ? {} : { text: s.text }), count: parsed.shaders?.length ?? -1, hash: fnv(text) };
});
const total = vectors.reduce((n, v) => n + Math.max(0, v.count), 0);
writeFileSync(resolve(outArg, "shaders.json"), `${JSON.stringify({ generator: "shagger tools/golden/shaders.mjs", shag: shagCommit, scripts: vectors }).replaceAll('},{"source"', '},\n{"source"')}\n`);
process.stderr.write(`${scripts.length} scripts, ${total} shaders\n`);
