use bytes::BytesMut;

use crate::packet::Packet;

#[derive(Debug, Default, Clone)]
pub struct Framer {
    buf: BytesMut,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("invalid frame length {0}")]
    InvalidLength(u32),
}

impl Framer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    pub fn next_packet(&mut self) -> Option<Result<Packet, FrameError>> {
        if self.buf.len() < 4 {
            return None;
        }
        let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
        if !(2..=i32::MAX as u32).contains(&len) {
            return Some(Err(FrameError::InvalidLength(len)));
        }
        let total = len as usize + 4;
        if self.buf.len() < total {
            return None;
        }
        let frame = self.buf.split_to(total).freeze();
        Some(Packet::from_frame(frame).map_err(|_| FrameError::InvalidLength(len)))
    }

    pub fn peek_length(&self) -> Option<u32> {
        let bytes = self.buf.get(..4)?;
        Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn take_remaining(&mut self) -> BytesMut {
        self.buf.split()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_a_frame_split_across_reads() {
        let p = Packet::builder(4000).string("hello").build();
        let bytes = p.frame().clone();
        let mut f = Framer::new();
        f.push(&bytes[..3]);
        assert!(f.next_packet().is_none());
        f.push(&bytes[3..8]);
        assert!(f.next_packet().is_none());
        f.push(&bytes[8..]);
        assert_eq!(f.next_packet(), Some(Ok(p)));
        assert_eq!(f.buffered(), 0);
    }

    #[test]
    fn yields_every_frame_in_one_read_and_keeps_the_leftover() {
        let a = Packet::new(1, &[1, 2, 3]);
        let b = Packet::new(2, &[]);
        let mut f = Framer::new();
        let mut stream = a.frame().to_vec();
        stream.extend_from_slice(b.frame());
        stream.extend_from_slice(&[0, 0]);
        f.push(&stream);
        assert_eq!(f.next_packet(), Some(Ok(a)));
        assert_eq!(f.next_packet(), Some(Ok(b)));
        assert_eq!(f.next_packet(), None);
        assert_eq!(f.buffered(), 2);
    }

    #[test]
    fn rejects_lengths_without_room_for_a_header() {
        let mut f = Framer::new();
        f.push(&[0, 0, 0, 1, 9]);
        assert_eq!(f.next_packet(), Some(Err(FrameError::InvalidLength(1))));
    }
}
