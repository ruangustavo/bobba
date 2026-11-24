use bobba_session::ConnectionInfo;
use rune::tok::TextStyle;
use rune::{El, Tx, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, IconButton, IconButtonVariant};

use crate::state::{App, KeyState, SessionField};
use crate::view::theme::{Paint, paint};

const TITLE: TextStyle = TextStyle::new(20.0, 650.0);

impl SessionField {
    fn key(self) -> &'static str {
        match self {
            SessionField::Hotel => "hotel",
            SessionField::Client => "client",
        }
    }

    fn label(self) -> &'static str {
        match self {
            SessionField::Hotel => "Hotel",
            SessionField::Client => "Client",
        }
    }
}

fn hotel_name(host: &str) -> String {
    let name = match host.split('.').next().unwrap_or(host) {
        "game-br" => "habbo.com.br",
        "game-us" => "habbo.com",
        "game-es" => "habbo.es",
        "game-de" => "habbo.de",
        "game-fi" => "habbo.fi",
        "game-fr" => "habbo.fr",
        "game-it" => "habbo.it",
        "game-nl" => "habbo.nl",
        "game-tr" => "habbo.com.tr",
        "game-s2" => "sandbox",
        _ => return host.to_lowercase(),
    };
    name.to_owned()
}

fn facts(info: &ConnectionInfo, key: KeyState) -> String {
    let kind = info.client_type.to_lowercase();
    let mut kind = kind.chars();
    let kind = kind.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(kind).collect()
    });
    let build = info
        .hotel_version
        .split('-')
        .next()
        .unwrap_or(&info.hotel_version);
    let key = match key {
        KeyState::Plain => "Not encrypted".to_owned(),
        KeyState::Decrypted { elapsed, .. } => {
            format!("Decrypted, key found in {} ms", elapsed.as_millis())
        }
    };
    format!("{kind} client {build} · {key}")
}

fn field(p: &Paint, s: &App, field: SessionField, value: String) -> El {
    let k = &p.kit;
    let key = field.key();
    let trailing = if s.copied == Some(field) {
        row()
            .key(format!("session-copied-{key}"))
            .h(24.0)
            .gap(6.0)
            .center_y()
            .child(icon(k.glyph.check, 14.0).color(p.green))
            .child(text("Copied").font(k.ty.small).color(p.green))
    } else {
        let copy = value.clone();
        IconButton::new(k.glyph.copy)
            .variant(IconButtonVariant::GhostMuted)
            .label(format!("Copy {}", field.label()))
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                tx.copy(copy.clone());
                s.copied = Some(field);
            })
            .build(k)
            .key(format!("session-copy-{key}"))
    };
    row()
        .key(format!("session-{key}"))
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .child(
            text(field.label())
                .font(k.ty.small)
                .color(k.c.text_muted)
                .w(72.0),
        )
        .child(
            text(value)
                .font(k.ty.mono)
                .color(k.c.text_base)
                .selectable(true)
                .truncate()
                .grow(1.0),
        )
        .child(trailing)
}

pub fn view(s: &App, info: &ConnectionInfo) -> El {
    let p = paint();
    let k = &p.kit;
    let header = row()
        .key("session-header")
        .w_full()
        .gap(10.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .gap(4.0)
                .child(
                    text(hotel_name(&info.host))
                        .key("session-title")
                        .font(TITLE)
                        .color(k.c.text_base),
                )
                .child(
                    text(facts(info, s.key))
                        .key("session-facts")
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .truncate(),
                ),
        )
        .child(
            Button::new("Disconnect")
                .variant(ButtonVariant::Danger)
                .on_press(App::abort)
                .build(k)
                .key("disconnect"),
        );
    let card = col()
        .key("session-card")
        .w_full()
        .radius(12.0)
        .fill(p.panel)
        .border(1.0, p.hairline)
        .child(field(
            p,
            s,
            SessionField::Hotel,
            format!("{}:{}", info.host, info.port),
        ))
        .child(el().w_full().h(1.0).fill(p.hairline))
        .child(field(
            p,
            s,
            SessionField::Client,
            info.hotel_version.clone(),
        ));
    col()
        .key("session")
        .w_full()
        .h_full()
        .padding(24.0)
        .gap(20.0)
        .child(header)
        .child(card)
}
