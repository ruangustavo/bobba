use std::collections::HashMap;

use serde::Deserialize;

use crate::direction::Direction;
use crate::packet::PacketBuilder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketInfoEntry {
    pub direction: Direction,
    pub header_id: u16,
    pub name: Option<String>,
    pub hash: Option<String>,
    pub structure: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Default)]
pub struct PacketInfo {
    hotel_version: String,
    entries: Vec<PacketInfoEntry>,
    by_id: HashMap<(Direction, u16), usize>,
    by_name: HashMap<(Direction, String), usize>,
    by_hash: HashMap<(Direction, String), usize>,
    fully_confident: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("Sulek response is not the expected JSON: {0}")]
pub struct SulekParseError(#[from] serde_json::Error);

#[derive(Deserialize)]
struct SulekResponse {
    messages: SulekMessages,
}

#[derive(Deserialize)]
struct SulekMessages {
    incoming: Vec<SulekMessage>,
    outgoing: Vec<SulekMessage>,
}

#[derive(Deserialize)]
struct SulekMessage {
    id: u16,
    name: String,
    #[serde(default)]
    confident: bool,
}

impl PacketInfo {
    pub fn empty(hotel_version: impl Into<String>) -> Self {
        Self {
            hotel_version: hotel_version.into(),
            ..Self::default()
        }
    }

    pub fn from_entries(
        hotel_version: impl Into<String>,
        entries: Vec<PacketInfoEntry>,
        fully_confident: bool,
    ) -> Self {
        let mut info = Self::empty(hotel_version);
        info.fully_confident = fully_confident;
        for entry in entries {
            info.insert(entry);
        }
        info
    }

    pub fn from_sulek_json(
        hotel_version: impl Into<String>,
        json: &str,
    ) -> Result<Self, SulekParseError> {
        let response: SulekResponse = serde_json::from_str(json)?;
        let SulekMessages { incoming, outgoing } = response.messages;
        let fully_confident = incoming.iter().chain(&outgoing).all(|m| m.confident);
        let tagged = incoming
            .into_iter()
            .map(|m| (Direction::ToClient, m))
            .chain(outgoing.into_iter().map(|m| (Direction::ToServer, m)));
        let entries = tagged
            .map(|(direction, message)| PacketInfoEntry {
                direction,
                header_id: message.id,
                name: Some(strip_message_suffix(&message.name).to_owned()),
                hash: None,
                structure: None,
                source: "Sulek".to_owned(),
            })
            .collect();
        Ok(Self::from_entries(hotel_version, entries, fully_confident))
    }

    fn insert(&mut self, entry: PacketInfoEntry) {
        if entry.name.is_none() && entry.hash.is_none() {
            return;
        }
        let index = self.entries.len();
        self.by_id
            .entry((entry.direction, entry.header_id))
            .or_insert(index);
        if let Some(name) = &entry.name {
            self.by_name
                .entry((entry.direction, name.clone()))
                .or_insert(index);
        }
        if let Some(hash) = &entry.hash {
            self.by_hash
                .entry((entry.direction, hash.clone()))
                .or_insert(index);
        }
        self.entries.push(entry);
    }

    pub fn hotel_version(&self) -> &str {
        &self.hotel_version
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_fully_confident(&self) -> bool {
        self.fully_confident
    }

    pub fn entries(&self) -> &[PacketInfoEntry] {
        &self.entries
    }

    pub fn entry(&self, direction: Direction, header_id: u16) -> Option<&PacketInfoEntry> {
        self.by_id
            .get(&(direction, header_id))
            .map(|&i| &self.entries[i])
    }

    pub fn name(&self, direction: Direction, header_id: u16) -> Option<&str> {
        self.entry(direction, header_id)
            .and_then(|e| e.name.as_deref().or(e.hash.as_deref()))
    }

    pub fn header_id(&self, direction: Direction, identifier: &str) -> Option<u16> {
        let key = (direction, identifier.to_owned());
        self.by_name
            .get(&key)
            .or_else(|| self.by_hash.get(&key))
            .map(|&i| self.entries[i].header_id)
    }

    pub fn names(&self, direction: Direction) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(move |e| e.direction == direction)
            .filter_map(|e| e.name.as_deref())
    }

    pub fn write_to(&self, builder: PacketBuilder) -> PacketBuilder {
        let count = i32::try_from(self.entries.len()).unwrap_or(i32::MAX);
        self.entries.iter().fold(builder.i32(count), |b, e| {
            b.i32(i32::from(e.header_id))
                .string(e.hash.as_deref().unwrap_or("NULL"))
                .string(e.name.as_deref().unwrap_or("NULL"))
                .string(e.structure.as_deref().unwrap_or("NULL"))
                .bool(e.direction == Direction::ToServer)
                .string(&e.source)
        })
    }
}

fn strip_message_suffix(name: &str) -> &str {
    ["MessageComposer", "Composer", "MessageEvent", "Event"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::Expression;

    const SULEK: &str = r#"{"messages":{
        "incoming":[{"id":2350,"name":"ChatMessageEvent","confident":true},{"id":10,"name":"UsersEvent","confident":true}],
        "outgoing":[{"id":597,"name":"ChatMessageComposer","confident":true},{"id":481,"name":"ShoutMessageComposer","confident":true}]
    }}"#;

    fn info() -> PacketInfo {
        PacketInfo::from_sulek_json("MAC63-202611201617-213322612", SULEK).expect("fixture parses")
    }

    #[test]
    fn strips_gearth_name_suffixes() {
        let info = info();
        assert_eq!(info.name(Direction::ToServer, 597), Some("Chat"));
        assert_eq!(info.name(Direction::ToClient, 2350), Some("Chat"));
        assert_eq!(info.name(Direction::ToClient, 10), Some("Users"));
        assert_eq!(info.name(Direction::ToServer, 2350), None);
    }

    #[test]
    fn resolves_named_expressions_by_direction() {
        let packet = Expression::parse("{out:Shout}{s:\"oi\"}{i:0}")
            .and_then(|e| e.resolve(&info()))
            .expect("Shout exists going out");
        assert_eq!(packet.header_id(), 481);
    }

    #[test]
    fn unknown_names_are_an_error() {
        let result = Expression::parse("{in:Shout}").and_then(|e| e.resolve(&info()));
        assert!(result.is_err());
    }

    #[test]
    fn reports_whether_every_name_is_confident() {
        assert!(info().is_fully_confident());
        let unsure = SULEK.replacen("\"confident\":true", "\"confident\":false", 1);
        assert!(
            !PacketInfo::from_sulek_json("v", &unsure)
                .expect("parses")
                .is_fully_confident()
        );
    }

    #[test]
    fn serializes_like_gearth_packet_info_manager() {
        let single = PacketInfo::from_entries(
            "v",
            vec![PacketInfoEntry {
                direction: Direction::ToServer,
                header_id: 597,
                name: Some("Chat".into()),
                hash: None,
                structure: None,
                source: "Sulek".into(),
            }],
            true,
        );
        let packet = single.write_to(PacketBuilder::new(5)).build();
        let mut r = packet.reader();
        assert_eq!(r.read_i32(), Ok(1));
        assert_eq!(r.read_i32(), Ok(597));
        assert_eq!(r.read_string().as_deref(), Ok("NULL"));
        assert_eq!(r.read_string().as_deref(), Ok("Chat"));
        assert_eq!(r.read_string().as_deref(), Ok("NULL"));
        assert_eq!(r.read_bool(), Ok(true));
        assert_eq!(r.read_string().as_deref(), Ok("Sulek"));
        assert!(r.is_at_end());
    }
}
