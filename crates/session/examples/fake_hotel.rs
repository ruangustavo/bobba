use std::time::Duration;

use bobba_protocol::{Framer, Packet};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const HOTEL_ADDRESS: &str = "127.0.0.1:30001";
const PROXY_ADDRESS: &str = "127.0.0.1:30000";
const MAC_RELEASE: &str = "MAC63-202611041140-63291706";
const CHAT_OUT: u16 = 597;
const SHOUT_OUT: u16 = 481;
const CHAT_IN: u16 = 2350;
const LINES: [&str; 5] = [
    "oi",
    "alguém aí?",
    "bobba",
    "olá, mundo",
    "\"aspas\" e [colchetes]",
];

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let hotel = TcpListener::bind(HOTEL_ADDRESS).await?;
    println!(
        "fake hotel on {HOTEL_ADDRESS}; in Bobba connect to 127.0.0.1:30001 listening on 30000"
    );
    tokio::spawn(run_client());
    loop {
        let (stream, _) = hotel.accept().await?;
        tokio::spawn(run_hotel(stream));
    }
}

async fn run_hotel(mut stream: TcpStream) {
    let mut framer = Framer::new();
    let mut buffer = [0u8; 4096];
    while let Ok(n) = stream.read(&mut buffer).await {
        if n == 0 {
            return;
        }
        framer.push(&buffer[..n]);
        while let Some(Ok(packet)) = framer.next_packet() {
            let Ok(text) = packet.reader().read_string() else {
                continue;
            };
            let echo = Packet::builder(CHAT_IN)
                .i32(1)
                .string(&text)
                .i32(0)
                .i32(0)
                .i32(-1)
                .build();
            if stream.write_all(echo.frame()).await.is_err() {
                return;
            }
        }
    }
}

async fn run_client() {
    let mut stream = loop {
        if let Ok(stream) = TcpStream::connect(PROXY_ADDRESS).await {
            break stream;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    println!("client connected through Bobba");
    let hello = Packet::builder(4000)
        .string(MAC_RELEASE)
        .string("FLASH31")
        .i32(5)
        .i32(4)
        .build();
    if stream.write_all(hello.frame()).await.is_err() {
        return;
    }
    let (mut reader, mut writer) = stream.into_split();
    tokio::spawn(async move {
        let mut sink = [0u8; 4096];
        while matches!(reader.read(&mut sink).await, Ok(n) if n > 0) {}
    });
    for (i, line) in LINES.iter().cycle().enumerate() {
        let header = if i % 4 == 3 { SHOUT_OUT } else { CHAT_OUT };
        let packet = Packet::builder(header)
            .string_bytes(line.as_bytes())
            .i32(0)
            .i32(-1)
            .build();
        if writer.write_all(packet.frame()).await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}
