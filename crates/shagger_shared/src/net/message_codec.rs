//! Message envelopes (SHAG `src/engine/network/protocol/MessageCodec.ts`).
//!
//! Every message starts with its [`MessageType`] byte. Encoders return the
//! finished bytes; decoders check the type byte first, as the TypeScript does.

use super::DecodeError;
use super::binary_protocol::{BinaryDecoder, BinaryEncoder};
use super::command::{self, Command};
use super::game_state::GameState;
use super::snapshot::{self, EventCodec};

/// `MessageType`. Server to client is 0x01 to 0x0f, client to server 0x81 to 0x8f.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageType {
    Welcome = 0x01,
    JoinAccept = 0x02,
    JoinReject = 0x03,
    /// Unidentified baseline; unsafe to decode on the live network.
    LegacySnapshot = 0x04,
    PlayerJoined = 0x05,
    PlayerLeft = 0x06,
    GameStart = 0x07,
    Ack = 0x08,
    ServerMessage = 0x09,
    Pong = 0x0a,
    Snapshot = 0x0b,
    ResumeToken = 0x0c,
    Join = 0x81,
    Ready = 0x82,
    Input = 0x83,
    Disconnect = 0x84,
    Ping = 0x85,
    JoinGame = 0x86,
    Spectate = 0x87,
    RequestSpawn = 0x88,
    Chat = 0x89,
    SetLoadout = 0x8a,
    Resume = 0x8b,
    ClientPrefs = 0x8c,
}

impl MessageType {
    /// The type for a first byte, or `None` for a byte the protocol does not define.
    #[must_use]
    pub fn from_byte(byte: u8) -> Option<Self> {
        use MessageType as M;
        Some(match byte {
            0x01 => M::Welcome,
            0x02 => M::JoinAccept,
            0x03 => M::JoinReject,
            0x04 => M::LegacySnapshot,
            0x05 => M::PlayerJoined,
            0x06 => M::PlayerLeft,
            0x07 => M::GameStart,
            0x08 => M::Ack,
            0x09 => M::ServerMessage,
            0x0a => M::Pong,
            0x0b => M::Snapshot,
            0x0c => M::ResumeToken,
            0x81 => M::Join,
            0x82 => M::Ready,
            0x83 => M::Input,
            0x84 => M::Disconnect,
            0x85 => M::Ping,
            0x86 => M::JoinGame,
            0x87 => M::Spectate,
            0x88 => M::RequestSpawn,
            0x89 => M::Chat,
            0x8a => M::SetLoadout,
            0x8b => M::Resume,
            0x8c => M::ClientPrefs,
            _ => return None,
        })
    }
}

/// Type byte, full/delta byte, baseline tick.
pub const SNAPSHOT_HEADER_BYTES: usize = 6;

/// `SnapshotHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotHeader {
    pub tick: u32,
    /// `None` for a full snapshot.
    pub baseline_tick: Option<u32>,
}

/// `peekMessageType`: the first byte, `None` for empty data.
#[must_use]
pub fn peek_message_type(data: &[u8]) -> Option<u8> {
    data.first().copied()
}

fn begin(data: &[u8], expected: MessageType) -> Result<BinaryDecoder<'_>, DecodeError> {
    let mut decoder = BinaryDecoder::new(data);
    let got = decoder.read_uint8()?;
    if got != expected as u8 {
        return Err(DecodeError::WrongMessageType {
            expected: expected as u8,
            got,
        });
    }
    Ok(decoder)
}

fn message(kind: MessageType) -> BinaryEncoder {
    let mut encoder = BinaryEncoder::new();
    encoder.write_uint8(kind as u8);
    encoder
}

/// `readSnapshotHeader`.
///
/// # Errors
/// A legacy snapshot, another message, or a malformed baseline header.
pub fn read_snapshot_header(data: &[u8]) -> Result<SnapshotHeader, DecodeError> {
    let mut decoder = BinaryDecoder::new(data);
    let kind = decoder.read_uint8()?;
    if kind == MessageType::LegacySnapshot as u8 {
        return Err(DecodeError::Invalid(
            "Unsupported legacy network snapshot: update server and clients together",
        ));
    }
    if kind != MessageType::Snapshot as u8 {
        return Err(DecodeError::WrongMessageType {
            expected: MessageType::Snapshot as u8,
            got: kind,
        });
    }
    let has_baseline = decoder.read_uint8()?;
    let baseline_tick = decoder.read_uint32()?;
    if has_baseline > 1 || (has_baseline == 0 && baseline_tick != 0) {
        return Err(DecodeError::Invalid("Invalid snapshot baseline header"));
    }
    Ok(SnapshotHeader {
        tick: decoder.read_uint32()?,
        baseline_tick: (has_baseline != 0).then_some(baseline_tick),
    })
}

/// `MessageCodec.decodeSnapshot`. `baselines` finds a retained state by tick.
///
/// # Errors
/// A bad header, a baseline the caller does not hold, or a bad body.
pub fn decode_snapshot<'b, C: EventCodec>(
    data: &[u8],
    baselines: impl Fn(u32) -> Option<&'b GameState<C::Event>>,
    codec: &C,
) -> Result<GameState<C::Event>, DecodeError>
where
    C::Event: 'b,
{
    let header = read_snapshot_header(data)?;
    let baseline = match header.baseline_tick {
        None => None,
        Some(tick) => match baselines(tick) {
            Some(b) if b.tick == tick => Some(b),
            _ => return Err(DecodeError::MissingBaseline { baseline_tick: tick }),
        },
    };
    snapshot::decode_snapshot(&data[SNAPSHOT_HEADER_BYTES..], baseline, codec)
}

/// `encodeCommand`: an Input message.
#[must_use]
pub fn encode_command(player_id: &str, sequence: u32, command: &Command) -> Vec<u8> {
    let mut encoder = message(MessageType::Input);
    command::encode_command(&mut encoder, player_id, sequence, command);
    encoder.into_data()
}

/// `decodeCommand`.
///
/// # Errors
/// Another message, or truncated.
pub fn decode_command(data: &[u8]) -> Result<(String, u32, Command), DecodeError> {
    let mut decoder = begin(data, MessageType::Input)?;
    command::decode_command(&mut decoder)
}

/// `peekCommandSeq`: the sequence of an Input message without decoding it.
#[must_use]
pub fn peek_command_seq(data: &[u8]) -> Option<u32> {
    if data.len() < 3 || data[0] != MessageType::Input as u8 {
        return None;
    }
    let at = 3 + usize::from(u16::from_le_bytes([data[1], data[2]]));
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// The Welcome message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Welcome {
    pub server_id: String,
    pub server_name: String,
    pub map: String,
    pub tick_rate: u8,
    pub max_players: u8,
    pub current_players: u8,
}

#[must_use]
pub fn encode_welcome(w: &Welcome) -> Vec<u8> {
    let mut e = message(MessageType::Welcome);
    e.write_string(&w.server_id);
    e.write_string(&w.server_name);
    e.write_string(&w.map);
    e.write_uint8(w.tick_rate);
    e.write_uint8(w.max_players);
    e.write_uint8(w.current_players);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_welcome(data: &[u8]) -> Result<Welcome, DecodeError> {
    let mut d = begin(data, MessageType::Welcome)?;
    Ok(Welcome {
        server_id: d.read_string()?,
        server_name: d.read_string()?,
        map: d.read_string()?,
        tick_rate: d.read_uint8()?,
        max_players: d.read_uint8()?,
        current_players: d.read_uint8()?,
    })
}

/// The Join message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Join {
    pub player_name: String,
    pub is_bot: bool,
    pub admission_ticket: Option<String>,
}

/// `encodeJoin`. An empty ticket is left out, as JavaScript's truthiness does.
#[must_use]
pub fn encode_join(j: &Join) -> Vec<u8> {
    let mut e = message(MessageType::Join);
    e.write_string(&j.player_name);
    e.write_uint8(u8::from(j.is_bot));
    if let Some(ticket) = j.admission_ticket.as_deref().filter(|t| !t.is_empty()) {
        e.write_string(ticket);
    }
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_join(data: &[u8]) -> Result<Join, DecodeError> {
    let mut d = begin(data, MessageType::Join)?;
    let player_name = d.read_string()?;
    let is_bot = if d.has_more() { d.read_uint8()? != 0 } else { false };
    let admission_ticket = if d.has_more() { Some(d.read_string()?) } else { None };
    Ok(Join {
        player_name,
        is_bot,
        admission_ticket,
    })
}

/// The JoinAccept message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JoinAccept {
    pub client_id: String,
    pub player_id: String,
    pub entity_id: String,
}

#[must_use]
pub fn encode_join_accept(j: &JoinAccept) -> Vec<u8> {
    let mut e = message(MessageType::JoinAccept);
    e.write_string(&j.client_id);
    e.write_string(&j.player_id);
    e.write_string(&j.entity_id);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_join_accept(data: &[u8]) -> Result<JoinAccept, DecodeError> {
    let mut d = begin(data, MessageType::JoinAccept)?;
    Ok(JoinAccept {
        client_id: d.read_string()?,
        player_id: d.read_string()?,
        entity_id: d.read_string()?,
    })
}

/// The ResumeToken message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResumeToken {
    pub token: String,
    pub grace_ms: u32,
}

#[must_use]
pub fn encode_resume_token(r: &ResumeToken) -> Vec<u8> {
    let mut e = message(MessageType::ResumeToken);
    e.write_string(&r.token);
    e.write_uint32(r.grace_ms);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_resume_token(data: &[u8]) -> Result<ResumeToken, DecodeError> {
    let mut d = begin(data, MessageType::ResumeToken)?;
    Ok(ResumeToken {
        token: d.read_string()?,
        grace_ms: d.read_uint32()?,
    })
}

#[must_use]
pub fn encode_resume(token: &str) -> Vec<u8> {
    let mut e = message(MessageType::Resume);
    e.write_string(token);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_resume(data: &[u8]) -> Result<String, DecodeError> {
    begin(data, MessageType::Resume)?.read_string()
}

#[must_use]
pub fn encode_ack(tick: u32) -> Vec<u8> {
    let mut e = message(MessageType::Ack);
    e.write_uint32(tick);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_ack(data: &[u8]) -> Result<u32, DecodeError> {
    begin(data, MessageType::Ack)?.read_uint32()
}

#[must_use]
pub fn encode_ready() -> Vec<u8> {
    message(MessageType::Ready).into_data()
}

/// The JoinGame message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JoinGame {
    pub team_id: Option<String>,
    pub manual_pick: bool,
    pub admission_ticket: Option<String>,
}

/// `encodeJoinGame`. Empty strings count as absent, as in JavaScript.
#[must_use]
pub fn encode_join_game(j: &JoinGame) -> Vec<u8> {
    let mut e = message(MessageType::JoinGame);
    if let Some(team) = j.team_id.as_deref().filter(|t| !t.is_empty()) {
        e.write_uint8(if j.manual_pick { 2 } else { 1 });
        e.write_string(team);
    } else {
        e.write_uint8(0);
    }
    if let Some(ticket) = j.admission_ticket.as_deref().filter(|t| !t.is_empty()) {
        e.write_string(ticket);
    }
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_join_game(data: &[u8]) -> Result<JoinGame, DecodeError> {
    let mut d = begin(data, MessageType::JoinGame)?;
    let team_flag = d.read_uint8()?;
    let team_id = if team_flag > 0 { Some(d.read_string()?) } else { None };
    let admission_ticket = if d.has_more() { Some(d.read_string()?) } else { None };
    Ok(JoinGame {
        team_id,
        manual_pick: team_flag == 2,
        admission_ticket,
    })
}

#[must_use]
pub fn encode_spectate() -> Vec<u8> {
    message(MessageType::Spectate).into_data()
}

#[must_use]
pub fn encode_request_spawn() -> Vec<u8> {
    message(MessageType::RequestSpawn).into_data()
}

#[must_use]
pub fn encode_set_loadout(primary: &str, secondary: &str) -> Vec<u8> {
    let mut e = message(MessageType::SetLoadout);
    e.write_string(primary);
    e.write_string(secondary);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_set_loadout(data: &[u8]) -> Result<(String, String), DecodeError> {
    let mut d = begin(data, MessageType::SetLoadout)?;
    Ok((d.read_string()?, d.read_string()?))
}

/// `ClientPrefs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientPrefs {
    pub auto_switch: bool,
}

/// `encodeClientPrefs`: bit 0 set means auto-switch is off.
#[must_use]
pub fn encode_client_prefs(prefs: ClientPrefs) -> Vec<u8> {
    let mut e = message(MessageType::ClientPrefs);
    e.write_uint8(if prefs.auto_switch { 0 } else { 1 });
    e.into_data()
}

/// # Errors
/// Another message.
pub fn decode_client_prefs(data: &[u8]) -> Result<ClientPrefs, DecodeError> {
    let mut d = begin(data, MessageType::ClientPrefs)?;
    let flags = if d.has_more() { d.read_uint8()? } else { 0 };
    Ok(ClientPrefs {
        auto_switch: flags & 1 == 0,
    })
}

#[must_use]
pub fn encode_chat(text: &str) -> Vec<u8> {
    let mut e = message(MessageType::Chat);
    e.write_string(text);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_chat(data: &[u8]) -> Result<String, DecodeError> {
    begin(data, MessageType::Chat)?.read_string()
}

/// The ServerMessage message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerMessage {
    /// `"info"` by default; also queue and rejection kinds.
    pub kind: String,
    pub message: String,
}

#[must_use]
pub fn encode_server_message(m: &ServerMessage) -> Vec<u8> {
    let mut e = message(MessageType::ServerMessage);
    e.write_string(&m.kind);
    e.write_string(&m.message);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_server_message(data: &[u8]) -> Result<ServerMessage, DecodeError> {
    let mut d = begin(data, MessageType::ServerMessage)?;
    Ok(ServerMessage {
        kind: d.read_string()?,
        message: d.read_string()?,
    })
}

#[must_use]
pub fn encode_ping(client_send_time: f64) -> Vec<u8> {
    let mut e = message(MessageType::Ping);
    e.write_float64(client_send_time);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_ping(data: &[u8]) -> Result<f64, DecodeError> {
    begin(data, MessageType::Ping)?.read_float64()
}

/// The Pong message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pong {
    pub client_send_time: f64,
    pub server_time: f64,
}

#[must_use]
pub fn encode_pong(p: Pong) -> Vec<u8> {
    let mut e = message(MessageType::Pong);
    e.write_float64(p.client_send_time);
    e.write_float64(p.server_time);
    e.into_data()
}

/// # Errors
/// Another message, or truncated.
pub fn decode_pong(data: &[u8]) -> Result<Pong, DecodeError> {
    let mut d = begin(data, MessageType::Pong)?;
    Ok(Pong {
        client_send_time: d.read_float64()?,
        server_time: d.read_float64()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_type_byte_round_trips() {
        for byte in 0..=255u8 {
            if let Some(t) = MessageType::from_byte(byte) {
                assert_eq!(t as u8, byte);
            }
        }
    }

    #[test]
    fn peek_command_seq_reads_the_sequence() {
        let data = encode_command("p1", 0xdead_beef, &Command::default());
        assert_eq!(peek_command_seq(&data), Some(0xdead_beef));
        assert_eq!(peek_command_seq(&data[..6]), None);
        assert_eq!(peek_command_seq(&[0x82, 0, 0]), None);
    }

    #[test]
    fn snapshot_header_rejects_what_the_typescript_rejects() {
        let mut full = vec![0x0b, 0, 0, 0, 0, 0, 7, 0, 0, 0];
        assert_eq!(
            read_snapshot_header(&full),
            Ok(SnapshotHeader {
                tick: 7,
                baseline_tick: None
            })
        );
        full[2] = 1;
        assert!(matches!(read_snapshot_header(&full), Err(DecodeError::Invalid(_))));
        full[1] = 2;
        assert!(matches!(read_snapshot_header(&full), Err(DecodeError::Invalid(_))));
        full[0] = 0x04;
        assert!(matches!(read_snapshot_header(&full), Err(DecodeError::Invalid(_))));
    }
}
