# Input

> Behaviour: SHAG's `src/engine/input` (`InputSystem.ts`, `GamepadInput.ts`, `InputDevice.ts`). Actions, contexts, bindings and their saved format are the same, so a binding saved by the browser loads natively.

Source: `crates/shagger/src/input/`.

## Feeding raw input

The browser hands `InputSystem` DOM events. Natively the window does, under the browser's names, so saved bindings mean the same key on both:

- `key_codes::dom_code(KeyCode)` turns a winit key into its `KeyboardEvent.code` (`"KeyW"`, `"Space"`), and `dom_mouse_button` does the same for mouse buttons.
- `InputSystem::key_down`, `key_up`, `mouse_down`, `mouse_up` and `wheel` take them.
- `InputSystem::update()` runs before the frame's systems read actions (the browser's `onBeforeRender`), and `end_frame()` after (`onAfterRender`). Between the two, `is_active`, `was_pressed` and `was_released` answer for an action.

Like the cvar registry, the game thread's `InputSystem` sits behind `init_input(storage)` and `with_input(|input| ...)`.

## Saved bindings

Overrides are kept in the synced storage entry `shag_keybindings` (`input_system::BINDINGS_KEY`), in `synced-storage.json` in the folder the game passes to `InputSystem::new`. The settings sync ([Networking and lobby](./networking-and-lobby.md#settings-sync)) uploads the same entry, so bindings follow the account between the browser and native clients. `binding_validation` checks saved bindings as SHAG's `BindingValidation.ts` does before they load.

## Gamepads

The browser reads `navigator.getGamepads()` each animation frame. Natively, `gamepad_backend::GamepadBackend` reads pads through gilrs and returns them as the W3C standard layout when gilrs knows the pad's mapping (an SDL mapping, or a driver with the unified layout): buttons 0 to 16 in the standard order, axes 0 and 1 the left stick, 2 and 3 the right, down positive. A pad without a known layout reads with an empty mapping, which `GamepadInput` ignores, as the browser ignores a pad whose `mapping` is not `"standard"`.

The game polls the backend once a frame and hands the pads to `GamepadInput::sample`. `input_device` tracks whether the player is on keyboard and mouse or a pad, for prompts and the HUD.

## The cursor on macOS

The browser hides the cursor with pointer lock. winit's `set_cursor_visible(false)` hides it on macOS through cursor rectangles, which AppKit applies only when the cursor moves, and a locked cursor never moves. `native_cursor` hides it through `NSCursor` as well. It has no SHAG source.
