use bobba_protocol::Direction;
use rune::{Align, El, Icon, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Menu, MenuItem, Placement,
    SegmentedControl, TextArea,
};

use crate::recent::SentPacket;
use crate::repeat::{Repeat, State};
use crate::state::{App, Draft, SendMode, SendResult, Target};
use crate::view::theme::{Paint, paint};

static PAUSE: Icon = Icon {
    name: "bobba-pause",
    view: 16.0,
    body: r#"<rect x="4" y="3" width="2.75" height="10" rx="0.8" fill="currentColor"/><rect x="9.25" y="3" width="2.75" height="10" rx="0.8" fill="currentColor"/>"#,
};

static PLAY: Icon = Icon {
    name: "bobba-play-small",
    view: 16.0,
    body: r#"<path d="M4.75 3.5L12.55 8L4.75 12.5Z" fill="currentColor" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"/>"#,
};

fn direction(p: &Paint, direction: Direction) -> El {
    let (label, ink) = match direction {
        Direction::ToServer => ("↑ out", p.blue),
        Direction::ToClient => ("↓ in", p.red),
    };
    text(label).font(p.kit.ty.mono).color(ink).w(44.0)
}

fn item(p: &Paint, packet: &SentPacket) -> MenuItem {
    let expression = packet.expression.clone();
    MenuItem::new(packet.label.clone())
        .lead(direction(p, packet.direction))
        .description(packet.expression.clone())
        .on_select(move |s: &mut App| s.use_recent(expression.clone()))
}

fn recent(p: &Paint, s: &App, layer: &mut Vec<El>) -> El {
    if s.recent.is_empty() {
        return el();
    }
    let trigger = Button::new(format!("Recent · {}", s.recent.len()))
        .variant(ButtonVariant::GhostMuted)
        .expanded(s.recent_open)
        .on_press(|s: &mut App| s.recent_open = !s.recent_open)
        .build(&p.kit)
        .key("recent-trigger");
    s.recent
        .iter()
        .fold(
            Menu::new("recent")
                .trigger(trigger)
                .open(s.recent_open)
                .placement(Placement::BottomEnd)
                .min_width(380.0)
                .on_close(|s: &mut App| s.recent_open = false),
            |menu, packet| menu.item(item(p, packet)),
        )
        .build(&p.kit, layer)
}

fn side(direction: Direction) -> &'static str {
    match direction {
        Direction::ToServer => "server",
        Direction::ToClient => "client",
    }
}

fn arrow(p: &Paint, direction: Direction) -> El {
    let (glyph, ink) = match direction {
        Direction::ToServer => ("↑", p.blue),
        Direction::ToClient => ("↓", p.red),
    };
    text(glyph).w(10.0).font(p.kit.ty.ui_medium).color(ink)
}

fn rearm(s: &App, tx: &mut Tx) {
    if let Some(due) = s.repeats.next_due() {
        tx.wake_at(due);
    }
}

fn submit(s: &mut App, tx: &mut Tx) {
    match s.send_mode {
        SendMode::Once => s.send_injection(),
        SendMode::Repeat => {
            s.start_repeat(tx.clock());
            rearm(s, tx);
        }
    }
}

fn status(p: &Paint, s: &App) -> El {
    let k = &p.kit;
    let line = match (s.draft(), &s.last_send) {
        (Draft::Broken(reason), _) => text(reason).color(p.red),
        (Draft::Ready, Some(SendResult::Sent(Direction::ToServer))) => {
            text("Sent to the server").color(p.green)
        }
        (Draft::Ready, Some(SendResult::Sent(Direction::ToClient))) => {
            text("Sent to the client").color(p.green)
        }
        (Draft::Ready, Some(SendResult::Refused(reason))) => text(reason.clone()).color(p.red),
        _ => return el().key("injection-status").h(16.0),
    };
    line.key("injection-status")
        .font(k.ty.small)
        .truncate()
        .w_full()
}

fn interval(p: &Paint, s: &App) -> El {
    let k = &p.kit;
    row()
        .key("repeat-interval")
        .gap(2.0)
        .center_y()
        .child(
            Button::new("−")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Large)
                .disabled(s.every.shorter().is_none())
                .on_press(|s: &mut App| s.every = s.every.shorter().unwrap_or(s.every))
                .build(k)
                .key("repeat-shorter"),
        )
        .child(
            text(s.every.label())
                .key("repeat-every")
                .w(52.0)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .align(Align::Center),
        )
        .child(
            Button::new("+")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Large)
                .disabled(s.every.longer().is_none())
                .on_press(|s: &mut App| s.every = s.every.longer().unwrap_or(s.every))
                .build(k)
                .key("repeat-longer"),
        )
}

fn raw_direction(p: &Paint, s: &App) -> Option<El> {
    let Some(Target::Raw(direction)) = s.target() else {
        return None;
    };
    let picked = match direction {
        Direction::ToServer => 0,
        Direction::ToClient => 1,
    };
    Some(
        SegmentedControl::new(&["↑ Server", "↓ Client"], Some(picked))
            .label("Send raw packet to")
            .fit()
            .on_pick(|s: &mut App, i, _| {
                s.raw_direction = if i == 0 {
                    Direction::ToServer
                } else {
                    Direction::ToClient
                };
            })
            .build(&p.kit)
            .key("raw-direction"),
    )
}

fn actions(p: &Paint, s: &App, layer: &mut Vec<El>) -> El {
    let k = &p.kit;
    let target = s.target();
    let repeat = s.send_mode == SendMode::Repeat;
    let label = match (target, repeat) {
        (None, false) => "Send".to_owned(),
        (None, true) => "Repeat".to_owned(),
        (Some(target), false) => format!("Send to {}", side(target.direction())),
        (Some(target), true) => format!("Repeat to {} every", side(target.direction())),
    };
    let mode = SegmentedControl::new(&["Once", "Repeat"], Some(usize::from(repeat)))
        .label("How often")
        .fit()
        .on_pick(|s: &mut App, i, _| {
            s.send_mode = if i == 1 {
                SendMode::Repeat
            } else {
                SendMode::Once
            };
        })
        .build(k)
        .key("send-mode");
    row()
        .key("injection-actions")
        .w_full()
        .gap(10.0)
        .center_y()
        .child(mode)
        .children(raw_direction(p, s))
        .child(
            Button::new(label)
                .variant(ButtonVariant::Contrast)
                .control_size(ControlSize::Large)
                .shortcut("cmd+enter")
                .disabled(target.is_none())
                .on_press_tx(submit)
                .build(k)
                .key("send"),
        )
        .children(repeat.then(|| interval(p, s)))
        .child(el().grow(1.0))
        .child(recent(p, s, layer))
}

fn repeat_row(p: &Paint, repeat: &Repeat) -> El {
    let k = &p.kit;
    let id = repeat.id;
    let running = repeat.is_running();
    let (state, ink) = match &repeat.state {
        State::Running { .. } => (format!("every {}", repeat.every.label()), k.c.text_muted),
        State::Paused => ("paused".to_owned(), k.c.text_muted),
        State::Stopped(reason) => (reason.clone(), p.red),
    };
    row()
        .key(format!("repeat-{id}"))
        .w_full()
        .h(36.0)
        .px(6.0)
        .gap(10.0)
        .radius(8.0)
        .center_y()
        .hover(k.alpha(k.c.text_base, 0.04))
        .child(
            IconButton::new(if running { &PAUSE } else { &PLAY })
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label(if running { "Pause" } else { "Resume" })
                .on_press_tx(move |s: &mut App, tx| {
                    s.toggle_repeat(id, tx.clock());
                    rearm(s, tx);
                })
                .build(k)
                .key(format!("repeat-toggle-{id}")),
        )
        .child(arrow(p, repeat.direction))
        .child(
            text(repeat.label.clone())
                .w(96.0)
                .font(k.ty.small_medium)
                .color(if running {
                    k.c.text_base
                } else {
                    k.c.text_faint
                })
                .truncate(),
        )
        .child(
            text(repeat.expression.clone())
                .grow(1.0)
                .shrink()
                .font(k.ty.mono)
                .color(k.c.text_faint)
                .truncate(),
        )
        .child(
            text(state)
                .max_w(220.0)
                .font(k.ty.small)
                .color(ink)
                .truncate(),
        )
        .child(
            text(format!("{}×", repeat.sent))
                .key(format!("repeat-sent-{id}"))
                .w(52.0)
                .font(k.ty.small.features("'tnum' 1"))
                .color(k.c.text_muted)
                .align(Align::End),
        )
        .child(
            IconButton::new(k.glyph.close)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Stop and remove")
                .on_press(move |s: &mut App| s.repeats.remove(id))
                .build(k)
                .key(format!("repeat-remove-{id}")),
        )
        .on_press(move |s: &mut App| s.edit_repeat(id))
}

fn repeating(p: &Paint, s: &App) -> Option<El> {
    if s.repeats.is_empty() {
        return None;
    }
    let k = &p.kit;
    let list = s.repeats.iter().fold(
        col().key("repeat-list").w_full().gap(2.0),
        |list, repeat| list.child(repeat_row(p, repeat)),
    );
    Some(
        col()
            .key("repeating")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .gap(6.0)
            .child(
                text(format!(
                    "Repeating · {} of {}",
                    s.repeats.running(),
                    s.repeats.len()
                ))
                .key("repeating-title")
                .font(k.ty.small_medium)
                .color(k.c.text_muted),
            )
            .child(
                col()
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(list),
            ),
    )
}

pub fn view(s: &App, layer: &mut Vec<El>) -> El {
    let p = paint();
    let placeholder = if s.packet_info.as_ref().is_some_and(|info| !info.is_empty()) {
        "{out:Chat}{s:\"hello\"}{i:0}{i:-1}"
    } else {
        "Names like {out:Chat} resolve once a hotel is connected; until then use {h:597}."
    };
    let repeating = repeating(p, s);
    let editor = TextArea::new(&s.injection)
        .label("Packet expression")
        .placeholder(placeholder)
        .width(None)
        .rows(if repeating.is_some() { 5 } else { 12 })
        .wrap_anywhere()
        .on_edit(|s: &mut App| &mut s.injection)
        .on_submit(submit)
        .build(&p.mono_kit)
        .key("injection-editor");
    col()
        .key("injection")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(24.0)
        .gap(12.0)
        .child(editor)
        .child(actions(p, s, layer))
        .child(status(p, s))
        .children(repeating)
}
