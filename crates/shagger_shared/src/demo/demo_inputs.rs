//! SHAG `src/shared/demo/DemoInputs.ts`: the commands a demo records with its
//! state, from format version 3. The client only reads them; aggregating and
//! encoding them is the server's.

use crate::net::DecodeError;
use crate::net::binary_protocol::BinaryDecoder;

/// One player's input over the interval a frame covers.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedInput {
    pub player_id: String,
    /// Bitwise OR of every button held or pressed during the interval.
    pub buttons: u32,
    /// View angles as last applied, radians (f32 on the wire).
    pub yaw: f64,
    pub pitch: f64,
    /// Wish direction as last applied, each in [-1, 1].
    pub forward: f64,
    pub right: f64,
    /// Weapon slot as last applied.
    pub weapon_slot: u8,
    /// How many commands were aggregated into this record.
    pub commands: u8,
}

/// `decodeInputs`.
///
/// # Errors
/// When the record list is truncated.
pub fn decode_inputs(data: &[u8]) -> Result<Vec<RecordedInput>, DecodeError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let mut decoder = BinaryDecoder::new(data);
    let count = decoder.read_uint8()?;
    let mut inputs = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        inputs.push(RecordedInput {
            player_id: decoder.read_string()?,
            buttons: decoder.read_uint32()?,
            yaw: decoder.read_float32()?,
            pitch: decoder.read_float32()?,
            forward: decoder.read_float32()?,
            right: decoder.read_float32()?,
            weapon_slot: decoder.read_uint8()?,
            commands: decoder.read_uint8()?,
        });
    }
    Ok(inputs)
}
