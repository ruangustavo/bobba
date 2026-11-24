use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use bobba_session::LogEntry;
use rune::Waker;

use crate::packet_log::{DEFAULT_CAPACITY, PacketLog};

#[derive(Clone)]
pub struct LogFeed(Rc<RefCell<Feed>>);

struct Feed {
    log: PacketLog,
    paused: bool,
    owner: Option<Waker>,
    viewer: Option<Waker>,
    injection: Option<String>,
}

impl LogFeed {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Feed {
            log: PacketLog::new(DEFAULT_CAPACITY),
            paused: false,
            owner: None,
            viewer: None,
            injection: None,
        })))
    }

    pub fn log(&self) -> Ref<'_, PacketLog> {
        Ref::map(self.0.borrow(), |feed| &feed.log)
    }

    pub fn log_mut(&self) -> RefMut<'_, PacketLog> {
        RefMut::map(self.0.borrow_mut(), |feed| &mut feed.log)
    }

    pub fn record(&self, entries: Vec<LogEntry>) {
        let mut feed = self.0.borrow_mut();
        if feed.paused {
            return;
        }
        feed.log.push(entries);
        if let Some(wake) = &feed.viewer {
            wake();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.0.borrow().paused
    }

    pub fn toggle_pause(&self) {
        let mut feed = self.0.borrow_mut();
        feed.paused = !feed.paused;
    }

    pub fn serve(&self, owner: Waker) {
        self.0.borrow_mut().owner = Some(owner);
    }

    pub fn watch(&self, viewer: Waker) {
        self.0.borrow_mut().viewer = Some(viewer);
    }

    pub fn request_injection(&self, expression: String) {
        let mut feed = self.0.borrow_mut();
        feed.injection = Some(expression);
        if let Some(wake) = &feed.owner {
            wake();
        }
    }

    pub fn take_injection(&self) -> Option<String> {
        self.0.borrow_mut().injection.take()
    }
}

impl Default for LogFeed {
    fn default() -> Self {
        Self::new()
    }
}
