use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use bobba_store::Runtime;
use rune::{Pixels, Waker};

use crate::backend::{CatalogEvent, InstalledState, StoreInstall};

#[derive(Debug, Clone)]
pub enum StoreEvent {
    Catalog(CatalogEvent),
    MissingRuntimes(HashSet<Runtime>),
    Install {
        title: String,
        progress: StoreInstall,
    },
    Installed(Vec<InstalledState>),
    Icon {
        title: String,
        pixels: Pixels,
    },
    Show(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreRequest {
    LoadCatalog,
    CheckRuntimes,
    Install(String),
}

#[derive(Clone, Default)]
pub struct StoreFeed(Rc<RefCell<Feed>>);

#[derive(Default)]
struct Feed {
    events: Vec<StoreEvent>,
    requests: Vec<StoreRequest>,
    owner: Option<Waker>,
    viewer: Option<Waker>,
}

impl StoreFeed {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn serve(&self, owner: Waker) {
        self.0.borrow_mut().owner = Some(owner);
    }

    pub fn watch(&self, viewer: Waker) {
        self.0.borrow_mut().viewer = Some(viewer);
    }

    pub fn restart(&self) {
        let mut feed = self.0.borrow_mut();
        feed.events.clear();
        feed.viewer = None;
    }

    pub fn publish(&self, event: StoreEvent) {
        let mut feed = self.0.borrow_mut();
        feed.events.push(event);
        if let Some(wake) = &feed.viewer {
            wake();
        }
    }

    pub fn take_events(&self) -> Vec<StoreEvent> {
        std::mem::take(&mut self.0.borrow_mut().events)
    }

    pub fn request(&self, request: StoreRequest) {
        let mut feed = self.0.borrow_mut();
        feed.requests.push(request);
        if let Some(wake) = &feed.owner {
            wake();
        }
    }

    pub fn take_requests(&self) -> Vec<StoreRequest> {
        std::mem::take(&mut self.0.borrow_mut().requests)
    }
}
