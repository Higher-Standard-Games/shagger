// Lets Node import SHAG's TypeScript as it is: SHAG imports siblings without
// an extension ("./GameState"), which Node's resolver does not try on its own.
// Babylon.js subpaths ("@babylonjs/core/Maths/math.vector") get ".js" the same way.
export async function resolve(specifier, context, next) {
  try {
    return await next(specifier, context);
  } catch (error) {
    if (/^@babylonjs\/core\//.test(specifier) && !specifier.endsWith(".js")) return next(`${specifier}.js`, context);
    if (!/^\.\.?\//.test(specifier) || /\.[cm]?[jt]s$/.test(specifier)) throw error;
    for (const suffix of [".ts", "/index.ts"]) {
      try {
        return await next(specifier + suffix, context);
      } catch {}
    }
    throw error;
  }
}
