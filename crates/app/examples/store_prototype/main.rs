mod catalog;
mod detail;
mod in_place;
mod peek;
mod picker;
mod tab;

use std::cell::Cell;

use rune::{
    AppSpec, Cx, El, Key, KeyEvent, Spring, SpringPair, Titlebar, Transition, Tx, Waker, col, el,
};

use bobba::scenes::SCENES;
use bobba::state::{App, Tab};
use bobba::view::chrome::{self, TITLEBAR_HEIGHT};
use bobba::view::theme::paint;
use bobba::view::{connection, injection};

const WINDOW: (f32, f32) = (760.0, 540.0);
const SCENE: &str = "extensions";
const ENTER: SpringPair = SpringPair::new(Spring::new(0.25, 0.0), Spring::new(0.2, 0.0));

pub struct Variant {
    pub name: &'static str,
    pub view: fn(&App) -> El,
}

pub const VARIANTS: [Variant; 3] = [
    Variant {
        name: "Slide",
        view: peek::slide,
    },
    Variant {
        name: "Overlay",
        view: peek::overlay,
    },
    Variant {
        name: "Reveal",
        view: peek::reveal,
    },
];

thread_local! {
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
    static MOUNT: Cell<u64> = const { Cell::new(0) };
}

pub fn active() -> usize {
    ACTIVE.with(Cell::get)
}

fn mount() -> u64 {
    MOUNT.with(Cell::get)
}

fn reset(s: &mut App) {
    MOUNT.with(|m| m.set(m.get() + 1));
    catalog::reset(s);
    if std::env::var("PROTO_OPEN").is_ok_and(|v| v == "1") {
        catalog::with(|c| c.open = true);
    }
}

pub fn select(s: &mut App, index: usize) {
    if index < VARIANTS.len() {
        ACTIVE.with(|a| a.set(index));
        reset(s);
    }
}

pub fn replay(s: &mut App) {
    reset(s);
}

pub fn entering(e: El, delay: f32) -> El {
    e.transition(
        Transition::offset(0.0, 8.0)
            .combined(Transition::opacity())
            .springs(ENTER),
    )
    .enter_delay(delay)
}

fn init() -> App {
    let mut s = App::new();
    if let Some((_, apply)) = SCENES.iter().find(|(name, _)| *name == SCENE) {
        apply(&mut s);
    }
    let start = std::env::var("PROTO_V")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map_or(0, |v| v.saturating_sub(1));
    ACTIVE.with(|a| a.set(start.min(VARIANTS.len() - 1)));
    reset(&mut s);
    s
}

fn setup(_: &mut App, _: Waker) {}

fn poll(_: &mut App, clock: f64, tx: &mut Tx) -> bool {
    catalog::poll(clock, tx)
}

fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if e.mods.cmd || e.mods.ctrl || e.mods.alt {
        return false;
    }
    if matches!(e.key, Key::Escape) && catalog::with(|c| c.expanded.is_some()) {
        catalog::with(|c| c.expanded = None);
        if active() == 0 {
            tx.instant();
        }
        return true;
    }
    if s.hotel_port.focused {
        return false;
    }
    let count = VARIANTS.len();
    match &e.key {
        Key::Left => select(s, (active() + count - 1) % count),
        Key::Right => select(s, (active() + 1) % count),
        Key::Text(t) if t.eq_ignore_ascii_case("r") => replay(s),
        Key::Text(t) => match t.parse::<usize>() {
            Ok(n) if (1..=count).contains(&n) => select(s, n - 1),
            _ => return false,
        },
        _ => return false,
    }
    true
}

fn stage(s: &App, layer: &mut Vec<El>) -> El {
    match s.tab {
        Tab::Extensions => (VARIANTS[active()].view)(s),
        Tab::Injection => injection::view(s, layer),
        Tab::Connection => connection::view(s),
    }
}

fn root(s: &App, cx: &mut Cx<App>) -> El {
    let p = paint();
    let mut layer = Vec::new();
    let content = stage(s, &mut layer);
    let page = col()
        .w_full()
        .h_full()
        .fill(p.kit.c.bg_base)
        .child(chrome::titlebar(s, cx))
        .child(el().w_full().h(1.0).fill(p.hairline))
        .child(
            col()
                .key(format!("stage-{}-{}", active(), mount()))
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(content),
        )
        .child(el().w_full().h(1.0).fill(p.hairline))
        .child(chrome::status_bar(s));
    rune_kit::root(
        col().w_full().h_full().child(page).child(picker::view()),
        layer,
    )
}

fn main() {
    rune::App::new(AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Store prototypes", WINDOW, init, root)
    })
    .titlebar(Titlebar::new(TITLEBAR_HEIGHT))
    .run();
}
