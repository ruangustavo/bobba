use bobba_protocol::{Direction, Packet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Forwarded,
    Edited { by: String, sent: Packet },
    Blocked { by: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEntry {
    Intercepted {
        index: u32,
        direction: Direction,
        packet: Packet,
        outcome: Outcome,
    },
    Injected {
        direction: Direction,
        packet: Packet,
        by: String,
    },
}

impl LogEntry {
    pub fn direction(&self) -> Direction {
        match self {
            LogEntry::Intercepted { direction, .. } | LogEntry::Injected { direction, .. } => {
                *direction
            }
        }
    }

    pub fn packet(&self) -> &Packet {
        match self {
            LogEntry::Intercepted { packet, .. } | LogEntry::Injected { packet, .. } => packet,
        }
    }
}
