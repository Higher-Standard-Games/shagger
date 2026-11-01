//! The decoded game state (SHAG `src/shared/net/GameState.ts`, `Entity.ts`).
//!
//! The TypeScript stores untyped components by name. A client only ever sees
//! the shapes `BinaryProtocol.decodeEntities` builds, so here each one is a
//! typed, optional field with exactly those values.

use std::collections::HashMap;

/// A map that iterates in insertion order, like a JavaScript `Map`.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderedMap<T> {
    entries: Vec<(String, T)>,
    index: HashMap<String, usize>,
}

impl<T> Default for OrderedMap<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<T> OrderedMap<T> {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&T> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut T> {
        self.index.get(key).map(|&i| &mut self.entries[i].1)
    }

    /// The value under `key`, inserted with `make` if absent. An existing key
    /// keeps its position, as `Map.set` does.
    pub fn get_or_insert_with(&mut self, key: &str, make: impl FnOnce() -> T) -> &mut T {
        let i = match self.index.get(key) {
            Some(&i) => i,
            None => {
                self.entries.push((key.to_owned(), make()));
                self.index.insert(key.to_owned(), self.entries.len() - 1);
                self.entries.len() - 1
            }
        };
        &mut self.entries[i].1
    }

    pub fn insert(&mut self, key: &str, value: T) {
        if let Some(&i) = self.index.get(key) {
            self.entries[i].1 = value;
        } else {
            self.index.insert(key.to_owned(), self.entries.len());
            self.entries.push((key.to_owned(), value));
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|(_, v)| v)
    }
}

pub use crate::server::simulation::types::Vec3;

/// `transform`: position, and rotation as `{ x: pitch, y: yaw, z: 0, w: 1 }`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    /// `[x, y, z, w]`: the wire sets x to pitch and y to yaw.
    pub rotation: [f64; 4],
}

/// `velocity`, with the flags the decoder copies into it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Velocity {
    pub linear: Vec3,
    pub grounded: bool,
    pub is_crouching: bool,
    pub is_sliding: bool,
}

/// What a watcher needs to rebuild another player's screen (movement flag bit 7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementView {
    /// -1 left, 0 none, 1 right.
    pub strafe_input: i8,
    /// -1 back, 0 none, 1 forward.
    pub forward_input: i8,
    /// 0 to 1, in 1/255 steps.
    pub slide_phase: f64,
    pub ledge_grab_phase: f64,
    /// In quarter units.
    pub crouch_view_offset: f64,
}

/// `movementState`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MovementState {
    pub is_crouching: bool,
    pub is_sliding: bool,
    /// `wallJumpState === "charging"`; the wire carries nothing else.
    pub is_wall_jumping: bool,
    pub is_aiming: bool,
    pub view: Option<MovementView>,
}

/// `weaponMeta`, on `item_` entities.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeaponMeta {
    pub weapon_id: String,
    pub settled: bool,
}

/// One entity and the components the snapshot gave it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entity {
    /// The legacy `position` component. Set together with `transform` when
    /// the position travels, but a rotation alone makes a `transform` without it.
    pub position: Option<Vec3>,
    pub transform: Option<Transform>,
    pub velocity: Option<Velocity>,
    pub movement_state: Option<MovementState>,
    pub weapon_meta: Option<WeaponMeta>,
    /// Bit per entry of the game's powerup kinds.
    pub powerups: Option<u8>,
}

/// `PlayerState`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerState {
    pub entity_id: String,
    pub name: String,
    pub team: String,
    pub score: i32,
    pub deaths: u32,
    pub ping: u16,
}

/// A decoded snapshot. `E` is the game's event type: the event catalogue
/// belongs to the game (RUST.md 14.2), so the engine is generic over it.
#[derive(Debug, Clone, PartialEq)]
pub struct GameState<E> {
    pub tick: u32,
    /// The server's `Date.now()` when it encoded the snapshot.
    pub server_timestamp: f64,
    pub acked_input_seq: u32,
    pub entities: OrderedMap<Entity>,
    pub players: OrderedMap<PlayerState>,
    pub events: Vec<E>,
    /// Active state of every map pickup, by pickup id. Empty when absent.
    pub pickup_active_mask: Vec<bool>,
}

impl<E> Default for GameState<E> {
    fn default() -> Self {
        Self {
            tick: 0,
            server_timestamp: 0.0,
            acked_input_seq: 0,
            entities: OrderedMap::default(),
            players: OrderedMap::default(),
            events: Vec::new(),
            pickup_active_mask: Vec::new(),
        }
    }
}
