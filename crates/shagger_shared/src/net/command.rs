//! Input commands (`BinaryProtocol.encodeCommand` / `decodeCommand`).

use super::DecodeError;
use super::binary_protocol::{BinaryDecoder, BinaryEncoder};

/// A movement command as it travels: `NetworkCommand` in the TypeScript.
///
/// Floats are the `f64` values the sender holds; the wire carries them as
/// `f32`. RUST.md 19.6: predict with the `f32`-rounded values, because that
/// is what the server simulates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Command {
    pub direction: [f64; 3],
    pub pitch: f64,
    pub yaw: f64,
    pub buttons: u16,
    /// The view tick, used by the server for lag-compensation rewind.
    pub tick: u32,
    pub weapon_slot: u8,
}

impl Command {
    /// The command with every float rounded to the `f32` the wire carries.
    #[must_use]
    pub fn as_sent(&self) -> Self {
        let r = |x: f64| f64::from(x as f32);
        Self {
            direction: self.direction.map(r),
            pitch: r(self.pitch),
            yaw: r(self.yaw),
            ..*self
        }
    }
}

/// `encodeCommand(playerId, sequence, command)` after any envelope prefix.
pub fn encode_command(encoder: &mut BinaryEncoder, player_id: &str, sequence: u32, command: &Command) {
    encoder.write_string(player_id);
    encoder.write_uint32(sequence);
    for d in command.direction {
        encoder.write_float32(d);
    }
    encoder.write_float32(command.pitch);
    encoder.write_float32(command.yaw);
    encoder.write_uint16(command.buttons);
    encoder.write_uint32(command.tick);
    encoder.write_uint8(command.weapon_slot);
}

/// `decodeCommand`: the player id, the sequence and the command.
pub fn decode_command(decoder: &mut BinaryDecoder<'_>) -> Result<(String, u32, Command), DecodeError> {
    let player_id = decoder.read_string()?;
    let sequence = decoder.read_uint32()?;
    let direction = [
        decoder.read_float32()?,
        decoder.read_float32()?,
        decoder.read_float32()?,
    ];
    let command = Command {
        direction,
        pitch: decoder.read_float32()?,
        yaw: decoder.read_float32()?,
        buttons: decoder.read_uint16()?,
        tick: decoder.read_uint32()?,
        weapon_slot: decoder.read_uint8()?,
    };
    Ok((player_id, sequence, command))
}
