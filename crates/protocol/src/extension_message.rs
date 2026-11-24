use std::sync::Arc;

use bytes::Bytes;

use crate::direction::Direction;
use crate::latin1;
use crate::message::{HMessage, MessageParseError};
use crate::packet::{Packet, PacketBuilder, PacketError, PacketReader, ReadError};
use crate::packet_info::PacketInfo;

const EVA_WIRE_FORMAT: i32 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub packet_logger: String,
    pub version: String,
    pub attributes: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum HostToExtension {
    OnDoubleClick,
    InfoRequest,
    PacketIntercept(HMessage),
    FlagsCheck(Vec<String>),
    ConnectionStart {
        host: String,
        port: u16,
        hotel_version: String,
        client_identifier: String,
        client_type: String,
        packet_info: Arc<PacketInfo>,
    },
    ConnectionEnd,
    Init {
        connected: bool,
        host_info: HostInfo,
    },
    UpdateHostInfo(HostInfo),
    PacketToStringResponse {
        display: String,
        expression: String,
    },
    StringToPacketResponse(Option<Packet>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionInfo {
    pub title: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub on_click_used: bool,
    pub file: Option<String>,
    pub cookie: Option<String>,
    pub can_leave: bool,
    pub can_delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionToHost {
    ExtensionInfo(ExtensionInfo),
    ManipulatedPacket(HMessage),
    RequestFlags,
    SendMessage {
        direction: Direction,
        packet: Packet,
    },
    PacketToStringRequest(Packet),
    StringToPacketRequest(String),
    ConsoleLog(String),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExtensionMessageError {
    #[error("unknown extension message id {0}")]
    UnknownId(u16),
    #[error("truncated message: {0}")]
    Truncated(#[from] ReadError),
    #[error("manipulated packet is malformed: {0}")]
    Message(#[from] MessageParseError),
    #[error("packet sent by the extension is corrupted: {0}")]
    Packet(#[from] PacketError),
    #[error("expression is not valid UTF-8")]
    NotUtf8,
}

impl HostToExtension {
    pub fn encode(&self) -> Packet {
        match self {
            HostToExtension::OnDoubleClick => PacketBuilder::new(1).build(),
            HostToExtension::InfoRequest => PacketBuilder::new(2).build(),
            HostToExtension::PacketIntercept(message) => PacketBuilder::new(3)
                .long_string_bytes(&message.stringify())
                .i32(EVA_WIRE_FORMAT)
                .build(),
            HostToExtension::FlagsCheck(flags) => {
                let count = i32::try_from(flags.len()).unwrap_or(i32::MAX);
                flags
                    .iter()
                    .fold(PacketBuilder::new(4).i32(count), |b, flag| b.string(flag))
                    .build()
            }
            HostToExtension::ConnectionStart {
                host,
                port,
                hotel_version,
                client_identifier,
                client_type,
                packet_info,
            } => {
                let builder = PacketBuilder::new(5)
                    .string(host)
                    .i32(i32::from(*port))
                    .string(hotel_version)
                    .string(client_identifier)
                    .string(client_type);
                packet_info.write_to(builder).build()
            }
            HostToExtension::ConnectionEnd => PacketBuilder::new(6).build(),
            HostToExtension::Init {
                connected,
                host_info,
            } => write_host_info(PacketBuilder::new(7).bool(*connected), host_info).build(),
            HostToExtension::UpdateHostInfo(host_info) => {
                write_host_info(PacketBuilder::new(10), host_info).build()
            }
            HostToExtension::PacketToStringResponse {
                display,
                expression,
            } => PacketBuilder::new(20)
                .long_string_bytes(&latin1::encode(display))
                .long_string_bytes(expression.as_bytes())
                .build(),
            HostToExtension::StringToPacketResponse(packet) => {
                let mut stringified = vec![b'0'];
                if let Some(packet) = packet {
                    stringified.extend_from_slice(packet.frame());
                }
                PacketBuilder::new(21)
                    .long_string_bytes(&stringified)
                    .build()
            }
        }
    }
}

fn write_host_info(builder: PacketBuilder, info: &HostInfo) -> PacketBuilder {
    let count = i32::try_from(info.attributes.len()).unwrap_or(i32::MAX);
    info.attributes.iter().fold(
        builder
            .string(&info.packet_logger)
            .string(&info.version)
            .i32(count),
        |b, (key, value)| b.string(key).string(value),
    )
}

impl ExtensionToHost {
    pub fn decode(packet: &Packet) -> Result<Self, ExtensionMessageError> {
        let mut r = packet.reader();
        match packet.header_id() {
            1 => Ok(ExtensionToHost::ExtensionInfo(read_extension_info(&mut r)?)),
            2 => Ok(ExtensionToHost::ManipulatedPacket(HMessage::parse(
                r.read_long_string_bytes()?,
            )?)),
            3 => Ok(ExtensionToHost::RequestFlags),
            4 => {
                let direction = match r.read_u8()? {
                    0 => Direction::ToClient,
                    _ => Direction::ToServer,
                };
                let len = usize::try_from(r.read_i32()?).unwrap_or(0);
                let frame = Bytes::copy_from_slice(r.read_bytes(len)?);
                Ok(ExtensionToHost::SendMessage {
                    direction,
                    packet: Packet::from_frame(frame)?,
                })
            }
            20 => {
                let stringified = r.read_long_string_bytes()?;
                let frame = Bytes::copy_from_slice(stringified.get(1..).unwrap_or_default());
                Ok(ExtensionToHost::PacketToStringRequest(Packet::from_frame(
                    frame,
                )?))
            }
            21 => {
                let text = std::str::from_utf8(r.read_long_string_bytes()?)
                    .map_err(|_| ExtensionMessageError::NotUtf8)?;
                Ok(ExtensionToHost::StringToPacketRequest(text.to_owned()))
            }
            98 => Ok(ExtensionToHost::ConsoleLog(r.read_string()?)),
            other => Err(ExtensionMessageError::UnknownId(other)),
        }
    }

    pub fn encode(&self) -> Packet {
        match self {
            ExtensionToHost::ExtensionInfo(info) => PacketBuilder::new(1)
                .string(&info.title)
                .string(&info.author)
                .string(&info.version)
                .string(&info.description)
                .bool(info.on_click_used)
                .bool(info.file.is_some())
                .string(info.file.as_deref().unwrap_or_default())
                .string(info.cookie.as_deref().unwrap_or_default())
                .bool(info.can_leave)
                .bool(info.can_delete)
                .build(),
            ExtensionToHost::ManipulatedPacket(message) => PacketBuilder::new(2)
                .long_string_bytes(&message.stringify())
                .i32(EVA_WIRE_FORMAT)
                .build(),
            ExtensionToHost::RequestFlags => PacketBuilder::new(3).build(),
            ExtensionToHost::SendMessage { direction, packet } => PacketBuilder::new(4)
                .u8(u8::from(*direction == Direction::ToServer))
                .i32(i32::try_from(packet.frame().len()).unwrap_or(i32::MAX))
                .bytes(packet.frame())
                .i32(EVA_WIRE_FORMAT)
                .build(),
            ExtensionToHost::PacketToStringRequest(packet) => {
                let mut stringified = vec![b'0'];
                stringified.extend_from_slice(packet.frame());
                PacketBuilder::new(20)
                    .long_string_bytes(&stringified)
                    .i32(EVA_WIRE_FORMAT)
                    .build()
            }
            ExtensionToHost::StringToPacketRequest(text) => PacketBuilder::new(21)
                .long_string_bytes(text.as_bytes())
                .i32(EVA_WIRE_FORMAT)
                .build(),
            ExtensionToHost::ConsoleLog(text) => PacketBuilder::new(98).string(text).build(),
        }
    }
}

fn read_extension_info(r: &mut PacketReader<'_>) -> Result<ExtensionInfo, ReadError> {
    let title = r.read_string()?;
    let author = r.read_string()?;
    let version = r.read_string()?;
    let description = r.read_string()?;
    let on_click_used = r.read_bool()?;
    let has_file = r.read_bool()?;
    let file = r.read_string()?;
    let cookie = r.read_string()?;
    let can_leave = r.read_bool()?;
    let can_delete = r.read_bool()?;
    Ok(ExtensionInfo {
        title,
        author,
        version,
        description,
        on_click_used,
        file: has_file.then_some(file),
        cookie: (!cookie.is_empty()).then_some(cookie),
        can_leave,
        can_delete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(message: ExtensionToHost) {
        assert_eq!(ExtensionToHost::decode(&message.encode()), Ok(message));
    }

    #[test]
    fn extension_messages_roundtrip() {
        roundtrip(ExtensionToHost::ExtensionInfo(ExtensionInfo {
            title: "AntiSpam".into(),
            author: "ruan".into(),
            version: "1.0".into(),
            description: "blocks spam".into(),
            on_click_used: true,
            file: Some("antispam-x1.py".into()),
            cookie: Some("123".into()),
            can_leave: true,
            can_delete: false,
        }));
        roundtrip(ExtensionToHost::RequestFlags);
        roundtrip(ExtensionToHost::SendMessage {
            direction: Direction::ToClient,
            packet: Packet::builder(10).string("x").build(),
        });
        roundtrip(ExtensionToHost::ManipulatedPacket(HMessage::new(
            Packet::new(5, b"\t"),
            Direction::ToServer,
            3,
        )));
        roundtrip(ExtensionToHost::PacketToStringRequest(Packet::new(1, &[0])));
        roundtrip(ExtensionToHost::StringToPacketRequest(
            "{out:Chat}{s:\"olá\"}".into(),
        ));
        roundtrip(ExtensionToHost::ConsoleLog("hi".into()));
    }

    #[test]
    fn packet_intercept_carries_the_stringified_message() {
        let message = HMessage::new(Packet::new(597, b"oi"), Direction::ToServer, 42);
        let encoded = HostToExtension::PacketIntercept(message.clone()).encode();
        assert_eq!(encoded.header_id(), 3);
        let mut r = encoded.reader();
        assert_eq!(
            r.read_long_string_bytes(),
            Ok(message.stringify().as_slice())
        );
        assert_eq!(r.read_i32(), Ok(0));
        assert!(r.is_at_end());
    }

    #[test]
    fn init_writes_host_info_like_gearth() {
        let init = HostToExtension::Init {
            connected: false,
            host_info: HostInfo {
                packet_logger: "Bobba".into(),
                version: "0.1.0".into(),
                attributes: vec![("theme".into(), "dark".into())],
            },
        }
        .encode();
        let mut r = init.reader();
        assert_eq!(init.header_id(), 7);
        assert_eq!(r.read_bool(), Ok(false));
        assert_eq!(r.read_string().as_deref(), Ok("Bobba"));
        assert_eq!(r.read_string().as_deref(), Ok("0.1.0"));
        assert_eq!(r.read_i32(), Ok(1));
        assert_eq!(r.read_string().as_deref(), Ok("theme"));
        assert_eq!(r.read_string().as_deref(), Ok("dark"));
        assert!(r.is_at_end());
    }

    #[test]
    fn unknown_ids_are_rejected() {
        assert_eq!(
            ExtensionToHost::decode(&Packet::new(77, &[])),
            Err(ExtensionMessageError::UnknownId(77))
        );
    }
}
