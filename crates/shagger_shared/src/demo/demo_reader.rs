//! SHAG `src/shared/demo/DemoReader.ts`: the `.shagdemo` parser.
//!
//! A demo stores a full (non-delta) raw snapshot per frame, so any frame
//! decodes on its own through the index table. Layout (RUST.md 18.5), all
//! little-endian, the whole file optionally gzip:
//!
//! ```text
//! "SHAG" version:u8 (1..=6)
//! map:str gametype:str tickRate:u8 snapshotRate:u8 startTimeHigh:u32 startTimeLow:u32
//! v2+: matchId:str
//! v4+: visibility:u8 (0 public, 1 private)
//! playerCount:u8, then per player: playerId:str name:str team:str [v2+: accountId:str]
//! frames: tick:u32 frameLen:u32 raw-snapshot[frameLen] [v3+: inputLen:u32 inputs[inputLen]]
//! index: frameCount:u32, then frameCount × (tick:u32 offset:u32)
//! trailer: durationMs:u32 indexOffset:u32          (the last 8 bytes)
//! ```
//!
//! Reads keep the TypeScript's tolerance exactly: a string that runs past the
//! end is cut short rather than refused, a frame that fails to decode is
//! skipped, and inputs that fail to decode leave the frame with none.

use std::fmt;
use std::io::Read;

use flate2::bufread::GzDecoder;

use super::demo_inputs::{RecordedInput, decode_inputs};
use crate::js::js_string_to_number;
use crate::net::game_state::GameState;
use crate::net::snapshot::{EventCodec, decode_snapshot};

/// Why a demo cannot be read. The messages are the TypeScript's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DemoError {
    /// The gzip stream is damaged (`zlib.gunzipSync` throws).
    Gzip(String),
    /// `Not a valid .shagdemo file (bad magic bytes)`.
    BadMagic,
    /// `Unsupported demo version: N`.
    UnsupportedVersion(u8),
    /// `Invalid demo visibility`.
    InvalidVisibility,
    /// `Invalid demo index offset: N (file length: M)`.
    InvalidIndexOffset { index_offset: u32, file_length: usize },
    /// A `DataView` read past the end (`RangeError: Offset is outside the bounds of the DataView`).
    OutOfBounds,
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gzip(why) => write!(f, "gzip: {why}"),
            Self::BadMagic => f.write_str("Not a valid .shagdemo file (bad magic bytes)"),
            Self::UnsupportedVersion(v) => write!(f, "Unsupported demo version: {v}"),
            Self::InvalidVisibility => f.write_str("Invalid demo visibility"),
            Self::InvalidIndexOffset {
                index_offset,
                file_length,
            } => write!(
                f,
                "Invalid demo index offset: {index_offset} (file length: {file_length})"
            ),
            Self::OutOfBounds => f.write_str("Offset is outside the bounds of the DataView"),
        }
    }
}

impl std::error::Error for DemoError {}

#[derive(Debug, Clone, PartialEq)]
pub struct DemoHeader {
    /// Format version, 1 to 6.
    pub version: u8,
    pub map: String,
    /// Gametype (duel, ffa, tdm, and so on).
    pub gametype: String,
    pub tick_rate: u8,
    pub snapshot_rate: u8,
    /// Recording start, ms since the epoch.
    pub start_time: f64,
    /// Version 2+: the lobby's match id. `None` when absent or empty.
    pub match_id: Option<String>,
    /// Version 4+: lobby visibility. `None` for older demos.
    pub private: Option<bool>,
    pub players: Vec<DemoPlayerInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DemoPlayerInfo {
    pub player_id: String,
    pub name: String,
    pub team: String,
    /// Version 2+: the account from a verified seat, when the recorded text
    /// is a positive safe integer.
    pub account_id: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoIndexEntry {
    pub tick: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoIndex {
    pub frame_count: u32,
    pub entries: Vec<DemoIndexEntry>,
    pub duration_ms: u32,
    pub index_offset: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DemoFrame<E> {
    pub tick: u32,
    pub state: GameState<E>,
    /// Version 3+: the commands the server applied over this frame's interval.
    pub inputs: Vec<RecordedInput>,
}

/// `DemoReader`. Header and index are parsed once and kept.
#[derive(Debug, Clone)]
pub struct DemoReader {
    data: Vec<u8>,
    header: Option<DemoHeader>,
    index: Option<DemoIndex>,
}

/// Inflates every gzip member, as Node's `gunzipSync` does: after a member,
/// bytes that start another gzip header are inflated too, anything else is
/// dropped.
fn gunzip(raw: &[u8]) -> Result<Vec<u8>, DemoError> {
    let mut out = Vec::new();
    let mut input = raw;
    loop {
        let mut decoder = GzDecoder::new(input);
        decoder
            .read_to_end(&mut out)
            .map_err(|e| DemoError::Gzip(e.to_string()))?;
        // A `&[u8]` reader is advanced in place: what is left follows the member.
        input = decoder.into_inner();
        if !input.starts_with(&[0x1f, 0x8b]) {
            return Ok(out);
        }
    }
}

/// Reads at `offset` the way a `DataView` getter does: out of bounds throws.
fn bytes<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], DemoError> {
    data.get(offset..offset.checked_add(N).ok_or(DemoError::OutOfBounds)?)
        .map(|b| b.try_into().expect("length checked"))
        .ok_or(DemoError::OutOfBounds)
}

fn u8_at(data: &[u8], offset: usize) -> Result<u8, DemoError> {
    Ok(bytes::<1>(data, offset)?[0])
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, DemoError> {
    Ok(u16::from_le_bytes(bytes(data, offset)?))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, DemoError> {
    Ok(u32::from_le_bytes(bytes(data, offset)?))
}

/// `Uint8Array.slice`: clamped to the data, never an error.
fn slice(data: &[u8], start: usize, end: usize) -> &[u8] {
    let end = end.min(data.len());
    &data[start.min(end)..end]
}

/// `_readString`: u16 length, then that many bytes (fewer at the end of the
/// file) through `TextDecoder`. Returns the text and the bytes it claims.
fn read_string(data: &[u8], offset: usize) -> Result<(String, usize), DemoError> {
    let length = usize::from(u16_at(data, offset)?);
    let raw = slice(data, offset + 2, offset + 2 + length);
    let raw = raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(raw);
    Ok((String::from_utf8_lossy(raw).into_owned(), 2 + length))
}

impl DemoReader {
    /// `new DemoReader(data)`: inflates the file when it starts with the gzip
    /// magic bytes.
    ///
    /// # Errors
    /// When the gzip stream is damaged.
    pub fn new(data: Vec<u8>) -> Result<Self, DemoError> {
        let data = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
            gunzip(&data)?
        } else {
            data
        };
        Ok(Self {
            data,
            header: None,
            index: None,
        })
    }

    /// The file after inflating.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// `readHeader`: metadata and the player roster.
    ///
    /// # Errors
    /// When the file is not a `.shagdemo`, has an unknown version or
    /// visibility, or ends inside the header.
    pub fn read_header(&mut self) -> Result<&DemoHeader, DemoError> {
        if self.header.is_none() {
            self.header = Some(self.parse_header()?);
        }
        Ok(self.header.as_ref().expect("just set"))
    }

    fn parse_header(&self) -> Result<DemoHeader, DemoError> {
        let data = &self.data[..];
        if data.get(..4) != Some(b"SHAG") {
            return Err(DemoError::BadMagic);
        }
        let mut offset = 4;
        let version = u8_at(data, offset)?;
        offset += 1;
        if !(1..=6).contains(&version) {
            return Err(DemoError::UnsupportedVersion(version));
        }
        let (map, n) = read_string(data, offset)?;
        offset += n;
        let (gametype, n) = read_string(data, offset)?;
        offset += n;
        let tick_rate = u8_at(data, offset)?;
        offset += 1;
        let snapshot_rate = u8_at(data, offset)?;
        offset += 1;
        let high = u32_at(data, offset)?;
        offset += 4;
        let low = u32_at(data, offset)?;
        offset += 4;
        let start_time = f64::from(high) * 4_294_967_296.0 + f64::from(low);

        let mut match_id = None;
        if version >= 2 {
            let (value, n) = read_string(data, offset)?;
            offset += n;
            match_id = (!value.is_empty()).then_some(value);
        }
        let mut private = None;
        if version >= 4 {
            let visibility = u8_at(data, offset)?;
            offset += 1;
            if visibility > 1 {
                return Err(DemoError::InvalidVisibility);
            }
            private = Some(visibility == 1);
        }

        let count = u8_at(data, offset)?;
        offset += 1;
        let mut players = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let (player_id, n) = read_string(data, offset)?;
            offset += n;
            let (name, n) = read_string(data, offset)?;
            offset += n;
            let (team, n) = read_string(data, offset)?;
            offset += n;
            let mut account_id = None;
            if version >= 2 {
                let (value, n) = read_string(data, offset)?;
                offset += n;
                let parsed = js_string_to_number(&value);
                // Number.isSafeInteger(parsed) && parsed > 0
                if !value.is_empty() && parsed.fract() == 0.0 && parsed > 0.0 && parsed <= 9_007_199_254_740_991.0 {
                    account_id = Some(parsed);
                }
            }
            players.push(DemoPlayerInfo {
                player_id,
                name,
                team,
                account_id,
            });
        }
        Ok(DemoHeader {
            version,
            map,
            gametype,
            tick_rate,
            snapshot_rate,
            start_time,
            match_id,
            private,
            players,
        })
    }

    /// `readIndex`: the frame table, found through the trailer.
    ///
    /// # Errors
    /// When the trailer points outside the file or the table runs past it.
    pub fn read_index(&mut self) -> Result<&DemoIndex, DemoError> {
        if self.index.is_none() {
            self.index = Some(self.parse_index()?);
        }
        Ok(self.index.as_ref().expect("just set"))
    }

    fn parse_index(&self) -> Result<DemoIndex, DemoError> {
        let data = &self.data[..];
        let len = data.len();
        let trailer = |back: usize| len.checked_sub(back).ok_or(DemoError::OutOfBounds);
        let index_offset = u32_at(data, trailer(4)?)?;
        let duration_ms = u32_at(data, trailer(8)?)?;
        // indexOffset >= fileLen - 8 (in f64, so a file under 8 bytes refuses every offset)
        if f64::from(index_offset) >= len as f64 - 8.0 {
            return Err(DemoError::InvalidIndexOffset {
                index_offset,
                file_length: len,
            });
        }
        let mut offset = index_offset as usize;
        let frame_count = u32_at(data, offset)?;
        offset += 4;
        let mut entries = Vec::new();
        for _ in 0..frame_count {
            let tick = u32_at(data, offset)?;
            let frame_offset = u32_at(data, offset + 4)?;
            offset += 8;
            entries.push(DemoIndexEntry {
                tick,
                offset: frame_offset,
            });
        }
        Ok(DemoIndex {
            frame_count,
            entries,
            duration_ms,
            index_offset,
        })
    }

    /// `readAllFrames`: every frame the index lists, skipping those that fail
    /// to decode.
    ///
    /// # Errors
    /// When the header or the index cannot be read.
    pub fn read_all_frames<C: EventCodec>(&mut self, codec: &C) -> Result<Vec<DemoFrame<C::Event>>, DemoError> {
        self.read_header()?;
        self.read_index()?;
        let offsets: Vec<usize> = self
            .index
            .as_ref()
            .expect("read above")
            .entries
            .iter()
            .map(|e| e.offset as usize)
            .collect();
        let mut frames = Vec::with_capacity(offsets.len());
        for offset in offsets {
            if let Some(frame) = self.read_frame_at(offset, codec)? {
                frames.push(frame);
            }
        }
        Ok(frames)
    }

    /// `readFrame`: one frame by its place in the index (0-based). `None`
    /// when out of range or when the frame fails to decode.
    ///
    /// # Errors
    /// When the header or the index cannot be read.
    pub fn read_frame<C: EventCodec>(
        &mut self,
        frame_index: usize,
        codec: &C,
    ) -> Result<Option<DemoFrame<C::Event>>, DemoError> {
        self.read_index()?;
        let Some(entry) = self.index.as_ref().expect("read above").entries.get(frame_index) else {
            return Ok(None);
        };
        let offset = entry.offset as usize;
        self.read_frame_at(offset, codec)
    }

    /// `findFrameByTick`: the first frame whose tick is at least `tick`, or
    /// the last frame. 0 for an empty index.
    ///
    /// # Errors
    /// When the index cannot be read.
    pub fn find_frame_by_tick(&mut self, tick: f64) -> Result<usize, DemoError> {
        let entries = &self.read_index()?.entries;
        let (mut lo, mut hi) = (0usize, entries.len().saturating_sub(1));
        while lo < hi {
            let mid = (lo + hi) >> 1;
            if f64::from(entries[mid].tick) < tick {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    /// `_readFrameAt`. `Ok(None)` is the TypeScript's `null`: the frame
    /// header is out of bounds or the snapshot fails to decode.
    fn read_frame_at<C: EventCodec>(
        &mut self,
        byte_offset: usize,
        codec: &C,
    ) -> Result<Option<DemoFrame<C::Event>>, DemoError> {
        if byte_offset + 8 > self.data.len() {
            return Ok(None);
        }
        let mut offset = byte_offset;
        let tick = u32_at(&self.data, offset)?;
        let frame_len = u32_at(&self.data, offset + 4)? as usize;
        offset += 8;
        // As in the TypeScript, the header is needed (and may fail) only now.
        let version = self.read_header()?.version;
        let data = &self.data[..];
        let frame = slice(data, offset, offset + frame_len);
        offset += frame_len;

        let mut inputs = Vec::new();
        if version >= 3 && offset + 4 <= data.len() {
            let input_len = u32_at(data, offset)? as usize;
            offset += 4;
            if input_len > 0 && offset + input_len <= data.len() {
                inputs = decode_inputs(&data[offset..offset + input_len]).unwrap_or_default();
            }
        }
        Ok(decode_snapshot(frame, None, codec)
            .ok()
            .map(|state| DemoFrame { tick, state, inputs }))
    }
}
