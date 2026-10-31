//! `BinaryEncoder` and `BinaryDecoder` (SHAG `src/shared/net/BinaryProtocol.ts`).
//!
//! The TypeScript takes JavaScript numbers and lets `DataView` convert them.
//! Here every writer takes the wire type; a caller holding an `f64` that may
//! be fractional, negative or out of range converts with `crate::js::to_uint8`
//! and friends to get identical bytes.

use super::DecodeError;

/// Little-endian writer into a growing buffer.
#[derive(Debug, Default, Clone)]
pub struct BinaryEncoder {
    buffer: Vec<u8>,
}

impl BinaryEncoder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(256),
        }
    }

    pub fn write_uint8(&mut self, value: u8) {
        self.buffer.push(value);
    }

    pub fn write_int8(&mut self, value: i8) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_uint16(&mut self, value: u16) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_int16(&mut self, value: i16) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_uint32(&mut self, value: u32) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_int32(&mut self, value: i32) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// `setFloat32`: an `f64` rounds to the nearest `f32`, ties to even,
    /// which is what `as f32` does.
    pub fn write_float32(&mut self, value: f64) {
        self.buffer.extend_from_slice(&(value as f32).to_le_bytes());
    }

    pub fn write_float64(&mut self, value: f64) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// u16 byte length, then UTF-8. Like the TypeScript, a string longer than
    /// 65,535 bytes writes its length modulo 2^16 and all of its bytes.
    pub fn write_string(&mut self, value: &str) {
        self.write_uint16(value.len() as u16);
        self.buffer.extend_from_slice(value.as_bytes());
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_uint8(u8::from(value));
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    #[must_use]
    pub fn offset(&self) -> usize {
        self.buffer.len()
    }

    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.buffer
    }

    #[must_use]
    pub fn into_data(self) -> Vec<u8> {
        self.buffer
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
    }
}

/// Little-endian reader with bounds checks.
#[derive(Debug, Clone)]
pub struct BinaryDecoder<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> BinaryDecoder<'a> {
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let bytes = self.read_bytes(N)?;
        Ok(bytes.try_into().expect("length checked"))
    }

    pub fn read_uint8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take::<1>()?[0])
    }

    pub fn read_int8(&mut self) -> Result<i8, DecodeError> {
        Ok(i8::from_le_bytes(self.take()?))
    }

    pub fn read_uint16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    pub fn read_int16(&mut self) -> Result<i16, DecodeError> {
        Ok(i16::from_le_bytes(self.take()?))
    }

    pub fn read_uint32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    pub fn read_int32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.take()?))
    }

    /// `getFloat32`, widened to the `f64` JavaScript holds.
    pub fn read_float32(&mut self) -> Result<f64, DecodeError> {
        Ok(f64::from(f32::from_le_bytes(self.take()?)))
    }

    pub fn read_float64(&mut self) -> Result<f64, DecodeError> {
        Ok(f64::from_le_bytes(self.take()?))
    }

    /// u16 byte length, then UTF-8 decoded as `new TextDecoder().decode()`
    /// does: a leading byte-order mark is dropped, and invalid sequences
    /// become U+FFFD, one per maximal invalid subpart.
    pub fn read_string(&mut self) -> Result<String, DecodeError> {
        let length = usize::from(self.read_uint16()?);
        let bytes = self.read_bytes(length)?;
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn read_bool(&mut self) -> Result<bool, DecodeError> {
        Ok(self.read_uint8()? != 0)
    }

    pub fn read_bytes(&mut self, length: usize) -> Result<&'a [u8], DecodeError> {
        let have = self.data.len() - self.offset;
        if length > have {
            return Err(DecodeError::Underflow {
                offset: self.offset,
                need: length,
                have,
            });
        }
        let bytes = &self.data[self.offset..self.offset + length];
        self.offset += length;
        Ok(bytes)
    }

    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    #[must_use]
    pub fn has_more(&self) -> bool {
        self.offset < self.data.len()
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.data.len() - self.offset
    }

    /// The bytes not read yet.
    #[must_use]
    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.offset..]
    }
}
