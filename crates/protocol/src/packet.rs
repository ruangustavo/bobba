use bytes::{BufMut, Bytes, BytesMut};

use crate::latin1;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Packet {
    frame: Bytes,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PacketError {
    #[error("frame has {0} bytes, a packet needs at least 6")]
    TooShort(usize),
    #[error("length field says {declared}, frame carries {actual}")]
    LengthMismatch { declared: u32, actual: usize },
}

impl Packet {
    pub const HEADER_LEN: usize = 6;

    pub fn from_frame(frame: impl Into<Bytes>) -> Result<Self, PacketError> {
        let frame = frame.into();
        if frame.len() < Self::HEADER_LEN {
            return Err(PacketError::TooShort(frame.len()));
        }
        let declared = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
        if usize::try_from(declared).ok() != Some(frame.len() - 4) {
            return Err(PacketError::LengthMismatch {
                declared,
                actual: frame.len() - 4,
            });
        }
        Ok(Self { frame })
    }

    pub fn new(header_id: u16, body: &[u8]) -> Self {
        let mut frame = BytesMut::with_capacity(Self::HEADER_LEN + body.len());
        frame.put_u32(u32::try_from(body.len() + 2).unwrap_or(u32::MAX));
        frame.put_u16(header_id);
        frame.put_slice(body);
        Self {
            frame: frame.freeze(),
        }
    }

    pub fn builder(header_id: u16) -> PacketBuilder {
        PacketBuilder::new(header_id)
    }

    pub fn header_id(&self) -> u16 {
        u16::from_be_bytes([self.frame[4], self.frame[5]])
    }

    pub fn body(&self) -> &[u8] {
        &self.frame[Self::HEADER_LEN..]
    }

    pub fn frame(&self) -> &Bytes {
        &self.frame
    }

    pub fn reader(&self) -> PacketReader<'_> {
        PacketReader::new(self.body())
    }

    pub fn with_header(&self, header_id: u16) -> Self {
        Self::new(header_id, self.body())
    }
}

impl std::fmt::Debug for Packet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Packet({}: {})",
            self.header_id(),
            crate::expression::display_bytes(self.body())
        )
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("needed {needed} bytes at offset {offset}, {available} available")]
pub struct ReadError {
    pub offset: usize,
    pub needed: usize,
    pub available: usize,
}

#[derive(Debug, Clone)]
pub struct PacketReader<'a> {
    body: &'a [u8],
    offset: usize,
}

impl<'a> PacketReader<'a> {
    pub fn new(body: &'a [u8]) -> Self {
        Self { body, offset: 0 }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn remaining(&self) -> usize {
        self.body.len() - self.offset
    }

    pub fn is_at_end(&self) -> bool {
        self.remaining() == 0
    }

    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], ReadError> {
        if self.remaining() < n {
            return Err(ReadError {
                offset: self.offset,
                needed: n,
                available: self.remaining(),
            });
        }
        let slice = &self.body[self.offset..self.offset + n];
        self.offset += n;
        Ok(slice)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], ReadError> {
        let slice = self.read_bytes(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }

    pub fn read_u8(&mut self) -> Result<u8, ReadError> {
        Ok(self.read_array::<1>()?[0])
    }

    pub fn read_bool(&mut self) -> Result<bool, ReadError> {
        Ok(self.read_u8()? != 0)
    }

    pub fn read_i16(&mut self) -> Result<i16, ReadError> {
        Ok(i16::from_be_bytes(self.read_array()?))
    }

    pub fn read_u16(&mut self) -> Result<u16, ReadError> {
        Ok(u16::from_be_bytes(self.read_array()?))
    }

    pub fn read_i32(&mut self) -> Result<i32, ReadError> {
        Ok(i32::from_be_bytes(self.read_array()?))
    }

    pub fn read_i64(&mut self) -> Result<i64, ReadError> {
        Ok(i64::from_be_bytes(self.read_array()?))
    }

    pub fn read_f32(&mut self) -> Result<f32, ReadError> {
        Ok(f32::from_be_bytes(self.read_array()?))
    }

    pub fn read_f64(&mut self) -> Result<f64, ReadError> {
        Ok(f64::from_be_bytes(self.read_array()?))
    }

    pub fn read_string_bytes(&mut self) -> Result<&'a [u8], ReadError> {
        let len = self.read_u16()?;
        self.read_bytes(usize::from(len))
    }

    pub fn read_string(&mut self) -> Result<String, ReadError> {
        Ok(latin1::decode(self.read_string_bytes()?))
    }

    pub fn read_long_string_bytes(&mut self) -> Result<&'a [u8], ReadError> {
        let len = self.read_i32()?;
        let len = usize::try_from(len).map_err(|_| ReadError {
            offset: self.offset,
            needed: 0,
            available: self.remaining(),
        })?;
        self.read_bytes(len)
    }
}

#[derive(Debug, Clone)]
pub struct PacketBuilder {
    header_id: u16,
    body: BytesMut,
}

impl PacketBuilder {
    pub fn new(header_id: u16) -> Self {
        Self {
            header_id,
            body: BytesMut::new(),
        }
    }

    pub fn bytes(mut self, bytes: &[u8]) -> Self {
        self.body.put_slice(bytes);
        self
    }

    pub fn u8(mut self, v: u8) -> Self {
        self.body.put_u8(v);
        self
    }

    pub fn bool(self, v: bool) -> Self {
        self.u8(u8::from(v))
    }

    pub fn i16(mut self, v: i16) -> Self {
        self.body.put_i16(v);
        self
    }

    pub fn u16(mut self, v: u16) -> Self {
        self.body.put_u16(v);
        self
    }

    pub fn i32(mut self, v: i32) -> Self {
        self.body.put_i32(v);
        self
    }

    pub fn i64(mut self, v: i64) -> Self {
        self.body.put_i64(v);
        self
    }

    pub fn f64(mut self, v: f64) -> Self {
        self.body.put_f64(v);
        self
    }

    pub fn string_bytes(mut self, bytes: &[u8]) -> Self {
        let len = bytes.len().min(usize::from(u16::MAX));
        self.body.put_u16(u16::try_from(len).unwrap_or(u16::MAX));
        self.body.put_slice(&bytes[..len]);
        self
    }

    pub fn string(self, s: &str) -> Self {
        self.string_bytes(&latin1::encode(s))
    }

    pub fn long_string_bytes(mut self, bytes: &[u8]) -> Self {
        self.body
            .put_i32(i32::try_from(bytes.len()).unwrap_or(i32::MAX));
        self.body.put_slice(bytes);
        self
    }

    pub fn build(self) -> Packet {
        Packet::new(self.header_id, &self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_frame_with_length_prefix() {
        let p = Packet::builder(4000).string("ab").i32(7).bool(true).build();
        assert_eq!(
            p.frame().as_ref(),
            [0, 0, 0, 0x0b, 0x0f, 0xa0, 0, 2, b'a', b'b', 0, 0, 0, 7, 1]
        );
        assert_eq!(p.header_id(), 4000);
    }

    #[test]
    fn reads_back_what_was_written() {
        let p = Packet::builder(1)
            .i32(-5)
            .string("olá")
            .i64(1 << 40)
            .bool(false)
            .build();
        let mut r = p.reader();
        assert_eq!(r.read_i32(), Ok(-5));
        assert_eq!(r.read_string().as_deref(), Ok("olá"));
        assert_eq!(r.read_i64(), Ok(1 << 40));
        assert_eq!(r.read_bool(), Ok(false));
        assert!(r.is_at_end());
    }

    #[test]
    fn reading_past_the_end_is_an_error_not_a_panic() {
        let p = Packet::new(1, &[0, 0]);
        let mut r = p.reader();
        assert_eq!(
            r.read_i32(),
            Err(ReadError {
                offset: 0,
                needed: 4,
                available: 2
            })
        );
    }

    #[test]
    fn rejects_frames_whose_length_lies() {
        assert_eq!(
            Packet::from_frame(vec![0, 0, 0, 9, 0, 1]),
            Err(PacketError::LengthMismatch {
                declared: 9,
                actual: 2
            })
        );
        assert_eq!(
            Packet::from_frame(vec![0, 0, 0]),
            Err(PacketError::TooShort(3))
        );
    }
}
