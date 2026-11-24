use std::borrow::Cow;

use bobba_protocol::Direction;
use bobba_protocol::expression::display_bytes;
use bobba_session::{LogEntry, Outcome};
use rune::{Align, Color, El, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Cell, Switch, Table, TableColumn, TableState, TextInput};

use crate::packet_log::{LogRow, expression_for};
use crate::state::App;
use crate::view::theme::{Paint, paint};

fn table_state(s: &App) -> &TableState {
    &s.table
}

fn table_state_mut(s: &mut App) -> &mut TableState {
    &mut s.table
}

fn row_count(s: &App) -> usize {
    s.log.len()
}

fn row_id(s: &App, index: usize) -> Cow<'_, str> {
    Cow::Owned(s.log.number_at(index).unwrap_or_default().to_string())
}

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

fn cell(s: &App, index: usize, column: &str) -> Cell {
    let p = paint();
    let Some(row) = s.log.row(index) else {
        return Cell::text("");
    };
    match column {
        "number" => Cell::text(row.number.to_string()).muted().mono(),
        "direction" => match row.direction() {
            Direction::ToServer => Cell::text("↑ out").color(p.blue),
            Direction::ToClient => Cell::text("↓ in").color(p.green),
        },
        "header" => Cell::text(row.packet().header_id().to_string()).mono(),
        "name" => match s.log.name(row) {
            Some(name) => Cell::text(name),
            None => Cell::text("—").muted(),
        },
        "length" => Cell::text(row.packet().frame().len().to_string())
            .muted()
            .mono(),
        "status" => Cell::text(row.status()).color(outcome_color(p, row)),
        _ => Cell::text(expression_for(
            row.packet(),
            row.direction(),
            s.log.packet_info(),
        ))
        .mono(),
    }
}

fn toolbar(s: &App) -> El {
    let k = &paint().kit;
    row()
        .key("logger-toolbar")
        .w_full()
        .px(16.0)
        .py(10.0)
        .gap(14.0)
        .center_y()
        .child(
            TextInput::new(&s.filter)
                .label("Filter packets")
                .placeholder("Filter by name, header id or status")
                .leading_icon(k.glyph.search)
                .clear_button()
                .width(Some(320.0))
                .on_edit(|s: &mut App| &mut s.filter)
                .on_change(|s: &mut App, _| s.set_filter())
                .build(k)
                .key("logger-filter"),
        )
        .child(
            Switch::new(s.follow)
                .label("Follow")
                .on_toggle(|s: &mut App| s.follow = !s.follow)
                .build(k)
                .key("logger-follow"),
        )
        .child(
            Switch::new(s.paused)
                .label("Pause")
                .on_toggle(|s: &mut App| s.paused = !s.paused)
                .build(k)
                .key("logger-pause"),
        )
        .child(el().grow(1.0))
        .child(
            text(format!("{} packets", s.log.len()))
                .key("logger-count")
                .font(k.ty.small)
                .color(k.c.text_muted),
        )
        .child(
            Button::new("Clear")
                .variant(ButtonVariant::GhostMuted)
                .on_press(App::clear_log)
                .build(k)
                .key("logger-clear"),
        )
}

fn detail(s: &App, entry: &LogRow) -> El {
    let p = paint();
    let k = &p.kit;
    let info = s.log.packet_info().clone();
    let expression = expression_for(entry.packet(), entry.direction(), &info);
    let hex = entry
        .packet()
        .frame()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    let block = |label: &str, key: &str, body: String| {
        col()
            .key(format!("detail-{key}"))
            .gap(4.0)
            .child(text(label).font(k.ty.small).color(k.c.text_muted))
            .child(
                text(body)
                    .font(k.ty.mono)
                    .color(k.c.text_base)
                    .selectable(true)
                    .wrap_anywhere(),
            )
    };
    let use_expression = expression.clone();
    let mut body = col()
        .key("logger-detail")
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .fill(p.panel)
        .child(
            row()
                .center_y()
                .gap(10.0)
                .child(
                    text(format!("#{}", entry.number))
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base),
                )
                .child(
                    text(s.log.name(entry).unwrap_or("unknown"))
                        .font(k.ty.ui)
                        .color(k.c.text_muted),
                )
                .child(
                    text(entry.status())
                        .font(k.ty.small)
                        .color(outcome_color(p, entry)),
                )
                .child(el().grow(1.0))
                .child(
                    Button::new("Use in Injection")
                        .variant(ButtonVariant::Contrast)
                        .on_press(move |s: &mut App| s.use_in_injection(use_expression.clone()))
                        .build(k)
                        .key("logger-use"),
                ),
        )
        .child(block("Expression", "expression", expression));
    if let Some(sent) = entry.sent_instead() {
        body = body.child(block(
            "Sent instead",
            "sent",
            expression_for(sent, entry.direction(), &info),
        ));
    }
    body.child(block("Raw", "raw", display_bytes(entry.packet().frame())))
        .child(block("Hex", "hex", hex))
}

pub fn view(s: &App) -> El {
    let p = paint();
    let k = &p.kit;
    let table = Table::new(table_state, table_state_mut, row_count, row_id)
        .name("Packets")
        .column(
            TableColumn::new("number", "#")
                .width(64.0)
                .align(Align::End),
        )
        .column(TableColumn::new("direction", "").width(64.0))
        .column(
            TableColumn::new("header", "Header")
                .width(76.0)
                .align(Align::End),
        )
        .column(TableColumn::new("name", "Name").width(200.0).min(120.0))
        .column(
            TableColumn::new("length", "Len")
                .width(56.0)
                .align(Align::End),
        )
        .column(TableColumn::new("status", "Status").width(150.0))
        .column(
            TableColumn::new("expression", "Expression")
                .flex(1.0)
                .min(240.0),
        )
        .cell(cell)
        .build(s, k);
    let mut page = col()
        .key("logger")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(toolbar(s))
        .child(
            col()
                .key("logger-table")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(table.grow(1.0).min_h(0.0)),
        );
    if let Some(entry) = s.selected_row() {
        page = page.child(detail(s, entry));
    }
    page
}
