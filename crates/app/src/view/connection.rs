use bobba_session::ConnectionInfo;
use rune::tok::TextStyle;
use rune::{Color, El, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Loader, TextInput};

use crate::state::{App, Mode, Phase};
use crate::view::skull::skull;
use crate::view::theme::{Paint, paint};
use crate::view::{session, setup};

const HEADLINE: TextStyle = TextStyle::new(22.0, 650.0);

enum Badge {
    Skull(Color),
    Loading,
    Glyph(&'static rune::Icon, Color),
}

struct Hero {
    badge: Badge,
    title: String,
    body: String,
    actions: Vec<El>,
}

fn badge(p: &Paint, kind: &Badge) -> El {
    let inner = match *kind {
        Badge::Skull(ink) => skull(5.0, ink),
        Badge::Loading => Loader::new().size(32.0).color(p.amber).build(&p.kit),
        Badge::Glyph(glyph, ink) => icon(glyph, 36.0).color(ink),
    };
    el().key("hero-badge")
        .size(56.0, 56.0)
        .center()
        .child(inner)
}

fn button(label: &str, variant: ButtonVariant, key: &str, on: impl Fn(&mut App) + 'static) -> El {
    Button::new(label)
        .variant(variant)
        .on_press(on)
        .build(&paint().kit)
        .key(key.to_owned())
}

fn connect_button(label: &str) -> El {
    button(label, ButtonVariant::Contrast, "connect", App::connect)
}

fn abort_button() -> El {
    button("Abort", ButtonVariant::Danger, "abort", App::abort)
}

enum Screen<'a> {
    Hero(Hero),
    Setup,
    Session(&'a ConnectionInfo),
}

fn screen<'a>(p: &Paint, s: &'a App) -> Screen<'a> {
    let k = &p.kit;
    let automatic = s.mode == Mode::Automatic;
    Screen::Hero(match &s.phase {
        Phase::Connected(info) => return Screen::Session(info),
        Phase::Ended { packets } => Hero {
            badge: Badge::Skull(k.c.text_muted),
            title: "Session ended".into(),
            body: format!("{packets} packets stay in the Logger until you clear it."),
            actions: vec![connect_button("Connect again")],
        },
        _ if automatic => return Screen::Setup,
        Phase::Idle => Hero {
            badge: Badge::Skull(k.c.text_base),
            title: "Custom host".into(),
            body: "Point your client at the local port; Bobba forwards to the hotel.".into(),
            actions: vec![connect_button("Connect")],
        },
        Phase::Preparing(step) => Hero {
            badge: Badge::Loading,
            title: format!("{step}…"),
            body: "This takes a moment.".into(),
            actions: vec![abort_button()],
        },
        Phase::Listening(address) => Hero {
            badge: Badge::Loading,
            title: "Waiting for the client".into(),
            body: format!("Point your client at {address}."),
            actions: vec![abort_button()],
        },
        Phase::Failed(reason) => Hero {
            badge: Badge::Glyph(k.glyph.circle_x, p.red),
            title: reason.clone(),
            body: "Check the step above and try again.".into(),
            actions: vec![connect_button("Retry")],
        },
        Phase::Working(_)
        | Phase::NeedsResign(_)
        | Phase::Prepared(_)
        | Phase::AwaitingApproval
        | Phase::WaitingForClient(_)
        | Phase::SetupFailed { .. } => return Screen::Setup,
    })
}

fn manual_fields(s: &App) -> El {
    let k = &paint().kit;
    let field = |label: &str, key: &str, input: El| {
        col()
            .key(format!("field-{key}"))
            .gap(6.0)
            .child(text(label).font(k.ty.small).color(k.c.text_muted))
            .child(input)
    };
    row()
        .key("manual-fields")
        .gap(10.0)
        .child(field(
            "Hotel host",
            "host",
            TextInput::new(&s.hotel_host)
                .label("Hotel host")
                .width(Some(260.0))
                .on_edit(|s: &mut App| &mut s.hotel_host)
                .on_submit(|s: &mut App, _| s.connect())
                .build(k),
        ))
        .child(field(
            "Port",
            "port",
            TextInput::new(&s.hotel_port)
                .label("Hotel port")
                .width(Some(90.0))
                .on_edit(|s: &mut App| &mut s.hotel_port)
                .build(k),
        ))
        .child(field(
            "Listen on 127.0.0.1",
            "listen",
            TextInput::new(&s.listen_port)
                .label("Listen port")
                .width(Some(120.0))
                .on_edit(|s: &mut App| &mut s.listen_port)
                .build(k),
        ))
}

pub fn view(s: &App) -> El {
    let p = paint();
    match screen(p, s) {
        Screen::Session(info) => session::view(s, info),
        Screen::Setup => setup::view(s),
        Screen::Hero(hero) => self::hero(p, s, hero),
    }
}

fn hero(p: &Paint, s: &App, hero: Hero) -> El {
    let k = &p.kit;
    let mut column = col()
        .key("hero")
        .gap(14.0)
        .center()
        .w(640.0)
        .child(badge(p, &hero.badge));
    column = column.child(
        text(hero.title.clone())
            .key("hero-title")
            .font(HEADLINE)
            .color(k.c.text_base),
    );
    if !hero.body.is_empty() {
        column = column.child(
            text(hero.body.clone())
                .key("hero-body")
                .font(k.ty.body)
                .color(k.c.text_muted)
                .center(),
        );
    }
    if s.mode == Mode::Manual
        && matches!(
            s.phase,
            Phase::Idle | Phase::Failed(_) | Phase::Ended { .. }
        )
    {
        column = column.child(manual_fields(s));
    }
    column = column.child(el().h(4.0)).child(
        hero.actions
            .into_iter()
            .fold(row().gap(10.0).key("hero-actions"), El::child),
    );
    if !s.phase.is_busy() {
        let (label, next) = match s.mode {
            Mode::Automatic => ("Use a custom host and port", Mode::Manual),
            Mode::Manual => ("Back to the Habbo AIR client", Mode::Automatic),
        };
        column = column.child(el().h(6.0)).child(
            Button::new(label)
                .variant(ButtonVariant::GhostMuted)
                .on_press(move |s: &mut App| {
                    s.mode = next;
                    s.phase = Phase::Idle;
                })
                .build(k)
                .key("switch-mode"),
        );
    }
    col()
        .key("connection")
        .w_full()
        .h_full()
        .center()
        .child(column)
}
