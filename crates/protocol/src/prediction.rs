use crate::expression::{HeaderLabel, expression_from_structure};
use crate::packet::Packet;

pub fn predicted_expression(packet: &Packet, label: HeaderLabel<'_>) -> String {
    expression_from_structure(packet, &predict_structure(packet), label).unwrap_or_default()
}

pub fn predict_structure(packet: &Packet) -> String {
    let frame = Frame(packet.frame());
    let checkers = [
        Checker::Boolean,
        Checker::Byte,
        Checker::Integer,
        Checker::String,
    ];
    let end = frame.len();
    let mut best: Vec<Option<Step>> = vec![None; end - Packet::HEADER_LEN + 1];
    best[0] = Some(Step {
        previous: None,
        code: ' ',
        log_score: 0.0,
    });
    for index in Packet::HEADER_LEN..end {
        let Some(current) = best[index - Packet::HEADER_LEN] else {
            continue;
        };
        for checker in checkers {
            if !checker.can_read(&frame, index) {
                continue;
            }
            let log_score = current.log_score + checker.score(&frame, index).ln();
            let next = checker.next_index(&frame, index) - Packet::HEADER_LEN;
            if best[next].is_none_or(|step| log_score > step.log_score) {
                best[next] = Some(Step {
                    previous: Some(index - Packet::HEADER_LEN),
                    code: checker.code(),
                    log_score,
                });
            }
        }
    }
    let mut codes = Vec::new();
    let mut cursor = best.last().copied().flatten();
    while let Some(Step {
        previous: Some(previous),
        code,
        ..
    }) = cursor
    {
        codes.push(code);
        cursor = best[previous];
    }
    codes.iter().rev().collect()
}

#[derive(Debug, Clone, Copy)]
struct Step {
    previous: Option<usize>,
    code: char,
    log_score: f64,
}

struct Frame<'a>(&'a [u8]);

impl Frame<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn u16_at(&self, index: usize) -> u16 {
        u16::from_be_bytes([self.0[index], self.0[index + 1]])
    }

    fn i32_at(&self, index: usize) -> i32 {
        i32::from_be_bytes([
            self.0[index],
            self.0[index + 1],
            self.0[index + 2],
            self.0[index + 3],
        ])
    }

    fn can_read_string(&self, index: usize) -> bool {
        index >= Packet::HEADER_LEN
            && index + 2 <= self.len()
            && usize::from(self.u16_at(index)) + 2 + index <= self.len()
    }

    fn string_at(&self, index: usize) -> &[u8] {
        let len = usize::from(self.u16_at(index));
        &self.0[index + 2..index + 2 + len]
    }
}

#[derive(Debug, Clone, Copy)]
enum Checker {
    Boolean,
    Byte,
    Integer,
    String,
}

impl Checker {
    fn code(self) -> char {
        match self {
            Checker::Boolean => 'B',
            Checker::Byte => 'b',
            Checker::Integer => 'i',
            Checker::String => 's',
        }
    }

    fn can_read(self, frame: &Frame<'_>, index: usize) -> bool {
        match self {
            Checker::Boolean => index < frame.len() && frame.0[index] <= 1,
            Checker::Byte => index < frame.len(),
            Checker::Integer => index + 4 <= frame.len(),
            Checker::String => frame.can_read_string(index),
        }
    }

    fn next_index(self, frame: &Frame<'_>, index: usize) -> usize {
        match self {
            Checker::Boolean | Checker::Byte => index + 1,
            Checker::Integer => index + 4,
            Checker::String => index + 2 + frame.string_at(index).len(),
        }
    }

    fn score(self, frame: &Frame<'_>, index: usize) -> f64 {
        match self {
            Checker::Boolean if frame.0[index] == 1 => 0.6,
            Checker::Boolean => 0.5,
            Checker::Byte => 0.4,
            Checker::Integer => integer_score(frame, index),
            Checker::String => {
                (string_score(frame.string_at(index)) - index as f64 / 1_000_000_000_000.0).max(0.0)
            }
        }
    }
}

fn integer_score(frame: &Frame<'_>, index: usize) -> f64 {
    let value = frame.i32_at(index);
    let high = frame.u16_at(index);
    let low = frame.u16_at(index + 2);
    if high != 0 && matches!(low, 0 | 1 | 256 | 257) {
        return 0.05;
    }
    if value == 256 {
        return 0.04;
    }
    if high == 2 && frame.can_read_string(index) {
        return 1.0 - string_score(frame.string_at(index));
    }
    if frame.0[index..index + 4]
        .iter()
        .all(|&b| commonness(b) == Commonness::Very)
    {
        return 0.2;
    }
    if index > Packet::HEADER_LEN && frame.can_read_string(index - 1) {
        let s = frame.string_at(index - 1);
        let score = string_score(s);
        if s.len() > 2 && s.len() < 10 && score > 0.5 {
            return 1.0 - score;
        }
    }
    let offset = index as f64 / 1_000_000_000.0;
    match value {
        -1 => 1.0 - offset,
        0 => 0.99 - offset,
        v if v % 256 == 0 => 0.06 - offset,
        v if v != 1 && v % 256 == 1 => 0.06 - offset,
        v if v >= -1 => 0.92 - offset,
        _ => 0.8 - offset,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Commonness {
    Rare,
    Somewhat,
    Very,
}

fn commonness(b: u8) -> Commonness {
    match b {
        b'\n' | b'\r' | b'\t' => Commonness::Somewhat,
        32..=126 => Commonness::Very,
        _ => Commonness::Rare,
    }
}

fn string_score(s: &[u8]) -> f64 {
    if s.is_empty() {
        return 0.8;
    }
    let len = s.len() as f64;
    let rare = -1.0 / (len * 0.3 + 2.0) + 0.5;
    let somewhat = -1.0 / (len + 2.0) + 1.0;
    let multibyte = match s.len() {
        1 => 0.9,
        2 => 0.98,
        _ => 1.0,
    };
    let is_continuation = |i: usize| s.get(i).is_some_and(|b| b & 0b1100_0000 == 0b1000_0000);
    let mut score = 1.0;
    let mut i = 0;
    while i < s.len() {
        let continuation_bytes = match s[i] {
            b if b & 0b1110_0000 == 0b1100_0000 && i + 1 < s.len() && is_continuation(i + 1) => 1,
            b if b & 0b1111_0000 == 0b1110_0000
                && i + 2 < s.len()
                && (1..=2).all(|k| is_continuation(i + k)) =>
            {
                2
            }
            b if b & 0b1111_1000 == 0b1111_0000
                && i + 3 < s.len()
                && (1..=3).all(|k| is_continuation(i + k)) =>
            {
                3
            }
            _ => 0,
        };
        if continuation_bytes > 0 {
            score *= multibyte_penalty(multibyte, continuation_bytes + 1);
            i += continuation_bytes;
        } else {
            score *= match commonness(s[i]) {
                Commonness::Rare => rare,
                Commonness::Somewhat => somewhat,
                Commonness::Very => 1.0,
            };
            if score < 0.001 {
                return 0.0;
            }
        }
        i += 1;
    }
    score
}

fn multibyte_penalty(base: f64, bytes: usize) -> f64 {
    (0..bytes).fold(1.0, |acc, _| acc * base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicts_the_client_hello() {
        let p = Packet::builder(4000)
            .string("WIN63-202610131318-860371703")
            .string("FLASH31")
            .i32(6)
            .i32(4)
            .build();
        assert_eq!(predict_structure(&p), "ssii");
    }

    #[test]
    fn predicts_a_chat_message() {
        let p = Packet::builder(597)
            .string("hello there")
            .i32(0)
            .i32(-1)
            .build();
        assert_eq!(predict_structure(&p), "sii");
    }

    #[test]
    fn empty_body_has_empty_structure() {
        assert_eq!(predict_structure(&Packet::new(1, &[])), "");
    }

    #[test]
    fn prefers_integers_for_small_ids() {
        let p = Packet::builder(1).i32(12345).i32(0).build();
        assert_eq!(predict_structure(&p), "ii");
    }
}
