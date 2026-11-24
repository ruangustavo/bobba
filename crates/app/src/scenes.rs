use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bobba_extensions::{ProcessStatus, Runtime};
use bobba_protocol::extension_message::ExtensionInfo;
use bobba_protocol::{Direction, Packet, PacketInfo, PacketInfoEntry};
use bobba_session::{ConnectionInfo, LogEntry, Outcome};

use bobba_store::{Catalog, Origin, Os};

use crate::backend::{CatalogEvent, InstalledKind, InstalledState};
use crate::state::{App, GEarthOffer, KeyState, Mode, Phase, Tab, Traffic};
use crate::store::Showing;

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
    ("store", store),
    ("gearth", |s| {
        extensions(s);
        s.gearth = GEarthOffer::Found {
            folder: dirs::home_dir()
                .unwrap_or_default()
                .join("Downloads")
                .join("G-Earth"),
            count: 4,
        };
    }),
    ("gearth-windows", |s| {
        extensions(s);
        s.gearth = GEarthOffer::Found {
            folder: PathBuf::from(
                r"C:\Users\Jogador\OneDrive\Área de Trabalho\Programas do Habbo\G-Earth 1.5.4 (Windows)",
            ),
            count: 12,
        };
    }),
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
            id: "antispam-48120.py".into(),
            kind: InstalledKind::Single(Runtime::Python),
            status: ProcessStatus::Running,
        },
        InstalledState {
            id: "wired-helper-90314.jar".into(),
            kind: InstalledKind::Single(Runtime::Java),
            status: ProcessStatus::Stopped,
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

const STORE_CONFIG: &str = r#"{"categories": [{"name": "Building"}, {"name": "Trading"},
    {"name": "Usability"}, {"name": "Moderation"}, {"name": "G-Earth Utilities"}],
    "frameworks": [{"name": "G-Node", "installation": {"required": true,
    "instructions": "https://nodejs.org/en/download"}}]}"#;

fn store_listing(
    title: &str,
    description: &str,
    version: &str,
    category: &str,
    framework: &str,
    rating: u32,
) -> String {
    let program = if framework == "G-Node" {
        "node"
    } else {
        "java"
    };
    format!(
        r#"{{"title": "{title}", "description": "{description}", "authors": [{{"name": "pixelmint"}}],
            "version": "{version}", "categories": ["{category}"], "source": "https://example.test/{rating}",
            "framework": {{"name": "{framework}"}}, "rating": {rating}, "updateDate": "12-11-2026 10:00:00",
            "commands": {{"default": ["{program}", "extension"]}},
            "compatibility": {{"systems": ["Mac", "Windows", "Linux"], "clients": ["Flash"]}}}}"#
    )
}

fn store(s: &mut App) {
    extensions(s);
    s.installed.push(InstalledState {
        id: "Wired Helper_1.3.0".into(),
        kind: InstalledKind::Package {
            title: "Wired Helper".into(),
            version: "1.3.0".into(),
        },
        status: ProcessStatus::Running,
    });
    let listings = [
        store_listing(
            "Wired Helper",
            "Reads the wired in your room and lets you copy a stack to another room, check which triggers fire, and step through effects one at a time.",
            "1.4.2",
            "Building",
            "Native",
            284,
        ),
        store_listing(
            "Room Mapper",
            "Draws the current room from above with every furni at its tile, ready to share or to rebuild the room somewhere else.",
            "2.0.1",
            "Building",
            "G-Node",
            231,
        ),
        store_listing(
            "Trade Ledger",
            "Logs each trade you confirm, values both sides with recent marketplace averages, and warns before you accept a lopsided offer.",
            "0.9.4",
            "Trading",
            "Native",
            196,
        ),
        store_listing(
            "Chat Translator",
            "Shows a translation under each chat bubble and can translate what you type before it is sent.",
            "1.1.0",
            "Usability",
            "G-Python",
            171,
        ),
        store_listing(
            "Door Guard",
            "Keeps an allow list for your room and kicks anyone else who walks in, with a log of who tried.",
            "2.3.1",
            "Moderation",
            "Native",
            64,
        ),
        store_listing(
            "QX Scripter",
            "Write small C# scripts against the room, the chat and your inventory, or start from scripts shared by the community.",
            "0.1.41",
            "G-Earth Utilities",
            "QX",
            148,
        ),
    ];
    let extensions = format!("[{}]", listings.join(","));
    if let Ok(catalog) = Catalog::parse(STORE_CONFIG, &extensions, Os::current()) {
        s.store.handle_catalog(CatalogEvent::Loaded {
            catalog,
            origin: Origin::Fresh,
        });
    }
    s.store.refresh_programs(|program| program != "node");
    s.store.showing = Showing::Store;
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
