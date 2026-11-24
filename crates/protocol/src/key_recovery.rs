use rayon::prelude::*;

use crate::framer::Framer;
use crate::rc4::Rc4;

pub type SBox = [u8; 256];

const TABLE_HEX_LENGTH: usize = 256 * 8;
const MAX_CLIENT_FRAME: usize = 262_144 + 4;

pub fn parse_table_dump(output: &str) -> Vec<SBox> {
    let mut tables: Vec<SBox> = output
        .lines()
        .map(str::trim)
        .filter(|line| line.len() == TABLE_HEX_LENGTH)
        .filter_map(parse_table_line)
        .collect();
    tables.reverse();
    tables
}

fn parse_table_line(line: &str) -> Option<SBox> {
    let mut table = [0u8; 256];
    for (slot, chunk) in table.iter_mut().zip(line.as_bytes().chunks(8)) {
        let word = u32::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
        *slot = u8::try_from(word).ok()?;
    }
    Some(table)
}

pub fn format_table_line(table: &SBox) -> String {
    table
        .iter()
        .map(|value| format!("{:08x}", u32::from(*value)))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub fresh: Rc4,
    pub header_ids: Vec<u16>,
}

pub fn find_candidates(tables: &[SBox], encrypted: &[u8]) -> Vec<Candidate> {
    let encrypted_count = encrypted.len();
    let estimated_i = (encrypted_count % 256) as u8;
    tables
        .par_iter()
        .flat_map_iter(|table| (0..=u8::MAX).map(move |j| (table, j)))
        .filter_map(|(table, j)| {
            let mut candidate = Rc4::from_state(*table, estimated_i, j);
            candidate.rewind(encrypted_count);
            if !candidate.could_be_fresh() {
                return None;
            }
            let header_ids = decrypt_into_whole_frames(&candidate, encrypted)?;
            Some(Candidate {
                fresh: candidate,
                header_ids,
            })
        })
        .collect()
}

pub fn choose_key(
    candidates: Vec<Candidate>,
    is_known_outgoing: impl Fn(u16) -> bool,
) -> Option<Rc4> {
    let mut unique: Vec<Candidate> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if !unique.iter().any(|kept| kept.fresh == candidate.fresh) {
            unique.push(candidate);
        }
    }
    if unique.len() > 1 {
        let plausible: Vec<Candidate> = unique
            .iter()
            .filter(|c| c.header_ids.iter().all(|id| is_known_outgoing(*id)))
            .cloned()
            .collect();
        if !plausible.is_empty() {
            unique = plausible;
        }
    }
    unique.into_iter().next().map(|c| c.fresh)
}

fn decrypt_into_whole_frames(fresh: &Rc4, encrypted: &[u8]) -> Option<Vec<u16>> {
    let mut cipher = fresh.clone();
    let mut plain = encrypted.to_vec();
    cipher.apply(&mut plain);
    let mut framer = Framer::new();
    framer.push(&plain);
    let mut header_ids = Vec::new();
    while let Some(next) = framer.next_packet() {
        let packet = next.ok()?;
        if packet.frame().len() > MAX_CLIENT_FRAME {
            return None;
        }
        header_ids.push(packet.header_id());
    }
    (framer.buffered() == 0).then_some(header_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::Packet;

    fn client_stream() -> Vec<u8> {
        [
            Packet::builder(3226).string("ssoticket").i32(1).build(),
            Packet::builder(597).string("oi").i32(0).i32(-1).build(),
            Packet::builder(2596).build(),
        ]
        .iter()
        .flat_map(|p| p.frame().to_vec())
        .collect()
    }

    fn scraped_after(key: &[u8], plain: &[u8]) -> (Rc4, Vec<u8>, SBox) {
        let fresh = Rc4::new(key).expect("non-empty key");
        let mut cipher = fresh.clone();
        let mut encrypted = plain.to_vec();
        cipher.apply(&mut encrypted);
        (fresh, encrypted, *cipher.state())
    }

    #[test]
    fn finds_the_fresh_state_among_decoy_tables() {
        let (fresh, encrypted, scraped) = scraped_after(b"other key", &client_stream());
        let (_, _, decoy) = scraped_after(b"some other process", &client_stream());
        let candidates = find_candidates(&[decoy, scraped], &encrypted);
        assert_eq!(choose_key(candidates, |_| true), Some(fresh));
    }

    #[test]
    fn a_wrong_j_can_also_frame_cleanly_and_known_headers_break_the_tie() {
        let (fresh, encrypted, scraped) = scraped_after(b"shared-secret-from-dh", &client_stream());
        let candidates = find_candidates(&[scraped], &encrypted);
        assert!(
            candidates.len() > 1,
            "this stream is ambiguous on framing alone"
        );
        let known = [3226u16, 597, 2596];
        assert_eq!(
            choose_key(candidates, |id| known.contains(&id)),
            Some(fresh)
        );
    }

    #[test]
    fn rejects_tables_that_do_not_decrypt_to_frames() {
        let (_, encrypted, _) = scraped_after(b"real key", &client_stream());
        let (_, _, unrelated) = scraped_after(
            b"another key",
            b"unrelated bytes that are not packets at all",
        );
        assert!(find_candidates(&[unrelated], &encrypted).is_empty());
    }

    #[test]
    fn g_memz_dump_roundtrips_and_lists_the_newest_table_first() {
        let (_, _, a) = scraped_after(b"a", b"x");
        let (_, _, b) = scraped_after(b"b", b"y");
        let dump = format!(
            "noise\n{}\n{}\n",
            format_table_line(&a),
            format_table_line(&b)
        );
        assert_eq!(parse_table_dump(&dump), vec![b, a]);
    }

    #[test]
    fn ignores_lines_with_values_that_cannot_be_an_sbox() {
        let line = "000001ff".repeat(256);
        assert!(parse_table_dump(&line).is_empty());
    }
}
