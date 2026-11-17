# Viewport and weapons

> Behaviour: SHAG's [Camera system](https://shag.dev/guides/camera-system.html), [Viewport effects](https://shag.dev/guides/viewport-effects.html) and [Weapon system](https://shag.dev/guides/weapon-system.html). The effects, their cvars and their curves are the same.

Source: `crates/shagger/src/camera/`, `crates/shagger/src/viewport/`, `crates/shagger/src/weapon/`.

## Camera

| Module | SHAG | Does |
| --- | --- | --- |
| `fov_settings` | `FovSettings.ts` | `cl_fov` is horizontal degrees at 16:9; the camera wants vertical radians, held while the horizontal follows the window (Hor+) |
| `first_person_motion` | `FirstPersonMotion.ts` | the dip into a slide, the lift through a ledge grab, the ease up a step |
| `view_kick` | `controllers/ViewKick.ts` | the view's jump on our own shots; visual only |
| `camera_shake` | wash-fps `camera/FPSController.ts` | the decaying jolt of a nearby explosion or a hit |

## Viewport effects

Each effect in `viewport::effects` (idle breathing, landing impact, speed field of view, strafe lean, walk bob) reads the player's state (`viewport::PlayerState`) and returns a small camera delta (`viewport::ViewportDelta`): an offset in camera space, a rotation in degrees and a field-of-view multiplier. `FirstPersonEffectStack` sums them, and `viewport::apply_fps` puts the sum on the camera.

The browser reads each effect's cvars from the global `Cvar`. Here a frame's values are read once into `viewport::ViewCvars` and handed to every effect, so the effects stay pure functions of their inputs.

## The weapon view

`weapon::weapon_view_manager::WeaponView` ports `WeaponViewManager.ts`: the weapon camera (its own field of view, near 0.1, far 1000, and a depth clear before it draws), the weapon-only fill light, a GLB view model's orientation and material fix-ups, hip placement with handedness, and per frame the aim-down-sights lerp, bob (`weapon_bob`, from the `cl_gunbob*` cvars), recoil (`weapon_recoil`), the melee swing, and the reload, switch and holster animations (`weapon_reload_anim`, `weapon_switch_anim`, `weapon_holster_anim`). `weapon_charge` is the blade's charge. `WeaponView::followed_changed` tells it that the camera now follows another player, which is not a weapon switch.

The muzzle position is the model's `tag_flash` seen through the weapon camera, put on the main camera's matching screen ray, as `getMuzzleWorldPosition` does.

## Weapon effects

`weapon::hitscan_effects` owns the hitscan weapons' effects and ages them by one clock: bullet streaks (`tracer`), pellets, the rail (`rail`), the held lightning beam (`voltaic`), muzzle flashes (`muzzle`) and impact sparks (`impact`). Beside it:

- `projectile_manager` draws a projectile's MD3 model and its trail;
- `explosion` draws every live explosion's fireball, rays and sparks as three instanced meshes animated on the GPU;
- `marks::wall_marks` leaves bullet holes and scorches;
- `point_light_pool` is SHAG's eight effect lights: a free light parks at intensity 0, and when all are taken the oldest is stolen;
- `weapon_barrels` places barrel sub-models on a weapon in the world;
- `weapon_slash` is the melee swing and the ribbon its tip leaves.

Effect geometry is built on the CPU as SHAG builds it and drawn through `rendering::effect_quad_batch` and the effect renderers.
