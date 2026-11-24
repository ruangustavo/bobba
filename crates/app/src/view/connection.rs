use rune::tok::TextStyle;
use rune::{Color, El, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Loader, TextInput};

use crate::state::{App, KeyState, Mode, Phase};
use crate::view::skull::skull;
use crate::view::theme::{Paint, paint};

const HEADLINE: TextStyle = TextStyle::new(22.0, 650.0);
const STEPS: usize = 4;

enum Badge {
    Skull(Color),
    Loading,
    Glyph(&'static rune::Icon, Color),
}

struct Hero {
    badge: Badge,
    tint: Option<Color>,
    progress: Option<usize>,
    title: String,
    body: String,
    actions: Vec<El>,
}

fn badge(p: &Paint, hero: &Hero) -> El {
    let k = &p.kit;
    let tint = hero.tint.unwrap_or(k.c.text_faint);
    let inner = match hero.badge {
        Badge::Skull(ink) => skull(4.0, ink),
        Badge::Loading => Loader::new().size(26.0).color(tint).build(k),
        Badge::Glyph(glyph, ink) => icon(glyph, 26.0).color(ink),
    };
    el().key("hero-badge")
        .size(76.0, 76.0)
        .radius(22.0)
        .center()
        .fill(if hero.tint.is_some() {
            tint.alpha(0.12)
        } else {
            p.panel
        })
        .border(
            1.0,
            if hero.tint.is_some() {
                tint.alpha(0.35)
            } else {
                p.hairline
            },
        )
        .child(inner)
}

fn progress(p: &Paint, at: usize) -> El {
    (0..STEPS).fold(row().key("hero-progress").gap(6.0), |bar, i| {
        let fill = if i < at {
            p.kit.c.text_base
        } else if i == at {
            p.amber
        } else {
            p.hairline
        };
        bar.child(
            el().key(format!("step-{i}"))
                .size(28.0, 4.0)
                .radius(2.0)
                .fill(fill),
        )
    })
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

fn fact(p: &Paint, key: &str, label: &str, value: String, ink: Color) -> El {
    col()
        .key(format!("fact-{key}"))
        .gap(3.0)
        .padding(12.0)
        .radius(12.0)
        .fill(p.panel)
        .border(1.0, p.hairline)
        .child(text(label).font(p.kit.ty.small).color(p.kit.c.text_muted))
        .child(text(value).font(p.kit.ty.ui_medium).color(ink).truncate())
}

fn session_facts(p: &Paint, s: &App) -> Option<El> {
    let Phase::Connected(info) = &s.phase else {
        return None;
    };
    let k = &p.kit;
    let names = s.packet_info.as_ref().map_or_else(
        || "Loading…".to_owned(),
        |info| {
            if info.is_empty() {
                "Header ids only".into()
            } else {
                format!("{} from Sulek", info.entries().len())
            }
        },
    );
    let (encryption, ink) = match s.key {
        KeyState::Plain => ("Waiting for the key".to_owned(), k.c.text_base),
        KeyState::Decrypted {
            candidates,
            elapsed,
        } => (
            format!("RC4 · {} ms · {candidates} cand.", elapsed.as_millis()),
            p.green,
        ),
    };
    let grid = col()
        .key("session-facts")
        .w(600.0)
        .gap(10.0)
        .child(
            row()
                .gap(10.0)
                .equal_columns(3)
                .child(fact(p, "hotel", "Hotel", info.host.clone(), k.c.text_base))
                .child(fact(
                    p,
                    "client",
                    "Client",
                    info.hotel_version.clone(),
                    k.c.text_base,
                ))
                .child(fact(p, "names", "Packet names", names, k.c.text_base)),
        )
        .child(
            row()
                .gap(10.0)
                .equal_columns(3)
                .child(fact(p, "encryption", "Encryption", encryption, ink))
                .child(fact(
                    p,
                    "traffic",
                    "Traffic",
                    format!("{} out · {} in", s.traffic.to_server, s.traffic.to_client),
                    k.c.text_base,
                ))
                .child(fact(
                    p,
                    "extensions",
                    "Extensions",
                    format!("{} connected", s.extensions.len()),
                    k.c.text_base,
                )),
        );
    Some(grid)
}

fn hero(p: &Paint, s: &App) -> Hero {
    let k = &p.kit;
    let automatic = s.mode == Mode::Automatic;
    match &s.phase {
        Phase::Idle if automatic => Hero {
            badge: Badge::Skull(k.c.text_base),
            tint: None,
            progress: None,
            title: "Not connected".into(),
            body: "Click Connect, then open Habbo from the Habbo Launcher.".into(),
            actions: vec![connect_button("Connect")],
        },
        Phase::Idle => Hero {
            badge: Badge::Skull(k.c.text_base),
            tint: None,
            progress: None,
            title: "Custom host".into(),
            body: "Point your client at the local port; Bobba forwards to the hotel.".into(),
            actions: vec![connect_button("Connect")],
        },
        Phase::Preparing(step) => Hero {
            badge: Badge::Loading,
            tint: Some(p.amber),
            progress: automatic.then_some(0),
            title: format!("{step}…"),
            body: "This takes a moment.".into(),
            actions: vec![abort_button()],
        },
        Phase::NeedsResign(app) => {
            let app = app.clone();
            Hero {
                badge: Badge::Glyph(k.glyph.lock, p.amber),
                tint: Some(p.amber),
                progress: Some(0),
                title: "Habbo.app needs a one-time re-sign".into(),
                body: "So G-MemZ can read Habbo's encryption key. The original is kept as Habbo.bobba-original.app.".into(),
                actions: vec![
                    button("Not now", ButtonVariant::Ghost, "resign-cancel", App::abort),
                    button("Re-sign Habbo.app", ButtonVariant::Contrast, "resign", move |s: &mut App| {
                        s.resign(app.clone());
                    }),
                ],
            }
        }
        Phase::Prepared(original) => Hero {
            badge: Badge::Glyph(k.glyph.check, p.green),
            tint: Some(p.green),
            progress: Some(1),
            title: "Habbo.app is ready".into(),
            body: format!("The original is kept at {}.", original.display()),
            actions: vec![connect_button("Connect")],
        },
        Phase::AwaitingApproval => Hero {
            badge: Badge::Glyph(k.glyph.lock, p.amber),
            tint: Some(p.amber),
            progress: Some(1),
            title: "Approve the administrator prompt".into(),
            body: "Bobba points Habbo's game servers at itself until your client connects, then removes the redirect.".into(),
            actions: vec![abort_button()],
        },
        Phase::WaitingForClient(hotels) => Hero {
            badge: Badge::Loading,
            tint: Some(p.amber),
            progress: Some(2),
            title: "Open Habbo from the launcher".into(),
            body: format!("Bobba is listening on {hotels} hotels. The redirect goes away as soon as Habbo connects."),
            actions: vec![abort_button()],
        },
        Phase::Listening(address) => Hero {
            badge: Badge::Loading,
            tint: Some(p.amber),
            progress: None,
            title: "Waiting for the client".into(),
            body: format!("Point your client at {address}."),
            actions: vec![abort_button()],
        },
        Phase::Connected(info) => Hero {
            badge: Badge::Skull(p.green),
            tint: Some(p.green),
            progress: None,
            title: format!("Connected to {}", info.host.trim_end_matches(".habbo.com")),
            body: String::new(),
            actions: vec![
                button("Open Logger", ButtonVariant::Outline, "open-logger", |s: &mut App| {
                    s.tab = crate::state::Tab::Logger;
                }),
                button("Disconnect", ButtonVariant::Danger, "disconnect", App::abort),
            ],
        },
        Phase::Ended { packets } => Hero {
            badge: Badge::Skull(k.c.text_muted),
            tint: None,
            progress: None,
            title: "Session ended".into(),
            body: format!("{packets} packets stay in the Logger until you clear it."),
            actions: vec![connect_button("Connect again")],
        },
        Phase::Failed(reason) => Hero {
            badge: Badge::Glyph(k.glyph.circle_x, p.red),
            tint: Some(p.red),
            progress: None,
            title: reason.clone(),
            body: "Check the step above and try again.".into(),
            actions: vec![connect_button("Retry")],
        },
    }
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
    let k = &p.kit;
    let hero = hero(p, s);
    let mut column = col()
        .key("hero")
        .gap(14.0)
        .center()
        .w(640.0)
        .child(badge(p, &hero));
    if let Some(at) = hero.progress {
        column = column.child(progress(p, at));
    }
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
    if let Some(facts) = session_facts(p, s) {
        column = column.child(el().h(4.0)).child(facts);
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
    if !s.phase.is_busy() && !matches!(s.phase, Phase::Connected(_)) {
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
