use std::time::{Duration, Instant};

use bobba_protocol::{Framer, Packet};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const ROUND_TRIPS: usize = 10_000;
const FLOOD: usize = 200_000;

async fn read_packets(
    stream: &mut tokio::net::tcp::OwnedReadHalf,
    framer: &mut Framer,
    buffer: &mut [u8],
    count: usize,
) {
    let mut seen = 0;
    while seen < count {
        while let Some(Ok(_)) = framer.next_packet() {
            seen += 1;
            if seen == count {
                return;
            }
        }
        let n = stream.read(buffer).await.unwrap_or(0);
        if n == 0 {
            eprintln!("connection closed after {seen} packets");
            std::process::exit(1);
        }
        framer.push(&buffer[..n]);
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(31000);
    let label = args.next().unwrap_or_else(|| "target".into());
    let stream = TcpStream::connect(("127.0.0.1", port)).await?;
    stream.set_nodelay(true)?;
    let (mut reader, mut writer) = stream.into_split();
    let mut framer = Framer::new();
    let mut buffer = vec![0u8; 256 * 1024];

    let hello = Packet::builder(4000)
        .string("WIN63-202611041140-327405090")
        .string("FLASH31")
        .i32(6)
        .i32(4)
        .build();
    writer.write_all(hello.frame()).await?;
    read_packets(&mut reader, &mut framer, &mut buffer, 1).await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    let chat = Packet::builder(597)
        .string("hello from the benchmark")
        .i32(0)
        .i32(-1)
        .build();
    for _ in 0..1000 {
        writer.write_all(chat.frame()).await?;
        read_packets(&mut reader, &mut framer, &mut buffer, 1).await;
    }
    let mut rtts: Vec<Duration> = Vec::with_capacity(ROUND_TRIPS);
    for _ in 0..ROUND_TRIPS {
        let started = Instant::now();
        writer.write_all(chat.frame()).await?;
        read_packets(&mut reader, &mut framer, &mut buffer, 1).await;
        rtts.push(started.elapsed());
    }
    rtts.sort();
    let percentile = |p: f64| rtts[((rtts.len() as f64 * p) as usize).min(rtts.len() - 1)];

    let flood: Vec<u8> = (0..FLOOD).flat_map(|_| chat.frame().to_vec()).collect();
    let started = Instant::now();
    let send = tokio::spawn(async move {
        let _ = writer.write_all(&flood).await;
        writer
    });
    read_packets(&mut reader, &mut framer, &mut buffer, FLOOD).await;
    let elapsed = started.elapsed();
    let _writer = send.await;

    println!(
        "{label:<8} round trip p50 {:>7.1} µs  p99 {:>7.1} µs  max {:>8.1} µs | flood {FLOOD} packets in {:>7.1} ms = {:>9.0} packets/s",
        percentile(0.50).as_secs_f64() * 1e6,
        percentile(0.99).as_secs_f64() * 1e6,
        rtts[rtts.len() - 1].as_secs_f64() * 1e6,
        elapsed.as_secs_f64() * 1e3,
        FLOOD as f64 / elapsed.as_secs_f64()
    );
    Ok(())
}
