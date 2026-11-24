use bytes::Bytes;

use crate::direction::Direction;
use crate::latin1;
use crate::packet::{Packet, PacketError, PacketReader, ReadError};
use crate::packet_info::PacketInfo;

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Complete(Packet),
    Named {
        direction: Direction,
        identifier: String,
        body: Bytes,
    },
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ExpressionError {
    #[error("unterminated string starting at character {0}")]
    UnterminatedString(usize),
    #[error("unknown escape \\{0} inside a string")]
    UnknownEscape(char),
    #[error("invalid value in {{{token}:{value}}}")]
    InvalidValue { token: char, value: String },
    #[error("byte [{0}] is out of range 0-255")]
    ByteOutOfRange(u32),
    #[error("unexpected brace at character {0}")]
    UnexpectedBrace(usize),
    #[error("the expression does not form a packet: {0}")]
    NotAPacket(#[from] PacketError),
    #[error("no header named {identifier} going {direction:?} in this hotel version")]
    UnknownName {
        direction: Direction,
        identifier: String,
    },
}

impl Expression {
    pub fn parse(input: &str) -> Result<Self, ExpressionError> {
        let chars: Vec<char> = input.chars().collect();
        let named = parse_named_header(&chars);
        let start = named.as_ref().map_or(0, |(_, _, consumed)| *consumed);
        let bytes = parse_body(&chars, start)?;
        match named {
            Some((direction, identifier, _)) => Ok(Expression::Named {
                direction,
                identifier,
                body: Bytes::from(bytes),
            }),
            None if input.starts_with("{h:") => {
                Ok(Expression::Complete(frame_with_length(&bytes)?))
            }
            None => Ok(Expression::Complete(Packet::from_frame(bytes)?)),
        }
    }

    pub fn direction(&self) -> Option<Direction> {
        match self {
            Expression::Complete(_) => None,
            Expression::Named { direction, .. } => Some(*direction),
        }
    }

    pub fn resolve(self, packet_info: &PacketInfo) -> Result<Packet, ExpressionError> {
        match self {
            Expression::Complete(packet) => Ok(packet),
            Expression::Named {
                direction,
                identifier,
                body,
            } => packet_info
                .header_id(direction, &identifier)
                .map(|header_id| Packet::new(header_id, &body))
                .ok_or(ExpressionError::UnknownName {
                    direction,
                    identifier,
                }),
        }
    }
}

fn frame_with_length(content: &[u8]) -> Result<Packet, ExpressionError> {
    let declared = u32::try_from(content.len()).unwrap_or(u32::MAX);
    let mut frame = declared.to_be_bytes().to_vec();
    frame.extend_from_slice(content);
    Ok(Packet::from_frame(frame)?)
}

fn parse_named_header(chars: &[char]) -> Option<(Direction, String, usize)> {
    let text: String = chars.iter().take(256).collect();
    let (direction, rest) = [Direction::ToClient, Direction::ToServer]
        .into_iter()
        .find_map(|direction| {
            let prefix = format!("{{{}:", direction.expression_prefix());
            text.strip_prefix(prefix.as_str())
                .map(|rest| (direction, rest))
        })?;
    let close = rest.find('}')?;
    let identifier = &rest[..close];
    if identifier.contains([':', '{']) {
        return None;
    }
    let prefix_len = text.len() - rest.len();
    let consumed = text[..prefix_len + close + 1].chars().count();
    Some((direction, identifier.to_owned(), consumed))
}

fn parse_body(chars: &[char], start: usize) -> Result<Vec<u8>, ExpressionError> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos < chars.len() {
        if chars[pos] == '{' {
            pos = parse_token(chars, pos, &mut out)?;
        } else if chars[pos] == '}' {
            return Err(ExpressionError::UnexpectedBrace(pos));
        } else if let Some((byte, next)) = parse_raw_byte(chars, pos)? {
            out.push(byte);
            pos = next;
        } else {
            out.extend(latin1::encode(&chars[pos].to_string()));
            pos += 1;
        }
    }
    Ok(out)
}

fn parse_raw_byte(chars: &[char], pos: usize) -> Result<Option<(u8, usize)>, ExpressionError> {
    if chars[pos] != '[' {
        return Ok(None);
    }
    let digits: String = chars[pos + 1..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() || digits.len() > 3 || chars.get(pos + 1 + digits.len()) != Some(&']') {
        return Ok(None);
    }
    let value: u32 = digits.parse().unwrap_or(u32::MAX);
    let byte = u8::try_from(value).map_err(|_| ExpressionError::ByteOutOfRange(value))?;
    Ok(Some((byte, pos + digits.len() + 2)))
}

fn parse_token(chars: &[char], pos: usize, out: &mut Vec<u8>) -> Result<usize, ExpressionError> {
    let kind = chars.get(pos + 1).copied();
    if chars.get(pos + 2) != Some(&':') {
        return Err(ExpressionError::UnexpectedBrace(pos));
    }
    if kind == Some('s') && chars.get(pos + 3) == Some(&'"') {
        return parse_string(chars, pos, out);
    }
    let Some(close) = chars[pos..].iter().position(|&c| c == '}') else {
        return Err(ExpressionError::UnexpectedBrace(pos));
    };
    let value: String = chars[pos + 3..pos + close].iter().collect();
    let invalid = |token: char| ExpressionError::InvalidValue {
        token,
        value: value.clone(),
    };
    match kind {
        Some('i') => out.extend(
            parse_integer::<i32>(&value)
                .ok_or_else(|| invalid('i'))?
                .to_be_bytes(),
        ),
        Some('l') => out.extend(
            parse_integer::<i64>(&value)
                .ok_or_else(|| invalid('l'))?
                .to_be_bytes(),
        ),
        Some('h') => out.extend(
            parse_integer::<i16>(&value)
                .ok_or_else(|| invalid('h'))?
                .to_be_bytes(),
        ),
        Some('u') => {
            let value = parse_unsigned_short(&value).ok_or_else(|| invalid('u'))?;
            out.extend(value.to_be_bytes());
        }
        Some('d') => out.extend(
            parse_decimal::<f64>(&value)
                .ok_or_else(|| invalid('d'))?
                .to_be_bytes(),
        ),
        Some('f') => out.extend(
            parse_decimal::<f32>(&value)
                .ok_or_else(|| invalid('f'))?
                .to_be_bytes(),
        ),
        Some('b') => out.push(parse_byte_token(&value).ok_or_else(|| invalid('b'))?),
        _ => return Err(ExpressionError::UnexpectedBrace(pos)),
    }
    Ok(pos + close + 1)
}

fn parse_integer<T: std::str::FromStr>(value: &str) -> Option<T> {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn parse_unsigned_short(value: &str) -> Option<u16> {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn parse_decimal<T: std::str::FromStr>(value: &str) -> Option<T> {
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    let (whole, fraction) = unsigned.split_once('.')?;
    let all_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if !all_digits(whole) || !all_digits(fraction) || (whole.is_empty() && fraction.is_empty()) {
        return None;
    }
    value.parse().ok()
}

fn parse_byte_token(value: &str) -> Option<u8> {
    match value {
        "true" | "True" => Some(1),
        "false" | "False" => Some(0),
        digits if (1..=3).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit()) => {
            digits.parse().ok()
        }
        _ => None,
    }
}

fn parse_string(chars: &[char], pos: usize, out: &mut Vec<u8>) -> Result<usize, ExpressionError> {
    let mut text = String::new();
    let mut cursor = pos + 4;
    loop {
        match (chars.get(cursor), chars.get(cursor + 1)) {
            (Some('"'), Some('}')) => break,
            (Some('\\'), Some(escaped)) => {
                text.push(match escaped {
                    '"' => '"',
                    'r' => '\r',
                    '\\' => '\\',
                    other => return Err(ExpressionError::UnknownEscape(*other)),
                });
                cursor += 2;
            }
            (Some(c), _) => {
                text.push(*c);
                cursor += 1;
            }
            (None, _) => return Err(ExpressionError::UnterminatedString(pos)),
        }
    }
    let encoded = text.as_bytes();
    let len = u16::try_from(encoded.len()).unwrap_or(u16::MAX);
    out.extend(len.to_be_bytes());
    out.extend_from_slice(&encoded[..usize::from(len)]);
    Ok(cursor + 2)
}

pub fn display_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        let escaped =
            b < 32 || (128..160).contains(&b) || matches!(b, b'[' | b']' | b'{' | b'}' | 127);
        if escaped {
            out.push('[');
            out.push_str(&b.to_string());
            out.push(']');
        } else {
            out.push(char::from(b));
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderLabel<'a> {
    Id(u16),
    Name(Direction, &'a str),
}

pub fn expression_from_structure(
    packet: &Packet,
    structure: &str,
    label: HeaderLabel<'_>,
) -> Result<String, ReadError> {
    let mut out = match label {
        HeaderLabel::Id(id) => format!("{{h:{id}}}"),
        HeaderLabel::Name(direction, name) => {
            format!("{{{}:{name}}}", direction.expression_prefix())
        }
    };
    let structure: Vec<char> = structure.chars().collect();
    let mut reader = packet.reader();
    write_structure(&mut reader, &structure, 0, &mut out)?;
    Ok(out)
}

fn write_structure(
    reader: &mut PacketReader<'_>,
    structure: &[char],
    mut index: usize,
    out: &mut String,
) -> Result<(), ReadError> {
    let mut previous_int = 0i32;
    while let Some(&c) = structure.get(index) {
        index += 1;
        match c {
            '(' => {
                for _ in 0..previous_int.max(0) {
                    write_structure(reader, structure, index, out)?;
                }
                let mut depth = 1;
                while depth > 0 {
                    match structure.get(index) {
                        Some('(') => depth += 1,
                        Some(')') => depth -= 1,
                        Some(_) => {}
                        None => return Ok(()),
                    }
                    index += 1;
                }
            }
            'i' => {
                previous_int = reader.read_i32()?;
                out.push_str(&format!("{{i:{previous_int}}}"));
            }
            's' => {
                let text = String::from_utf8_lossy(reader.read_string_bytes()?);
                let escaped = text
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\r', "\\r");
                out.push_str(&format!("{{s:\"{escaped}\"}}"));
            }
            'd' => out.push_str(&format!("{{d:{:?}}}", reader.read_f64()?)),
            'f' => out.push_str(&format!("{{f:{:?}}}", reader.read_f32()?)),
            'b' => out.push_str(&format!("{{b:{}}}", reader.read_u8()?)),
            'B' => out.push_str(&format!("{{b:{}}}", reader.read_bool()?)),
            'l' => out.push_str(&format!("{{l:{}}}", reader.read_i64()?)),
            'u' => out.push_str(&format!("{{u:{}}}", reader.read_i16()?)),
            _ => return Ok(()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(input: &str) -> Packet {
        match Expression::parse(input) {
            Ok(Expression::Complete(packet)) => packet,
            other => panic!("expected a complete packet, got {other:?}"),
        }
    }

    #[test]
    fn header_expression_gets_its_length_fixed() {
        let p = complete("{h:4000}{s:\"ab\"}{i:7}{b:true}");
        assert_eq!(
            p,
            Packet::builder(4000).string("ab").i32(7).bool(true).build()
        );
    }

    #[test]
    fn every_value_token_encodes_big_endian() {
        let p = complete("{h:1}{l:-2}{u:258}{d:1.5}{b:200}{b:False}[7]x");
        let expected = Packet::builder(1)
            .i64(-2)
            .u16(258)
            .f64(1.5)
            .u8(200)
            .u8(0)
            .u8(7)
            .u8(b'x')
            .build();
        assert_eq!(p, expected);
    }

    #[test]
    fn string_escapes_match_gearth() {
        let p = complete(r#"{h:1}{s:"a\"b\\c\rd"}"#);
        assert_eq!(p.reader().read_string().as_deref(), Ok("a\"b\\c\rd"));
    }

    #[test]
    fn strings_are_utf8_like_gearth() {
        let p = complete(r#"{h:1}{s:"você 王"}"#);
        assert_eq!(p.reader().read_string_bytes(), Ok("você 王".as_bytes()));
        let label = HeaderLabel::Id(1);
        assert_eq!(
            expression_from_structure(&p, "s", label),
            Ok(r#"{h:1}{s:"você 王"}"#.to_owned())
        );
    }

    #[test]
    fn braces_inside_strings_are_literal() {
        let p = complete(r#"{h:1}{s:"{i:1}"}"#);
        assert_eq!(p.reader().read_string().as_deref(), Ok("{i:1}"));
    }

    #[test]
    fn named_header_waits_for_packet_info() {
        let parsed = Expression::parse("{out:Chat}{s:\"oi\"}{i:0}").expect("valid expression");
        let Expression::Named {
            direction,
            identifier,
            body,
        } = &parsed
        else {
            panic!("expected a named expression");
        };
        assert_eq!(
            (*direction, identifier.as_str()),
            (Direction::ToServer, "Chat")
        );
        assert_eq!(body.as_ref(), [0, 2, b'o', b'i', 0, 0, 0, 0]);
    }

    #[test]
    fn raw_expression_must_carry_its_own_length() {
        assert_eq!(complete("[0][0][0][2][0][9]"), Packet::new(9, &[]));
        assert!(Expression::parse("[0][0][0][9][0][9]").is_err());
    }

    #[test]
    fn rejects_malformed_tokens() {
        assert_eq!(
            Expression::parse("{h:1}{b:256}"),
            Err(ExpressionError::InvalidValue {
                token: 'b',
                value: "256".into()
            })
        );
        assert_eq!(
            Expression::parse("{h:1}[300]"),
            Err(ExpressionError::ByteOutOfRange(300))
        );
        assert_eq!(
            Expression::parse("{h:1}{x:1}"),
            Err(ExpressionError::UnexpectedBrace(5))
        );
        assert_eq!(
            Expression::parse(r#"{h:1}{s:"oops"#),
            Err(ExpressionError::UnterminatedString(5))
        );
        assert_eq!(
            Expression::parse(r#"{h:1}{s:"\n"}"#),
            Err(ExpressionError::UnknownEscape('n'))
        );
    }

    #[test]
    fn display_escapes_control_and_bracket_bytes() {
        assert_eq!(
            display_bytes(&[0, 0, 0, 6, b'[', b'a', 0x7f, 0x85, 0xa0, 0xe1]),
            "[0][0][0][6][91]a[127][133]\u{a0}á"
        );
    }

    #[test]
    fn displayed_bytes_parse_back_to_the_same_packet() {
        let p = Packet::builder(4000)
            .string("WIN63-202611041140")
            .string("FLASH31")
            .i32(6)
            .i32(4)
            .build();
        assert_eq!(complete(&display_bytes(p.frame())), p);
    }

    #[test]
    fn expression_from_structure_repeats_groups_by_the_previous_int() {
        let p = Packet::builder(10)
            .i32(2)
            .string("a")
            .bool(true)
            .string("b")
            .bool(false)
            .build();
        let label = HeaderLabel::Name(Direction::ToClient, "Users");
        assert_eq!(
            expression_from_structure(&p, "i(sB)", label),
            Ok(r#"{in:Users}{i:2}{s:"a"}{b:true}{s:"b"}{b:false}"#.to_owned())
        );
    }
}
