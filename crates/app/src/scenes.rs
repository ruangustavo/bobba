use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bobba_extensions::Runtime;
use bobba_protocol::extension_message::ExtensionInfo;
use bobba_protocol::{Direction, Packet, PacketInfo, PacketInfoEntry};
use bobba_session::{ConnectionInfo, LogEntry, Outcome};

use crate::backend::InstalledState;
use crate::state::{App, KeyState, Mode, Phase, Tab, Traffic};

const RELEASE: &str = "MAC63-202611201617-213322612";
const HEADLESS_FLAGS: [&str; 7] = [
    "--snap",
    "--tree",
    "--script",
    "--trace",
    "--strip",
    "--frames",
    "--list-scenes",
];

pub fn is_headless() -> bool {
    std::env::args().any(|arg| HEADLESS_FLAGS.contains(&arg.as_str()))
}

pub type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("idle", |_| {}),
    ("resign", |s| {
        s.phase = Phase::NeedsResign(PathBuf::from("/Applications/Habbo.app"))
    }),
    ("approval", |s| s.phase = Phase::AwaitingApproval),
    ("waiting", |s| s.phase = Phase::WaitingForClient(10)),
    ("connected", connected),
    ("failed", |s| {
        s.phase = Phase::Failed("Could not find Habbo's encryption key".into())
    }),
    ("manual", |s| s.mode = Mode::Manual),
    ("injection", injection),
    ("extensions", extensions),
    ("permission", permission),
];

fn packet_info() -> Arc<PacketInfo> {
    let entry = |direction, header_id, name: &str| PacketInfoEntry {
        direction,
        header_id,
        name: Some(name.to_owned()),
        hash: None,
        structure: None,
        source: "Sulek".to_owned(),
    };
    Arc::new(PacketInfo::from_entries(
        RELEASE,
        vec![
            entry(Direction::ToServer, 4000, "ClientHello"),
            entry(Direction::ToServer, 597, "Chat"),
            entry(Direction::ToServer, 481, "Shout"),
            entry(Direction::ToServer, 3320, "MoveAvatar"),
            entry(Direction::ToClient, 2350, "Chat"),
            entry(Direction::ToClient, 1640, "UserUpdate"),
        ],
        true,
    ))
}

fn traffic() -> Vec<LogEntry> {
    let lines = [
        "oi",
        "alguém aí?",
        "bobba",
        "olá, mundo",
        "\"aspas\" e [colchetes]",
    ];
    let mut entries = vec![LogEntry::Intercepted {
        index: 0,
        direction: Direction::ToServer,
        packet: Packet::builder(4000)
            .string(RELEASE)
            .string("FLASH31")
            .i32(5)
            .i32(4)
            .build(),
        outcome: Outcome::Forwarded,
    }];
    for (i, line) in lines.iter().cycle().take(36).enumerate() {
        let index = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let outcome = match i {
            7 => Outcome::Blocked {
                by: "AntiSpam".into(),
            },
            12 => Outcome::Edited {
                by: "Translator".into(),
                sent: Packet::builder(597)
                    .string_bytes("hello, world".as_bytes())
                    .i32(0)
                    .i32(-1)
                    .build(),
            },
            _ => Outcome::Forwarded,
        };
        let header = if i % 6 == 5 { 481 } else { 597 };
        entries.push(LogEntry::Intercepted {
            index,
            direction: Direction::ToServer,
            packet: Packet::builder(header)
                .string_bytes(line.as_bytes())
                .i32(0)
                .i32(-1)
                .build(),
            outcome,
        });
        entries.push(LogEntry::Intercepted {
            index,
            direction: Direction::ToClient,
            packet: Packet::builder(2350)
                .i32(7)
                .string_bytes(line.as_bytes())
                .i32(0)
                .i32(0)
                .i32(-1)
                .build(),
            outcome: Outcome::Forwarded,
        });
        if i % 9 == 4 {
            entries.push(LogEntry::Intercepted {
                index,
                direction: Direction::ToServer,
                packet: Packet::builder(3320).i32(12).i32(8).build(),
                outcome: Outcome::Forwarded,
            });
        }
    }
    entries.push(LogEntry::Injected {
        direction: Direction::ToServer,
        packet: Packet::builder(481).string("injected shout").i32(0).build(),
        by: "Injection".into(),
    });
    entries
}

fn connected(s: &mut App) {
    s.phase = Phase::Connected(ConnectionInfo {
        host: "game-br.habbo.com".into(),
        port: 30000,
        hotel_version: RELEASE.into(),
        client_identifier: "FLASH31".into(),
        client_type: "FLASH".into(),
    });
    s.key = KeyState::Decrypted {
        candidates: 3,
        elapsed: Duration::from_millis(2),
    };
    let info = packet_info();
    s.feed.log_mut().set_packet_info(info.clone());
    s.packet_info = Some(info);
    s.handle(crate::backend::UiEvent::Log(traffic()));
    s.traffic = Traffic {
        to_server: 1204,
        to_client: 3881,
    };
    s.extension_port = Some(9092);
    s.extensions = vec![
        (1, extension("AntiSpam", Some("antispam-48120.py"))),
        (2, extension("Translator", None)),
    ];
}

fn injection(s: &mut App) {
    connected(s);
    s.tab = Tab::Injection;
    s.injection
        .replace_text("{out:Chat}{s:\"hello from Bobba\"}{i:0}{i:-1}");
    s.last_send = Some(crate::state::SendResult::Sent(Direction::ToServer));
    let info = s.packet_info.clone();
    for (direction, expression) in [
        (Direction::ToServer, "{out:Chat}{s:\"oi\"}{i:0}{i:-1}"),
        (
            Direction::ToClient,
            "{in:Chat}{i:7}{s:\"bobba\"}{i:0}{i:0}{i:-1}",
        ),
        (Direction::ToServer, "{out:MoveAvatar}{i:12}{i:8}"),
        (Direction::ToServer, "{out:Shout}{s:\"alguém aí?\"}{i:0}"),
    ] {
        s.recent.remember(direction, expression, info.as_deref());
    }
}

fn extension(title: &str, file: Option<&str>) -> ExtensionInfo {
    ExtensionInfo {
        title: title.into(),
        author: "ruan".into(),
        version: "1.0".into(),
        description: String::new(),
        on_click_used: false,
        file: file.map(str::to_owned),
        cookie: None,
        can_leave: true,
        can_delete: true,
    }
}

fn extensions(s: &mut App) {
    connected(s);
    s.tab = Tab::Extensions;
    s.installed = vec![
        InstalledState {
            file_name: "antispam-48120.py".into(),
            runtime: Runtime::Python,
            running: true,
        },
        InstalledState {
            file_name: "wired-helper-90314.jar".into(),
            runtime: Runtime::Java,
            running: false,
        },
    ];
    for line in [
        "[antispam-48120.py] AntiSpam ready, watching Chat",
        "[AntiSpam] blocked \"buy spam now\"",
        "Packet #812 ToServer went through after waiting too long for Translator",
    ] {
        s.log_console(line.into());
    }
}

fn permission(s: &mut App) {
    connected(s);
    let (respond, _) = tokio::sync::oneshot::channel();
    s.permission_requests
        .push_back(bobba_extensions::PermissionRequest {
            info: extension("Room Mapper", None),
            respond,
        });
}
