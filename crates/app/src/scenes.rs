use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bobba_extensions::{ProcessStatus, Runtime};
use bobba_protocol::extension_message::ExtensionInfo;
use bobba_protocol::{Direction, Packet, PacketInfo, PacketInfoEntry};
use bobba_session::{ConnectionInfo, LogEntry, Outcome};

use bobba_store::{Catalog, Origin, Os};

use crate::backend::{CatalogEvent, InstalledKind, InstalledState};
use crate::logger::Logger;
use crate::state::{App, GEarthOffer, KeyState, Mode, Phase, Tab, Traffic};
use crate::store_window::StoreWindow;

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
    s.store.handle_missing(std::collections::HashSet::from([
        bobba_store::Runtime::NodeJs,
    ]));
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

pub type Break = (&'static str, &'static str, fn(&mut StoreWindow));

pub const BREAKS: &[Break] = &[
    ("store", "Worst case", store_worst),
    ("store", "Worst details", store_worst_details),
    ("store", "Details: one long word", |s| {
        store_worst(s);
        s.detail = Some("Benachrichtigungseinstellungen".into());
    }),
    ("store", "Details: long title failed", |s| {
        store_worst(s);
        s.detail = Some("Copiador de Roupas Automático para Quartos Lotados".into());
    }),
    ("store", "Details: no authors", |s| {
        store_worst(s);
        s.detail = Some("🦊 Fox Follower".into());
    }),
    ("store", "Empty", store_empty),
    ("store", "One", store_one),
    ("store", "1,000 rows", store_huge),
];

const WORST_CONFIG: &str = r#"{"categories": [{"name": "Fun"}, {"name": "Games"},
    {"name": "Trading"}, {"name": "Building"}, {"name": "Moderation"}, {"name": "Usability"},
    {"name": "G-Earth Utilities"}, {"name": "Others"}],
    "frameworks": [{"name": "G-Node", "installation": {"required": true,
    "instructions": "https://nodejs.org/en/download"}}]}"#;

const WORST_EXTENSIONS: &str = r#"[
    {"title": "G-Presets - Building & Wired Presets",
     "description": "Save any stack of furni and wired as a preset and place it again in any room.\n\n:preset save name - Save the selected area\n:preset load name - Place a preset where you stand\n:preset list - List your presets\n:preset delete name - Delete a preset\n(rotation: 0-7, if empty the saved rotation will be used.)\n:preset rotate x - Rotate the preset before placing it\n:preset mirror - Mirror the preset\n:preset stop - Stop placing\n\nRe-enter the room after installing.\nWorks best with the Wired Helper extension.\nReport bugs on the G-Earth Discord.",
     "authors": [{"name": "sirjonasxx"}, {"name": "Roboroads"}, {"name": "WiredSpast"}],
     "version": "1.2.0.0", "categories": ["Building", "Usability", "G-Earth Utilities"],
     "source": "https://example.test/g-presets/tree/main/src/main/java/extension",
     "framework": {"name": "Native"}, "rating": 2265,
     "commands": {"default": ["java", "-jar", "g-presets.jar"]},
     "compatibility": {"systems": ["Mac", "Windows"], "clients": ["Flash"]}},
    {"title": "[AIO] Gamba Suite",
     "description": "Dice, roulette and 13 other casino games with automatic payouts.",
     "authors": [{"name": "pixelmint"}], "version": "3.0.4", "categories": ["Games"],
     "framework": {"name": "G-Python"}, "rating": 1284, "updateDate": "02-09-2021 18:08:10",
     "commands": {"default": ["node", "gamba.js"]},
     "compatibility": {"systems": ["Mac", "Windows"], "clients": ["Flash"]}},
    {"title": "Copiador de Roupas Automático para Quartos Lotados",
     "description": "Copia a roupa de qualquer pessoa no quarto com um clique.",
     "authors": [{"name": "Đặng Thị Ngọc Hân"}], "version": "0.1.41", "categories": ["Fun"],
     "framework": {"name": "Native"}, "rating": 1000, "updateDate": "01-11-2025 00:00:00",
     "commands": {"default": ["java", "-jar", "copiador.jar"]},
     "compatibility": {"systems": ["Mac", "Windows"], "clients": ["Flash"]}},
    {"title": "autoclick", "description": "clicks", "authors": [{"name": "dana"}],
     "version": "1.2.13", "categories": ["Usability"], "framework": {"name": "Native"},
     "rating": 999, "updateDate": "12-11-2026 10:00:00",
     "commands": {"default": ["java", "-jar", "autoclick.jar"]},
     "compatibility": {"systems": ["Mac", "Windows"], "clients": ["Flash"]}},
    {"title": "🦊 Fox Follower", "authors": [], "version": "1.0",
     "categories": ["Fun"], "framework": {"name": "Native"},
     "commands": {"default": ["java", "-jar", "fox.jar"]},
     "compatibility": {"systems": ["Mac", "Windows"], "clients": ["Flash"]}},
    {"title": "Benachrichtigungseinstellungen",
     "description": "Docs: https://example.test/benachrichtigungseinstellungen/wiki/Konfiguration-und-Installation-unter-Windows-und-macOS",
     "authors": [{"name": "Konstantin Oberhauser-Wettstein"}], "version": "2.3.1",
     "categories": ["Others"], "framework": {"name": "Native"}, "rating": 12,
     "commands": {"default": ["java", "-jar", "b.jar"]},
     "compatibility": {"systems": ["Mac", "Windows"], "clients": ["Flash"]}}
]"#;

fn load_catalog(s: &mut StoreWindow, config: &str, extensions: &str, origin: Origin) {
    if let Ok(catalog) = Catalog::parse(config, extensions, Os::current()) {
        s.store
            .handle_catalog(CatalogEvent::Loaded { catalog, origin });
    }
}

fn store_worst(s: &mut StoreWindow) {
    s.installed = vec![InstalledState {
        id: "autoclick_1.2.0".into(),
        kind: InstalledKind::Package {
            title: "autoclick".into(),
            version: "1.2.0".into(),
        },
        status: ProcessStatus::Running,
    }];
    load_catalog(s, WORST_CONFIG, WORST_EXTENSIONS, Origin::Saved);
    s.store.handle_missing(std::collections::HashSet::from([
        bobba_store::Runtime::NodeJs,
        bobba_store::Runtime::Python,
    ]));
    s.store.handle_install(
        "Copiador de Roupas Automático para Quartos Lotados".into(),
        crate::backend::StoreInstall::Failed(
            "error sending request for url (https://raw.githubusercontent.com/sirjonasxx/G-ExtensionStore/repo/1.5.3/store/extensions/Copiador%20de%20Roupas/extension.zip)".into(),
        ),
    );
    s.store.handle_install(
        "🦊 Fox Follower".into(),
        crate::backend::StoreInstall::Downloading,
    );
}

fn store_worst_details(s: &mut StoreWindow) {
    store_worst(s);
    s.detail = Some("G-Presets - Building & Wired Presets".into());
}

fn store_empty(s: &mut StoreWindow) {
    load_catalog(s, STORE_CONFIG, "[]", Origin::Fresh);
}

fn store_one(s: &mut StoreWindow) {
    let one = format!(
        "[{}]",
        store_listing(
            "Trade Ledger",
            "Logs each trade.",
            "0.9.4",
            "Trading",
            "Native",
            1
        )
    );
    load_catalog(s, STORE_CONFIG, &one, Origin::Fresh);
}

fn store_huge(s: &mut StoreWindow) {
    let listings: Vec<String> = (0..1000u32)
        .map(|i| {
            store_listing(
                &format!("Extension {i}"),
                "Logs each trade you confirm and values both sides.",
                "1.0.0",
                "Trading",
                "Native",
                i,
            )
        })
        .collect();
    load_catalog(
        s,
        STORE_CONFIG,
        &format!("[{}]", listings.join(",")),
        Origin::Fresh,
    );
}

pub type LoggerScene = (&'static str, fn(&mut Logger));

pub const LOGGER_SCENES: &[LoggerScene] = &[("logger", logger_demo)];

pub type LoggerBreak = (&'static str, &'static str, fn(&mut Logger));

pub const LOGGER_BREAKS: &[LoggerBreak] = &[
    ("logger", "Worst case", logger_worst),
    ("logger", "Open: floor plan", |s| open_worst(s, 2)),
    ("logger", "Open: 600 furni", |s| open_worst(s, 3)),
    ("logger", "Open: edited, long title", |s| open_worst(s, 4)),
    ("logger", "Every toggle on", logger_every_toggle),
    ("logger", "Empty", |s| s.feed.log_mut().clear()),
    ("logger", "One", logger_one),
    ("logger", "100,000 rows", logger_huge),
];

fn logger_demo(s: &mut Logger) {
    s.feed.log_mut().set_packet_info(packet_info());
    s.feed.record(traffic());
}

fn worst_packet_info() -> Arc<PacketInfo> {
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
            entry(Direction::ToServer, 597, "Chat"),
            entry(
                Direction::ToClient,
                2690,
                "NavigatorSearchResultBlocksMessageEvent",
            ),
            entry(Direction::ToClient, 1301, "FloorHeightMap"),
            entry(Direction::ToClient, 1778, "Objects"),
            entry(Direction::ToClient, 2350, "Chat"),
            entry(
                Direction::ToClient,
                3400,
                "CatalogPageWithEarliestExpiryMessageEvent",
            ),
        ],
        true,
    ))
}

fn floor_plan() -> String {
    (0..64)
        .map(|y| {
            (0..64)
                .map(|x| if (x + y) % 17 == 0 { 'x' } else { '0' })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\r")
}

fn furni(count: i32) -> Packet {
    let mut packet = Packet::builder(1778)
        .i32(1)
        .i32(1)
        .string("Synthetic Owner")
        .i32(count);
    for id in 0..count {
        packet = packet
            .i32(100_000 + id)
            .i32(3897)
            .i32(id % 12)
            .i32(id / 12)
            .i32(2)
            .string("0.0")
            .string("1.0")
            .i32(0)
            .i32(0)
            .string("0")
            .i32(-1)
            .i32(1)
            .i32(1);
    }
    packet.build()
}

fn chat_to_client(index: u32, line: &str) -> LogEntry {
    LogEntry::Intercepted {
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
    }
}

fn logger_worst(s: &mut Logger) {
    s.feed.log_mut().clear();
    s.feed.log_mut().set_packet_info(worst_packet_info());
    let entries = vec![
        LogEntry::Intercepted {
            index: 0,
            direction: Direction::ToClient,
            packet: Packet::builder(3400)
                .i32(42)
                .string("catalog_frontpage_2026_black_friday_bundle_offer")
                .build(),
            outcome: Outcome::Forwarded,
        },
        LogEntry::Intercepted {
            index: 1,
            direction: Direction::ToClient,
            packet: Packet::builder(4093)
                .string_bytes(&[0, 1, 2, 255, 254, 0x80, b'{', b'}'])
                .build(),
            outcome: Outcome::Forwarded,
        },
        LogEntry::Intercepted {
            index: 2,
            direction: Direction::ToClient,
            packet: Packet::builder(1301)
                .bool(true)
                .i32(-1)
                .string(&floor_plan())
                .build(),
            outcome: Outcome::Forwarded,
        },
        LogEntry::Intercepted {
            index: 3,
            direction: Direction::ToClient,
            packet: furni(600),
            outcome: Outcome::Forwarded,
        },
        LogEntry::Intercepted {
            index: 4,
            direction: Direction::ToServer,
            packet: Packet::builder(597)
                .string_bytes("alguém quer trocar o trono de gelo por 3 dragões?".as_bytes())
                .i32(0)
                .i32(-1)
                .build(),
            outcome: Outcome::Edited {
                by: "Copiador de Roupas Automático para Quartos Lotados".into(),
                sent: Packet::builder(597)
                    .string(&"[spam] ".repeat(60))
                    .i32(0)
                    .i32(-1)
                    .build(),
            },
        },
        LogEntry::Intercepted {
            index: 5,
            direction: Direction::ToServer,
            packet: Packet::builder(597)
                .string("Benachrichtigungseinstellungen")
                .i32(0)
                .i32(-1)
                .build(),
            outcome: Outcome::Blocked {
                by: "G-Presets - Building & Wired Presets".into(),
            },
        },
        chat_to_client(6, "王秀英: 你好，有人想交易吗？"),
        chat_to_client(7, "نور الهدى: مرحبا بالجميع"),
        chat_to_client(8, "👩🏽‍💻 Priya: line one\nline two"),
        LogEntry::Intercepted {
            index: 9,
            direction: Direction::ToServer,
            packet: Packet::builder(2596).build(),
            outcome: Outcome::Forwarded,
        },
        LogEntry::Injected {
            direction: Direction::ToClient,
            packet: Packet::builder(2350)
                .i32(7)
                .string_bytes("🎉".repeat(200).as_bytes())
                .i32(0)
                .i32(0)
                .i32(-1)
                .build(),
            by: "Konstantin Oberhauser-Wettstein's Party Toolkit".into(),
        },
    ];
    s.feed.record(traffic());
    s.feed.record(entries);
}

fn open_worst(s: &mut Logger, row: usize) {
    logger_worst(s);
    let first = s.feed.log().len() - 11;
    let number = s.feed.log().number_at(first + row);
    if let Some(number) = number {
        s.open(number);
    }
}

fn logger_every_toggle(s: &mut Logger) {
    logger_huge(s);
    s.feed.toggle_pause();
    s.autoscroll = false;
    s.directions = crate::packet_log::Directions {
        incoming: true,
        outgoing: false,
    };
    s.filter = rune::TextEdit::new("chat");
    s.set_filter();
}

fn logger_one(s: &mut Logger) {
    s.feed.log_mut().clear();
    s.feed.log_mut().set_packet_info(packet_info());
    s.feed.record(vec![chat_to_client(0, "oi")]);
}

fn logger_huge(s: &mut Logger) {
    s.feed.log_mut().clear();
    s.feed.log_mut().set_packet_info(worst_packet_info());
    let entries = (0..100_000u32)
        .map(|i| chat_to_client(i, "olá, mundo"))
        .collect();
    s.feed.record(entries);
    s.feed.record(vec![LogEntry::Intercepted {
        index: 100_000,
        direction: Direction::ToClient,
        packet: Packet::builder(1301)
            .bool(true)
            .i32(-1)
            .string("x0")
            .build(),
        outcome: Outcome::Forwarded,
    }]);
}
