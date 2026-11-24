pub mod backend;
pub mod packet_log;
pub mod scenes;
pub mod state;
pub mod view;

use rune::Tx;

use crate::state::App;

pub fn poll(s: &mut App, _: f64, tx: &mut Tx) -> bool {
    let before = s.log.len();
    let changed = s.drain_events();
    if s.follow
        && s.log.len() > before
        && let Some(last) = s.log.number_at(s.log.len() - 1)
    {
        tx.scroll_into_view_in("packets", last.to_string());
    }
    changed
}
