#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    ToClient,
    ToServer,
}

impl Direction {
    pub fn gearth_name(self) -> &'static str {
        match self {
            Direction::ToClient => "TOCLIENT",
            Direction::ToServer => "TOSERVER",
        }
    }

    pub fn from_gearth_name(name: &str) -> Option<Self> {
        match name {
            "TOCLIENT" => Some(Direction::ToClient),
            "TOSERVER" => Some(Direction::ToServer),
            _ => None,
        }
    }

    pub fn expression_prefix(self) -> &'static str {
        match self {
            Direction::ToClient => "in",
            Direction::ToServer => "out",
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Direction::ToClient => Direction::ToServer,
            Direction::ToServer => Direction::ToClient,
        }
    }
}
