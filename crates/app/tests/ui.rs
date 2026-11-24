use bobba::app::{AppTab, BobbaApp, ConnectionStatus, Traffic};
use bobba::backend::{Backend, UiEvent};
use bobba::injection::SendResult;
use bobba::logger::LoggerEvent;
use bobba_extensions::{Permission, PermissionRequest};
use bobba_protocol::extension_message::ExtensionInfo;
use bobba_protocol::{Direction, Packet};
use bobba_session::{ConnectionInfo, LogEntry, Outcome, SessionEvent};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Bounds, Entity, Point, TestAppContext, WindowBounds, WindowHandle, WindowOptions,
    base::Root, px, size,
};

fn open_app(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<BobbaApp>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        let (ui, events) = smol::channel::unbounded();
        let backend = Backend::spawn(ui);
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1180.), px(760.)),
        };
        let (window, app) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| BobbaApp::new(backend, events, window, cx)),
        )
        .expect("window opens");
        (window.downcast::<Root>().expect("root window"), app)
    })
}

fn send_events(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    app: &Entity<BobbaApp>,
    events: Vec<UiEvent>,
) {
    cx.update_window(window.into(), |_, window, cx| {
        app.update(cx, |app, cx| {
            for event in events {
                app.handle(event, window, cx);
            }
        });
    })
    .expect("window alive");
}

fn connected() -> UiEvent {
    UiEvent::Session(SessionEvent::Connected(ConnectionInfo {
        host: "127.0.0.1".into(),
        port: 30001,
        hotel_version: "MAC63-1".into(),
        client_identifier: "FLASH31".into(),
        client_type: "FLASH".into(),
    }))
}

fn chat(direction: Direction, outcome: Outcome) -> LogEntry {
    LogEntry::Intercepted {
        index: 0,
        direction,
        packet: Packet::builder(597).string("oi").i32(0).i32(-1).build(),
        outcome,
    }
}

#[gpui_kit::test]
fn traffic_reaches_the_logger_and_the_status_bar(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    send_events(
        cx,
        window,
        &app,
        vec![
            connected(),
            UiEvent::Log(vec![
                chat(Direction::ToServer, Outcome::Forwarded),
                chat(
                    Direction::ToServer,
                    Outcome::Blocked {
                        by: "AntiSpam".into(),
                    },
                ),
                chat(Direction::ToClient, Outcome::Forwarded),
            ]),
        ],
    );
    cx.update(|cx| {
        let app = app.read(cx);
        assert!(matches!(app.status(), ConnectionStatus::Connected(_)));
        assert_eq!(
            app.traffic(),
            Traffic {
                to_server: 2,
                to_client: 1
            }
        );
        assert_eq!(app.logger().read(cx).row_count(cx), 3);
    });
    cx.update_window(window.into(), |_, window, cx| {
        app.update(cx, |app, cx| app.select_tab(AppTab::Logger, cx));
        window.render_frame(cx);
        assert!(window.find("logger-filter").visible());
        assert!(window.find("traffic-out").visible());
    })
    .expect("window alive");
}

#[gpui_kit::test]
fn injection_without_a_hotel_explains_why_nothing_was_sent(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    cx.update_window(window.into(), |_, window, cx| {
        let injection = app.read(cx).injection().clone();
        injection.update(cx, |panel, cx| {
            panel.load("{h:597}{s:\"oi\"}".into(), window, cx)
        });
        app.update(cx, |app, cx| app.select_tab(AppTab::Injection, cx));
        window.render_frame(cx);
        window.click("send-to-server", cx);
        assert_eq!(
            injection.read(cx).last_result(),
            Some(&SendResult::Refused("not connected to a hotel".into()))
        );
    })
    .expect("window alive");
}

#[gpui_kit::test]
fn use_in_injection_switches_tab_and_fills_the_editor(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    cx.update_window(window.into(), |_, _, cx| {
        let logger = app.read(cx).logger().clone();
        logger.update(cx, |_, cx| {
            cx.emit(LoggerEvent::UseInInjection {
                expression: "{out:Chat}{s:\"oi\"}".into(),
                direction: Direction::ToServer,
            });
        });
    })
    .expect("window alive");
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(app.read(cx).tab(), AppTab::Injection));
}

#[gpui_kit::test]
fn permission_banner_asks_and_answers_the_extension(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    let (respond, mut answer) = tokio::sync::oneshot::channel();
    let request = PermissionRequest {
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
    };
    send_events(cx, window, &app, vec![UiEvent::Permission(request)]);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("permission-banner").visible());
        window.click("permission-allow", cx);
        window.render_frame(cx);
        assert!(window.try_find("permission-banner").is_none());
    })
    .expect("window alive");
    assert_eq!(answer.try_recv(), Ok(Permission::Allow));
}

#[gpui_kit::test]
fn automatic_mode_offers_to_prepare_the_client_and_manual_mode_shows_fields(
    cx: &mut TestAppContext,
) {
    let (window, app) = open_app(cx);
    send_events(
        cx,
        window,
        &app,
        vec![UiEvent::Automatic(
            bobba::backend::AutomaticStep::NeedsPreparation("/Applications/Habbo.app".into()),
        )],
    );
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("prepare-client").visible());
        assert!(window.find("automatic-step").visible());
        assert!(window.try_find("hotel-host").is_none());
        window.click("mode-manual", cx);
        window.render_frame(cx);
        assert!(window.find("hotel-host").visible());
    })
    .expect("window alive");
}
