use std::collections::HashSet;

use bobba::backend::{
    AutomaticStep, CatalogEvent, InstalledKind, InstalledState, StoreInstall, UiEvent,
};
use bobba::log_feed::LogFeed;
use bobba::logger::Logger;
use bobba::scenes::{BREAKS, LOGGER_BREAKS, LOGGER_SCENES, SCENES};
use bobba::setup::Step;
use bobba::state::{App, Phase, SendResult, Tab};
use bobba::store::CatalogState;
use bobba::store_feed::StoreRequest;
use bobba::store_hint::StoreHint;
use bobba::store_window::StoreWindow;
use bobba::view::root;
use bobba_extensions::{ExtensionOutput, HostEvent, Permission, PermissionRequest, ProcessStatus};
use bobba_protocol::Direction;
use bobba_protocol::extension_message::ExtensionInfo;
use bobba_session::{KeyRecoveryFailure, SessionEvent, TablesUnavailable};
use rune::{Env, Key, KeyEvent, Mods, Runtime};

const SIZE: (f32, f32) = (1180.0, 760.0);
const STORE_SIZE: (f32, f32) = (720.0, 520.0);
const STORE_MIN_SIZE: (f32, f32) = (640.0, 440.0);

fn runtime(scene: &str) -> Runtime<App> {
    let mut app = App::new();
    if let Some((_, apply)) = SCENES.iter().find(|(name, _)| *name == scene) {
        apply(&mut app);
    }
    let mut rt = Runtime::new(app, root, SIZE, Env::new(true, 2.0));
    rt.on_key = Some(bobba::on_key);
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn logger_beside(app: &App) -> Runtime<Logger> {
    let mut rt = Runtime::new(
        Logger::new(app.feed.clone()),
        bobba::view::logger::root,
        SIZE,
        Env::new(true, 2.0),
    );
    rt.on_key = Some(bobba::logger::on_key);
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn settle<S: 'static>(rt: &mut Runtime<S>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

fn press<S: 'static>(rt: &mut Runtime<S>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key} is on screen"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn shown<S: 'static>(rt: &Runtime<S>, key: &str) -> bool {
    rt.elements()
        .into_iter()
        .any(|e| e.key.as_deref() == Some(key) && !e.exiting)
}

fn text_of<S: 'static>(rt: &Runtime<S>, key: &str) -> String {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
        .and_then(|e| e.text)
        .unwrap_or_else(|| panic!("{key} shows text"))
}

#[test]
fn setup_says_the_password_is_asked_on_every_connect() {
    let rt = runtime("approval");
    let subtitle = text_of(&rt, "setup-subtitle");
    assert!(subtitle.contains("every time"), "{subtitle}");
}

#[test]
fn a_failed_setup_step_names_what_failed() {
    let mut rt = runtime("resign");
    rt.update(|s, _| {
        s.handle(UiEvent::Automatic(AutomaticStep::Failed(
            "codesign failed".into(),
        )))
    });
    settle(&mut rt);
    assert_eq!(
        text_of(&rt, "setup-said"),
        "Couldn't re-sign Habbo.app: codesign failed."
    );
}

#[test]
fn a_session_that_cannot_read_habbo_says_why() {
    let mut rt = runtime("idle");
    rt.update(|s, _| {
        s.handle(UiEvent::Session(SessionEvent::KeyRecoveryFailed(
            KeyRecoveryFailure::TablesUnavailable(TablesUnavailable("Habbo isn't running".into())),
        )))
    });
    settle(&mut rt);
    assert_eq!(
        text_of(&rt, "setup-said"),
        "The session with Habbo failed: Couldn't read Habbo's memory: Habbo isn't running."
    );
}

#[test]
fn connect_starts_the_automatic_flow_and_offers_abort() {
    let mut rt = runtime("idle");
    press(&mut rt, "connect");
    assert_eq!(rt.state.phase, Phase::Working(Step::Prepare));
    assert!(rt.find("abort").is_some());
}

#[test]
fn first_run_lists_every_setup_step_before_connecting() {
    let rt = runtime("idle");
    for step in 0..3 {
        assert!(
            rt.find(&format!("setup-step-{step}")).is_some(),
            "step {step} is listed"
        );
    }
    assert!(rt.find("setup-step-3").is_none());
    assert!(rt.find("connect").is_some());
}

#[test]
fn the_step_waiting_on_you_opens_with_its_action() {
    let mut rt = runtime("resign");
    assert!(rt.find("setup-open-0").is_some());
    press(&mut rt, "resign");
    assert_eq!(rt.state.phase, Phase::Working(Step::Prepare));
}

#[test]
fn setup_markers_hold_their_place_as_their_step_changes() {
    let idle = runtime("idle");
    let resign = runtime("resign");
    for step in 0..1 {
        let key = format!("setup-marker-{step}");
        assert_eq!(idle.find(&key), resign.find(&key), "{key} stays put");
    }
}

#[test]
fn a_failed_setup_step_offers_to_try_again() {
    let mut rt = runtime("resign");
    rt.update(|s, _| {
        s.handle(UiEvent::Automatic(AutomaticStep::Failed(
            "codesign failed".into(),
        )))
    });
    settle(&mut rt);
    assert_eq!(
        rt.state.phase,
        Phase::SetupFailed {
            at: Step::Prepare,
            reason: "codesign failed".into()
        }
    );
    press(&mut rt, "connect");
    assert_eq!(rt.state.phase, Phase::Working(Step::Prepare));
}

#[test]
fn switching_to_manual_shows_the_host_fields() {
    let mut rt = runtime("idle");
    assert!(rt.find("manual-fields").is_none());
    press(&mut rt, "switch-mode");
    assert!(rt.find("manual-fields").is_some());
}

#[test]
fn use_in_injection_in_the_logger_window_fills_the_main_editor() {
    let mut main = runtime("connected");
    let mut logger = logger_beside(&main.state);
    let number = main
        .state
        .feed
        .log()
        .number_at(26)
        .expect("traffic is logged");
    logger.update(move |s, _| s.open(number));
    settle(&mut logger);
    assert!(shown(&logger, "logger-sheet"));
    press(&mut logger, "logger-use");
    main.update(|s, _| {
        s.take_logger_requests();
    });
    assert_eq!(main.state.tab, Tab::Injection);
    assert!(
        main.state.injection.text.starts_with("{out:Chat}"),
        "row {number} became an expression"
    );
}

#[test]
fn pressing_a_packet_opens_its_sheet_and_escape_closes_it() {
    let main = runtime("connected");
    let mut logger = logger_beside(&main.state);
    logger.update(|s, _| {
        s.filter.replace_text("edited");
        s.set_filter();
    });
    settle(&mut logger);
    assert!(!shown(&logger, "logger-sheet"));

    press(&mut logger, "line-26");
    assert_eq!(text_of(&logger, "sheet-name"), "Chat");
    assert_eq!(text_of(&logger, "structure-type-0"), "string");
    assert!(text_of(&logger, "sheet-sent").starts_with(r#"{out:Chat}{s:"hello, world"}"#));

    logger.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    settle(&mut logger);
    assert!(!shown(&logger, "logger-sheet"));
}

#[test]
fn hiding_outgoing_from_the_view_menu_leaves_only_incoming_packets() {
    let main = runtime("connected");
    let mut logger = logger_beside(&main.state);
    let total = logger.state.feed.log().len();
    press(&mut logger, "logger-view");
    press(&mut logger, "logger-view-item2");
    let log = logger.state.feed.log();
    assert!(log.len() < total);
    assert!((0..log.len()).all(|i| {
        log.row(i)
            .is_some_and(|row| row.direction() == Direction::ToClient)
    }));
    drop(log);
    assert!(
        logger
            .elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Incoming only") && !e.exiting),
        "the View button names what is hidden"
    );
}

#[test]
fn the_logger_is_not_a_tab_and_opens_only_from_extensions() {
    assert!(!Tab::ALL.iter().any(|tab| tab.label() == "Logger"));
    assert!(runtime("extensions").find("ext-logger-open").is_some());
    assert!(runtime("connected").find("open-logger").is_none());
}

#[test]
fn injection_without_a_hotel_explains_why_nothing_was_sent() {
    let mut rt = runtime("idle");
    rt.update(|s, _| {
        s.tab = Tab::Injection;
        s.injection.replace_text("{h:597}{s:\"oi\"}");
    });
    settle(&mut rt);
    press(&mut rt, "send");
    assert_eq!(
        rt.state.last_send,
        Some(SendResult::Refused("not connected to a hotel".into()))
    );
}

#[test]
fn an_empty_or_broken_expression_cannot_be_sent() {
    let mut rt = runtime("connected");
    rt.update(|s, _| s.tab = Tab::Injection);
    settle(&mut rt);
    press(&mut rt, "send");
    assert_eq!(rt.state.last_send, None, "an empty editor sends nothing");

    rt.update(|s, _| s.injection.replace_text("{out:Chat}{s:\"oi"));
    settle(&mut rt);
    press(&mut rt, "send");
    assert_eq!(
        rt.state.last_send, None,
        "a broken expression sends nothing"
    );
    assert!(text_of(&rt, "injection-status").contains("unterminated string"));
}

fn shows_text<S: 'static>(rt: &Runtime<S>, wanted: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| !e.exiting && e.text.as_deref().is_some_and(|t| t.contains(wanted)))
}

#[test]
fn repeating_a_packet_lists_it_and_says_why_it_stopped_without_a_hotel() {
    let mut rt = runtime("injection");
    assert!(
        shows_text(&rt, "Send to server"),
        "the expression names the side"
    );
    assert!(rt.find("raw-direction").is_none());

    press(&mut rt, "send-mode/segment1");
    press(&mut rt, "repeat-longer");
    assert_eq!(text_of(&rt, "repeat-every"), "750 ms");
    assert!(shows_text(&rt, "Repeat to server every"));

    press(&mut rt, "send");
    assert_eq!(rt.state.repeats.len(), 1);
    assert!(rt.find("repeat-1").is_some());
    assert!(shows_text(&rt, "not connected to a hotel"));

    rt.update(|s, _| s.injection.replace_text("{h:597}{s:\"oi\"}{i:0}{i:-1}"));
    settle(&mut rt);
    assert!(
        rt.find("raw-direction").is_some(),
        "a raw packet asks where to go"
    );
}

fn height_of<S: 'static>(rt: &Runtime<S>, key: &str) -> f32 {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
        .map(|e| e.rect[3])
        .unwrap_or_else(|| panic!("{key} is on screen"))
}

#[test]
fn the_send_controls_share_one_height() {
    let mut rt = runtime("injection");
    press(&mut rt, "send-mode/segment1");
    rt.update(|s, _| s.injection.replace_text("{h:597}{s:\"oi\"}{i:0}{i:-1}"));
    settle(&mut rt);
    let mode = height_of(&rt, "send-mode");
    for key in ["raw-direction", "send", "repeat-shorter", "repeat-longer"] {
        assert_eq!(height_of(&rt, key), mode, "{key} matches Once/Repeat");
    }
}

#[test]
fn recent_packets_open_on_demand_and_refill_the_editor() {
    let mut rt = runtime("injection");
    assert!(rt.find("recent-menu").is_none(), "recent stays closed");
    press(&mut rt, "recent-trigger");
    press(&mut rt, "recent-item1");
    assert_eq!(rt.state.injection.text, "{out:MoveAvatar}{i:12}{i:8}");
    assert!(rt.find("recent-menu").is_none(), "picking closes it");
}

#[test]
fn recent_is_hidden_until_something_was_sent() {
    let mut rt = runtime("idle");
    rt.update(|s, _| s.tab = Tab::Injection);
    settle(&mut rt);
    assert!(rt.find("recent-trigger").is_none());
}

#[test]
fn permission_banner_answers_the_extension() {
    let mut rt = runtime("idle");
    let (respond, mut answer) = tokio::sync::oneshot::channel();
    let request = UiEvent::Permission(PermissionRequest {
        info: ExtensionInfo {
            title: "AntiSpam".into(),
            author: "ruan".into(),
            version: "1".into(),
            description: String::new(),
            on_click_used: false,
            file: None,
            cookie: None,
            can_leave: true,
            can_delete: true,
        },
        respond,
    });
    rt.update(move |s, _| s.handle(request));
    settle(&mut rt);
    assert!(rt.find("permission-banner").is_some());
    press(&mut rt, "permission-allow");
    assert_eq!(answer.try_recv(), Ok(Permission::Allow));
    assert!(rt.find("permission-banner").is_none());
}

#[test]
fn connected_session_lists_hotel_and_client() {
    let rt = runtime("connected");
    for key in ["session-hotel", "session-client", "disconnect"] {
        assert!(rt.find(key).is_some(), "{key} is shown");
    }
    for key in ["hero-badge", "relay-client", "fact-traffic", "open-logger"] {
        assert!(rt.find(key).is_none(), "{key} is gone");
    }
}

#[test]
fn connected_session_is_titled_by_the_hotel_name_with_facts_instead_of_a_tagline() {
    let rt = runtime("connected");
    let tree = rt.accessibility_json().to_string();
    assert!(
        tree.contains("\"habbo.com.br\""),
        "the hotel is named in lowercase"
    );
    assert!(tree.contains("Flash client MAC63 · Decrypted, key found in 2 ms"));
    assert!(!tree.contains("Bobba sits between"));
}

#[test]
fn copying_a_session_value_confirms_it() {
    let mut rt = runtime("connected");
    press(&mut rt, "session-copy-client");
    assert!(rt.find("session-copied-client").is_some());
    assert!(rt.find("session-copy-client").is_none());
    assert!(rt.find("session-copy-hotel").is_some());
}

#[test]
fn events_sent_through_the_sink_wake_the_window_and_reach_the_screen() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bobba::backend::UiSink;
    use bobba_protocol::Packet;
    use bobba_session::{LogEntry, Outcome};

    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let (sink, events) = UiSink::new(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    let mut app = App::new();
    app.listen(events);
    let mut rt = Runtime::new(app, root, SIZE, Env::new(true, 2.0));
    rt.poll = Some(bobba::poll);
    rt.frame(0.0);

    let chat = Packet::builder(597).string("oi").i32(0).i32(-1).build();
    sink.emit(UiEvent::Log(vec![LogEntry::Intercepted {
        index: 0,
        direction: Direction::ToServer,
        packet: chat,
        outcome: Outcome::Forwarded,
    }]));
    settle(&mut rt);

    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert_eq!(rt.state.feed.log().len(), 1);
    assert_eq!(rt.state.traffic.to_server, 1);
    assert!(logger_beside(&rt.state).find("logger-count").is_some());
}

#[test]
fn extensions_tab_lists_extensions_without_their_output() {
    let rt = runtime("extensions");
    assert!(rt.find("extension-list").is_some());
    assert!(rt.find("extension-divider-0").is_some());
    assert!(rt.find("extension-console").is_none());
}

fn store_window_at(app: &App, size: (f32, f32)) -> Runtime<StoreWindow> {
    let mut rt = Runtime::new(
        app.store_window_state(),
        bobba::view::store::root,
        size,
        Env::new(true, 2.0),
    );
    rt.on_key = Some(bobba::store_window::on_key);
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn store_window(app: &App) -> Runtime<StoreWindow> {
    store_window_at(app, STORE_SIZE)
}

fn deliver(store: &mut Runtime<StoreWindow>) {
    store.update(|s, tx| {
        bobba::store_window::poll(s, 0.0, tx);
    });
    settle(store);
}

#[test]
fn the_store_is_a_built_in_extension_that_asks_for_the_catalog() {
    let mut rt = runtime("extensions");
    assert!(shown(&rt, "ext-store-open"));
    assert!(!shown(&rt, "extensions-showing"));
    press(&mut rt, "ext-store-open");
    assert_eq!(rt.state.store.catalog, CatalogState::Loading);
    assert!(shown(&rt, "install-from-file"));
}

#[test]
fn a_store_listing_opens_its_details_beside_the_list_and_escape_closes_them() {
    let mut rt = store_window(&runtime("store").state);
    assert!(shown(&rt, "store-row-Trade Ledger"));
    assert!(!shown(&rt, "store-detail"));

    press(&mut rt, "store-row-Trade Ledger");
    assert_eq!(text_of(&rt, "store-detail-title"), "Trade Ledger");
    assert!(
        shown(&rt, "store-action-Room Mapper"),
        "rows keep their actions while the details are open"
    );

    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert_eq!(rt.state.detail, None);
    assert!(!shown(&rt, "store-detail"));
}

#[test]
fn a_store_listing_shows_install_only_while_its_row_is_hovered() {
    let mut rt = store_window(&runtime("store").state);
    let opacity = |rt: &Runtime<StoreWindow>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("store-action-Trade Ledger") && !e.exiting)
            .map(|e| e.opacity)
            .unwrap_or_else(|| panic!("store-action-Trade Ledger is on screen"))
    };
    assert_eq!(opacity(&rt), 0.0);

    let (x, y) = rt.find("store-row-Trade Ledger").expect("row is on screen");
    rt.pointer_move(x, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(opacity(&rt), 1.0);

    rt.pointer_move(0.0, 0.0);
    rt.frame(1.0 / 60.0);
    assert_eq!(opacity(&rt), 0.0);
}

#[test]
fn a_store_listing_shows_its_download_and_its_failure() {
    let mut app = runtime("store");
    let mut store = store_window(&app.state);
    app.update(|s, _| {
        s.handle(UiEvent::StoreInstall {
            title: "Trade Ledger".into(),
            progress: StoreInstall::Downloading,
        })
    });
    deliver(&mut store);
    assert!(shows_text(&store, "Installing"));

    app.update(|s, _| {
        s.handle(UiEvent::StoreInstall {
            title: "Trade Ledger".into(),
            progress: StoreInstall::Failed("404 Not Found".into()),
        })
    });
    deliver(&mut store);
    assert!(shows_text(&store, "404 Not Found"));
}

#[test]
fn installing_from_the_store_window_asks_the_main_window() {
    let app = runtime("store");
    let mut store = store_window(&app.state);
    press(&mut store, "store-action-Wired Helper");
    assert_eq!(
        app.state.store_feed.take_requests(),
        [StoreRequest::Install("Wired Helper".into())]
    );
}

#[test]
fn an_unreachable_store_says_installed_extensions_keep_working() {
    let mut app = runtime("store");
    let mut store = store_window(&app.state);
    app.update(|s, _| {
        s.handle(UiEvent::Catalog(CatalogEvent::Unreachable(
            "offline".into(),
        )))
    });
    deliver(&mut store);
    assert!(shows_text(&store, "keep working"));
    press(&mut store, "store-retry");
    assert_eq!(store.state.store.catalog, CatalogState::Loading);
    app.update(|s, _| {
        s.take_store_requests();
    });
    assert_eq!(app.state.store.catalog, CatalogState::Loading);
}

#[test]
fn a_connected_extension_with_a_window_offers_to_open_it() {
    let mut rt = runtime("extensions");
    assert!(!shown(&rt, "installed-open-antispam-48120.py"));
    rt.update(|s, _| {
        s.extensions.clear();
        s.handle(UiEvent::Host(HostEvent::ExtensionConnected {
            id: 7,
            info: ExtensionInfo {
                title: "AntiSpam".into(),
                author: "ruan".into(),
                version: "1.0".into(),
                description: String::new(),
                on_click_used: true,
                file: Some("antispam-48120.py".into()),
                cookie: None,
                can_leave: true,
                can_delete: true,
            },
        }));
    });
    settle(&mut rt);
    assert!(shown(&rt, "installed-open-antispam-48120.py"));
    press(&mut rt, "installed-open-antispam-48120.py");
}

#[test]
fn an_extension_row_offers_the_two_actions_of_its_state() {
    let mut rt = runtime("extensions");
    assert!(shown(&rt, "installed-close-antispam-48120.py"));
    assert!(!shown(&rt, "installed-uninstall-antispam-48120.py"));
    assert!(shown(&rt, "installed-restart-wired-helper-90314.jar"));
    assert!(!shown(&rt, "installed-close-wired-helper-90314.jar"));

    press(&mut rt, "installed-uninstall-wired-helper-90314.jar");
    assert!(shown(&rt, "installed-confirm-wired-helper-90314.jar"));
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert!(!shown(&rt, "installed-confirm-wired-helper-90314.jar"));
    assert!(shown(&rt, "installed-uninstall-wired-helper-90314.jar"));
}

#[test]
fn an_available_update_leads_to_the_listing_in_the_store() {
    let mut app = runtime("store");
    press(&mut app, "installed-update-Wired Helper_1.3.0");
    let mut store = store_window(&app.state);
    deliver(&mut store);
    assert_eq!(store.state.detail.as_deref(), Some("Wired Helper"));
    assert!(shown(&store, "store-detail"));
}

#[test]
fn an_extension_that_quits_on_its_own_says_why_and_can_start_again() {
    let mut rt = runtime("extensions");
    rt.update(|s, _| {
        s.handle(UiEvent::ExtensionOutput(ExtensionOutput {
            file_name: "presets_1.2.13".into(),
            line: "Unable to locate a Java Runtime.".into(),
        }));
        s.handle(UiEvent::Installed(vec![InstalledState {
            id: "presets_1.2.13".into(),
            kind: InstalledKind::Package {
                title: "Presets".into(),
                version: "1.2.13".into(),
            },
            status: ProcessStatus::Exited { code: Some(1) },
        }]));
    });
    settle(&mut rt);
    assert!(shows_text(&rt, "exited · Unable to locate a Java Runtime."));
    assert!(!shows_text(&rt, "starting"));
    assert!(shown(&rt, "installed-restart-presets_1.2.13"));
}

#[test]
fn the_store_follows_the_runtimes_the_backend_finds() {
    let mut app = runtime("store");
    let mut store = store_window(&app.state);
    assert!(shows_text(&store, "Get Node.js"));
    app.update(|s, _| s.handle(UiEvent::MissingRuntimes(HashSet::new())));
    deliver(&mut store);
    assert!(!shows_text(&store, "Get Node.js"));
    assert!(!shows_text(&store, "Needs Node.js"));
}

#[test]
fn an_outdated_store_extension_offers_its_update_in_the_installed_list() {
    let rt = runtime("store");
    assert!(shown(&rt, "installed-update-Wired Helper_1.3.0"));
}

#[test]
fn with_no_extensions_yet_a_hint_points_to_the_store_and_to_g_earth() {
    let mut rt = runtime("extensions-empty");
    assert!(shown(&rt, "store-hint-gearth"));
    press(&mut rt, "store-hint-browse");
    assert_eq!(rt.state.store.catalog, CatalogState::Loading);
}

#[test]
fn the_store_hint_stays_closed_once_dismissed() {
    let mut rt = runtime("extensions-empty");
    press(&mut rt, "store-hint-dismiss");
    assert!(!shown(&rt, "store-hint"));
}

#[test]
fn a_store_hint_dismissed_in_an_earlier_run_never_shows() {
    let mut rt = runtime("extensions-empty");
    rt.update(|s, _| s.handle(UiEvent::StoreHint(StoreHint::Dismissed)));
    settle(&mut rt);
    assert!(!shown(&rt, "store-hint"));
}

#[test]
fn the_store_hint_waits_while_g_earth_extensions_are_offered() {
    let mut rt = runtime("extensions-empty");
    gearth_found(&mut rt);
    assert!(shown(&rt, "gearth-import"));
    assert!(!shown(&rt, "store-hint"));
}

fn gearth_found(rt: &mut Runtime<App>) {
    rt.update(|s, _| {
        s.handle(UiEvent::GEarthFound {
            folder: std::path::PathBuf::from("/Users/player/Downloads/G-Earth"),
            count: 4,
        })
    });
    settle(rt);
}

#[test]
fn extensions_found_in_gearth_are_offered_until_dismissed() {
    let mut rt = runtime("extensions");
    assert!(!shown(&rt, "gearth-import"));
    gearth_found(&mut rt);
    assert!(shows_text(&rt, "Found 4 extensions in G-Earth"));

    press(&mut rt, "gearth-import-dismiss");
    assert!(!shown(&rt, "gearth-import"));
}

#[test]
fn importing_from_gearth_says_it_is_working_then_how_many_came() {
    let mut rt = runtime("extensions");
    gearth_found(&mut rt);
    press(&mut rt, "gearth-import-accept");
    assert!(shows_text(&rt, "Importing"));

    rt.update(|s, _| s.handle(UiEvent::GEarthImported(Ok(4))));
    settle(&mut rt);
    assert!(!shown(&rt, "gearth-import"));
    assert!(
        rt.state
            .console
            .iter()
            .any(|line| line.contains("Imported 4"))
    );
}

fn store_break(case: &str) -> Runtime<StoreWindow> {
    store_break_at(case, STORE_SIZE)
}

fn store_break_at(case: &str, size: (f32, f32)) -> Runtime<StoreWindow> {
    let mut app = App::new();
    if let Some((_, scene)) = SCENES.iter().find(|(name, _)| *name == "store") {
        scene(&mut app);
    }
    let Some((_, _, apply)) = BREAKS.iter().find(|(_, name, _)| *name == case) else {
        panic!("{case} is a store break");
    };
    let mut window = app.store_window_state();
    apply(&mut window);
    let mut rt = Runtime::new(window, bobba::view::store::root, size, Env::new(true, 2.0));
    rt.on_key = Some(bobba::store_window::on_key);
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn texts_in<S: 'static>(rt: &Runtime<S>, key: &str) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.path.contains(key))
        .filter_map(|e| e.text)
        .collect()
}

fn helps<S: 'static>(rt: &Runtime<S>, wanted: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| !e.exiting && e.help.as_deref().is_some_and(|h| h.contains(wanted)))
}

#[test]
fn a_multi_line_description_keeps_its_store_row_to_one_line() {
    for case in ["Worst case", "Worst details"] {
        let rt = store_break(case);
        let height = height_of(&rt, "store-row-G-Presets - Building & Wired Presets");
        assert!(height < 80.0, "{case}: the row is {height} tall");
        assert!(shows_text(
            &rt,
            "Save any stack of furni and wired as a preset and place it again in any room."
        ));
    }
}

#[test]
fn store_tiles_take_initials_from_the_letters_of_the_title() {
    let rt = store_break("Worst case");
    for (row, initials) in [
        ("store-row-G-Presets - Building & Wired Presets", "GB"),
        ("store-row-[AIO] Gamba Suite", "AG"),
        ("store-row-autoclick", "A"),
        ("store-row-🦊 Fox Follower", "FF"),
    ] {
        let texts = texts_in(&rt, row);
        assert!(texts.iter().any(|t| t == initials), "{row}: {texts:?}");
    }
}

#[test]
fn a_store_row_cut_short_shows_the_rest_on_hover() {
    let rt = store_break_at("Worst details", STORE_MIN_SIZE);
    assert!(helps(&rt, "extension.zip"), "the failure reason");
    assert!(
        helps(&rt, "Konfiguration-und-Installation"),
        "the description"
    );
}

#[test]
fn store_details_leave_out_what_the_listing_does_not_say() {
    let rt = store_break("Worst details");
    assert!(!shows_text(&rt, "Updated"));
    let fox = texts_in(&rt, "store-row-🦊 Fox Follower");
    assert!(
        !fox.iter().any(|t| t == "0" || t.is_empty()),
        "an unrated listing without a description: {fox:?}"
    );
}

#[test]
fn store_details_show_the_whole_title_and_category() {
    let rt = store_break("Worst details");
    assert!(
        height_of(&rt, "store-detail-title") > 24.0,
        "the title wraps"
    );
    assert!(helps(&rt, "Building, Usability, G-Earth Utilities"));
}

#[test]
fn a_thousand_store_listings_build_only_the_rows_on_screen() {
    let rt = store_break("1,000 rows");
    assert!(shown(&rt, "store-row-Extension 999"));
    assert!(!shown(&rt, "store-row-Extension 0"));
}

#[test]
fn an_empty_store_does_not_blame_a_category() {
    let rt = store_break("Empty");
    assert!(shows_text(
        &rt,
        "No extension in the store runs on this computer yet."
    ));
    assert!(!shows_text(&rt, "this category"));
}

fn rect_of<S: 'static>(rt: &Runtime<S>, key: &str) -> [f32; 4] {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
        .map(|e| e.rect)
        .unwrap_or_else(|| panic!("{key} is on screen"))
}

#[test]
fn store_details_keep_the_title_and_actions_on_one_line_above_the_description() {
    let mut rt = store_window(&runtime("store").state);
    press(&mut rt, "store-row-Trade Ledger");
    let title = rect_of(&rt, "store-detail-title");
    let close = rect_of(&rt, "store-detail-close");
    let action = rect_of(&rt, "store-detail-action");
    for (name, rect) in [("close", close), ("action", action)] {
        assert!(
            rect[1] < title[1] + title[3] && title[1] < rect[1] + rect[3],
            "{name} sits beside the title: {rect:?} vs {title:?}"
        );
    }
    let panel = rect_of(&rt, "store-detail");
    let description = rt
        .elements()
        .into_iter()
        .find(|e| {
            !e.exiting
                && e.text.as_deref()
                    == Some("Logs each trade you confirm, values both sides with recent marketplace averages, and warns before you accept a lopsided offer.")
                && e.path.contains("store-detail")
        })
        .map(|e| e.rect)
        .unwrap_or_else(|| panic!("the description is in the details"));
    let head = description[1] - panel[1];
    assert!(head < 80.0, "the description starts {head} below the top");
    assert!(
        !texts_in(&rt, "store-detail").iter().any(|t| t == "TL"),
        "the details leave the tile to the selected row"
    );
}

#[test]
fn store_details_without_authors_or_description_leave_no_gaps() {
    let rt = store_break("Details: no authors");
    let texts = texts_in(&rt, "store-detail");
    assert!(
        !texts.iter().any(|t| t.trim() == "by"),
        "no byline without authors: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.is_empty()),
        "no empty description: {texts:?}"
    );
}

#[test]
fn a_store_title_of_one_long_word_uses_both_lines() {
    let rt = store_break("Details: one long word");
    let height = height_of(&rt, "store-detail-title");
    assert!(height > 24.0, "the title is {height} tall");
}

fn lines_of_text<S: 'static>(rt: &Runtime<S>, starts: &str) -> f32 {
    rt.elements()
        .into_iter()
        .find(|e| {
            !e.exiting
                && e.path.contains("store-detail")
                && e.text.as_deref().is_some_and(|t| t.starts_with(starts))
        })
        .and_then(|e| e.text_style.map(|style| e.rect[3] / style.line_height))
        .unwrap_or_else(|| panic!("a text starting with {starts:?} is on screen"))
}

#[test]
fn store_details_wrap_links_inside_the_panel() {
    for (case, starts) in [
        ("Details: one long word", "Docs: https://"),
        (
            "Details: long title failed",
            "Could not install: error sending",
        ),
    ] {
        let rt = store_break(case);
        let lines = lines_of_text(&rt, starts);
        assert!(
            lines >= 4.0,
            "{case}: {starts:?} takes {lines} lines, so its link runs past the panel"
        );
    }
}

#[test]
fn a_store_listing_shows_its_badge_once_it_arrives() {
    let mut app = runtime("store");
    let mut store = store_window(&app.state);
    assert!(
        texts_in(&store, "store-row-Trade Ledger")
            .iter()
            .any(|t| t == "TL")
    );
    assert!(!shown(&store, "store-icon-Trade Ledger"));

    app.update(|s, _| {
        s.handle(UiEvent::StoreIcon {
            title: "Trade Ledger".into(),
            icon: bobba_store::Icon {
                rgba: vec![255; (bobba_store::ICON_SIDE * bobba_store::ICON_SIDE * 4) as usize],
            },
        })
    });
    deliver(&mut store);

    assert!(shown(&store, "store-icon-Trade Ledger"));
    assert!(
        !texts_in(&store, "store-row-Trade Ledger")
            .iter()
            .any(|t| t == "TL")
    );
    assert!(
        texts_in(&store, "store-row-Room Mapper")
            .iter()
            .any(|t| t == "RM")
    );
}

#[test]
fn store_categories_are_tabs_under_the_search() {
    let mut store = store_window(&runtime("store").state);
    assert!(!shown(&store, "store-chips"));
    assert!(shown(&store, "store-category-All"));
    assert!(shown(&store, "store-row-Room Mapper"));

    press(&mut store, "store-category-Trading");
    assert_eq!(store.state.category.as_deref(), Some("Trading"));
    assert!(shown(&store, "store-row-Trade Ledger"));
    assert!(!shown(&store, "store-row-Room Mapper"));

    let search = rect_of(&store, "store-search");
    let tab = rect_of(&store, "store-category-Trading");
    assert!(
        tab[1] >= search[1] + search[3],
        "the tabs sit under the search"
    );
}

const LOGGER_SIZE: (f32, f32) = (840.0, 580.0);
const LOGGER_MIN_SIZE: (f32, f32) = (720.0, 420.0);

fn logger_break_at(case: &str, size: (f32, f32)) -> Runtime<Logger> {
    let mut logger = Logger::new(LogFeed::new());
    for (_, scene) in LOGGER_SCENES {
        scene(&mut logger);
    }
    if case != "Demo data" {
        let Some((_, _, apply)) = LOGGER_BREAKS.iter().find(|(_, name, _)| *name == case) else {
            panic!("{case} is not in LOGGER_BREAKS");
        };
        apply(&mut logger);
    }
    let mut rt = Runtime::new(logger, bobba::view::logger::root, size, Env::new(true, 2.0));
    rt.on_key = Some(bobba::logger::on_key);
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn inside(inner: [f32; 4], outer: [f32; 4]) -> bool {
    inner[0] >= outer[0] - 0.5
        && inner[1] >= outer[1] - 0.5
        && inner[0] + inner[2] <= outer[0] + outer[2] + 0.5
        && inner[1] + inner[3] <= outer[1] + outer[3] + 0.5
}

fn fits_in_its_parent<S: 'static>(rt: &Runtime<S>, wanted: &str) -> bool {
    let all = rt.elements();
    let matches: Vec<_> = all
        .iter()
        .filter(|e| !e.exiting && e.text.as_deref().is_some_and(|t| t.contains(wanted)))
        .collect();
    !matches.is_empty()
        && matches.iter().all(|e| {
            e.parent
                .and_then(|p| all.get(p))
                .is_some_and(|parent| inside(e.rect, parent.rect))
        })
}

#[test]
fn a_huge_packet_keeps_the_sheet_actions_on_screen() {
    for case in ["Open: floor plan", "Open: 600 furni"] {
        let rt = logger_break_at(case, LOGGER_SIZE);
        let window = [0.0, 0.0, LOGGER_SIZE.0, LOGGER_SIZE.1];
        for key in ["logger-use", "logger-copy"] {
            assert!(inside(rect_of(&rt, key), window), "{case}: {key}");
        }
    }
}

#[test]
fn a_huge_packet_draws_only_what_fits_and_says_how_much_is_left() {
    let rt = logger_break_at("Open: 600 furni", LOGGER_SIZE);
    let elements = rt.elements();
    assert!(elements.len() < 2_000, "{} elements", elements.len());
    let longest = elements
        .iter()
        .filter_map(|e| e.text.as_ref().map(String::len))
        .max()
        .unwrap_or_default();
    assert!(longest < 5_000, "a text of {longest} bytes");
    assert!(shows_text(&rt, "7,612 more fields"));
}

#[test]
fn a_huge_packet_goes_to_injection_whole() {
    let mut rt = logger_break_at("Open: 600 furni", LOGGER_SIZE);
    press(&mut rt, "logger-use");
    let sent = rt.state.feed.take_injection().unwrap_or_default();
    assert_eq!(sent.len(), 51_112);
}

#[test]
fn every_view_toggle_keeps_the_count_and_clear_in_the_window() {
    let rt = logger_break_at("Every toggle on", LOGGER_MIN_SIZE);
    let window = [0.0, 0.0, LOGGER_MIN_SIZE.0, LOGGER_MIN_SIZE.1];
    for key in ["logger-count", "logger-clear"] {
        assert!(inside(rect_of(&rt, key), window), "{key}");
    }
    assert_eq!(text_of(&rt, "logger-count"), "99,999 of 100,000");
}

#[test]
fn one_packet_is_counted_in_the_singular() {
    let rt = logger_break_at("One", LOGGER_SIZE);
    assert_eq!(text_of(&rt, "logger-count"), "1 packet");
}

#[test]
fn a_long_extension_name_in_a_status_stays_in_its_row_and_shows_on_hover() {
    let status = "Copiador de Roupas Automático para Quartos Lotados";
    for (case, size) in [
        ("Worst case", LOGGER_SIZE),
        ("Open: floor plan", LOGGER_MIN_SIZE),
        ("Open: edited, long title", LOGGER_SIZE),
    ] {
        let rt = logger_break_at(case, size);
        assert!(fits_in_its_parent(&rt, status), "{case}");
        assert!(helps(&rt, status), "{case}");
    }
    let rt = logger_break_at("Worst case", LOGGER_SIZE);
    assert!(shows_text(&rt, "{out:Chat}{s:\"alguém quer trocar"));
}

#[test]
fn a_line_break_in_a_string_keeps_its_packet_on_one_line() {
    let rt = logger_break_at("Worst case", LOGGER_SIZE);
    assert!(shows_text(&rt, "Priya: line one"));
    assert!(!rt.elements().iter().any(|e| {
        e.text
            .as_deref()
            .is_some_and(|t| t.contains("line one") && t.contains('\n'))
    }));
}

#[test]
fn strings_from_the_client_read_as_utf8() {
    let rt = logger_break_at("Worst case", LOGGER_SIZE);
    assert!(shows_text(&rt, "王秀英: 你好"));
    assert!(shows_text(&rt, "نور الهدى"));
    let rt = logger_break_at("Demo data", LOGGER_SIZE);
    assert!(shows_text(&rt, "alguém aí?"));
}

#[test]
fn an_empty_log_says_whether_nothing_came_or_the_filter_hides_it() {
    let rt = logger_break_at("Empty", LOGGER_SIZE);
    assert!(shows_text(&rt, "No packets yet"));
    let mut rt = logger_break_at("Every toggle on", LOGGER_SIZE);
    rt.update(|s, _| {
        s.filter = rune::TextEdit::new("zzz");
        s.set_filter();
    });
    settle(&mut rt);
    assert!(shows_text(&rt, "No packets match"));
}

#[test]
fn a_long_view_summary_stays_clear_of_the_count() {
    let rt = logger_break_at("Every toggle on", LOGGER_MIN_SIZE);
    let summary = rt
        .elements()
        .into_iter()
        .find(|e| {
            !e.exiting && e.text.as_deref() == Some("Paused · Incoming only · Autoscroll off")
        })
        .map(|e| e.visible)
        .unwrap_or_else(|| panic!("the summary is on screen"));
    let count = rect_of(&rt, "logger-count");
    assert!(
        summary[0] + summary[2] <= count[0],
        "{summary:?} runs into {count:?}"
    );
    assert!(helps(&rt, "Paused · Incoming only · Autoscroll off"));
}

#[test]
fn the_structure_reads_strings_as_utf8() {
    let rt = logger_break_at("Open: edited, long title", LOGGER_SIZE);
    let fields = texts_in(&rt, "sheet-structure");
    assert!(
        fields.iter().any(|t| t.contains("alguém quer trocar")),
        "{fields:?}"
    );
}
