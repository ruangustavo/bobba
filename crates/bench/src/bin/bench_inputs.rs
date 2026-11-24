use std::path::PathBuf;

use bobba_bench::{Rng, hex};
use bobba_protocol::expression::{HeaderLabel, expression_from_structure};
use bobba_protocol::key_recovery::format_table_line;
use bobba_protocol::{Packet, PacketBuilder, Rc4};

const DECOY_TABLES: usize = 8;
const EXPRESSION_COUNT: usize = 10_000;

fn login_stream() -> Vec<u8> {
    [
        Packet::builder(1484)
            .i32(401)
            .string("app:/")
            .string("https://www.habbo.com.br/gamedata/external_variables/1")
            .build(),
        Packet::builder(2760)
            .string("~0f2b3c4d5e6f708192a3b4c5d6e7f801")
            .string("b6b5a4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7")
            .string("FLASH31")
            .build(),
        Packet::builder(520)
            .string("1b2c3d4e-0000-4000-8000-000000000000-10000000.V4")
            .i32(812)
            .build(),
        Packet::builder(939).build(),
        Packet::builder(1487)
            .string("Login")
            .string("socket")
            .string("client.auth_ok")
            .string("")
            .i32(0)
            .build(),
        Packet::builder(3690).build(),
    ]
    .iter()
    .flat_map(|p| p.frame().to_vec())
    .collect()
}

fn scraped_table(key: &[u8], plain: &[u8]) -> ([u8; 256], Vec<u8>) {
    let mut cipher = Rc4::new(key).unwrap_or_else(|| std::process::exit(1));
    let mut encrypted = plain.to_vec();
    cipher.apply(&mut encrypted);
    (*cipher.state(), encrypted)
}

fn random_packet(rng: &mut Rng) -> (Packet, String) {
    let header = u16::try_from(rng.below(4000)).unwrap_or(1);
    let mut builder = PacketBuilder::new(header);
    let mut structure = String::new();
    for _ in 0..rng.below(8) + 1 {
        match rng.below(4) {
            0 => {
                builder = builder.i32(i32::try_from(rng.below(100_000)).unwrap_or(0) - 50);
                structure.push('i');
            }
            1 => {
                let words = [
                    "oi",
                    "bobba",
                    "olá mundo",
                    "Habbo Hotel",
                    "\"aspas\"",
                    "room 42",
                    "",
                ];
                let text = words[usize::try_from(rng.below(words.len() as u64)).unwrap_or(0)];
                builder = builder.string_bytes(text.as_bytes());
                structure.push('s');
            }
            2 => {
                builder = builder.bool(rng.below(2) == 1);
                structure.push('B');
            }
            _ => {
                builder = builder.i32(-1);
                structure.push('i');
            }
        }
    }
    let packet = builder.build();
    let expression =
        expression_from_structure(&packet, &structure, HeaderLabel::Id(header)).unwrap_or_default();
    (packet, expression)
}

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "bench/inputs".into()),
    );
    std::fs::create_dir_all(&dir)?;
    let mut rng = Rng::new(0xB0BBA);
    let plain = login_stream();
    let key: Vec<u8> = (0..128)
        .map(|_| u8::try_from(rng.below(256)).unwrap_or(0))
        .collect();
    let (real, encrypted) = scraped_table(&key, &plain);
    let fresh = Rc4::new(&key).unwrap_or_else(|| std::process::exit(1));
    std::fs::write(
        dir.join("expected_fresh.txt"),
        format_table_line(fresh.state()) + "\n",
    )?;
    let decoys: Vec<[u8; 256]> = (0..DECOY_TABLES)
        .map(|i| {
            let decoy_key: Vec<u8> = (0..128)
                .map(|_| u8::try_from(rng.below(256)).unwrap_or(0))
                .collect();
            scraped_table(&decoy_key, &plain[..plain.len() - i * 3]).0
        })
        .collect();
    let table_lines = |tables: &[[u8; 256]]| {
        tables
            .iter()
            .map(format_table_line)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut with_real = decoys.clone();
    with_real.push(real);
    std::fs::write(
        dir.join("recovery_success.txt"),
        format!("{}\n{}\n", hex(&encrypted), table_lines(&with_real)),
    )?;
    std::fs::write(
        dir.join("recovery_failure.txt"),
        format!("{}\n{}\n", hex(&encrypted), table_lines(&decoys)),
    )?;

    let (packets, expressions): (Vec<String>, Vec<String>) = (0..EXPRESSION_COUNT)
        .map(|_| random_packet(&mut rng))
        .map(|(packet, expression)| (hex(packet.frame()), expression))
        .unzip();
    std::fs::write(dir.join("packets.hex"), packets.join("\n") + "\n")?;
    std::fs::write(dir.join("expressions.txt"), expressions.join("\n") + "\n")?;
    println!(
        "wrote inputs to {}: {} encrypted bytes, {} tables, {EXPRESSION_COUNT} packets",
        dir.display(),
        encrypted.len(),
        with_real.len()
    );
    Ok(())
}
