//! Snapshot decoding (SHAG `src/shared/net/BinaryProtocol.ts`:
//! `decodeSnapshot`, `decodeEntities`, `decodePlayers`, `decodePickupState`).
//!
//! The event catalogue belongs to the game (RUST.md 14.2), so events are read
//! through an [`EventCodec`] the game supplies.

use super::DecodeError;
use super::binary_protocol::BinaryDecoder;
use super::game_state::{Entity, GameState, MovementState, MovementView, PlayerState, Transform, Vec3, Velocity};

/// Reads one event body after its type code.
pub trait EventCodec {
    type Event;

    /// Reads the event with type code `code`. `Ok(None)` means the code is
    /// unknown: like the TypeScript switch, nothing is read and nothing is
    /// pushed.
    ///
    /// # Errors
    /// When the event body is truncated.
    fn decode_event(&self, code: u8, decoder: &mut BinaryDecoder<'_>) -> Result<Option<Self::Event>, DecodeError>;
}

/// An [`EventCodec`] that knows no events, for games and tests without them.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoEvents;

impl EventCodec for NoEvents {
    type Event = std::convert::Infallible;

    fn decode_event(&self, _code: u8, _decoder: &mut BinaryDecoder<'_>) -> Result<Option<Self::Event>, DecodeError> {
        Ok(None)
    }
}

/// `BinaryProtocol.decodeSnapshot`: decodes a raw snapshot (no message
/// envelope) against an optional baseline.
///
/// # Errors
/// When the payload is truncated or an event body fails to decode.
pub fn decode_snapshot<C: EventCodec>(
    data: &[u8],
    baseline: Option<&GameState<C::Event>>,
    codec: &C,
) -> Result<GameState<C::Event>, DecodeError> {
    let mut decoder = BinaryDecoder::new(data);
    let mut state = GameState::<C::Event> {
        tick: decoder.read_uint32()?,
        server_timestamp: decoder.read_float64()?,
        acked_input_seq: decoder.read_uint32()?,
        ..GameState::default()
    };
    decode_entities(&mut decoder, &mut state, baseline)?;
    decode_players(&mut decoder, &mut state, baseline)?;
    decode_events(&mut decoder, &mut state, codec)?;
    if decoder.has_more() {
        decode_pickup_state(&mut decoder, &mut state)?;
    }
    Ok(state)
}

const IDENTITY_ROTATION: [f64; 4] = [0.0, 0.0, 0.0, 1.0];

fn key_of(bits: u8) -> i8 {
    match bits {
        1 => 1,
        2 => -1,
        _ => 0,
    }
}

fn decode_entities<E>(
    decoder: &mut BinaryDecoder<'_>,
    state: &mut GameState<E>,
    baseline: Option<&GameState<E>>,
) -> Result<(), DecodeError> {
    let count = decoder.read_uint16()?;
    for _ in 0..count {
        let entity_id = decoder.read_string()?;
        let flags = decoder.read_uint8()?;
        let base = baseline.and_then(|b| b.entities.get(&entity_id));
        let entity = state.entities.get_or_insert_with(&entity_id, Entity::default);
        decode_entity(decoder, entity, base, flags)?;
    }
    Ok(())
}

fn decode_entity(
    decoder: &mut BinaryDecoder<'_>,
    entity: &mut Entity,
    base: Option<&Entity>,
    flags: u8,
) -> Result<(), DecodeError> {
    if flags & 0x01 != 0 {
        let position = Vec3 {
            x: decoder.read_float32()?,
            y: decoder.read_float32()?,
            z: decoder.read_float32()?,
        };
        entity.position = Some(position);
        let rotation = entity
            .transform
            .or_else(|| base.and_then(|b| b.transform))
            .map_or(IDENTITY_ROTATION, |t| t.rotation);
        entity.transform = Some(Transform { position, rotation });
    } else if let Some(base) = base {
        if base.position.is_some() {
            entity.position = base.position;
        }
        if base.transform.is_some() {
            entity.transform = base.transform;
        }
    }

    let base_velocity = base.and_then(|b| b.velocity);
    let mut grounded = base_velocity.is_some_and(|v| v.grounded);
    let mut linear = Vec3::default();
    let mut has_velocity_data = false;
    if flags & 0x02 != 0 {
        linear = Vec3 {
            x: decoder.read_float32()?,
            y: decoder.read_float32()?,
            z: decoder.read_float32()?,
        };
        has_velocity_data = true;
    } else if let Some(v) = base_velocity {
        linear = v.linear;
        has_velocity_data = true;
    }
    if flags & 0x04 != 0 {
        grounded = decoder.read_uint8()? != 0;
    }

    if flags & 0x08 != 0 {
        let yaw = decoder.read_float32()?;
        let pitch = decoder.read_float32()?;
        let rotation = [pitch, yaw, 0.0, 1.0];
        match &mut entity.transform {
            Some(t) => t.rotation = rotation,
            None => {
                entity.transform = Some(Transform {
                    position: Vec3::default(),
                    rotation,
                })
            }
        }
    }

    let mut movement = MovementState::default();
    if flags & 0x10 != 0 {
        let bits = decoder.read_uint8()?;
        movement.is_crouching = bits & 1 != 0;
        movement.is_sliding = bits & 2 != 0;
        movement.is_wall_jumping = bits & 4 != 0;
        movement.is_aiming = bits & 8 != 0;
        if bits & 0x80 != 0 {
            let keys = decoder.read_uint8()?;
            movement.view = Some(MovementView {
                strafe_input: key_of(keys & 3),
                forward_input: key_of((keys >> 2) & 3),
                slide_phase: f64::from(decoder.read_uint8()?) / 255.0,
                ledge_grab_phase: f64::from(decoder.read_uint8()?) / 255.0,
                crouch_view_offset: f64::from(decoder.read_int8()?) / 4.0,
            });
        }
    } else if let Some(m) = base.and_then(|b| b.movement_state) {
        // The view fields are not carried forward.
        movement = MovementState { view: None, ..m };
    }

    if has_velocity_data || flags & 0x14 != 0 {
        entity.velocity = Some(Velocity {
            linear,
            grounded,
            is_crouching: movement.is_crouching,
            is_sliding: movement.is_sliding,
        });
        entity.movement_state = Some(movement);
    }

    if flags & 0x20 != 0 {
        let weapon_id = decoder.read_string()?;
        let settled = decoder.read_uint8()? != 0;
        entity.weapon_meta = Some(super::game_state::WeaponMeta { weapon_id, settled });
    } else if let Some(meta) = base.and_then(|b| b.weapon_meta.as_ref()) {
        entity.weapon_meta = Some(meta.clone());
    }

    // Not carried forward: the encoder sends it every snapshot.
    if flags & 0x40 != 0 {
        entity.powerups = Some(decoder.read_uint8()?);
    }
    Ok(())
}

fn decode_players<E>(
    decoder: &mut BinaryDecoder<'_>,
    state: &mut GameState<E>,
    baseline: Option<&GameState<E>>,
) -> Result<(), DecodeError> {
    let count = decoder.read_uint16()?;
    for _ in 0..count {
        let player_id = decoder.read_string()?;
        let flags = decoder.read_uint8()?;
        let base = baseline.and_then(|b| b.players.get(&player_id));
        let player = state.players.get_or_insert_with(&player_id, PlayerState::default);
        if flags & 0x01 != 0 {
            player.entity_id = decoder.read_string()?;
        } else if let Some(b) = base {
            player.entity_id.clone_from(&b.entity_id);
        }
        if flags & 0x02 != 0 {
            player.name = decoder.read_string()?;
        } else if let Some(b) = base {
            player.name.clone_from(&b.name);
        }
        if flags & 0x04 != 0 {
            player.score = decoder.read_int32()?;
        } else if let Some(b) = base {
            player.score = b.score;
        }
        if flags & 0x08 != 0 {
            player.deaths = decoder.read_uint32()?;
        } else if let Some(b) = base {
            player.deaths = b.deaths;
        }
        if flags & 0x10 != 0 {
            player.ping = decoder.read_uint16()?;
        } else if let Some(b) = base {
            player.ping = b.ping;
        }
        if flags & 0x20 != 0 {
            player.team = decoder.read_string()?;
        } else if let Some(b) = base {
            player.team.clone_from(&b.team);
        }
        if flags & 0x40 != 0 {
            player.look = Some(decoder.read_uint8()?);
        } else if let Some(b) = base {
            player.look = b.look;
        }
    }
    Ok(())
}

fn decode_events<C: EventCodec>(
    decoder: &mut BinaryDecoder<'_>,
    state: &mut GameState<C::Event>,
    codec: &C,
) -> Result<(), DecodeError> {
    let count = decoder.read_uint16()?;
    for _ in 0..count {
        let code = decoder.read_uint8()?;
        if let Some(event) = codec.decode_event(code, decoder)? {
            state.events.push(event);
        }
    }
    Ok(())
}

fn decode_pickup_state<E>(decoder: &mut BinaryDecoder<'_>, state: &mut GameState<E>) -> Result<(), DecodeError> {
    let count = usize::from(decoder.read_uint16()?);
    let mut mask = vec![false; count];
    for b in 0..count.div_ceil(8) {
        let byte = decoder.read_uint8()?;
        for bit in 0..8 {
            let idx = b * 8 + bit;
            if idx < count {
                mask[idx] = byte & (1 << bit) != 0;
            }
        }
    }
    state.pickup_active_mask = mask;
    Ok(())
}
