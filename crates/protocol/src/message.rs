use crate::direction::Direction;
use crate::packet::{Packet, PacketError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HMessage {
    pub packet: Packet,
    pub direction: Direction,
    pub index: u32,
    pub blocked: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MessageParseError {
    #[error("expected blocked, index, direction and packet separated by tabs")]
    MissingField,
    #[error("blocked flag must be 0 or 1")]
    InvalidBlockedFlag,
    #[error("index is not a non-negative integer")]
    InvalidIndex,
    #[error("unknown direction")]
    InvalidDirection,
    #[error("packet is corrupted: {0}")]
    CorruptedPacket(#[from] PacketError),
}

impl HMessage {
    pub fn new(packet: Packet, direction: Direction, index: u32) -> Self {
        Self {
            packet,
            direction,
            index,
            blocked: false,
        }
    }

    pub fn stringify(&self) -> Vec<u8> {
        let mut out = format!(
            "{}\t{}\t{}\t0",
            u8::from(self.blocked),
            self.index,
            self.direction.gearth_name()
        )
        .into_bytes();
        out.extend_from_slice(self.packet.frame());
        out
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, MessageParseError> {
        let mut fields = bytes.splitn(4, |&b| b == b'\t');
        let mut next = || fields.next().ok_or(MessageParseError::MissingField);
        let blocked = match next()? {
            b"0" => false,
            b"1" => true,
            _ => return Err(MessageParseError::InvalidBlockedFlag),
        };
        let index = std::str::from_utf8(next()?)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(MessageParseError::InvalidIndex)?;
        let direction = std::str::from_utf8(next()?)
            .ok()
            .and_then(Direction::from_gearth_name)
            .ok_or(MessageParseError::InvalidDirection)?;
        let frame = next()?.get(1..).ok_or(MessageParseError::MissingField)?;
        Ok(Self {
            packet: Packet::from_frame(frame.to_vec())?,
            direction,
            index,
            blocked,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stringifies_like_gearth() {
        let message = HMessage::new(Packet::new(597, b"\t"), Direction::ToServer, 1042);
        let mut expected = b"0\t1042\tTOSERVER\t0".to_vec();
        expected.extend_from_slice(&[0, 0, 0, 3, 2, 0x55, b'\t']);
        assert_eq!(message.stringify(), expected);
    }

    #[test]
    fn parses_an_edited_and_blocked_reply_with_tabs_in_the_payload() {
        let mut reply = b"1\t7\tTOCLIENT\t1".to_vec();
        reply.extend_from_slice(Packet::new(9, b"\t\t").frame());
        let message = HMessage::parse(&reply).expect("valid reply");
        assert!(message.blocked);
        assert_eq!((message.index, message.direction), (7, Direction::ToClient));
        assert_eq!(message.packet, Packet::new(9, b"\t\t"));
    }

    #[test]
    fn rejects_a_corrupted_packet() {
        let reply = b"0\t1\tTOSERVER\t0\0\0\0\x09\0\x01".to_vec();
        assert!(matches!(
            HMessage::parse(&reply),
            Err(MessageParseError::CorruptedPacket(_))
        ));
    }
}
