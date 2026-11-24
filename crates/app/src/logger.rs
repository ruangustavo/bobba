use rune::{AppSpec, Key, KeyEvent, TextEdit, Titlebar, Tx, Waker, WindowHandle, WindowSpec};

use crate::log_feed::LogFeed;
use crate::packet_log::{Directions, Filter, LogRow};
use crate::view::chrome::TITLEBAR_HEIGHT;

const WINDOW: (f32, f32) = (840.0, 580.0);
pub const LIST: &str = "packets";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sheet {
    pub number: u64,
    pub copied: bool,
}

pub struct Logger {
    pub feed: LogFeed,
    pub filter: TextEdit,
    pub autoscroll: bool,
    pub directions: Directions,
    pub sheet: Option<Sheet>,
    pub view_open: bool,
    seen: usize,
}

impl Logger {
    pub fn new(feed: LogFeed) -> Self {
        let seen = feed.log().len();
        Self {
            feed,
            filter: TextEdit::new(""),
            autoscroll: true,
            directions: Directions::ALL,
            sheet: None,
            view_open: false,
            seen,
        }
    }

    fn detached() -> Self {
        Self::new(LogFeed::new())
    }

    pub fn set_filter(&mut self) {
        self.feed
            .log_mut()
            .set_filter(Filter::new(&self.filter.text).directions(self.directions));
    }

    pub fn toggle_incoming(&mut self) {
        self.directions.incoming = !self.directions.incoming;
        self.set_filter();
    }

    pub fn toggle_outgoing(&mut self) {
        self.directions.outgoing = !self.directions.outgoing;
        self.set_filter();
    }

    pub fn clear(&mut self) {
        self.feed.log_mut().clear();
        self.sheet = None;
    }

    pub fn open(&mut self, number: u64) {
        self.sheet = Some(Sheet {
            number,
            copied: false,
        });
    }

    pub fn close(&mut self) {
        self.sheet = None;
    }

    pub fn opened_row(&self) -> Option<LogRow> {
        let sheet = self.sheet?;
        self.feed.log().row_by_number(sheet.number).cloned()
    }

    pub fn copy(&mut self, expression: String, tx: &mut Tx) {
        tx.copy(expression);
        if let Some(sheet) = &mut self.sheet {
            sheet.copied = true;
        }
    }

    pub fn use_in_injection(&mut self, expression: String) {
        self.feed.request_injection(expression);
    }
}

pub fn on_key(s: &mut Logger, e: &KeyEvent, _: &mut Tx) -> bool {
    if e.key != Key::Escape || s.sheet.is_none() {
        return false;
    }
    s.close();
    true
}

fn setup(s: &mut Logger, wake: Waker) {
    s.feed.watch(wake);
}

fn poll(s: &mut Logger, _: f64, tx: &mut Tx) -> bool {
    let len = s.feed.log().len();
    if len == s.seen {
        return false;
    }
    if s.autoscroll && len > s.seen {
        tx.scroll_to_bottom(LIST);
    }
    s.seen = len;
    true
}

pub fn open(feed: &LogFeed) -> WindowHandle {
    rune::open_window(
        WindowSpec::new(
            "logger",
            AppSpec {
                min_size: Some((720.0, 420.0)),
                setup: Some(setup),
                poll: Some(poll),
                on_key: Some(on_key),
                ..AppSpec::new(
                    "Bobba Logger",
                    WINDOW,
                    Logger::detached,
                    crate::view::logger::root,
                )
            },
        )
        .titlebar(Titlebar::new(TITLEBAR_HEIGHT))
        .state(Logger::new(feed.clone())),
    )
}
