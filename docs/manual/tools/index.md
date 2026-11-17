# Tools

| Tool | Use it for |
| --- | --- |
| [Golden vectors](./golden-vectors.md) | Proving a port gives the TypeScript's results, bit for bit |
| [Parser fuzzing](#parser-fuzzing) | Proving the parsers of downloaded and network data never panic |

SHAG's CLI, server and headless tools ([SHAG Tools](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/tools/index.md)) stay in TypeScript. Run them from a SHAG checkout; the native client talks to the servers they start.

## Parser fuzzing

`crates/shagger/tests/fuzz_parsers.rs` mutates real files (bit flips, boundary values, truncation, splices) and feeds them to every parser that reads bytes from a downloaded package or from the network. Each parser must return an error rather than panic, since a panic is a crash in the game. It runs on stable Rust with no extra tools, and is ignored by default.

`FUZZ_DATA` names a folder of `.pk3` packages (the synced game data), whose files are the seeds:

```sh
FUZZ_DATA=<dir> FUZZ_ITERS=2000 cargo test --release -p shagger \
    --test fuzz_parsers -- --ignored --nocapture
```

| Variable | Meaning | Default |
| --- | --- | --- |
| `FUZZ_DATA` | the folder of seed packages | required |
| `FUZZ_ITERS` | mutations per seed | 500 |
| `FUZZ_SEEDS` | how many seed files to sample per target (every model is always taken, and at most three maps) | 12 |
| `FUZZ_ONLY` | a comma-separated list of targets to run | every target |
| `FUZZ_OUT` | where failing inputs are written | `target/fuzz` |

Each failing input is written to `FUZZ_OUT` and reported by target and seed file. To replay one, set `FUZZ_REPLAY` to the file and `FUZZ_TARGET` to its target, and run the ignored `fuzz_replay` test.
