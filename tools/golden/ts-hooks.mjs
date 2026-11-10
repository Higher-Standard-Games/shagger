// Lets Node import SHAG's TypeScript as it is: SHAG imports siblings without
// an extension ("./GameState"), which Node's resolver does not try on its own.
// Babylon.js subpaths ("@babylonjs/core/Maths/math.vector") get ".js" the same way.
// JSON imports load as modules, and asset imports (shaders, CSS, "?url") as
// empty strings, which nothing headless reads.
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

export async function resolve(specifier, context, next) {
  try {
    return await next(specifier, context);
  } catch (error) {
    if (/^@babylonjs\//.test(specifier) && !specifier.endsWith(".js")) return next(`${specifier}.js`, context);
    if (!/^\.\.?\//.test(specifier) || /\.[cm]?[jt]s$/.test(specifier)) throw error;
    for (const suffix of [".ts", "/index.ts"]) {
      try {
        return await next(specifier + suffix, context);
      } catch {}
    }
    throw error;
  }
}

export async function load(url, context, next) {
  if (url.endsWith(".json")) {
    const text = await readFile(fileURLToPath(url), "utf8");
    return { format: "module", source: `export default ${text};`, shortCircuit: true };
  }
  if (/\.(css|wgsl|glsl|fx)(\?.*)?$/.test(url) || url.includes("?url") || url.includes("?raw")) {
    return { format: "module", source: `export default "";`, shortCircuit: true };
  }
  return next(url, context);
}
