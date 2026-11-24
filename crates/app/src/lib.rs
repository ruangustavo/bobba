pub mod backend;
pub mod log_feed;
pub mod logger;
pub mod packet_log;
pub mod recent;
pub mod repeat;
pub mod scenes;
pub mod setup;
pub mod state;
pub mod store;
pub mod view;

use rune::Tx;

use crate::state::App;

pub fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    let events = s.drain_events();
    let requests = s.take_logger_requests();
    let repeats = s.fire_repeats(clock);
    if let Some(due) = s.repeats.next_due() {
        tx.wake_at(due);
    }
    events || requests || repeats
}
