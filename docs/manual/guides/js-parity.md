# JavaScript parity

The game server and the browser run V8. A native client that predicts movement, decodes snapshots or cuts a reel has to get the same bits V8 gets, or it drifts from the server. Most of Rust's `f64` arithmetic is IEEE 754 and already matches. The exceptions are the functions JavaScript defines differently and the ones whose results depend on the platform's maths library. `shagger_shared::js` and its siblings cover them.

This page has no SHAG counterpart: the TypeScript gets these semantics from the language.

Source: `crates/shagger_shared/src/{js, fdlibm, json, math_random}.rs`. RUST.md 6.2 and 19.6 list where each function is used.

## The rule

In simulation and protocol code, wherever the TypeScript calls a `Math` function or relies on a JavaScript conversion, call the function here instead of the Rust standard library's:

| TypeScript | Use | Not | Why |
| --- | --- | --- | --- |
| `Math.round(x)` | `js_round(x)` | `f64::round` | ties go toward positive infinity: `Math.round(-2.5)` is `-2`, and a negative input that rounds to zero gives `-0` |
| `Math.hypot(...xs)` | `js_hypot(&xs)` | `f64::hypot` | V8 scales by the largest value and sums with Kahan compensation; the input sanitizer and the ledge grab use it on every command |
| `Math.sin`, `Math.cos`, `Math.atan2` | `js_sin`, `js_cos`, `js_atan2` | `f64::sin` and friends | V8 uses fdlibm; the platform's `sin` differs in the last bit for about 3% of inputs, and differs again across platforms |
| `Math.max`, `Math.min`, `Math.sign` | `js_max`, `js_min`, `js_sign` | `f64::max`, `f64::min`, `f64::signum` | NaN and signed zero follow JavaScript |
| `DataView.setUint32(x)`, `x >>> 0`, `x \| 0` | `to_uint32`, `to_int32`, and the 16- and 8-bit forms | `as` casts | NaN and infinities become 0; everything else truncates toward zero and wraps |
| `Number(s)`, `parseFloat(s)`, `parseInt(s, 10)` | `js_string_to_number`, `js_parse_float`, `js_parse_int10` | `str::parse` | JavaScript's grammar, white space and partial parses |
| `String(x)` | `js_number_to_string` | `format!` | the shortest round trip in JavaScript's notation |
| `s.trim()` | `js_trim`, `is_js_whitespace` | `str::trim` | ECMAScript white space is Unicode's without U+0085, plus U+FEFF |

`fdlibm` is the port of Sun's fdlibm 5.3 that V8 uses (`src/base/ieee754.cc`), kept digit for digit with its notice at the top of the file. It is the one file in `shagger_shared` that is not MIT ([Contributing](../contributing/index.md#licences)).

`Math.tan`, `Math.sqrt` and `Math.asin` are not covered. `sqrt` is exact in both. Where the TypeScript uses `tan` or `asin`, as the pellet cone in `spread_utils` does, the Rust may differ in the last bit, and the code says so. Pellets are drawn on the client only; the server's own pellets judge the hits.

## JSON

A few wire fields carry JSON (`match_state`, the `weapon_state` lists, `agent_directives`). `shagger_shared::json::parse` reads values exactly as V8's `JSON.parse` would and keeps object keys in JavaScript's property order, which is what the TypeScript iterates. It is not a general JSON library: `shagger` uses `serde_json` for everything else (the lobby, cvar files, glTF).

Where a JSON text must be byte for byte what `JSON.stringify` writes, such as the synced settings blob, the module that writes it says so and is tested against the TypeScript's output.

## Random numbers

- **Seeded and checked:** `spread_utils::mulberry32`, SHAG's seeded generator, for anything the simulation or a golden vector depends on.
- **Cosmetic:** `math_random::MathRandom`, a xorshift64* that stands in for `Math.random` in spins, shuffles and jitters nothing else depends on. `MathRandom::from_entropy()` seeds it differently each run, as `Math.random` is, `new(seed)` and `from_text(text)` seed it for tests and reproducible renders.

## Proving it

Each function is checked against V8's own results on thousands of inputs, including the edge cases (ties, signed zero, subnormals, NaN, the largest values): `tests/js_math_golden.rs`, `js_trig_golden.rs`, `js_atan2_golden.rs` and `js_string_number_golden.rs`. The vectors are written by `tools/golden/generate.mjs` ([Golden vectors](../tools/golden-vectors.md)).
