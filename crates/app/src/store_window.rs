use bobba_store::Listing;
use rune::{AppSpec, Key, KeyEvent, TextEdit, Titlebar, Tx, Waker, WindowHandle, WindowSpec};

use crate::backend::InstalledState;
use crate::store::{CatalogState, StoreState};
use crate::store_feed::{StoreEvent, StoreFeed, StoreRequest};
use crate::view::chrome::TITLEBAR_HEIGHT;

const WINDOW: (f32, f32) = (720.0, 520.0);
const MIN_SIZE: (f32, f32) = (640.0, 440.0);

pub struct StoreWindow {
    pub feed: StoreFeed,
    pub store: StoreState,
    pub installed: Vec<InstalledState>,
    pub query: TextEdit,
    pub category: Option<String>,
    pub detail: Option<String>,
}

impl StoreWindow {
    pub fn new(feed: StoreFeed, store: StoreState, installed: Vec<InstalledState>) -> Self {
        Self {
            feed,
            store,
            installed,
            query: TextEdit::new(""),
            category: None,
            detail: None,
        }
    }

    fn detached() -> Self {
        Self::new(StoreFeed::new(), StoreState::default(), Vec::new())
    }

    pub fn apply(&mut self, event: StoreEvent) {
        match event {
            StoreEvent::Catalog(event) => self.store.handle_catalog(event),
            StoreEvent::MissingRuntimes(missing) => self.store.handle_missing(missing),
            StoreEvent::Install { title, progress } => self.store.handle_install(title, progress),
            StoreEvent::Installed(installed) => self.installed = installed,
            StoreEvent::Icon { title, pixels } => self.store.show_icon(title, pixels),
            StoreEvent::Show(title) => self.show(title),
        }
    }

    pub fn show(&mut self, title: String) {
        self.category = None;
        self.query = TextEdit::new("");
        self.detail = Some(title);
    }

    pub fn visible(&self) -> Vec<&Listing> {
        self.store
            .visible(self.category.as_deref(), &self.query.text)
    }

    pub fn open_detail(&self, visible: &[&Listing]) -> Option<String> {
        self.detail
            .clone()
            .filter(|title| visible.iter().any(|l| &l.title == title))
    }

    pub fn toggle_detail(&mut self, title: &str) {
        self.detail = if self.detail.as_deref() == Some(title) {
            None
        } else {
            Some(title.to_owned())
        };
    }

    pub fn install(&mut self, title: String) {
        self.feed.request(StoreRequest::Install(title));
    }

    pub fn load_catalog(&mut self) {
        if self.store.needs_catalog() {
            self.store.catalog = CatalogState::Loading;
            self.feed.request(StoreRequest::LoadCatalog);
        }
    }

    pub fn check_runtimes(&mut self) {
        if self.store.misses_runtimes() {
            self.feed.request(StoreRequest::CheckRuntimes);
        }
    }
}

pub fn on_key(s: &mut StoreWindow, e: &KeyEvent, tx: &mut Tx) -> bool {
    if e.key != Key::Escape || s.detail.is_none() {
        return false;
    }
    s.detail = None;
    tx.instant();
    true
}

fn setup(s: &mut StoreWindow, wake: Waker) {
    s.feed.watch(wake);
}

pub fn poll(s: &mut StoreWindow, _: f64, _: &mut Tx) -> bool {
    let events = s.feed.take_events();
    let changed = !events.is_empty();
    for event in events {
        s.apply(event);
    }
    changed
}

pub fn open(window: StoreWindow) -> WindowHandle {
    rune::open_window(
        WindowSpec::new(
            "store",
            AppSpec {
                min_size: Some(MIN_SIZE),
                setup: Some(setup),
                poll: Some(poll),
                on_key: Some(on_key),
                ..AppSpec::new(
                    "Bobba Store",
                    WINDOW,
                    StoreWindow::detached,
                    crate::view::store::root,
                )
            },
        )
        .titlebar(Titlebar::new(TITLEBAR_HEIGHT))
        .state(window),
    )
}

#[cfg(test)]
mod tests {
    use bobba_store::{Catalog, Origin, Os};

    use super::*;
    use crate::backend::CatalogEvent;

    const CONFIG: &str =
        r#"{"categories": [{"name": "Building"}, {"name": "Trading"}], "frameworks": []}"#;
    const EXTENSIONS: &str = r#"[
        {"title": "Room Mapper", "description": "Draws the floor plan", "version": "2.0.1",
         "categories": ["Building"], "framework": {"name": "Native"}, "rating": 9,
         "commands": {"default": ["java", "-jar", "m.jar"]},
         "compatibility": {"systems": ["Mac", "Windows"], "clients": ["Flash"]}},
        {"title": "Trade Ledger", "description": "Logs each trade", "version": "0.9.4",
         "categories": ["Trading"], "framework": {"name": "Native"}, "rating": 5,
         "commands": {"default": ["java", "-jar", "t.jar"]},
         "compatibility": {"systems": ["Mac", "Windows"], "clients": ["Flash"]}}
    ]"#;

    fn loaded() -> StoreWindow {
        let mut window = StoreWindow::detached();
        let catalog = Catalog::parse(CONFIG, EXTENSIONS, Os::Mac).expect("parses");
        window.apply(StoreEvent::Catalog(CatalogEvent::Loaded {
            catalog,
            origin: Origin::Fresh,
        }));
        window
    }

    #[test]
    fn the_detail_closes_when_its_listing_is_filtered_out() {
        let mut window = loaded();
        window.detail = Some("Room Mapper".into());
        assert_eq!(
            window.open_detail(&window.visible()).as_deref(),
            Some("Room Mapper")
        );
        window.category = Some("Trading".into());
        assert_eq!(window.open_detail(&window.visible()), None);
    }

    #[test]
    fn showing_a_listing_clears_the_filters_that_would_hide_it() {
        let mut window = loaded();
        window.category = Some("Trading".into());
        window.query = TextEdit::new("ledger");
        window.apply(StoreEvent::Show("Room Mapper".into()));
        assert_eq!(
            window.open_detail(&window.visible()).as_deref(),
            Some("Room Mapper")
        );
    }

    #[test]
    fn asking_for_the_catalog_twice_sends_one_request() {
        let mut window = StoreWindow::detached();
        window.load_catalog();
        window.load_catalog();
        assert_eq!(window.feed.take_requests(), [StoreRequest::LoadCatalog]);
        assert_eq!(window.store.catalog, CatalogState::Loading);
    }
}
