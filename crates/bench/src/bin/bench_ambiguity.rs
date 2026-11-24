use bobba_bench::Rng;
use bobba_protocol::key_recovery::{Probation, ProbationVerdict, find_candidates, shortlist};
use bobba_protocol::{Framer, Packet, Rc4};

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
    ]
    .iter()
    .flat_map(|p| p.frame().to_vec())
    .collect()
}

fn gearth_choice(table: &[u8; 256], encrypted: &[u8]) -> Option<Rc4> {
    let q = (encrypted.len() % 256) as u8;
    (0..=u8::MAX).find_map(|j| {
        let mut candidate = Rc4::from_state(*table, q, j);
        candidate.rewind(encrypted.len());
        if !candidate.could_be_fresh() {
            return None;
        }
        let mut plain = encrypted.to_vec();
        candidate.clone().apply(&mut plain);
        let mut framer = Framer::new();
        framer.push(&plain);
        while let Some(next) = framer.next_packet() {
            next.ok()?;
        }
        (framer.buffered() == 0).then_some(candidate)
    })
}

fn main() {
    let trials = 1000;
    let mut rng = Rng::new(0x5EED);
    let plain = login_stream();
    let follow_up = Packet::builder(939).build().frame().to_vec();
    let (mut gearth_wrong, mut ambiguous, mut bobba_wrong) = (0, 0, 0);
    for _ in 0..trials {
        let key: Vec<u8> = (0..128)
            .map(|_| u8::try_from(rng.below(256)).unwrap_or(0))
            .collect();
        let fresh = Rc4::new(&key).unwrap_or_else(|| std::process::exit(1));
        let mut client = fresh.clone();
        let mut encrypted = plain.clone();
        client.apply(&mut encrypted);
        let table = *client.state();

        if gearth_choice(&table, &encrypted).as_ref() != Some(&fresh) {
            gearth_wrong += 1;
        }
        let states = shortlist(find_candidates(&[table], &encrypted), |_| true);
        if states.len() > 1 {
            ambiguous += 1;
        }
        let mut probation = Probation::new(states, &encrypted);
        let mut next = follow_up.clone();
        client.apply(&mut next);
        probation.feed(&next);
        let chosen = match probation.verdict() {
            ProbationVerdict::Decided(state) => Some(*state),
            _ => probation.best_guess(),
        };
        if chosen.as_ref() != Some(&fresh) {
            bobba_wrong += 1;
        }
    }
    println!("{trials} login handshakes with random keys:");
    println!("  ambiguous (more than one state frames cleanly): {ambiguous}");
    println!("  G-Earth first-match picks the wrong key:        {gearth_wrong}");
    println!("  Bobba (probation on the next packet) wrong:     {bobba_wrong}");
}
