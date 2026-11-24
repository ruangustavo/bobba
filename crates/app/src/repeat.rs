use bobba_protocol::Direction;

pub const INTERVALS_MS: [u32; 10] = [100, 250, 500, 750, 1000, 1500, 2000, 3000, 5000, 10000];
const DEFAULT_INTERVAL: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval(usize);

impl Interval {
    pub fn ms(self) -> u32 {
        INTERVALS_MS[self.0]
    }

    pub fn seconds(self) -> f64 {
        f64::from(self.ms()) / 1000.0
    }

    pub fn shorter(self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }

    pub fn longer(self) -> Option<Self> {
        (self.0 + 1 < INTERVALS_MS.len()).then_some(Self(self.0 + 1))
    }

    pub fn label(self) -> String {
        match self.ms() {
            ms if ms < 1000 => format!("{ms} ms"),
            ms if ms % 1000 == 0 => format!("{} s", ms / 1000),
            ms => format!("{:.1} s", f64::from(ms) / 1000.0),
        }
    }
}

impl Default for Interval {
    fn default() -> Self {
        Self(DEFAULT_INTERVAL)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Running { next_at: f64 },
    Paused,
    Stopped(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Repeat {
    pub id: u64,
    pub label: String,
    pub expression: String,
    pub direction: Direction,
    pub every: Interval,
    pub sent: u64,
    pub state: State,
}

impl Repeat {
    pub fn is_running(&self) -> bool {
        matches!(self.state, State::Running { .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Due {
    pub id: u64,
    pub expression: String,
    pub direction: Direction,
}

#[derive(Debug, Default)]
pub struct Repeats {
    items: Vec<Repeat>,
    next_id: u64,
}

impl Repeats {
    pub fn iter(&self) -> impl Iterator<Item = &Repeat> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn running(&self) -> usize {
        self.items.iter().filter(|r| r.is_running()).count()
    }

    pub fn start(
        &mut self,
        label: String,
        expression: String,
        direction: Direction,
        every: Interval,
        now: f64,
    ) -> u64 {
        self.next_id += 1;
        self.items.push(Repeat {
            id: self.next_id,
            label,
            expression,
            direction,
            every,
            sent: 0,
            state: State::Running { next_at: now },
        });
        self.next_id
    }

    pub fn due(&mut self, now: f64) -> Vec<Due> {
        let mut due = Vec::new();
        for repeat in &mut self.items {
            let State::Running { next_at } = &mut repeat.state else {
                continue;
            };
            if now < *next_at {
                continue;
            }
            while *next_at <= now {
                *next_at += repeat.every.seconds();
            }
            due.push(Due {
                id: repeat.id,
                expression: repeat.expression.clone(),
                direction: repeat.direction,
            });
        }
        due
    }

    pub fn sent(&mut self, id: u64) {
        if let Some(repeat) = self.item(id) {
            repeat.sent += 1;
        }
    }

    pub fn stop(&mut self, id: u64, reason: String) {
        if let Some(repeat) = self.item(id) {
            repeat.state = State::Stopped(reason);
        }
    }

    pub fn toggle(&mut self, id: u64, now: f64) {
        if let Some(repeat) = self.item(id) {
            repeat.state = match repeat.state {
                State::Running { .. } => State::Paused,
                State::Paused | State::Stopped(_) => State::Running { next_at: now },
            };
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|r| r.id != id);
    }

    pub fn pause_all(&mut self) {
        for repeat in &mut self.items {
            if repeat.is_running() {
                repeat.state = State::Paused;
            }
        }
    }

    pub fn next_due(&self) -> Option<f64> {
        self.items
            .iter()
            .filter_map(|r| match r.state {
                State::Running { next_at } => Some(next_at),
                _ => None,
            })
            .reduce(f64::min)
    }

    fn item(&mut self, id: u64) -> Option<&mut Repeat> {
        self.items.iter_mut().find(|r| r.id == id)
    }

    pub fn get(&self, id: u64) -> Option<&Repeat> {
        self.items.iter().find(|r| r.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(repeats: &mut Repeats, every: Interval, now: f64) -> u64 {
        repeats.start(
            "Chat".into(),
            "{out:Chat}{s:\"oi\"}{i:0}{i:-1}".into(),
            Direction::ToServer,
            every,
            now,
        )
    }

    fn ids(due: &[Due]) -> Vec<u64> {
        due.iter().map(|d| d.id).collect()
    }

    #[test]
    fn a_new_repeat_is_due_at_once_then_every_interval() {
        let mut repeats = Repeats::default();
        let id = chat(&mut repeats, Interval::default(), 10.0);
        assert_eq!(ids(&repeats.due(10.0)), [id]);
        assert!(repeats.due(10.2).is_empty());
        assert_eq!(repeats.next_due(), Some(10.5));
        assert_eq!(ids(&repeats.due(10.5)), [id]);
    }

    #[test]
    fn a_late_wake_sends_once_instead_of_catching_up() {
        let mut repeats = Repeats::default();
        let id = chat(&mut repeats, Interval::default(), 0.0);
        repeats.due(0.0);
        assert_eq!(ids(&repeats.due(3.2)), [id]);
        assert_eq!(repeats.next_due(), Some(3.5));
    }

    #[test]
    fn paused_and_stopped_repeats_are_never_due_and_resume_at_once() {
        let mut repeats = Repeats::default();
        let id = chat(&mut repeats, Interval::default(), 0.0);
        repeats.due(0.0);
        repeats.toggle(id, 0.1);
        assert!(repeats.due(5.0).is_empty());
        assert_eq!(repeats.next_due(), None);
        repeats.toggle(id, 6.0);
        assert_eq!(ids(&repeats.due(6.0)), [id]);
        repeats.stop(id, "not connected to a hotel".into());
        assert!(repeats.due(9.0).is_empty());
        assert_eq!(
            repeats.get(id).map(|r| r.state.clone()),
            Some(State::Stopped("not connected to a hotel".into()))
        );
    }

    #[test]
    fn pausing_all_keeps_every_count_and_removing_drops_one() {
        let mut repeats = Repeats::default();
        let first = chat(&mut repeats, Interval::default(), 0.0);
        let second = chat(&mut repeats, Interval::default(), 0.0);
        for due in repeats.due(0.0) {
            repeats.sent(due.id);
        }
        repeats.pause_all();
        assert_eq!(repeats.running(), 0);
        assert_eq!(repeats.get(first).map(|r| r.sent), Some(1));
        repeats.remove(second);
        assert_eq!(repeats.len(), 1);
    }

    #[test]
    fn intervals_step_through_the_presets_within_bounds() {
        let every = Interval::default();
        assert_eq!(every.label(), "500 ms");
        assert_eq!(
            every.longer().map(Interval::label).as_deref(),
            Some("750 ms")
        );
        let mut shortest = every;
        while let Some(shorter) = shortest.shorter() {
            shortest = shorter;
        }
        assert_eq!(shortest.ms(), 100);
        assert_eq!(Interval(INTERVALS_MS.len() - 1).longer(), None);
        assert_eq!(Interval(5).label(), "1.5 s");
    }
}
