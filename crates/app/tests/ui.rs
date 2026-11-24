use std::collections::HashSet;

use bobba::backend::{
    AutomaticStep, CatalogEvent, InstalledKind, InstalledState, StoreInstall, UiEvent,
};
use bobba::logger::Logger;
use bobba::scenes::SCENES;
use bobba::setup::Step;
use bobba::state::{App, Phase, SendResult, Tab};
use bobba::store::{CatalogState, Showing};
use bobba::view::root;
use bobba_extensions::{ExtensionOutput, Permission, PermissionRequest, ProcessStatus};
use bobba_protocol::Direction;
use bobba_protocol::extension_message::ExtensionInfo;
use bobba_session::{KeyRecoveryFailure, SessionEvent, TablesUnavailable};
use rune::{Env, Key, KeyEvent, Mods, Runtime};

const SIZE: (f32, f32) = (1180.0, 760.0);

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

#[test]
fn the_store_opens_on_the_extensions_tab_and_asks_for_the_catalog() {
    let mut rt = runtime("extensions");
    assert!(shown(&rt, "install-from-file"));
    press(&mut rt, "extensions-showing/segment1");
    assert_eq!(rt.state.store.showing, Showing::Store);
    assert_eq!(rt.state.store.catalog, CatalogState::Loading);
    assert!(!shown(&rt, "install-from-file"));
}

#[test]
fn the_installed_and_store_switch_counts_each_side() {
    let rt = runtime("store");
    assert_eq!(text_of(&rt, "extensions-count-installed"), "4");
    assert_eq!(text_of(&rt, "extensions-count-store"), "6");
    assert!(!shows_text(&rt, "connect on port"));
}

#[test]
fn a_store_listing_opens_its_details_beside_the_list_and_escape_closes_them() {
    let mut rt = runtime("store");
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
    assert_eq!(rt.state.store.detail, None);
    assert!(!shown(&rt, "store-detail"));
}

#[test]
fn a_store_listing_shows_install_only_while_its_row_is_hovered() {
    let mut rt = runtime("store");
    let opacity = |rt: &Runtime<App>| {
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
    let mut rt = runtime("store");
    rt.update(|s, _| {
        s.handle(UiEvent::StoreInstall {
            title: "Trade Ledger".into(),
            progress: StoreInstall::Downloading,
        })
    });
    settle(&mut rt);
    assert!(shows_text(&rt, "Installing"));

    rt.update(|s, _| {
        s.handle(UiEvent::StoreInstall {
            title: "Trade Ledger".into(),
            progress: StoreInstall::Failed("404 Not Found".into()),
        })
    });
    settle(&mut rt);
    assert!(shows_text(&rt, "404 Not Found"));
}

#[test]
fn an_unreachable_store_says_installed_extensions_keep_working() {
    let mut rt = runtime("store");
    rt.update(|s, _| {
        s.handle(UiEvent::Catalog(CatalogEvent::Unreachable(
            "offline".into(),
        )))
    });
    settle(&mut rt);
    assert!(shows_text(&rt, "keep working"));
    press(&mut rt, "store-retry");
    assert_eq!(rt.state.store.catalog, CatalogState::Loading);
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
    assert!(shown(&rt, "installed-start-presets_1.2.13"));
}

#[test]
fn the_store_follows_the_runtimes_the_backend_finds() {
    let mut rt = runtime("store");
    assert!(shows_text(&rt, "Get Node.js"));
    rt.update(|s, _| s.handle(UiEvent::MissingRuntimes(HashSet::new())));
    settle(&mut rt);
    assert!(!shows_text(&rt, "Get Node.js"));
    assert!(!shows_text(&rt, "Needs Node.js"));
}

#[test]
fn an_outdated_store_extension_offers_its_update_in_the_installed_list() {
    let mut rt = runtime("store");
    press(&mut rt, "extensions-showing/segment0");
    assert!(shown(&rt, "installed-update-Wired Helper_1.3.0"));
}

#[test]
fn with_no_extensions_yet_the_list_points_to_the_store() {
    let mut rt = runtime("extensions");
    rt.update(|s, _| {
        s.installed.clear();
        s.extensions.clear();
    });
    settle(&mut rt);
    assert!(shown(&rt, "extensions-empty-gearth"));
    assert!(
        !shown(&rt, "gearth-import-choose"),
        "the empty list offers the G-Earth import once"
    );
    press(&mut rt, "extensions-empty-browse");
    assert_eq!(rt.state.store.showing, Showing::Store);
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
    assert!(shown(&rt, "gearth-import-choose"));
    gearth_found(&mut rt);
    assert!(shows_text(&rt, "Found 4 extensions in G-Earth"));
    assert!(
        !shown(&rt, "gearth-import-choose"),
        "the banner is the only G-Earth import while it is open"
    );

    press(&mut rt, "gearth-import-dismiss");
    assert!(!shown(&rt, "gearth-import"));
    assert!(shown(&rt, "gearth-import-choose"));
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
