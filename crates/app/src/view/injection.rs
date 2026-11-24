use bobba_protocol::Direction;
use rune::tok::TextStyle;
use rune::{El, col, el, row, text};
use rune_kit::{Button, ButtonVariant, TextArea};

use crate::state::{App, SendResult};
use crate::view::theme::paint;

pub fn view(s: &App) -> El {
    let p = paint();
    let k = &p.kit;
    let names = s
        .packet_info
        .as_ref()
        .filter(|info| !info.is_empty())
        .map_or_else(
            || {
                "Names like {out:Chat} resolve once a hotel is connected; until then use {h:597}."
                    .to_owned()
            },
            |info| {
                format!(
                    "{} packet names loaded for {}.",
                    info.entries().len(),
                    info.hotel_version()
                )
            },
        );
    let result = match &s.last_send {
        None => el(),
        Some(SendResult::Sent(Direction::ToServer)) => {
            text("Sent to the server").font(k.ty.ui).color(p.green)
        }
        Some(SendResult::Sent(Direction::ToClient)) => {
            text("Sent to the client").font(k.ty.ui).color(p.green)
        }
        Some(SendResult::Refused(reason)) => text(reason.clone())
            .font(k.ty.ui)
            .color(p.red)
            .wrap_anywhere(),
    };
    let editor = TextArea::new(&s.injection)
        .label("Packet expression")
        .placeholder("{out:Chat}{s:\"hello\"}{i:0}{i:-1}")
        .width(None)
        .rows(8)
        .wrap_anywhere()
        .on_edit(|s: &mut App| &mut s.injection)
        .on_submit(|s: &mut App, _| s.send_injection(Direction::ToServer))
        .build(&p.mono_kit)
        .key("injection-editor");
    col().key("injection").w_full().h_full().center().child(
        col()
            .w(720.0)
            .gap(14.0)
            .child(
                text("Send a packet")
                    .font(TextStyle::new(20.0, 650.0))
                    .color(k.c.text_base),
            )
            .child(text(names).font(k.ty.body).color(k.c.text_muted))
            .child(editor)
            .child(
                row()
                    .gap(10.0)
                    .center_y()
                    .child(
                        Button::new("Send to server")
                            .variant(ButtonVariant::Contrast)
                            .shortcut("⌘↩")
                            .on_press(|s: &mut App| s.send_injection(Direction::ToServer))
                            .build(k)
                            .key("send-to-server"),
                    )
                    .child(
                        Button::new("Send to client")
                            .variant(ButtonVariant::Outline)
                            .on_press(|s: &mut App| s.send_injection(Direction::ToClient))
                            .build(k)
                            .key("send-to-client"),
                    )
                    .child(el().w(6.0))
                    .child(result.key("injection-status").grow(1.0)),
            ),
    )
}
