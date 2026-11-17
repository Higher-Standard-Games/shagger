# Rendering

> Behaviour: SHAG's [Render variables](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/render-variables.md), [Lighting system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/lighting-system.md) and [Shader system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/shader-system.md). The goal is the browser's picture (RUST.md 8 and 21).

Source: `crates/shagger/src/rendering/`, `lighting/`, `effects/`, `babylon/`.

SHAG draws with Babylon.js on WebGL2 or WebGPU. SHAGGER draws with wgpu, which runs on Vulkan, Metal, DirectX 12 and OpenGL, and ports the parts of Babylon the picture depends on into `babylon/`. Shaders are WGSL, kept beside the renderers that use them (`world.wgsl`, `model.wgsl`, `sky.wgsl`, `fog.wgsl`, `water/unified_water.wgsl`, `effect_quad.wgsl`, `hud_2d.wgsl`).

## The device

`rendering::gpu::Gpu` holds wgpu's instance, adapter, device and queue.

- `Gpu::new(instance, Some(&surface))` makes a device that can present to a window's surface.
- `Gpu::headless()` makes one with no surface, for tests, screenshots and machines without a display. It finds a software adapter (llvmpipe, WARP) when there is no GPU.
- `Gpu::describe()` names the adapter and backend, for logs.

The window and its event loop belong to the game (wash-fps-rust's `window.rs`), which owns winit and the surface.

## What draws what

| Renderer | Draws | SHAG |
| --- | --- | --- |
| `world_geometry::WorldGeometry` | the map's faces as vertex and index buffers, batched as `BspMeshBuilder.buildAll` batches them | `loader/bsp/BspMeshBuilder.ts` |
| `world_renderer::WorldRenderer` | the world's material passes with their blend, alpha test, cull and depth state; animated stages; lightmaps and deluxe maps; normal, gloss and glow maps; authored cubemaps; movers; soft particles | Babylon materials and SHAG's material plugins |
| `world_culling`, `frustum` | PVS and frustum culling | `loader/bsp/PvsCuller.ts` |
| `sky_renderer::SkyRenderer` | the far box, first, unlit and without depth | `SkyboxLoader.ts`, `BspMeshBuilder.applySkybox` |
| `model_renderer::ModelRenderer` | glTF models lit as Babylon's `PBRMaterial` with the scene's hemispheric and directional lights, skinned or not, with outlines | Babylon's glTF loader and PBR material |
| `water::*` | every liquid surface through one material, with disturbances, probes, reflections of players and lighting | `rendering/water/` |
| `world_fog_controller` | the world fog's uniforms from the map's fog and the `r_fog` cvars, as Warfork's `r_main.c` decides | `WorldFogController.ts`, `AtmosphericFogShader.ts` |
| `dynamic_lights` | point lights from the weapon's `PointLightPool`, each stopping at its range | `DynamicLightCluster` |
| `effect_quad_batch`, `weapon::*` | particles, trails, beams, explosions, muzzle flashes, sparks and marks | `src/engine/weapon` |
| `lighting::*` | the sky lights and emissive map surfaces | `src/engine/lighting` |

Babylon culls dynamic lights by screen tile and depth slice. Since every light still stops at its range, each surface here loops over all of them.

## The frame

As SHAG's `DefaultRenderingPipeline` with `hdr: true` does, the scene renders into `hdr_target::HdrTarget` (half floats with depth), then image processing (`babylon::post_processes::image_processing_post_process`: exposure, contrast and the vignette) writes the output, and FXAA (`fxaa_post_process`) smooths its edges. `effects::color_correction_post_process` adds the colour-blindness correction. The 2D HUD and egui draw over the result ([UI](./ui.md)).

## Offscreen frames

`rendering::offscreen` renders into an `Rgba8Unorm` image and reads it back, for screenshots, the visual parity harness (RUST.md 8.3) and tests. `render_to_rgba` takes a closure that records into the main pass; the `render_hdr_*` variants run the HDR target and image processing, with an overlay or a UI pass on top. `begin_main_pass`, `begin_overlay_pass` and `begin_ui_pass` give a windowed frame the same passes.

`rendering::model_stage` is the native side of SHAG's preview scenes (wash-fps `WeaponPreview.ts`, `PlayerTeamPreview.ts`): a few models drawn into an image that an egui panel shows.

## Render variables

`render_vars::register_render_vars(cvars)` registers SHAG's `r_*` variables with their defaults, ranges and words, plus the ones SHAG's `PostProcessingManager.ts` owns (`r_cel_outlines`, `r_colorcorrection`, `r_colorcorrection_strength`, `r_colorsimulation`). The settings screen and the console read and write them as in the browser. `render_toggles` ports `RenderToggles.ts`, the frame-cost switches (`r_dynamiclight`, `r_marks`, `r_gibs`, `r_teleport_ghost`) that the video page's presets turn off.

## BC7 texture compression

Native only. Map and model textures are compressed to BC7 on the GPU as they upload, by `block_compression`'s WGSL port of Intel's ISPC texture compressor. SHAG's browser build uploads RGBA8, four bytes a texel; BC7 takes one, so a map's textures need a quarter of the memory.

`r_texture_compression` chooses:

| Value | Effect |
| --- | --- |
| 0 | off: every texture uploads as RGBA8 |
| 1 (default) | on a hardware GPU |
| 2 | on a software renderer too |

A software renderer decodes BC7 on every sample and has no video memory to save, so it keeps RGBA8 unless the value is 2. Textures whose size is not a multiple of the 4×4 block, and devices without BC support, upload uncompressed. BC7 is lossy: the picture differs from the browser's by the compression error, which `r_texture_compression 0` removes.

## The Babylon folder

`crates/shagger/src/babylon/` holds the code ported from Babylon.js, under Apache-2.0: the `Vector3` arithmetic whose operation order the collision code must reproduce, the texture matrix, `StandardMaterial`'s lighting terms, mesh builders, image processing and FXAA, `AnimatorAvatar`'s retargeting and the ragdoll helper. Babylon code goes in that folder and nowhere else ([Contributing](../contributing/index.md#licences)).

## Not drawn as SHAG draws yet

The module comments say where the port is short of SHAG. At the time of writing, `world_renderer` leaves out `$cubemap` reflection passes, and `skybox_loader` the cloud layers and the sky sphere, which no shipped map uses.
