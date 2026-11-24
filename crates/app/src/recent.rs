use std::collections::VecDeque;

use bobba_protocol::{Direction, Expression, PacketInfo};

const LIMIT: usize = 10;

pub struct SentPacket {
    pub direction: Direction,
    pub label: String,
    pub expression: String,
}

#[derive(Default)]
pub struct Recent {
    packets: VecDeque<SentPacket>,
}

impl Recent {
    pub fn remember(&mut self, direction: Direction, expression: &str, info: Option<&PacketInfo>) {
        self.packets
            .retain(|p| !(p.direction == direction && p.expression == expression));
        self.packets.push_front(SentPacket {
            direction,
            label: label(direction, expression, info),
            expression: expression.to_owned(),
        });
        self.packets.truncate(LIMIT);
    }

    pub fn iter(&self) -> impl Iterator<Item = &SentPacket> {
        self.packets.iter()
    }

    pub fn len(&self) -> usize {
        self.packets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
}

pub fn label(direction: Direction, expression: &str, info: Option<&PacketInfo>) -> String {
    match Expression::parse(expression) {
        Ok(Expression::Named { identifier, .. }) => identifier,
        Ok(Expression::Complete(packet)) => {
            let header = packet.header_id();
            info.and_then(|info| info.name(direction, header))
                .map_or_else(|| format!("Header {header}"), str::to_owned)
        }
        Err(_) => expression.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use bobba_protocol::PacketInfoEntry;

    use super::*;

    fn info() -> PacketInfo {
        PacketInfo::from_entries(
            "MAC63",
            vec![PacketInfoEntry {
                direction: Direction::ToServer,
                header_id: 597,
                name: Some("Chat".into()),
                hash: None,
                structure: None,
                source: "Sulek".into(),
            }],
            true,
        )
    }

    fn expressions(recent: &Recent) -> Vec<&str> {
        recent.iter().map(|p| p.expression.as_str()).collect()
    }

    #[test]
    fn newest_comes_first_and_a_repeat_moves_to_the_top() {
        let mut recent = Recent::default();
        recent.remember(Direction::ToServer, "{out:Chat}{s:\"a\"}", None);
        recent.remember(Direction::ToServer, "{out:Shout}{s:\"b\"}", None);
        recent.remember(Direction::ToServer, "{out:Chat}{s:\"a\"}", None);
        assert_eq!(
            expressions(&recent),
            ["{out:Chat}{s:\"a\"}", "{out:Shout}{s:\"b\"}"]
        );
    }

    #[test]
    fn the_same_expression_sent_the_other_way_is_its_own_entry() {
        let mut recent = Recent::default();
        recent.remember(Direction::ToServer, "{h:597}", None);
        recent.remember(Direction::ToClient, "{h:597}", None);
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn only_the_last_ten_are_kept() {
        let mut recent = Recent::default();
        for i in 0..12 {
            recent.remember(Direction::ToServer, &format!("{{h:597}}{{i:{i}}}"), None);
        }
        assert_eq!(recent.len(), 10);
        assert_eq!(expressions(&recent)[0], "{h:597}{i:11}");
    }

    #[test]
    fn packets_are_labelled_by_name_falling_back_to_the_header() {
        let mut recent = Recent::default();
        let info = info();
        recent.remember(Direction::ToServer, "{out:Shout}{s:\"oi\"}", Some(&info));
        recent.remember(Direction::ToServer, "{h:597}{s:\"oi\"}", Some(&info));
        recent.remember(Direction::ToServer, "{h:481}{s:\"oi\"}", Some(&info));
        let labels: Vec<_> = recent.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["Header 481", "Chat", "Shout"]);
    }
}
