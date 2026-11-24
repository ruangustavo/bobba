use std::sync::Arc;

use bobba_protocol::Framer;
use bobba_protocol::extension_message::{ExtensionInfo, ExtensionToHost, HostToExtension};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::host::{ExtensionHost, ExtensionId, PacketInfoLoader};

const READ_BUFFER_SIZE: usize = 64 * 1024;

pub async fn serve<L: PacketInfoLoader>(listener: TcpListener, host: Arc<ExtensionHost<L>>) {
    while let Ok((stream, _)) = listener.accept().await {
        let _ = stream.set_nodelay(true);
        tokio::spawn(handle_connection(stream, host.clone()));
    }
}

struct Inbox {
    reader: OwnedReadHalf,
    framer: Framer,
    buffer: Vec<u8>,
}

impl Inbox {
    async fn next(&mut self) -> Option<ExtensionToHost> {
        loop {
            match self.framer.next_packet() {
                Some(Ok(packet)) => match ExtensionToHost::decode(&packet) {
                    Ok(message) => return Some(message),
                    Err(_) => continue,
                },
                Some(Err(_)) => return None,
                None => {}
            }
            let n = self
                .reader
                .read(&mut self.buffer)
                .await
                .ok()
                .filter(|n| *n > 0)?;
            self.framer.push(&self.buffer[..n]);
        }
    }
}

async fn handle_connection<L: PacketInfoLoader>(stream: TcpStream, host: Arc<ExtensionHost<L>>) {
    let (reader, mut writer) = stream.into_split();
    let mut inbox = Inbox {
        reader,
        framer: Framer::new(),
        buffer: vec![0u8; READ_BUFFER_SIZE],
    };
    if writer
        .write_all(HostToExtension::InfoRequest.encode().frame())
        .await
        .is_err()
    {
        return;
    }
    let Some(info) = wait_for_info(&mut inbox).await else {
        return;
    };
    if !host.authorize(&info).await {
        return;
    }
    let (outbox, mut outgoing) = mpsc::unbounded_channel::<HostToExtension>();
    let writer_task = tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            if writer.write_all(message.encode().frame()).await.is_err() {
                break;
            }
        }
    });
    let id = host.register(info, outbox.clone());
    while let Some(message) = inbox.next().await {
        handle_message(&host, id, &outbox, message);
    }
    host.unregister(id);
    writer_task.abort();
}

async fn wait_for_info(inbox: &mut Inbox) -> Option<ExtensionInfo> {
    loop {
        if let ExtensionToHost::ExtensionInfo(info) = inbox.next().await? {
            return Some(info);
        }
    }
}

fn handle_message<L: PacketInfoLoader>(
    host: &ExtensionHost<L>,
    id: ExtensionId,
    outbox: &mpsc::UnboundedSender<HostToExtension>,
    message: ExtensionToHost,
) {
    let reply = match message {
        ExtensionToHost::ManipulatedPacket(message) => {
            host.reply(id, message);
            None
        }
        ExtensionToHost::SendMessage { direction, packet } => {
            host.send_from_extension(id, direction, packet);
            None
        }
        ExtensionToHost::RequestFlags => Some(HostToExtension::FlagsCheck(Vec::new())),
        ExtensionToHost::PacketToStringRequest(packet) => Some(host.describe_packet(&packet)),
        ExtensionToHost::StringToPacketRequest(text) => Some(host.packet_from_text(&text)),
        ExtensionToHost::ConsoleLog(text) => {
            host.log_from_extension(id, text);
            None
        }
        ExtensionToHost::ExtensionInfo(_) => None,
    };
    if let Some(reply) = reply {
        let _ = outbox.send(reply);
    }
}
