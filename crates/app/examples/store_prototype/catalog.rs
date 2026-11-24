use std::cell::RefCell;

use bobba::state::App;
use bobba::view::theme::paint;
use rune::tok::TextStyle;
use rune::{El, Icon, Tx, col, el, icon, oklch, row, text};
use rune_kit::{Button, ButtonVariant, TextInput};

pub static STAR: Icon = Icon {
    name: "proto-star",
    view: 16.0,
    body: r#"<path d="M8 1.9l1.85 3.86 4.2.53-3.08 2.92.78 4.17L8 11.33l-3.75 2.05.78-4.17L1.95 6.29l4.2-.53z" fill="currentColor"/>"#,
};

pub static ALERT: Icon = Icon {
    name: "proto-alert",
    view: 16.0,
    body: r#"<path d="M8 2.2L14.4 13.4H1.6Z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/><path d="M8 6.4v3.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><circle cx="8" cy="11.6" r="0.85" fill="currentColor"/>"#,
};

pub static CLOUD_OFF: Icon = Icon {
    name: "proto-cloud-off",
    view: 16.0,
    body: r#"<path d="M4.6 12.5h7a2.9 2.9 0 0 0 .7-5.7 4.2 4.2 0 0 0-7.6-1.4A3.6 3.6 0 0 0 4.6 12.5z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/><path d="M2.5 2.5l11 11" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
};

pub static UPDATE: Icon = Icon {
    name: "proto-update",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="6.2" fill="none" stroke="currentColor" stroke-width="1.3"/><path d="M8 11V5.2M5.6 7.4L8 5l2.4 2.4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Available,
    Installing,
    Installed,
    Update { installed: &'static str },
    NeedsRuntime { runtime: &'static str },
}

pub struct Listing {
    pub title: &'static str,
    pub summary: &'static str,
    pub description: &'static str,
    pub authors: &'static str,
    pub version: &'static str,
    pub framework: &'static str,
    pub category: &'static str,
    pub rating: u32,
    pub updated: &'static str,
    pub hue: f32,
    pub status: Status,
}

pub const CATEGORIES: [&str; 6] = [
    "Building",
    "Trading",
    "Usability",
    "Moderation",
    "Games",
    "G-Earth Utilities",
];

pub const LISTINGS: [Listing; 10] = [
    Listing {
        title: "Wired Helper",
        summary: "Copy, paste and test wired stacks without guessing",
        description: "Reads the wired in your room and lets you copy a stack to another room, check which triggers fire, and step through effects one at a time.",
        authors: "nightowl, pixelmint",
        version: "1.4.2",
        framework: "Native",
        category: "Building",
        rating: 284,
        updated: "12 Nov 2026",
        hue: 250.0,
        status: Status::Update { installed: "1.3.0" },
    },
    Listing {
        title: "Room Mapper",
        summary: "Exports the floor plan and furni positions as an image",
        description: "Draws the current room from above with every furni at its tile, ready to share or to rebuild the room somewhere else.",
        authors: "tilecraft",
        version: "2.0.1",
        framework: "G-Node",
        category: "Building",
        rating: 231,
        updated: "3 Oct 2026",
        hue: 160.0,
        status: Status::NeedsRuntime { runtime: "Node.js" },
    },
    Listing {
        title: "Trade Ledger",
        summary: "Keeps a history of every trade with prices from the marketplace",
        description: "Logs each trade you confirm, values both sides with recent marketplace averages, and warns before you accept a lopsided offer.",
        authors: "goldfinch",
        version: "0.9.4",
        framework: "Native",
        category: "Trading",
        rating: 196,
        updated: "28 Oct 2026",
        hue: 80.0,
        status: Status::Available,
    },
    Listing {
        title: "Chat Translator",
        summary: "Translates the room chat into your language as it arrives",
        description: "Shows a translation under each chat bubble and can translate what you type before it is sent. Pick the languages in its window.",
        authors: "lingua",
        version: "1.1.0",
        framework: "G-Python",
        category: "Usability",
        rating: 171,
        updated: "19 Sep 2026",
        hue: 300.0,
        status: Status::Available,
    },
    Listing {
        title: "Furni Stacker",
        summary: "Places furni at exact heights with a stack tile helper",
        description: "Sets the stack height in 0.01 steps and remembers the heights you use the most, so tall builds stop being trial and error.",
        authors: "nightowl",
        version: "3.2.0",
        framework: "Native",
        category: "Building",
        rating: 158,
        updated: "1 Nov 2026",
        hue: 30.0,
        status: Status::Installing,
    },
    Listing {
        title: "QX Scripter",
        summary: "C# scripting with an editor and community scripts",
        description: "Write small C# scripts against the room, the chat and your inventory, or start from scripts shared by the community. Opens its own window.",
        authors: "qx tools",
        version: "0.1.41",
        framework: "QX",
        category: "G-Earth Utilities",
        rating: 148,
        updated: "31 Oct 2026",
        hue: 200.0,
        status: Status::Available,
    },
    Listing {
        title: "AntiSpam",
        summary: "Hides repeated and advertising messages in the chat",
        description: "Learns the usual spam patterns and hides them before they reach your chat. Hidden messages stay one click away.",
        authors: "quietroom",
        version: "1.2.0",
        framework: "G-Python",
        category: "Moderation",
        rating: 133,
        updated: "8 Aug 2026",
        hue: 20.0,
        status: Status::Installed,
    },
    Listing {
        title: "Badge Inspector",
        summary: "Shows the badges of anyone in the room, with dates",
        description: "Click a user to see every badge they wear and when each was earned, with filters for rare and event badges.",
        authors: "collectorz",
        version: "1.0.3",
        framework: "Native",
        category: "Usability",
        rating: 119,
        updated: "22 Jul 2026",
        hue: 110.0,
        status: Status::Available,
    },
    Listing {
        title: "Snowstorm Scoreboard",
        summary: "Live scores and hit counts for Snowstorm matches",
        description: "Follows a Snowstorm match and shows a live table with hits, throws and the score of each team.",
        authors: "arcade crew",
        version: "0.6.0",
        framework: "G-Node",
        category: "Games",
        rating: 92,
        updated: "14 Jun 2026",
        hue: 230.0,
        status: Status::Available,
    },
    Listing {
        title: "Door Guard",
        summary: "Lets only your list of friends into the room",
        description: "Keeps an allow list for your room and kicks anyone else who walks in, with a log of who tried.",
        authors: "quietroom",
        version: "2.3.1",
        framework: "Native",
        category: "Moderation",
        rating: 64,
        updated: "2 May 2026",
        hue: 350.0,
        status: Status::Available,
    },
];

pub struct Session {
    pub statuses: Vec<Status>,
    pub category: Option<&'static str>,
    pub expanded: Option<usize>,
    pub open: bool,
    pub installing_until: Vec<(usize, f64)>,
}

impl Session {
    fn new() -> Self {
        Self {
            statuses: LISTINGS.iter().map(|l| l.status).collect(),
            category: None,
            expanded: None,
            open: false,
            installing_until: Vec::new(),
        }
    }
}

thread_local! {
    static SESSION: RefCell<Session> = RefCell::new(Session::new());
}

pub fn with<R>(f: impl FnOnce(&mut Session) -> R) -> R {
    SESSION.with(|s| f(&mut s.borrow_mut()))
}

pub fn reset(s: &mut App) {
    with(|session| *session = Session::new());
    s.hotel_port = rune::TextEdit::new("");
}

pub fn offline() -> bool {
    std::env::var("PROTO_OFFLINE").is_ok_and(|v| v == "1")
}

pub fn status(index: usize) -> Status {
    with(|s| s.statuses[index])
}

pub fn query(s: &App) -> String {
    s.hotel_port.text.trim().to_lowercase()
}

pub fn visible(s: &App) -> Vec<usize> {
    let query = query(s);
    let category = with(|s| s.category);
    LISTINGS
        .iter()
        .enumerate()
        .filter(|(_, l)| category.is_none_or(|c| c == l.category))
        .filter(|(_, l)| {
            query.is_empty()
                || l.title.to_lowercase().contains(&query)
                || l.summary.to_lowercase().contains(&query)
        })
        .map(|(i, _)| i)
        .collect()
}

pub fn installed() -> Vec<usize> {
    (0..LISTINGS.len())
        .filter(|&i| matches!(status(i), Status::Installed | Status::Update { .. }))
        .collect()
}

pub fn poll(clock: f64, tx: &mut Tx) -> bool {
    with(|s| {
        let (done, waiting): (Vec<_>, Vec<_>) =
            s.installing_until.iter().partition(|(_, at)| *at <= clock);
        s.installing_until = waiting;
        for (index, _) in &done {
            s.statuses[*index] = Status::Installed;
        }
        if let Some(next) = s
            .installing_until
            .iter()
            .map(|(_, at)| *at)
            .reduce(f64::min)
        {
            tx.wake_after(std::time::Duration::from_secs_f64((next - clock).max(0.0)));
        }
        !done.is_empty()
    })
}

fn install(index: usize, tx: &mut Tx) {
    let until = tx.clock() + 1.4;
    with(|s| {
        s.statuses[index] = Status::Installing;
        s.installing_until.push((index, until));
    });
    tx.wake_after(std::time::Duration::from_secs_f64(1.4));
}

pub fn tile(index: usize, size: f32) -> El {
    let listing = &LISTINGS[index];
    let initials: String = listing
        .title
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect();
    row()
        .size(size, size)
        .radius(size * 0.26)
        .fill(oklch(0.42, 0.10, listing.hue))
        .border(1.0, oklch(0.55, 0.10, listing.hue))
        .center()
        .child(
            text(initials)
                .font(TextStyle::new(size * 0.36, 650.0))
                .color(oklch(0.96, 0.03, listing.hue)),
        )
}

pub fn rating(index: usize) -> El {
    let p = paint();
    let k = &p.kit;
    row()
        .gap(3.0)
        .center_y()
        .child(icon(&STAR, 11.0).color(p.amber))
        .child(
            text(LISTINGS[index].rating.to_string())
                .font(k.ty.small)
                .color(k.c.text_muted),
        )
}

pub fn action(index: usize, key: &str) -> El {
    let k = &paint().kit;
    let key = format!("{key}-action-{index}");
    match status(index) {
        Status::Available => Button::new("Install")
            .variant(ButtonVariant::Contrast)
            .on_press_tx(move |_: &mut App, tx: &mut Tx| install(index, tx))
            .build(k)
            .key(key),
        Status::Installing => Button::new("Installing")
            .variant(ButtonVariant::Loading)
            .disabled(true)
            .build(k)
            .key(key),
        Status::Installed => Button::new("Installed")
            .variant(ButtonVariant::GhostMuted)
            .disabled(true)
            .build(k)
            .key(key),
        Status::Update { .. } => Button::new("Update")
            .variant(ButtonVariant::Outline)
            .on_press_tx(move |_: &mut App, tx: &mut Tx| install(index, tx))
            .build(k)
            .key(key),
        Status::NeedsRuntime { runtime } => Button::new(format!("Get {runtime}"))
            .variant(ButtonVariant::Outline)
            .help(format!(
                "{} runs on {runtime}, which is not installed. Opens the install guide.",
                LISTINGS[index].title
            ))
            .on_press_tx(|_: &mut App, tx: &mut Tx| {
                let _ = tx.open_url("https://nodejs.org/en/download");
            })
            .build(k)
            .key(key),
    }
}

pub fn notice(index: usize) -> Option<El> {
    let p = paint();
    let k = &p.kit;
    let (glyph, color, message) = match status(index) {
        Status::NeedsRuntime { runtime } => (
            &ALERT,
            p.amber,
            format!("Needs {runtime}. Install it, then come back to install this extension."),
        ),
        Status::Update { installed } => (
            &UPDATE,
            p.blue,
            format!(
                "You have {installed}. Version {} is available.",
                LISTINGS[index].version
            ),
        ),
        _ => return None,
    };
    Some(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(icon(glyph, 13.0).color(color))
            .child(
                text(message)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap()
                    .grow(1.0)
                    .shrink(),
            ),
    )
}

pub fn search(s: &App, width: Option<f32>) -> El {
    let k = &paint().kit;
    TextInput::new(&s.hotel_port)
        .label("Search extensions")
        .placeholder("Search 73 extensions")
        .leading_icon(k.glyph.search)
        .clear_button()
        .width(width)
        .on_edit(|s: &mut App| &mut s.hotel_port)
        .build(k)
        .key("store-search")
}

fn chip(label: &'static str, value: Option<&'static str>, on: bool) -> El {
    let p = paint();
    let k = &p.kit;
    row()
        .key(format!("store-chip-{label}"))
        .role("button")
        .label(label)
        .h(26.0)
        .px(10.0)
        .radius(13.0)
        .center()
        .fill(if on { k.c.text_base } else { p.panel_raised })
        .border(1.0, if on { k.c.text_base } else { p.hairline })
        .child(text(label).font(k.ty.small_medium).color(if on {
            k.c.bg_base
        } else {
            k.c.text_muted
        }))
        .on_press(move |_: &mut App| with(|s| s.category = value))
}

pub fn chips() -> El {
    let current = with(|s| s.category);
    let chips = CATEGORIES.iter().fold(
        row().gap(6.0).child(chip("All", None, current.is_none())),
        |r, c| r.child(chip(c, Some(c), current == Some(c))),
    );
    row()
        .key("store-chips")
        .w_full()
        .min_w(0.0)
        .scroll_x()
        .scroll_edge_fade(16.0)
        .child(chips)
}

pub fn empty(query: &str) -> El {
    let k = &paint().kit;
    col()
        .key("store-empty")
        .w_full()
        .padding(32.0)
        .gap(6.0)
        .center()
        .child(
            text(format!("No extensions match “{query}”"))
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        )
        .child(
            text("Try another word or pick All categories.")
                .font(k.ty.small)
                .color(k.c.text_muted),
        )
}

pub fn offline_state() -> El {
    let p = paint();
    let k = &p.kit;
    col()
        .key("store-offline")
        .w_full()
        .grow(1.0)
        .padding(32.0)
        .gap(10.0)
        .center()
        .child(icon(&CLOUD_OFF, 28.0).color(k.c.text_faint))
        .child(
            text("The store is unreachable")
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        )
        .child(
            text("Bobba could not reach GitHub and has no saved copy of the catalog yet. Your installed extensions keep working.")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap()
                .max_w(360.0),
        )
        .child(
            Button::new("Try again")
                .variant(ButtonVariant::Outline)
                .build(k)
                .key("store-retry"),
        )
}

pub fn divider() -> El {
    el().w_full().h(1.0).fill(paint().hairline)
}
