use std::borrow::Cow;

use bobba_protocol::Direction;
use bobba_session::{LogEntry, Outcome};
use rune::{
    Align, Color, Cx, El, GrowthSpec, Icon, ScrollSpec, Spring, SpringPair, TextStyle, Transition,
    col, el, oklcha, row, text,
};
use rune_kit::{
    Button, ButtonVariant, IconButton, IconButtonVariant, Menu, MenuItem, Placement, TextInput,
};

use crate::logger::{LIST, Logger, on_key};
use crate::packet_log::{Directions, LogRow, expression_for, fields_of};
use crate::view::chrome::TITLEBAR_HEIGHT;
use crate::view::theme::{Paint, paint};

static SLIDERS: Icon = Icon {
    name: "bobba-sliders",
    view: 16.0,
    body: r#"<path d="M2.5 5H9.5M12.5 5H13.5M2.5 11H4.5M7.5 11H13.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><circle cx="11" cy="5" r="1.6" fill="none" stroke="currentColor" stroke-width="1.5"/><circle cx="6" cy="11" r="1.6" fill="none" stroke="currentColor" stroke-width="1.5"/>"#,
};

const SHEET: f32 = 420.0;
const LINE: f32 = 36.0;
const LINE_CHARS: usize = 300;
const SHEET_CHARS: usize = 360;
const STATUS_WIDTH: f32 = 240.0;
const FIELDS: usize = 200;
const ENTER: SpringPair = SpringPair::new(Spring::new(0.25, 0.0), Spring::new(0.2, 0.0));

fn outcome_color(p: &Paint, row: &LogRow) -> Color {
    match &row.entry {
        LogEntry::Injected { .. } => p.blue,
        LogEntry::Intercepted {
            outcome: Outcome::Blocked { .. },
            ..
        } => p.red,
        LogEntry::Intercepted {
            outcome: Outcome::Edited { .. },
            ..
        } => p.amber,
        LogEntry::Intercepted {
            outcome: Outcome::Forwarded,
            ..
        } => p.kit.c.text_faint,
    }
}

fn arrow(direction: Direction) -> &'static str {
    match direction {
        Direction::ToServer => "↑",
        Direction::ToClient => "↓",
    }
}

fn direction_color(p: &Paint, direction: Direction) -> Color {
    match direction {
        Direction::ToServer => p.blue,
        Direction::ToClient => p.red,
    }
}

fn heading(direction: Direction) -> &'static str {
    match direction {
        Direction::ToServer => "↑ to server",
        Direction::ToClient => "↓ to client",
    }
}

fn expression(s: &Logger, row: &LogRow) -> String {
    expression_for(row.packet(), row.direction(), s.feed.log().packet_info())
}

fn start_of(expression: &str, chars: usize) -> Option<String> {
    let (cut, _) = expression.char_indices().nth(chars)?;
    Some(format!("{}…", &expression[..cut]))
}

fn one_line(expression: &str) -> String {
    let start = start_of(expression, LINE_CHARS).unwrap_or_else(|| expression.to_owned());
    start.replace('\n', "↵")
}

fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn line(s: &Logger, index: usize) -> El {
    let p = paint();
    let k = &p.kit;
    let Some(entry) = s.feed.log().row(index).cloned() else {
        return el();
    };
    let number = entry.number;
    let open = s.sheet.is_some_and(|sheet| sheet.number == number);
    let expression = expression(s, &entry);
    let inject = expression.clone();
    let status = entry.status();
    let (fill, hover) = if open {
        (k.alpha(k.c.text_base, 0.08), k.alpha(k.c.text_base, 0.08))
    } else {
        (oklcha(0.0, 0.0, 0.0, 0.0), k.alpha(k.c.text_base, 0.035))
    };
    row()
        .w_full()
        .h(LINE)
        .px(16.0)
        .gap(14.0)
        .center_y()
        .fill(fill)
        .hover(hover)
        .child(
            text(arrow(entry.direction()))
                .w(12.0)
                .font(k.ty.ui_medium)
                .color(direction_color(p, entry.direction())),
        )
        .child(
            text(one_line(&expression))
                .grow(1.0)
                .shrink()
                .font(k.ty.mono)
                .color(if open { k.c.text_base } else { k.c.text_muted })
                .truncate(),
        )
        .children((!status.is_empty()).then(|| {
            text(status.clone())
                .max_w(STATUS_WIDTH)
                .font(k.ty.text_xsmall_medium)
                .color(outcome_color(p, &entry))
                .truncate()
                .help_when_truncated(status)
        }))
        .on_press(move |s: &mut Logger| s.open(number))
        .on_double_press(move |s: &mut Logger| s.use_in_injection(inject.clone()))
}

fn summary(s: &Logger) -> String {
    let mut parts = Vec::new();
    if s.feed.is_paused() {
        parts.push("Paused");
    }
    match s.directions {
        Directions {
            incoming: true,
            outgoing: true,
        } => {}
        Directions {
            incoming: true,
            outgoing: false,
        } => parts.push("Incoming only"),
        Directions {
            incoming: false,
            outgoing: true,
        } => parts.push("Outgoing only"),
        Directions {
            incoming: false,
            outgoing: false,
        } => parts.push("Nothing shown"),
    }
    if !s.autoscroll {
        parts.push("Autoscroll off");
    }
    if parts.is_empty() {
        "View".to_owned()
    } else {
        parts.join(" · ")
    }
}

fn swatch(p: &Paint, direction: Direction) -> El {
    el().size(8.0, 8.0)
        .radius(4.0)
        .fill(direction_color(p, direction))
}

fn view_menu(s: &Logger, layer: &mut Vec<El>) -> El {
    let p = paint();
    let k = &p.kit;
    let label = summary(s);
    let trigger = Button::new(label.clone())
        .icon(&SLIDERS)
        .variant(ButtonVariant::GhostMuted)
        .label_with(move |font, ink| {
            text(label.clone())
                .font(font)
                .color(ink)
                .truncate()
                .help_when_truncated(label)
        })
        .expanded(s.view_open)
        .on_press(|s: &mut Logger| s.view_open = !s.view_open)
        .build(k)
        .key("logger-view")
        .shrink()
        .min_w(0.0);
    Menu::new("logger-view")
        .trigger(trigger)
        .open(s.view_open)
        .placement(Placement::BottomStart)
        .min_width(220.0)
        .on_close(|s: &mut Logger| s.view_open = false)
        .group_label("Show")
        .item(
            MenuItem::new("Incoming")
                .lead(swatch(p, Direction::ToClient))
                .checked(s.directions.incoming)
                .on_select(Logger::toggle_incoming),
        )
        .item(
            MenuItem::new("Outgoing")
                .lead(swatch(p, Direction::ToServer))
                .checked(s.directions.outgoing)
                .on_select(Logger::toggle_outgoing),
        )
        .separator()
        .item(
            MenuItem::new("Autoscroll")
                .checked(s.autoscroll)
                .on_select(|s: &mut Logger| s.autoscroll = !s.autoscroll),
        )
        .item(
            MenuItem::new("Pause capture")
                .checked(s.feed.is_paused())
                .on_select(|s: &mut Logger| s.feed.toggle_pause()),
        )
        .build(k, layer)
        .shrink()
        .min_w(0.0)
}

fn toolbar(s: &Logger, cx: &mut Cx<Logger>, layer: &mut Vec<El>) -> El {
    let k = &paint().kit;
    let log = s.feed.log();
    let count = match (log.len(), log.total()) {
        (1, 1) => "1 packet".to_owned(),
        (shown, total) if shown == total => format!("{} packets", grouped(total)),
        (shown, total) => format!("{} of {}", grouped(shown), grouped(total)),
    };
    drop(log);
    row()
        .key("logger-toolbar")
        .w_full()
        .h(TITLEBAR_HEIGHT)
        .px(14.0)
        .gap(14.0)
        .center_y()
        .window_drag()
        .child(el().w(cx.titlebar_inset()))
        .child(
            TextInput::new(&s.filter)
                .label("Filter packets")
                .placeholder("Filter by name, like Chat or MoveAvatar")
                .leading_icon(k.glyph.search)
                .clear_button()
                .width(Some(320.0))
                .on_edit(|s: &mut Logger| &mut s.filter)
                .on_change(|s: &mut Logger, _| s.set_filter())
                .build(k)
                .key("logger-filter")
                .shrink()
                .min_w(180.0),
        )
        .child(view_menu(s, layer))
        .child(el().grow(1.0))
        .child(
            text(count)
                .key("logger-count")
                .font(k.ty.small)
                .color(k.c.text_muted),
        )
        .child(
            Button::new("Clear")
                .variant(ButtonVariant::GhostMuted)
                .on_press(Logger::clear)
                .build(k)
                .key("logger-clear"),
        )
}

fn label(p: &Paint, title: &str) -> El {
    text(title.to_owned())
        .font(p.kit.ty.text_xsmall_medium)
        .color(p.kit.c.text_faint)
}

fn structure(p: &Paint, entry: &LogRow) -> El {
    let k = &p.kit;
    let fields = fields_of(entry.packet());
    if fields.is_empty() {
        return text("No values").font(k.ty.small).color(k.c.text_faint);
    }
    let rest = fields.len().saturating_sub(FIELDS);
    let list = fields.into_iter().take(FIELDS).enumerate().fold(
        col().key("sheet-structure").w_full().gap(2.0),
        |list, (i, field)| {
            list.child(
                row()
                    .key(format!("structure-{i}"))
                    .w_full()
                    .min_h(28.0)
                    .gap(12.0)
                    .center_y()
                    .child(
                        text((i + 1).to_string())
                            .w(16.0)
                            .font(k.ty.small)
                            .color(k.c.text_faint)
                            .align(Align::End),
                    )
                    .child(
                        text(field.kind.label())
                            .key(format!("structure-type-{i}"))
                            .w(52.0)
                            .font(k.ty.small)
                            .color(k.c.text_muted),
                    )
                    .child(
                        text(field.value)
                            .grow(1.0)
                            .shrink()
                            .font(k.ty.mono)
                            .color(k.c.text_base)
                            .selectable(true)
                            .wrap_anywhere(),
                    ),
            )
        },
    );
    list.children((rest > 0).then(|| {
        text(match rest {
            1 => "1 more field".to_owned(),
            _ => format!("{} more fields", grouped(rest)),
        })
        .key("sheet-more-fields")
        .font(k.ty.small)
        .color(k.c.text_faint)
    }))
}

fn sheet(s: &Logger, entry: &LogRow, copied: bool) -> El {
    let p = paint();
    let k = &p.kit;
    let log = s.feed.log();
    let info = log.packet_info().clone();
    let name = log.name(entry).map_or_else(
        || format!("Header {}", entry.packet().header_id()),
        str::to_owned,
    );
    drop(log);
    let expression = expression_for(entry.packet(), entry.direction(), &info);
    let inject = expression.clone();
    let copy = expression.clone();
    let status = entry.status();
    let sent = entry
        .sent_instead()
        .map(|sent| expression_for(sent, entry.direction(), &info));
    let head = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .shrink()
                .gap(3.0)
                .child(
                    text(name)
                        .key("sheet-name")
                        .font(TextStyle::new(20.0, 650.0))
                        .color(k.c.text_base)
                        .truncate(),
                )
                .child(
                    row()
                        .w_full()
                        .gap(6.0)
                        .child(
                            text(heading(entry.direction()))
                                .font(k.ty.small)
                                .color(k.c.text_muted),
                        )
                        .children((!status.is_empty()).then(|| {
                            text(format!("· {status}"))
                                .font(k.ty.small)
                                .color(outcome_color(p, entry))
                                .truncate()
                                .help_when_truncated(status.clone())
                        })),
                ),
        )
        .child(
            IconButton::new(k.glyph.close)
                .variant(IconButtonVariant::Ghost)
                .label("Close")
                .shortcut("Esc")
                .on_press(Logger::close)
                .build(k)
                .key("sheet-close"),
        );
    let mut body = col()
        .w_full()
        .px(22.0)
        .pt(18.0)
        .pb(24.0)
        .gap(16.0)
        .child(head)
        .child(
            col()
                .key("sheet-expression")
                .w_full()
                .padding(14.0)
                .radius(10.0)
                .fill(k.c.bg_base)
                .border(1.0, p.hairline)
                .child(
                    text(start_of(&expression, SHEET_CHARS).unwrap_or_else(|| expression.clone()))
                        .font(k.ty.mono)
                        .color(k.c.text_base)
                        .selectable(true)
                        .wrap_anywhere(),
                ),
        )
        .children(start_of(&expression, SHEET_CHARS).map(|_| {
            text(format!(
                "Showing the start of {} characters. Copy and Use in Injection take the whole packet.",
                grouped(expression.chars().count())
            ))
            .key("sheet-cut")
            .w_full()
            .font(k.ty.small)
            .color(k.c.text_faint)
            .wrap()
        }))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(
                    Button::new("Use in Injection")
                        .variant(ButtonVariant::Contrast)
                        .on_press(move |s: &mut Logger| s.use_in_injection(inject.clone()))
                        .build(k)
                        .key("logger-use")
                        .grow(1.0),
                )
                .child(
                    Button::new(if copied { "Copied" } else { "Copy" })
                        .variant(ButtonVariant::Outline)
                        .on_press_tx(move |s: &mut Logger, tx| s.copy(copy.clone(), tx))
                        .build(k)
                        .key("logger-copy")
                        .grow(1.0),
                ),
        )
        .child(el().h(4.0))
        .child(label(p, "Structure"))
        .child(structure(p, entry));
    if let Some(sent) = sent {
        body = body
            .child(el().h(4.0))
            .child(label(p, "Sent instead"))
            .child(
                text(start_of(&sent, SHEET_CHARS).unwrap_or(sent))
                    .key("sheet-sent")
                    .font(k.ty.mono)
                    .color(k.c.text_muted)
                    .selectable(true)
                    .wrap_anywhere(),
            );
    }
    row()
        .key("logger-sheet")
        .h_full()
        .min_h(0.0)
        .transition(
            Transition::offset(24.0, 0.0)
                .combined(Transition::opacity())
                .springs(ENTER),
        )
        .child(el().w(1.0).h_full().fill(p.hairline))
        .child(
            col()
                .w(SHEET)
                .h_full()
                .min_h(0.0)
                .fill(p.panel)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(body),
        )
}

fn empty(s: &Logger) -> Option<El> {
    let log = s.feed.log();
    if !log.is_empty() {
        return None;
    }
    let (title, detail) = if log.total() == 0 {
        (
            "No packets yet",
            "Packets show here as soon as Habbo talks to the hotel.",
        )
    } else {
        (
            "No packets match",
            "Change the filter or show both directions in View.",
        )
    };
    let k = &paint().kit;
    Some(
        col()
            .key("logger-empty")
            .w_full()
            .h_full()
            .center()
            .gap(4.0)
            .px(24.0)
            .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
            .child(
                text(detail)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .align(Align::Center)
                    .wrap(),
            ),
    )
}

pub fn body(s: &Logger) -> El {
    let opened = s
        .sheet
        .and_then(|open| s.opened_row().map(|entry| sheet(s, &entry, open.copied)));
    let list = empty(s).unwrap_or_else(|| list(s));
    row()
        .key("logger-body")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(col().grow(1.0).shrink().h_full().min_h(0.0).child(list))
        .children(opened)
}

fn list(s: &Logger) -> El {
    col()
        .key(LIST)
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: true })
        .child(
            col()
                .key("packet-lines")
                .w_full()
                .py(6.0)
                .growth(GrowthSpec::default())
                .virtual_rows(
                    s.feed.log().len(),
                    LINE,
                    |s: &Logger, i| {
                        Cow::Owned(format!(
                            "line-{}",
                            s.feed.log().number_at(i).unwrap_or_default()
                        ))
                    },
                    line,
                ),
        )
}

pub fn root(s: &Logger, cx: &mut Cx<Logger>) -> El {
    let p = paint();
    let k = &p.kit;
    let mut layer = Vec::new();
    let page = col()
        .key("logger")
        .w_full()
        .h_full()
        .min_h(0.0)
        .fill(k.c.bg_base)
        .on_key(on_key)
        .child(toolbar(s, cx, &mut layer))
        .child(el().key("titlebar-rule").w_full().h(1.0).fill(p.hairline))
        .child(body(s));
    rune_kit::root(page, layer)
}
