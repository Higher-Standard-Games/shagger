# Models and animation

> Behaviour: SHAG's [Model loading](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/model-loading.md) and [Pickup system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/pickup-system.md), and `src/engine/animation`.

Source: `crates/shagger/src/loader/model/`, `crates/shagger/src/animation/`, `crates/shagger/src/world/`.

## glTF

RUST.md 20.10 named the `gltf` crate, but Washoi's models use `EXT_texture_webp`, whose textures carry no core `source`, and that crate rejects them. So `loader::model::glb` reads the parts of the format the models use with serde, as Babylon's glTF loader reads them. Draco-compressed bodies decode through `draco-core`.

- `gltf_model::GltfModel::from_glb(bytes)` gives a model as SHAG's pickup cache hands it out: only the meshes with vertices, in the loader's order, under Babylon's `__root__` handedness turn (scaling (1, 1, −1) and a half turn about Y).
- `GltfModel::from_glb_rigged(bytes)` keeps the node tree and skins, for bodies that animate.
- `gltf_animation` turns glTF channels into frames at the loader's `targetFps` of 60, as `_loadAnimationChannelAsync` does: quaternions by slerp, vectors linearly, cubic splines by their Hermite tangents. Outside its keys a channel holds its first or last value.

## MD3 and tags

`md3_loader::parse_md3` and `create_surface_mesh` port `MD3Loader.ts`: Quake's axes become Babylon's by swapping Y and Z with the winding reversed, and each surface's material comes from its shader (`create_material_from_shader`). `tags_loader` reads `tags.txt` attachment tags.

## The player model

`animation::player_model_manager::PlayerModel` is SHAG's `PlayerModelManager.ts`: one rigged body and its clips, with each clip retargeted to the body's joints by name and its position tracks removed. SHAG gives every player a cloned rig. Here an instance (`PlayerModel::create_instance`) is only the rig's local transforms and the clips it plays, and `PlayerModel::joint_matrices` turns it into the skin palette the renderer draws with. The caller puts the anchor, the fit's lift and the scale in front (`PlayerModel::root_matrix`).

- **Retargeting.** The packaged bodies share one set of clips made on another rig. They are retargeted with Babylon's `AnimatorAvatar`, ported in `babylon::animations::animator_avatar` and checked against Babylon itself in `tests/player_model_golden.rs`.
- **Fit.** `player_model_fit` measures each body once at load, as `PlayerModelFit.ts` does: its standing height, where its palm and front are for a held gun, and how a blade stands.
- **Aim.** `player_aim_pose` bends the spine toward the aim pitch. SHAG writes the bend into the posed quaternions and restores them before the next evaluation; here the clips pose fresh locals every frame, so the bend is applied to those and never accumulates.
- **Assets.** `player_model_assets` lists the bodies and clips from SHAG's `config/player-model.json` and `config/player-animations.json`. While the published packages lack the newer bodies, the native client falls back to Vanguard, which keeps its own copy of the clips.

## Pickups

`world::pickup` ports `Pickup.ts`'s `loadModel`: the first renderable mesh leads, placed at the pickup's position, turned by its yaw and scaled by the pickup scale (1.5 when none is given), and the other meshes follow it. A `bobbing` model floats, bobs and turns as in SHAG. A tossed pickup's physics is not here: map weapons arrive already landed.
