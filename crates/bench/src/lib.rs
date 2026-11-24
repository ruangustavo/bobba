use std::time::{Duration, Instant};

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

pub struct Measurement {
    pub name: &'static str,
    pub median: Duration,
    pub best: Duration,
    pub rounds: usize,
}

impl Measurement {
    pub fn report(&self, unit_count: usize) -> String {
        let per_unit = |d: Duration| d.as_secs_f64() * 1e6 / unit_count.max(1) as f64;
        format!(
            "{:<34} median {:>10.3} ms  best {:>10.3} ms  per item {:>9.3} µs  ({} rounds)",
            self.name,
            self.median.as_secs_f64() * 1e3,
            self.best.as_secs_f64() * 1e3,
            per_unit(self.median),
            self.rounds
        )
    }
}

pub fn measure(
    name: &'static str,
    warmup: usize,
    rounds: usize,
    mut run: impl FnMut(),
) -> Measurement {
    for _ in 0..warmup {
        run();
    }
    let mut samples: Vec<Duration> = (0..rounds)
        .map(|_| {
            let started = Instant::now();
            run();
            started.elapsed()
        })
        .collect();
    samples.sort();
    Measurement {
        name,
        median: samples[samples.len() / 2],
        best: samples[0],
        rounds,
    }
}
