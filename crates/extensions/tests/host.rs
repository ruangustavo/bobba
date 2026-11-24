mod common;

use std::time::Duration;

use bobba_extensions::{HostEvent, Permission, SendError};
use bobba_protocol::extension_message::{ExtensionInfo, ExtensionToHost};
use bobba_protocol::{Direction, Packet};
use bobba_session::LogEntry;
use common::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn extension_can_block_edit_and_forward() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Editor").await;
    connect_hotel(&mut w, &mut ext).await;

    let spam = Packet::builder(597).string("spam").build();
    let hello = Packet::builder(597).string("hello").build();
    let walk = Packet::builder(3320).i32(1).i32(2).build();
    send(&mut w.client, &[spam, hello, walk.clone()]).await;

    let mut blocked = ext.next_intercept().await;
    blocked.blocked = true;
    ext.answer(blocked).await;
    let mut edited = ext.next_intercept().await;
    edited.packet = Packet::builder(597).string("HELLO").build();
    ext.answer(edited).await;
    let untouched = ext.next_intercept().await;
    ext.answer(untouched).await;

    let received = read_packets(&mut w.server, 2).await;
    assert_eq!(
        received,
        vec![Packet::builder(597).string("HELLO").build(), walk]
    );
}

#[tokio::test]
async fn replies_out_of_order_still_reach_the_server_in_order() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Async").await;
    connect_hotel(&mut w, &mut ext).await;

    let first = Packet::new(1, b"first");
    let second = Packet::new(2, b"second");
    send(&mut w.client, &[first.clone(), second.clone()]).await;
    let one = ext.next_intercept().await;
    let two = ext.next_intercept().await;
    ext.answer(two).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    ext.answer(one).await;
    assert_eq!(read_packets(&mut w.server, 2).await, vec![first, second]);
}

#[tokio::test]
async fn silent_extension_times_out_and_the_packet_goes_through() {
    let mut w = world(Some(Duration::from_millis(100))).await;
    let mut ext = FakeExtension::connect(&mut w, "Sleepy").await;
    connect_hotel(&mut w, &mut ext).await;

    let packet = Packet::new(5, b"x");
    send(&mut w.client, std::slice::from_ref(&packet)).await;
    assert_eq!(read_packets(&mut w.server, 1).await, vec![packet]);
    let timed_out = loop {
        match tokio::time::timeout(TIMEOUT, w.events.recv())
            .await
            .expect("event")
            .expect("open")
        {
            HostEvent::InterceptTimedOut { waiting_on, .. } => break waiting_on,
            _ => continue,
        }
    };
    assert_eq!(timed_out, vec!["Sleepy".to_owned()]);
}

#[tokio::test]
async fn disconnecting_extension_releases_its_pending_packets() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Crashy").await;
    connect_hotel(&mut w, &mut ext).await;

    let packet = Packet::new(5, b"x");
    send(&mut w.client, std::slice::from_ref(&packet)).await;
    let _ = ext.next_intercept().await;
    drop(ext);
    assert_eq!(read_packets(&mut w.server, 1).await, vec![packet]);
}

#[tokio::test]
async fn extension_sends_only_headers_already_seen_unless_developer_mode() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Sender").await;
    connect_hotel(&mut w, &mut ext).await;

    let unseen = Packet::builder(2350).i32(1).string("oi").build();
    ext.send(ExtensionToHost::SendMessage {
        direction: Direction::ToClient,
        packet: unseen.clone(),
    })
    .await;
    let refused = loop {
        match tokio::time::timeout(TIMEOUT, w.events.recv())
            .await
            .expect("event")
            .expect("open")
        {
            HostEvent::SendRefused { reason, .. } => break reason,
            _ => continue,
        }
    };
    assert_eq!(
        refused,
        SendError::Unsafe {
            header_id: 2350,
            direction: Direction::ToClient
        }
    );

    w.host.set_developer_mode(true);
    ext.send(ExtensionToHost::SendMessage {
        direction: Direction::ToClient,
        packet: unseen.clone(),
    })
    .await;
    assert_eq!(read_packets(&mut w.client, 1).await, vec![unseen.clone()]);
    let injected = loop {
        match tokio::time::timeout(TIMEOUT, w.log.recv())
            .await
            .expect("log")
            .expect("open")
        {
            entry @ LogEntry::Injected { .. } => break entry,
            _ => continue,
        }
    };
    assert_eq!(
        injected,
        LogEntry::Injected {
            direction: Direction::ToClient,
            packet: unseen,
            by: "Sender".into()
        }
    );
}

#[tokio::test]
async fn connection_start_carries_sulek_names_and_expressions_resolve() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Names").await;
    send(&mut w.client, &[version_packet()]).await;
    let start = loop {
        let packet = ext.next().await;
        if packet.header_id() == 5 {
            break packet;
        }
    };
    let mut reader = start.reader();
    assert_eq!(reader.read_string().as_deref(), Ok("127.0.0.1"));
    let _port = reader.read_i32();
    assert_eq!(reader.read_string().as_deref(), Ok("WIN63-1"));
    assert_eq!(reader.read_string().as_deref(), Ok("FLASH31"));
    assert_eq!(reader.read_string().as_deref(), Ok("FLASH"));
    assert_eq!(reader.read_i32(), Ok(2));

    let version = ext.next_intercept().await;
    ext.answer(version).await;
    read_packets(&mut w.server, 1).await;
    let chat = w
        .host
        .parse_expression("{out:Chat}{s:\"oi\"}{i:0}{i:-1}")
        .expect("resolves");
    assert_eq!(
        chat,
        (
            Packet::builder(597).string("oi").i32(0).i32(-1).build(),
            Some(Direction::ToServer)
        )
    );
}

#[tokio::test]
async fn denied_extensions_are_disconnected() {
    let mut w = world(None).await;
    let mut stream = TcpStream::connect(("127.0.0.1", w.extension_port))
        .await
        .expect("connects");
    let info = ExtensionToHost::ExtensionInfo(ExtensionInfo {
        title: "Rogue".into(),
        author: String::new(),
        version: String::new(),
        description: String::new(),
        on_click_used: false,
        file: None,
        cookie: None,
        can_leave: true,
        can_delete: true,
    });
    stream
        .write_all(info.encode().frame())
        .await
        .expect("writes");
    let request = w.permissions.recv().await.expect("asked");
    assert_eq!(request.info.title, "Rogue");
    let _ = request.respond.send(Permission::Deny);
    let mut rest = Vec::new();
    let closed = tokio::time::timeout(TIMEOUT, stream.read_to_end(&mut rest)).await;
    assert!(closed.is_ok(), "host closes the connection");
    assert!(w.host.extensions().is_empty());
}

#[tokio::test]
async fn installed_extension_with_its_cookie_skips_the_prompt() {
    let mut w = world(None).await;
    w.host
        .issue_install_cookie("antispam-x1.py", "secret".into());
    let mut stream = TcpStream::connect(("127.0.0.1", w.extension_port))
        .await
        .expect("connects");
    let info = ExtensionToHost::ExtensionInfo(ExtensionInfo {
        title: "AntiSpam".into(),
        author: String::new(),
        version: String::new(),
        description: String::new(),
        on_click_used: false,
        file: Some("antispam-x1.py".into()),
        cookie: Some("secret".into()),
        can_leave: true,
        can_delete: true,
    });
    stream
        .write_all(info.encode().frame())
        .await
        .expect("writes");
    let connected = loop {
        match tokio::time::timeout(TIMEOUT, w.events.recv())
            .await
            .expect("event")
            .expect("open")
        {
            HostEvent::ExtensionConnected { info, .. } => break info.title,
            _ => continue,
        }
    };
    assert_eq!(connected, "AntiSpam");
    assert!(w.permissions.try_recv().is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn store_package_runs_its_command_inside_its_folder_with_placeholders() {
    let dir = tempfile::tempdir().expect("temp dir");
    let installer = bobba_extensions::Installer::new(dir.path());
    let command = [
        "/bin/sh",
        "-c",
        "printf '%s %s %s' \"$1\" \"$2\" \"$3\" > args.txt",
        "--",
        "{cookie}",
        "{port}",
        "{filename}",
    ]
    .map(String::from);
    let installed = installer
        .install_package("Echo", "1.0", &command, &empty_archive())
        .expect("installs");
    let w = world(None).await;
    let (output, _lines) = tokio::sync::mpsc::unbounded_channel();

    let mut child =
        bobba_extensions::launch(&installed, w.extension_port, &w.host, output).expect("launches");
    child.wait().await.expect("exits");

    let written = std::fs::read_to_string(dir.path().join("Echo_1.0/extension/args.txt"))
        .expect("ran inside the extension folder");
    let parts: Vec<&str> = written.split(' ').collect();
    assert_eq!(parts.len(), 3, "{written}");
    assert!(parts[0].len() == 40 && parts[0].bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(parts[1], w.extension_port.to_string());
    assert_eq!(parts[2], "Echo_1.0");
}

fn empty_archive() -> Vec<u8> {
    let writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer.finish().expect("finishes archive").into_inner()
}
