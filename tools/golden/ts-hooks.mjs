// Lets Node import SHAG's TypeScript as it is: SHAG imports siblings without
// an extension ("./GameState"), which Node's resolver does not try on its own.
export async function resolve(specifier, context, next) {
  try {
    return await next(specifier, context);
  } catch (error) {
    if (!/^\.\.?\//.test(specifier) || /\.[cm]?[jt]s$/.test(specifier)) throw error;
    for (const suffix of [".ts", "/index.ts"]) {
      try {
        return await next(specifier + suffix, context);
      } catch {}
    }
    throw error;
  }
}
