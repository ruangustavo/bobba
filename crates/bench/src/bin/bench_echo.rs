use bobba_protocol::Framer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(31000);
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    println!("echo hotel on {port}");
    loop {
        let (stream, _) = listener.accept().await?;
        stream.set_nodelay(true)?;
        tokio::spawn(echo(stream));
    }
}

async fn echo(mut stream: TcpStream) {
    let mut framer = Framer::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut out = Vec::with_capacity(64 * 1024);
    while let Ok(n) = stream.read(&mut buffer).await {
        if n == 0 {
            return;
        }
        framer.push(&buffer[..n]);
        out.clear();
        while let Some(Ok(packet)) = framer.next_packet() {
            out.extend_from_slice(packet.frame());
        }
        if !out.is_empty() && stream.write_all(&out).await.is_err() {
            return;
        }
    }
}
