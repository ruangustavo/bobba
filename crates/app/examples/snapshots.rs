use std::path::PathBuf;
use std::sync::Arc;

use bobba::app::{AppTab, BobbaApp};
use bobba::backend::{Backend, UiEvent};
use bobba_protocol::{Direction, Packet, PacketInfo};
use bobba_session::{ConnectionInfo, LogEntry, Outcome, SessionEvent};
use gpui_kit::base::Root;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{AppContext, HeadlessAppContext, px, size};

const RELEASE: &str = "MAC63-202611041140-63291706";

fn main() -> anyhow_free::Result {
    let mut args = std::env::args().skip(1);
    let sulek_json = args
        .next()
        .ok_or("usage: snapshots <sulek.json> <out-dir>")?;
    let out_dir = PathBuf::from(
        args.next()
            .ok_or("usage: snapshots <sulek.json> <out-dir>")?,
    );
    let json = std::fs::read_to_string(sulek_json)?;
    let info = Arc::new(PacketInfo::from_sulek_json(RELEASE, &json)?);

    let text_system = Arc::new(gpui_apple::AppleTextSystem::new());
    let mut cx =
        HeadlessAppContext::with_platform(text_system, Arc::new(gpui_kit::assets::Assets), || {
            gpui_kit::platform::current_headless_renderer()
        });
    cx.update(gpui_kit::init);
    let (ui, events) = smol::channel::unbounded();
    let backend = Backend::spawn(ui);
    let mut app = None;
    let window = cx
        .open_window(size(px(1180.), px(760.)), |window, cx| {
            Theme::change(ThemeMode::Dark, Some(window), cx);
            let view = cx.new(|cx| BobbaApp::new(backend, events, window, cx));
            app = Some(view.clone());
            cx.new(|cx| Root::new(view, window, cx))
        })
        .map_err(|e| e.to_string())?;
    let app = app.ok_or("window did not build")?;

    let connection = ConnectionInfo {
        host: "game-br.habbo.com".into(),
        port: 30000,
        hotel_version: RELEASE.into(),
        client_identifier: "FLASH31".into(),
        client_type: "FLASH".into(),
    };
    cx.update_window(window.into(), |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle(
                UiEvent::Session(SessionEvent::Connected(connection)),
                window,
                cx,
            );
            app.handle(
                UiEvent::Host(bobba_extensions::HostEvent::PacketInfoLoaded(info.clone())),
                window,
                cx,
            );
            app.handle(UiEvent::Log(sample_traffic()), window, cx);
        });
    })
    .map_err(|e| e.to_string())?;
    cx.run_until_parked();

    std::fs::create_dir_all(&out_dir)?;
    for tab in [
        AppTab::Connection,
        AppTab::Logger,
        AppTab::Injection,
        AppTab::Extensions,
    ] {
        cx.update_window(window.into(), |_, _, cx| {
            app.update(cx, |app, cx| app.select_tab(tab, cx))
        })
        .map_err(|e| e.to_string())?;
        cx.run_until_parked();
        for _ in 0..40 {
            cx.advance_clock(std::time::Duration::from_millis(16));
            cx.run_until_parked();
            cx.capture_screenshot(window.into())
                .map_err(|e| e.to_string())?;
        }
        let image = cx
            .capture_screenshot(window.into())
            .map_err(|e| e.to_string())?;
        let path = out_dir.join(format!("{tab:?}.png").to_lowercase());
        image.save(&path)?;
        println!("{}", path.display());
    }
    Ok(())
}

fn chat(direction: Direction, index: u32, text: &str, outcome: Outcome) -> LogEntry {
    let packet = match direction {
        Direction::ToServer => Packet::builder(597)
            .string_bytes(text.as_bytes())
            .i32(0)
            .i32(-1)
            .build(),
        Direction::ToClient => Packet::builder(2350)
            .i32(7)
            .string_bytes(text.as_bytes())
            .i32(0)
            .i32(0)
            .i32(-1)
            .build(),
    };
    LogEntry::Intercepted {
        index,
        direction,
        packet,
        outcome,
    }
}

fn sample_traffic() -> Vec<LogEntry> {
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
    let lines = [
        "oi",
        "alguém aí?",
        "bobba",
        "olá, mundo",
        "\"aspas\" e [colchetes]",
    ];
    for (i, line) in lines.iter().cycle().take(24).enumerate() {
        let index = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let outcome = match i {
            5 => Outcome::Blocked {
                by: "AntiSpam".into(),
            },
            9 => Outcome::Edited {
                by: "Translator".into(),
                sent: Packet::builder(597)
                    .string("hello, world")
                    .i32(0)
                    .i32(-1)
                    .build(),
            },
            _ => Outcome::Forwarded,
        };
        entries.push(chat(Direction::ToServer, index, line, outcome));
        entries.push(chat(Direction::ToClient, index, line, Outcome::Forwarded));
    }
    entries.push(LogEntry::Injected {
        direction: Direction::ToServer,
        packet: Packet::builder(481).string("injected shout").i32(0).build(),
        by: "Injection".into(),
    });
    entries
}

mod anyhow_free {
    pub type Result = std::result::Result<(), Box<dyn std::error::Error>>;
}
