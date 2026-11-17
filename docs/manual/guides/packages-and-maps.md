# Packages and maps

> Behaviour: SHAG's [PK3 filesystem](https://shag.dev/guides/pk3-filesystem.html), [BSP system](https://shag.dev/guides/bsp-system.html), [Shader system](https://shag.dev/guides/shader-system.html) and [Skybox system](https://shag.dev/guides/skybox-system.html). Package order, path matching, the published index's rules, the BSP lumps and the shader keywords are the same.

Source: `crates/shagger/src/loader/`.

## Packages

`loader::pk3::pk3_loader::Pk3Loader` holds packages in memory and one index from every file path to the package that wins it. Paths match as in SHAG: lower case, with `\` turned into `/`. Packages are ordered by file name with `localeCompare` (`pk3_loader::locale_compare`), and a later package overrides an earlier one.

```rust
use shagger::loader::pk3::{node_pk3, pk3_loader::Pk3Loader};

let mut pk3 = Pk3Loader::new();
let mounted = node_pk3::ingest_pk3_dir(&mut pk3, data_dir);
let bytes = pk3.get_file("maps/washoi_dm2.bsp");
```

`node_pk3` is the port of `NodePk3.ts`: packages read off disk. `ingest_pk3_dir_holding` mounts only the packages that hold the given paths, reading each package's directory to decide, so a screen that needs a few files (the lobby's music) does not read every package.

## The published index and the package store

The browser fetches the manifest, then the published index it names, then each package the first time a map needs it, and keeps them in CacheStorage. The native client keeps them in a folder instead, and brings the whole folder up to date at start (RUST.md 20.2):

```rust
use std::sync::atomic::AtomicBool;
use shagger::loader::pk3::package_store;

let cancel = AtomicBool::new(false);
let report = package_store::sync_packages(
    "https://eu.washoi.net/data",
    &data_dir,
    &[],            // extra files to fetch beside the packages
    &cancel,
    &mut |progress| println!("{} {}/{}", progress.package, progress.received, progress.total),
)?;
```

`sync_packages`:

1. takes `sync.lock` in the folder, and returns `SyncError::Busy` if another copy of the game holds it;
2. fetches the manifest and the index it names, and refuses the whole snapshot unless `published_index::parse_published_index` accepts it (`BadManifest`, `BadIndex`);
3. fetches each package the folder does not already hold at the index's size and SHA-256, with `?sha256=` as the browser's cache key, and moves it into place only once whole (`Mismatch` when it arrives different);
4. removes packages the index no longer lists;
5. writes the index last, as `index.json`.

A folder with an `index.json` therefore holds a complete set. `package_store::is_synced(dir)` checks that, and a start without the network can mount the folder as it stands. `has_packages(dir)` says whether there is anything at all to try.

`published_index::pk3_index_hash` is SHAG's `pk3IndexHash`, checked against V8 in `tests/pk3_index_hash_golden.rs`.

## Maps

`loader::bsp::bsp_loader` reads Quake 3 IBSP (version 46) and Warsow FBSP (version 1) files. It is a direct translation over `&[u8]`:

- `bsp_loader::load(bytes)` returns a `BspFile` with every lump; `load_collision(bytes)` reads only what collision needs.
- File floats stay `f32` as stored, and widen to `f64` where the TypeScript computes with them.
- Where the TypeScript throws (a lump outside the file, a bad lightmap size), the Rust returns a `BspError`. Where it only warns, the Rust does nothing.

The rest of `loader::bsp` ports the map's other readers: fog volumes and world fog, the light grid, liquid classification, ambient entities, map particles and the PVS culler. `loader::bsp::mesh` turns faces into vertex and index arrays and lightmap pages into texture bytes. The Babylon mesh and texture objects are left out: the renderer uploads what these return.

## Shaders

`loader::shader::q3_shader_registry::Q3ShaderRegistry` holds the parsed shader scripts of the mounted packages. Unlike SHAG's, it is not a singleton: the caller owns it and passes the `Pk3Loader` it loads from.

```rust
use shagger::loader::shader::q3_shader_registry::Q3ShaderRegistry;

let mut shaders = Q3ShaderRegistry::new();
shaders.load_shaders(&mut pk3);
let sky = shaders.get_shader("textures/skies/washoi_sky");
```

SHAG caches resolved material props by key. Resolution is cheap and pure, so the port resolves every time. Stage identity, which the TypeScript gets by comparing objects, is `Q3ShaderStage::id`. `q3_stage_evaluation` steps a stage's `tcMod`, `rgbGen`, `alphaGen` and `animMap` as SHAG's `ShaderAnimationSystem` does.

## Textures

- `texture_extensions` finds the file behind a texture name. Shaders and models name textures by their source extension (`foo.tga`), but map texture packages carry their TGA and PNG sources as WebP under the same name, so the WebP form is looked for first.
- `ktx1_loader` decodes KTX 1 textures to RGBA8 on the CPU. Desktop GPUs take no ETC1, so this port always takes SHAG's software path. Only the base level is kept; the renderer makes its own mip chain.
- `skybox_loader::load_skybox` reads a sky's six faces in the slot order SHAG hands Babylon's `RawCubeTexture` (+X, −X, +Y, −Y, +Z, −Z). The cloud layers and the sky sphere are not ported: no shipped map draws them.

Decoded textures are compressed to BC7 on upload; see [Rendering](./rendering.md#bc7-texture-compression).
