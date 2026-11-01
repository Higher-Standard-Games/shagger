//! Q3 content and surface flags as stored in the BSP
//! (SHAG `src/engine/physics/ContentFlags.ts`). Raw map bits: not the shared
//! set in `shagger_shared::collision::collision_world::contents`.

#![allow(clippy::unreadable_literal)]

pub mod content {
    pub const SOLID: i32 = 0x1;
    pub const LAVA: i32 = 0x8;
    pub const SLIME: i32 = 0x10;
    pub const WATER: i32 = 0x20;
    pub const FOG: i32 = 0x40;
    pub const AREAPORTAL: i32 = 0x8000;
    pub const PLAYERCLIP: i32 = 0x10000;
    pub const MONSTERCLIP: i32 = 0x20000;
    pub const ORIGIN: i32 = 0x1000000;
    pub const DETAIL: i32 = 0x8000000;
    pub const STRUCTURAL: i32 = 0x10000000;
    pub const TRANSLUCENT: i32 = 0x20000000;
    pub const TRIGGER: i32 = 0x40000000;
    pub const NODROP: i32 = i32::MIN;
}

pub mod surface {
    pub const NODAMAGE: i32 = 0x1;
    pub const SLICK: i32 = 0x2;
    pub const SKY: i32 = 0x4;
    pub const LADDER: i32 = 0x8;
    pub const NOIMPACT: i32 = 0x10;
    pub const NOMARKS: i32 = 0x20;
    pub const FLESH: i32 = 0x40;
    pub const NODRAW: i32 = 0x80;
    pub const HINT: i32 = 0x100;
    pub const SKIP: i32 = 0x200;
    pub const NOLIGHTMAP: i32 = 0x400;
    pub const POINTLIGHT: i32 = 0x800;
    pub const METALSTEPS: i32 = 0x1000;
    pub const NOSTEPS: i32 = 0x2000;
    pub const NONSOLID: i32 = 0x4000;
    pub const LIGHTFILTER: i32 = 0x8000;
    pub const ALPHASHADOW: i32 = 0x10000;
    pub const NODLIGHT: i32 = 0x20000;
    pub const DUST: i32 = 0x40000;
}

/// `CollisionMasks`.
pub mod masks {
    use super::content;
    pub const PLAYERSOLID: i32 = content::SOLID | content::PLAYERCLIP;
    pub const ALL: i32 = content::SOLID | content::PLAYERCLIP | content::MONSTERCLIP;
    pub const SHOT: i32 = content::SOLID;
    pub const LIQUID: i32 = content::WATER | content::LAVA | content::SLIME;
    pub const WATER: i32 = LIQUID;
    pub const OPAQUE: i32 = content::SOLID;
}

#[must_use]
pub fn is_player_solid(flags: i32) -> bool {
    flags & masks::PLAYERSOLID != 0
}

#[must_use]
pub fn is_solid(flags: i32) -> bool {
    flags & content::SOLID != 0
}

#[must_use]
pub fn is_liquid(flags: i32) -> bool {
    flags & masks::LIQUID != 0
}

/// `liquidKindOf`: lava wins over slime over water.
#[must_use]
pub fn liquid_kind_of(flags: i32) -> Option<&'static str> {
    if flags & content::LAVA != 0 {
        Some("lava")
    } else if flags & content::SLIME != 0 {
        Some("slime")
    } else if flags & content::WATER != 0 {
        Some("water")
    } else {
        None
    }
}
