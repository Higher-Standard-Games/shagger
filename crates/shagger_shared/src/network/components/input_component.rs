//! Input button bits (SHAG `src/engine/network/components/InputComponent.ts`,
//! `InputButtons`). The component itself is not ported yet.

/// `InputButtons`.
pub mod input_buttons {
    pub const FORWARD: u32 = 1 << 0;
    pub const BACK: u32 = 1 << 1;
    pub const LEFT: u32 = 1 << 2;
    pub const RIGHT: u32 = 1 << 3;
    pub const JUMP: u32 = 1 << 4;
    pub const CROUCH: u32 = 1 << 5;
    pub const SPRINT: u32 = 1 << 6;
    pub const WALK: u32 = 1 << 7;
    pub const FIRE: u32 = 1 << 8;
    pub const ALT_FIRE: u32 = 1 << 9;
    pub const RELOAD: u32 = 1 << 10;
    pub const USE: u32 = 1 << 11;
    /// ADS requested by primary fire, rather than the player's aim button.
    pub const AUTO_ADS: u32 = 1 << 12;
    /// Aliases [`AUTO_ADS`]; games using automatic aim reserve that bit.
    pub const ACTION1: u32 = 1 << 12;
    pub const ACTION2: u32 = 1 << 13;
    pub const ACTION3: u32 = 1 << 14;
    pub const ACTION4: u32 = 1 << 15;
}
