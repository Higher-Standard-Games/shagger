//! Input (SHAG `src/engine/input`): actions bound to keys, mouse buttons and
//! the wheel, the bindings a player saves, and the actions' state each frame;
//! pads (`gamepad_input`, read through gilrs in `gamepad_backend`) and the
//! device in use (`input_device`).

pub mod binding_validation;
pub mod gamepad_backend;
pub mod gamepad_input;
pub mod input_device;
pub mod input_system;
pub mod input_types;
pub mod key_codes;
