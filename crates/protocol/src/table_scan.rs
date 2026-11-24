use crate::key_recovery::SBox;

const SLOT: usize = 4;
pub const TABLE_BYTES: usize = 256 * SLOT;

pub fn scan_tables(memory: &[u8]) -> Vec<SBox> {
    let mut last_seen = [None::<usize>; 256];
    let mut distinct_since = 0;
    let mut found = Vec::new();
    for (index, slot) in memory.as_chunks::<SLOT>().0.iter().enumerate() {
        let Some(value) = slot_value(slot) else {
            distinct_since = index + 1;
            continue;
        };
        if let Some(previous) = last_seen[usize::from(value)] {
            distinct_since = distinct_since.max(previous + 1);
        }
        last_seen[usize::from(value)] = Some(index);
        if index + 1 - distinct_since == 256 {
            let table = table_ending_at(memory, index);
            if !is_rotated_identity(&table) {
                found.push(table);
            }
        }
    }
    found.reverse();
    found
}

fn slot_value(slot: &[u8; SLOT]) -> Option<u8> {
    u8::try_from(u32::from_le_bytes(*slot)).ok()
}

fn table_ending_at(memory: &[u8], last_slot: usize) -> SBox {
    let start = (last_slot + 1 - 256) * SLOT;
    let mut table = [0u8; 256];
    for (entry, slot) in table
        .iter_mut()
        .zip(memory[start..start + TABLE_BYTES].as_chunks::<SLOT>().0)
    {
        *entry = slot[0];
    }
    table
}

fn is_rotated_identity(table: &SBox) -> bool {
    table
        .windows(2)
        .all(|pair| pair[1] == pair[0].wrapping_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permutation(seed: u8) -> SBox {
        let mut table = [0u8; 256];
        for (slot, value) in table.iter_mut().zip(0u8..=u8::MAX) {
            *slot = value.wrapping_mul(37).wrapping_add(seed);
        }
        table
    }

    fn as_memory(table: &SBox) -> Vec<u8> {
        table
            .iter()
            .flat_map(|value| u32::from(*value).to_le_bytes())
            .collect()
    }

    fn noise(slots: usize) -> Vec<u8> {
        (0..slots)
            .flat_map(|i| (0xDEAD_0000u32 + i as u32).to_le_bytes())
            .collect()
    }

    #[test]
    fn finds_a_table_stored_as_u32_slots_between_noise() {
        let table = permutation(7);
        let memory = [noise(40), as_memory(&table), noise(40)].concat();
        assert_eq!(scan_tables(&memory), vec![table]);
    }

    #[test]
    fn ignores_256_small_values_with_a_repeat() {
        let mut table = permutation(7);
        table[200] = table[10];
        let memory = [noise(4), as_memory(&table), noise(4)].concat();
        assert!(scan_tables(&memory).is_empty());
    }

    #[test]
    fn a_slot_above_255_breaks_the_window() {
        let table = as_memory(&permutation(7));
        let mut memory = [noise(4), table, noise(4)].concat();
        let middle = (4 + 128) * SLOT;
        memory[middle + 1] = 1;
        assert!(scan_tables(&memory).is_empty());
    }

    #[test]
    fn ignores_a_table_that_is_not_aligned_to_its_slots() {
        let memory = [vec![0u8; 2], as_memory(&permutation(7)), vec![0u8; 2]].concat();
        assert!(scan_tables(&memory).is_empty());
    }

    #[test]
    fn ignores_a_counting_buffer_that_wraps_around() {
        let counting: Vec<u8> = (0..352u32).flat_map(|i| (i % 256).to_le_bytes()).collect();
        let memory = [noise(4), counting, noise(4)].concat();
        assert!(scan_tables(&memory).is_empty());
    }

    #[test]
    fn returns_the_last_table_in_memory_first() {
        let first = permutation(7);
        let second = permutation(99);
        let memory = [
            noise(4),
            as_memory(&first),
            noise(4),
            as_memory(&second),
            noise(4),
        ]
        .concat();
        assert_eq!(scan_tables(&memory), vec![second, first]);
    }
}
