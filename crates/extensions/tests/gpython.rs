mod common;

use std::path::Path;
use std::process::Stdio;

use bobba_extensions::{HostEvent, Permission};
use bobba_protocol::Packet;
use common::*;
use tokio::process::Command;

#[tokio::test]
#[ignore = "needs G-Python: BOBBA_PYTHON=/path/to/python-with-g-python"]
async fn real_gpython_extension_blocks_and_answers_through_bobba() {
    let python = std::env::var("BOBBA_PYTHON")
        .expect("BOBBA_PYTHON points at a python with g-python installed");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/antispam.py");
    let mut w = world(None).await;
    let mut extension = Command::new(python)
        .arg("-I")
        .arg(&fixture)
        .args(["-p", &w.extension_port.to_string()])
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("python starts");

    let request = tokio::time::timeout(TIMEOUT, w.permissions.recv())
        .await
        .expect("G-Python asks to connect")
        .expect("open");
    assert_eq!(request.info.title, "AntiSpam");
    let _ = request.respond.send(Permission::Allow);
    loop {
        let event = tokio::time::timeout(TIMEOUT, w.events.recv())
            .await
            .expect("event")
            .expect("open");
        if matches!(event, HostEvent::ExtensionConnected { .. }) {
            break;
        }
    }

    send(&mut w.client, &[version_packet()]).await;
    read_packets(&mut w.server, 1).await;
    let welcome = Packet::builder(2350)
        .i32(7)
        .string("bem-vindo")
        .i32(0)
        .i32(0)
        .i32(-1)
        .build();
    send(&mut w.server, std::slice::from_ref(&welcome)).await;
    assert_eq!(read_packets(&mut w.client, 1).await, vec![welcome]);

    let chat = |text: &str| Packet::builder(597).string(text).i32(0).i32(-1).build();
    send(
        &mut w.client,
        &[chat("buy spam now"), chat("hello"), chat("!ping")],
    )
    .await;
    assert_eq!(read_packets(&mut w.server, 1).await, vec![chat("hello")]);
    let pong = Packet::builder(2350)
        .i32(7)
        .string("pong")
        .i32(0)
        .i32(0)
        .i32(-1)
        .build();
    assert_eq!(read_packets(&mut w.client, 1).await, vec![pong]);

    extension.kill().await.ok();
}
