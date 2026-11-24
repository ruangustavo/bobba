use std::collections::{HashMap, HashSet};

use bobba_store::{Catalog, Listing, Origin, Runtime, is_newer};
use rune::TextEdit;

use crate::backend::{CatalogEvent, InstalledKind, InstalledState, StoreInstall};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogState {
    NotLoaded,
    Loading,
    Loaded { catalog: Catalog, origin: Origin },
    Unreachable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Showing {
    Installed,
    Store,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingStatus {
    Available,
    Downloading,
    Installed,
    Update { installed: String },
    NeedsRuntime { first: Runtime, rest: Vec<Runtime> },
    Failed(String),
}

pub struct StoreState {
    pub catalog: CatalogState,
    pub showing: Showing,
    pub detail: Option<String>,
    pub category: Option<String>,
    pub query: TextEdit,
    installs: HashMap<String, StoreInstall>,
    missing_runtimes: HashSet<Runtime>,
}

impl Default for StoreState {
    fn default() -> Self {
        Self {
            catalog: CatalogState::NotLoaded,
            showing: Showing::Installed,
            detail: None,
            category: None,
            query: TextEdit::new(""),
            installs: HashMap::new(),
            missing_runtimes: HashSet::new(),
        }
    }
}

impl StoreState {
    pub fn handle_catalog(&mut self, event: CatalogEvent) {
        self.catalog = match event {
            CatalogEvent::Loaded { catalog, origin } => CatalogState::Loaded { catalog, origin },
            CatalogEvent::Unreachable(reason) => CatalogState::Unreachable(reason),
        };
    }

    pub fn handle_install(&mut self, title: String, progress: StoreInstall) {
        match progress {
            StoreInstall::Installed => {
                self.installs.remove(&title);
            }
            progress => {
                self.installs.insert(title, progress);
            }
        }
    }

    pub fn handle_missing(&mut self, missing: HashSet<Runtime>) {
        self.missing_runtimes = missing;
    }

    pub fn misses_runtimes(&self) -> bool {
        !self.missing_runtimes.is_empty()
    }

    pub fn needs_catalog(&self) -> bool {
        matches!(
            self.catalog,
            CatalogState::NotLoaded | CatalogState::Unreachable(_)
        )
    }

    pub fn catalog(&self) -> Option<&Catalog> {
        match &self.catalog {
            CatalogState::Loaded { catalog, .. } => Some(catalog),
            _ => None,
        }
    }

    pub fn visible(&self) -> Vec<&Listing> {
        let Some(catalog) = self.catalog() else {
            return Vec::new();
        };
        let query = self.query.text.trim().to_lowercase();
        catalog
            .listings
            .iter()
            .filter(|l| {
                self.category
                    .as_ref()
                    .is_none_or(|c| l.categories.contains(c))
            })
            .filter(|l| {
                query.is_empty()
                    || l.title.to_lowercase().contains(&query)
                    || l.description.to_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn open_detail(&self, visible: &[&Listing]) -> Option<String> {
        self.detail
            .clone()
            .filter(|title| visible.iter().any(|l| &l.title == title))
    }

    pub fn status(&self, listing: &Listing, installed: &[InstalledState]) -> ListingStatus {
        match self.installs.get(&listing.title) {
            Some(StoreInstall::Downloading) => return ListingStatus::Downloading,
            Some(StoreInstall::Failed(reason)) => return ListingStatus::Failed(reason.clone()),
            Some(StoreInstall::Installed) | None => {}
        }
        let local = installed.iter().find_map(|state| match &state.kind {
            InstalledKind::Package { title, version } if title == &listing.title => {
                Some(version.clone())
            }
            _ => None,
        });
        match local {
            Some(version) if is_newer(&listing.version, &version) => {
                ListingStatus::Update { installed: version }
            }
            Some(_) => ListingStatus::Installed,
            None => {
                let mut missing = listing
                    .runtimes()
                    .into_iter()
                    .filter(|runtime| self.missing_runtimes.contains(runtime));
                match missing.next() {
                    Some(first) => ListingStatus::NeedsRuntime {
                        first,
                        rest: missing.collect(),
                    },
                    None => ListingStatus::Available,
                }
            }
        }
    }

    pub fn update_for(&self, title: &str, version: &str) -> Option<&Listing> {
        self.catalog()?
            .listing(title)
            .filter(|listing| is_newer(&listing.version, version))
    }
}

#[cfg(test)]
mod tests {
    use bobba_extensions::ProcessStatus;
    use std::collections::HashSet;

    use bobba_store::{Os, Runtime};

    use super::*;

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

    fn loaded() -> StoreState {
        let mut store = StoreState::default();
        let catalog = Catalog::parse(CONFIG, EXTENSIONS, Os::Mac).expect("parses");
        store.handle_catalog(CatalogEvent::Loaded {
            catalog,
            origin: Origin::Fresh,
        });
        store
    }

    fn package(title: &str, version: &str) -> InstalledState {
        InstalledState {
            id: format!("{title}_{version}"),
            kind: InstalledKind::Package {
                title: title.into(),
                version: version.into(),
            },
            status: ProcessStatus::Running,
        }
    }

    fn titles(store: &StoreState) -> Vec<&str> {
        store.visible().iter().map(|l| l.title.as_str()).collect()
    }

    #[test]
    fn filters_by_category_and_search() {
        let mut store = loaded();
        assert_eq!(titles(&store), ["Room Mapper", "Trade Ledger"]);
        store.category = Some("Trading".into());
        assert_eq!(titles(&store), ["Trade Ledger"]);
        store.category = None;
        store.query = TextEdit::new("floor");
        assert_eq!(titles(&store), ["Room Mapper"]);
    }

    #[test]
    fn status_follows_downloads_and_installed_versions() {
        let mut store = loaded();
        let mapper = store.catalog().expect("loaded").listings[0].clone();
        assert_eq!(store.status(&mapper, &[]), ListingStatus::Available);

        store.handle_install("Room Mapper".into(), StoreInstall::Downloading);
        assert_eq!(store.status(&mapper, &[]), ListingStatus::Downloading);

        store.handle_install("Room Mapper".into(), StoreInstall::Installed);
        assert_eq!(
            store.status(&mapper, &[package("Room Mapper", "2.0.1")]),
            ListingStatus::Installed
        );
        assert_eq!(
            store.status(&mapper, &[package("Room Mapper", "1.9")]),
            ListingStatus::Update {
                installed: "1.9".into()
            }
        );

        store.handle_install("Room Mapper".into(), StoreInstall::Failed("404".into()));
        assert_eq!(
            store.status(&mapper, &[]),
            ListingStatus::Failed("404".into())
        );
    }

    #[test]
    fn the_detail_closes_when_its_listing_is_filtered_out() {
        let mut store = loaded();
        store.detail = Some("Room Mapper".into());
        assert_eq!(
            store.open_detail(&store.visible()).as_deref(),
            Some("Room Mapper")
        );
        store.category = Some("Trading".into());
        assert_eq!(store.open_detail(&store.visible()), None);
    }

    #[test]
    fn a_listing_whose_program_is_missing_points_to_its_runtime() {
        let mut store = loaded();
        let mapper = store.catalog().expect("loaded").listings[0].clone();

        store.handle_missing(HashSet::from([Runtime::Java]));
        assert_eq!(
            store.status(&mapper, &[]),
            ListingStatus::NeedsRuntime {
                first: Runtime::Java,
                rest: vec![],
            }
        );
        assert_eq!(
            store.status(&mapper, &[package("Room Mapper", "2.0.1")]),
            ListingStatus::Installed
        );

        store.handle_missing(HashSet::new());
        assert_eq!(store.status(&mapper, &[]), ListingStatus::Available);
    }

    #[test]
    fn a_node_extension_started_by_java_needs_both() {
        let mut store = StoreState::default();
        let extensions = EXTENSIONS.replacen(
            r#""framework": {"name": "Native"}"#,
            r#""framework": {"name": "G-Node"}"#,
            1,
        );
        let catalog = Catalog::parse(CONFIG, &extensions, Os::Mac).expect("parses");
        store.handle_catalog(CatalogEvent::Loaded {
            catalog,
            origin: Origin::Fresh,
        });
        let mapper = store.catalog().expect("loaded").listings[0].clone();

        store.handle_missing(HashSet::from([Runtime::Java, Runtime::NodeJs]));
        assert_eq!(
            store.status(&mapper, &[]),
            ListingStatus::NeedsRuntime {
                first: Runtime::Java,
                rest: vec![Runtime::NodeJs],
            }
        );
        store.handle_missing(HashSet::from([Runtime::NodeJs]));
        assert_eq!(
            store.status(&mapper, &[]),
            ListingStatus::NeedsRuntime {
                first: Runtime::NodeJs,
                rest: vec![],
            }
        );
    }

    #[test]
    fn an_unreachable_store_is_tried_again() {
        let mut store = StoreState::default();
        assert!(store.needs_catalog());
        store.handle_catalog(CatalogEvent::Unreachable("offline".into()));
        assert!(store.needs_catalog());
        assert!(!loaded().needs_catalog());
    }
}
