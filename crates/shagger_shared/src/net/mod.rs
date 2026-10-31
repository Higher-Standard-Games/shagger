//! The wire protocol (SHAG `src/shared/net` and `src/engine/network/protocol`).
//!
//! Byte for byte the TypeScript codec: little-endian, strings as a u16 byte
//! length then UTF-8. Every decoder here is checked against bytes and decoded
//! states produced by SHAG's own `BinaryProtocol` (`tools/golden`).

pub mod binary_protocol;
pub mod command;
pub mod game_state;
pub mod message_codec;
pub mod snapshot;

use std::fmt;

/// Why bytes could not be decoded.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodeError {
    /// `BinaryDecoder: Buffer underflow` in the TypeScript.
    Underflow { offset: usize, need: usize, have: usize },
    /// A JSON field that `JSON.parse` would throw on.
    Json(crate::json::ParseError),
    /// The first byte is not the message type this decoder reads.
    WrongMessageType { expected: u8, got: u8 },
    /// A message the TypeScript also refuses, with its reason.
    Invalid(&'static str),
    /// A delta snapshot whose baseline the caller does not hold
    /// (`MissingSnapshotBaselineError`).
    MissingBaseline { baseline_tick: u32 },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Underflow { offset, need, have } => {
                write!(f, "buffer underflow at offset {offset}, need {need} bytes, have {have}")
            }
            Self::Json(e) => e.fmt(f),
            Self::WrongMessageType { expected, got } => write!(f, "expected message 0x{expected:02x}, got 0x{got:02x}"),
            Self::Invalid(why) => f.write_str(why),
            Self::MissingBaseline { baseline_tick } => write!(f, "missing snapshot baseline {baseline_tick}"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<crate::json::ParseError> for DecodeError {
    fn from(e: crate::json::ParseError) -> Self {
        Self::Json(e)
    }
}
