# Collision and physics

> Behaviour: SHAG's [Physics system](https://shag.dev/guides/physics-system.html) and [Movement system](https://shag.dev/guides/movement-system.html). The sweep, the slide and step moves, contents and triggers are the same, bit for bit (RUST.md 6 and 19).

Source: `crates/shagger_shared/src/collision/`, `crates/shagger_shared/src/bsp/`, `crates/shagger/src/physics/`, `crates/shagger/src/movement/`.

There are two worlds, as in SHAG:

- **Gameplay collision** is the Quake 3 brush and patch sweep. Players, projectiles and triggers run on it, on the server and in the predicting client. It must give the server's results exactly.
- **Decoration physics** is a rigid-body world that ragdolls fall through. Gameplay never touches it and the server never simulates it. SHAG runs Havok; SHAGGER runs rapier3d.

## The sweep

`physics::sweep_collision_backend` ports `SweepCollisionBackend.ts`: BSP traversal, brush and patch-facet sweeps, the clip brush fallback, inline movers (lifts and `func_explosive`), Quake 3's slide and step-slide moves, point contents and ladder tests. As in the TypeScript, public methods take and return Babylon axes (y up) and everything inside is Q3 axes (z up). Its arithmetic runs through the `Vector3` operations in `babylon::maths`, in Babylon's operation order, so the results round as V8's do.

Around it: `patch_tessellator` (Bézier patches for collision), `facet_bevels` (Quake 3's `CM_AddFacetBevels`), `collision_spatial_index` (a static BVH over brushes and facets) and `content_flags` (the raw Q3 bits as stored in the BSP).

## The simulation's view

`shagger_shared::collision` is what the shared simulation sees, in game space (y up):

- `collision_world::CollisionWorld` ports `CollisionWorld.ts`. Its content flags are the shared, compact set (`collision_world::contents`), not the raw Q3 bits.
- `shared_collision_world` is `CollisionWorld` over a sweep backend, with ground detection.
- `physics::sweep_backend_adapter` hands the Q3 backend to it, translating raw contents to the shared set.

The split keeps the engine's map code out of `shagger_shared`, which stays free of anything but the simulation (see [Project structure](../getting-started/project-structure.md#the-two-crates)).

`shagger_shared::bsp` holds the map data the simulation reads: spawn points, teleporter and jumppad volumes (`trigger_table`), plats and doors (`lift_system`) and targetable inline geometry (`brush_model_state`). The server and the predicting client build these from the same entities, so they agree. Where the TypeScript keeps the trace function inside `LiftSystem`, `LiftSystem::step` takes it as an argument, because the backend that answers the trace also reads the lift offsets.

## Movement variables

`movement::movement_vars` registers SHAG's movement, physics and camera cvars (`cl_speed`, `m_sensitivity`, `cl_fov` and the rest), each a float with its bounds. The browser also mirrors them under `localStorage["engine.movementVars"]`; natively the cvar file holds the same values, so one store is enough.

The movement step itself is Washoi's simulation and lives in wash-fps-rust. Its parity with the TypeScript is proven there, on synthetic and shipped maps ([Golden vectors](../tools/golden-vectors.md)).

## Ragdolls

`physics::physics_system::PhysicsSystem` ports `PhysicsSystem.ts` on rapier3d, with the map as static triangles. `babylon::physics::v2::ragdoll` ports Babylon's `Ragdoll` helper: one box body per configured bone, jointed to the nearest configured ancestor.

What differs from Havok in the browser:

- Havok steps with the frame's delta. Rapier steps fixed 1/60 s substeps (`PhysicsSystem::step`), so a ragdoll falls the same at any frame rate, and `PhysicsSystem::interpolated` blends each body between its last two substeps.
- SHAG makes one trimesh per world mesh; here one trimesh holds every drawn batch.
- Ragdoll boxes collide with the map only. Havok lets a ragdoll's boxes push each other apart, which rapier turns explosive where they start overlapped.
- Rapier's math types are a different glam version than SHAGGER's, so values cross between them through arrays (`to_rapier`, `from_rapier`).
