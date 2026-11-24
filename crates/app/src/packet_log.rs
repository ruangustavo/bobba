use std::collections::VecDeque;
use std::sync::Arc;

use bobba_protocol::expression::HeaderLabel;
use bobba_protocol::prediction::predicted_expression;
use bobba_protocol::{Direction, Packet, PacketInfo};
use bobba_session::{LogEntry, Outcome};

pub const DEFAULT_CAPACITY: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRow {
    pub number: u64,
    pub entry: LogEntry,
}

impl LogRow {
    pub fn direction(&self) -> Direction {
        self.entry.direction()
    }

    pub fn packet(&self) -> &Packet {
        self.entry.packet()
    }

    pub fn status(&self) -> String {
        match &self.entry {
            LogEntry::Intercepted {
                outcome: Outcome::Forwarded,
                ..
            } => String::new(),
            LogEntry::Intercepted {
                outcome: Outcome::Edited { by, .. },
                ..
            } => format!("Edited · {by}"),
            LogEntry::Intercepted {
                outcome: Outcome::Blocked { by },
                ..
            } => format!("Blocked · {by}"),
            LogEntry::Injected { by, .. } => format!("Injected · {by}"),
        }
    }

    pub fn sent_instead(&self) -> Option<&Packet> {
        match &self.entry {
            LogEntry::Intercepted {
                outcome: Outcome::Edited { sent, .. },
                ..
            } => Some(sent),
            _ => None,
        }
    }
}

pub fn expression_for(packet: &Packet, direction: Direction, info: &PacketInfo) -> String {
    let label = match info.name(direction, packet.header_id()) {
        Some(name) => HeaderLabel::Name(direction, name),
        None => HeaderLabel::Id(packet.header_id()),
    };
    predicted_expression(packet, label)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    text: String,
}

impl Filter {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.trim().to_lowercase(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn matches(&self, row: &LogRow, info: &PacketInfo) -> bool {
        if self.text.is_empty() {
            return true;
        }
        let header_id = row.packet().header_id();
        let name = info
            .name(row.direction(), header_id)
            .unwrap_or_default()
            .to_lowercase();
        header_id.to_string() == self.text
            || name.contains(&self.text)
            || row.status().to_lowercase().contains(&self.text)
    }
}

pub struct PacketLog {
    rows: VecDeque<LogRow>,
    capacity: usize,
    next_number: u64,
    filter: Filter,
    visible: Vec<u64>,
    packet_info: Arc<PacketInfo>,
}

impl PacketLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            rows: VecDeque::new(),
            capacity,
            next_number: 0,
            filter: Filter::default(),
            visible: Vec::new(),
            packet_info: Arc::new(PacketInfo::empty("")),
        }
    }

    pub fn packet_info(&self) -> &Arc<PacketInfo> {
        &self.packet_info
    }

    pub fn set_packet_info(&mut self, info: Arc<PacketInfo>) {
        self.packet_info = info;
        self.rebuild_visible();
    }

    pub fn push(&mut self, entries: impl IntoIterator<Item = LogEntry>) {
        for entry in entries {
            let row = LogRow {
                number: self.next_number,
                entry,
            };
            self.next_number += 1;
            if self.filter.matches(&row, &self.packet_info) {
                self.visible.push(row.number);
            }
            self.rows.push_back(row);
        }
        let overflow = self.rows.len().saturating_sub(self.capacity);
        if overflow > 0 {
            self.rows.drain(..overflow);
            let first = self.first_number();
            let keep_from = self.visible.partition_point(|n| *n < first);
            self.visible.drain(..keep_from);
        }
    }

    fn first_number(&self) -> u64 {
        self.rows.front().map_or(self.next_number, |row| row.number)
    }

    pub fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
        self.rebuild_visible();
    }

    fn rebuild_visible(&mut self) {
        self.visible = self
            .rows
            .iter()
            .filter(|row| self.filter.matches(row, &self.packet_info))
            .map(|row| row.number)
            .collect();
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.visible.clear();
    }

    pub fn len(&self) -> usize {
        self.visible.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    pub fn total(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, visible_index: usize) -> Option<&LogRow> {
        let number = *self.visible.get(visible_index)?;
        let offset = usize::try_from(number - self.first_number()).ok()?;
        self.rows.get(offset)
    }

    pub fn row_by_number(&self, number: u64) -> Option<&LogRow> {
        let offset = usize::try_from(number.checked_sub(self.first_number())?).ok()?;
        self.rows.get(offset)
    }

    pub fn number_at(&self, visible_index: usize) -> Option<u64> {
        self.visible.get(visible_index).copied()
    }

    pub fn name(&self, row: &LogRow) -> Option<&str> {
        self.packet_info
            .name(row.direction(), row.packet().header_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SULEK: &str = r#"{"messages":{"incoming":[],"outgoing":[{"id":597,"name":"ChatMessageComposer","confident":true}]}}"#;

    fn intercepted(header_id: u16, outcome: Outcome) -> LogEntry {
        LogEntry::Intercepted {
            index: 0,
            direction: Direction::ToServer,
            packet: Packet::builder(header_id).string("oi").build(),
            outcome,
        }
    }

    #[test]
    fn keeps_only_the_newest_rows_within_capacity() {
        let mut log = PacketLog::new(3);
        log.push((0..5).map(|i| intercepted(i, Outcome::Forwarded)));
        assert_eq!(log.total(), 3);
        assert_eq!(log.len(), 3);
        let first = log.row(0).expect("row exists");
        assert_eq!((first.number, first.packet().header_id()), (2, 2));
    }

    #[test]
    fn filters_by_name_once_names_arrive() {
        let mut log = PacketLog::new(10);
        log.push([
            intercepted(597, Outcome::Forwarded),
            intercepted(10, Outcome::Forwarded),
        ]);
        log.set_filter(Filter::new("chat"));
        assert_eq!(log.len(), 0);
        log.set_packet_info(Arc::new(
            PacketInfo::from_sulek_json("v", SULEK).expect("parses"),
        ));
        assert_eq!(log.len(), 1);
        assert_eq!(log.row(0).map(|r| r.packet().header_id()), Some(597));
    }

    #[test]
    fn filters_by_exact_header_and_by_status() {
        let mut log = PacketLog::new(10);
        log.push([
            intercepted(
                597,
                Outcome::Blocked {
                    by: "AntiSpam".into(),
                },
            ),
            intercepted(59, Outcome::Forwarded),
        ]);
        log.set_filter(Filter::new("59"));
        assert_eq!(log.len(), 1);
        log.set_filter(Filter::new("antispam"));
        assert_eq!(
            log.row(0).map(LogRow::status).as_deref(),
            Some("Blocked · AntiSpam")
        );
    }

    #[test]
    fn expression_uses_the_name_when_known() {
        let info = PacketInfo::from_sulek_json("v", SULEK).expect("parses");
        let packet = Packet::builder(597)
            .string("hello there")
            .i32(0)
            .i32(-1)
            .build();
        assert_eq!(
            expression_for(&packet, Direction::ToServer, &info),
            r#"{out:Chat}{s:"hello there"}{i:0}{i:-1}"#
        );
        assert_eq!(
            expression_for(&packet, Direction::ToClient, &info),
            r#"{h:597}{s:"hello there"}{i:0}{i:-1}"#
        );
    }
}
