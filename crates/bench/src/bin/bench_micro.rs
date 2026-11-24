use std::hint::black_box;
use std::path::PathBuf;

use bobba_bench::{measure, unhex};
use bobba_protocol::expression::HeaderLabel;
use bobba_protocol::key_recovery::{SBox, find_candidates, parse_table_dump, shortlist};
use bobba_protocol::prediction::predicted_expression;
use bobba_protocol::{Expression, Packet};

fn recovery_input(path: &PathBuf) -> (Vec<u8>, Vec<SBox>) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let (first, rest) = text.split_once('\n').unwrap_or_default();
    let mut tables = parse_table_dump(rest);
    tables.reverse();
    (unhex(first), tables)
}

fn main() {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "bench/inputs".into()),
    );
    let (encrypted, tables) = recovery_input(&dir.join("recovery_success.txt"));
    let (_, decoys) = recovery_input(&dir.join("recovery_failure.txt"));
    let found = shortlist(find_candidates(&tables, &encrypted), |_| true);
    println!(
        "bobba: success input yields {} candidate state(s)",
        found.len()
    );
    let expected = parse_table_dump(
        &std::fs::read_to_string(dir.join("expected_fresh.txt")).unwrap_or_default(),
    );
    println!(
        "bobba: right key among candidates = {}",
        found
            .iter()
            .any(|state| expected.first() == Some(state.state()))
    );

    let success = measure("key recovery (key present)", 3, 20, || {
        black_box(shortlist(
            find_candidates(black_box(&tables), black_box(&encrypted)),
            |_| true,
        ));
    });
    println!("{}", success.report(1));
    let failure = measure("key recovery (no key, gives up)", 3, 20, || {
        black_box(find_candidates(black_box(&decoys), black_box(&encrypted)));
    });
    println!("{}", failure.report(1));

    let expressions: Vec<String> = std::fs::read_to_string(dir.join("expressions.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    let packets: Vec<Packet> = std::fs::read_to_string(dir.join("packets.hex"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| Packet::from_frame(unhex(line)).ok())
        .collect();
    let parse = measure("parse 10k expressions", 5, 30, || {
        for expression in &expressions {
            black_box(Expression::parse(black_box(expression)).ok());
        }
    });
    println!("{}", parse.report(expressions.len()));
    let predict = measure("predict+format 10k expressions", 5, 30, || {
        for packet in &packets {
            black_box(predicted_expression(
                black_box(packet),
                HeaderLabel::Id(packet.header_id()),
            ));
        }
    });
    println!("{}", predict.report(packets.len()));
}
