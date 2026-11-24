use std::collections::HashMap;
use std::sync::Arc;

use bobba_protocol::expression::display_bytes;
use bobba_protocol::{Direction, PacketInfo};
use bobba_session::LogEntry;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableEvent, TableState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::packet_log::{DEFAULT_CAPACITY, Filter, LogRow, PacketLog, expression_for};

const EXPRESSION_CACHE_LIMIT: usize = 4_096;

pub struct LogTable {
    log: PacketLog,
    columns: Vec<Column>,
    expressions: HashMap<u64, SharedString>,
}

impl LogTable {
    fn new() -> Self {
        Self {
            log: PacketLog::new(DEFAULT_CAPACITY),
            columns: vec![
                Column::new("number", "#").width(64.).text_right(),
                Column::new("direction", "").width(44.),
                Column::new("header", "Header").width(64.).text_right(),
                Column::new("name", "Name").width(220.),
                Column::new("length", "Len").width(56.).text_right(),
                Column::new("status", "Status").width(160.),
                Column::new("expression", "Expression").width(900.),
            ],
            expressions: HashMap::new(),
        }
    }

    fn expression(&mut self, row: &LogRow) -> SharedString {
        if let Some(cached) = self.expressions.get(&row.number) {
            return cached.clone();
        }
        if self.expressions.len() >= EXPRESSION_CACHE_LIMIT {
            self.expressions.clear();
        }
        let text: SharedString =
            expression_for(row.packet(), row.direction(), self.log.packet_info()).into();
        self.expressions.insert(row.number, text.clone());
        text
    }
}

impl TableDelegate for LogTable {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.log.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(row) = self.log.row(row_ix).cloned() else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let cell = div().text_xs().truncate();
        match col_ix {
            0 => cell
                .text_color(muted)
                .child(row.number.to_string())
                .into_any_element(),
            1 => {
                let (label, color) = match row.direction() {
                    Direction::ToServer => ("OUT", theme.blue),
                    Direction::ToClient => ("IN", theme.green),
                };
                cell.text_color(color).child(label).into_any_element()
            }
            2 => cell
                .child(row.packet().header_id().to_string())
                .into_any_element(),
            3 => {
                let name = self.log.name(&row).map(str::to_owned);
                match name {
                    Some(name) => cell.child(name).into_any_element(),
                    None => cell.text_color(muted).child("—").into_any_element(),
                }
            }
            4 => cell
                .text_color(muted)
                .child(row.packet().frame().len().to_string())
                .into_any_element(),
            5 => {
                let color = match &row.entry {
                    LogEntry::Injected { .. } => theme.blue,
                    _ if row.sent_instead().is_some() => theme.yellow,
                    _ if row.status().is_empty() => muted,
                    _ => theme.red,
                };
                cell.text_color(color)
                    .child(row.status())
                    .into_any_element()
            }
            _ => cell
                .font_family("Menlo")
                .child(self.expression(&row))
                .into_any_element(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum LoggerEvent {
    UseInInjection {
        expression: String,
        direction: Direction,
    },
}

impl EventEmitter<LoggerEvent> for LoggerPanel {}

pub struct LoggerPanel {
    table: Entity<TableState<LogTable>>,
    filter: Entity<InputState>,
    follow: bool,
    paused: bool,
    selected: Option<LogRow>,
    _subscriptions: Vec<Subscription>,
}

impl LoggerPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let table = cx.new(|cx| TableState::new(LogTable::new(), window, cx).row_selectable(true));
        let filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter by name, header id or status")
        });
        let subscriptions = vec![
            cx.subscribe_in(&table, window, |this, table, event: &TableEvent, _, cx| {
                if let TableEvent::SelectRow(ix) = event {
                    this.selected = table.read(cx).delegate().log.row(*ix).cloned();
                    this.follow = false;
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &filter,
                window,
                |this, filter, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let text = filter.read(cx).value();
                        this.table.update(cx, |table, cx| {
                            table.delegate_mut().log.set_filter(Filter::new(&text));
                            table.refresh(cx);
                        });
                    }
                },
            ),
        ];
        Self {
            table,
            filter,
            follow: true,
            paused: false,
            selected: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn append(&mut self, entries: Vec<LogEntry>, cx: &mut Context<Self>) {
        if self.paused {
            return;
        }
        let follow = self.follow;
        self.table.update(cx, |table, cx| {
            table.delegate_mut().log.push(entries);
            let rows = table.delegate().log.len();
            if follow && rows > 0 {
                table.scroll_to_row(rows - 1, cx);
            }
            cx.notify();
        });
        cx.notify();
    }

    pub fn set_packet_info(&mut self, info: Arc<PacketInfo>, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.log.set_packet_info(info);
            delegate.expressions.clear();
            table.refresh(cx);
        });
    }

    pub fn row_count(&self, cx: &App) -> usize {
        self.table.read(cx).delegate().log.len()
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.table.update(cx, |table, cx| {
            table.delegate_mut().log.clear();
            table.delegate_mut().expressions.clear();
            table.refresh(cx);
        });
        cx.notify();
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_3()
            .p_2()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .w(px(320.))
                    .child(Input::new(&self.filter).id("logger-filter").small()),
            )
            .child(
                Switch::new("logger-follow")
                    .label("Follow")
                    .checked(self.follow)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.follow = *checked;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("logger-pause")
                    .label("Pause")
                    .checked(self.paused)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.paused = *checked;
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("logger-count")
                    .test_support()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} packets", self.row_count(cx))),
            )
            .child(
                Button::new("logger-clear")
                    .label("Clear")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
            )
    }

    fn render_detail(&self, row: &LogRow, cx: &mut Context<Self>) -> impl IntoElement {
        let info = self.table.read(cx).delegate().log.packet_info().clone();
        let expression = expression_for(row.packet(), row.direction(), &info);
        let raw = display_bytes(row.packet().frame());
        let hex = row
            .packet()
            .frame()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        let sent = row
            .sent_instead()
            .map(|packet| expression_for(packet, row.direction(), &info));
        let direction = row.direction();
        let use_expression = expression.clone();
        let mono = |label: &'static str, text: String| {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(label),
                )
                .child(div().text_xs().font_family("Menlo").child(text))
        };
        v_flex()
            .id("logger-detail")
            .gap_3()
            .p_3()
            .max_h(px(220.))
            .overflow_y_scroll()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .child(format!("#{} · {}", row.number, row.status())),
                    )
                    .child(
                        Button::new("logger-use")
                            .label("Use in Injection")
                            .small()
                            .primary()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(LoggerEvent::UseInInjection {
                                    expression: use_expression.clone(),
                                    direction,
                                });
                            })),
                    ),
            )
            .child(mono("Expression", expression))
            .when_some(sent, |this, sent| this.child(mono("Sent instead", sent)))
            .child(mono("Raw", raw))
            .child(mono("Hex", hex))
    }
}

impl Render for LoggerPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.clone();
        v_flex()
            .size_full()
            .child(self.render_toolbar(cx))
            .child(
                div().flex_1().min_h_0().child(
                    DataTable::new(&self.table)
                        .stripe(true)
                        .bordered(false)
                        .small(),
                ),
            )
            .when_some(selected, |this, row| {
                this.child(self.render_detail(&row, cx))
            })
    }
}
