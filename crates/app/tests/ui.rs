use bobba::backend::UiEvent;
use bobba::scenes::SCENES;
use bobba::state::{App, Phase, SendResult, Tab};
use bobba::view::root;
use bobba_extensions::{Permission, PermissionRequest};
use bobba_protocol::extension_message::ExtensionInfo;
use rune::{Env, Runtime};

const SIZE: (f32, f32) = (1180.0, 760.0);

fn runtime(scene: &str) -> Runtime<App> {
    let mut app = App::new();
    if let Some((_, apply)) = SCENES.iter().find(|(name, _)| *name == scene) {
        apply(&mut app);
    }
    let mut rt = Runtime::new(app, root, SIZE, Env::new(true, 2.0));
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<App>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

fn press(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key} is on screen"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

#[test]
fn connect_starts_the_automatic_flow_and_offers_abort() {
    let mut rt = runtime("idle");
    press(&mut rt, "connect");
    assert_eq!(rt.state.phase, Phase::Preparing("Starting".into()));
    assert!(rt.find("abort").is_some());
}

#[test]
fn switching_to_manual_shows_the_host_fields() {
    let mut rt = runtime("idle");
    assert!(rt.find("manual-fields").is_none());
    press(&mut rt, "switch-mode");
    assert!(rt.find("manual-fields").is_some());
}

#[test]
fn use_in_injection_fills_the_editor_and_switches_tab() {
    let mut rt = runtime("logger-selected");
    let expected = rt
        .state
        .selected_row()
        .map(|row| row.number)
        .expect("a row is selected");
    assert!(rt.find("logger-detail").is_some());
    press(&mut rt, "logger-use");
    assert_eq!(rt.state.tab, Tab::Injection);
    assert!(
        rt.state.injection.text.starts_with("{out:Chat}"),
        "row {expected} became an expression"
    );
}

#[test]
fn injection_without_a_hotel_explains_why_nothing_was_sent() {
    let mut rt = runtime("idle");
    rt.update(|s, _| {
        s.tab = Tab::Injection;
        s.injection.replace_text("{h:597}{s:\"oi\"}");
    });
    settle(&mut rt);
    press(&mut rt, "send-to-server");
    assert_eq!(
        rt.state.last_send,
        Some(SendResult::Refused("not connected to a hotel".into()))
    );
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
fn connected_session_shows_its_facts() {
    let rt = runtime("connected");
    for key in [
        "fact-hotel",
        "fact-client",
        "fact-names",
        "fact-encryption",
        "fact-traffic",
        "fact-extensions",
    ] {
        assert!(rt.find(key).is_some(), "{key} is shown");
    }
}

#[test]
fn events_sent_through_the_sink_wake_the_window_and_reach_the_screen() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bobba::backend::UiSink;
    use bobba_protocol::{Direction, Packet};
    use bobba_session::{LogEntry, Outcome};

    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let (sink, events) = UiSink::new(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    let mut app = App::new();
    app.listen(events);
    app.tab = Tab::Logger;
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
    assert_eq!(rt.state.log.len(), 1);
    assert_eq!(rt.state.traffic.to_server, 1);
    assert!(rt.find("logger-count").is_some());
}
