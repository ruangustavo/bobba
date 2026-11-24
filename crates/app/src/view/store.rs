use bobba_store::{Listing, Origin};
use rune::tok::TextStyle;
use rune::{
    Anim, El, Icon, ScrollSpec, Spring, SpringPair, Transition, Tx, col, el, icon, oklch, row, text,
};
use rune_kit::{Button, ButtonVariant, IconButton, IconButtonVariant, Loader, TextInput};

use crate::state::App;
use crate::store::{CatalogState, ListingStatus};
use crate::view::theme::paint;

const DETAIL_WIDTH: f32 = 340.0;
const SLIDE: SpringPair = SpringPair::new(Spring::new(0.22, 0.0), Spring::new(0.2, 0.0));

static STAR: Icon = Icon {
    name: "bobba-star",
    view: 16.0,
    body: r#"<path d="M8 1.9l1.85 3.86 4.2.53-3.08 2.92.78 4.17L8 11.33l-3.75 2.05.78-4.17L1.95 6.29l4.2-.53z" fill="currentColor"/>"#,
};

static ALERT: Icon = Icon {
    name: "bobba-alert",
    view: 16.0,
    body: r#"<path d="M8 2.2L14.4 13.4H1.6Z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/><path d="M8 6.4v3.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><circle cx="8" cy="11.6" r="0.85" fill="currentColor"/>"#,
};

static UPDATE: Icon = Icon {
    name: "bobba-update",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="6.2" fill="none" stroke="currentColor" stroke-width="1.3"/><path d="M8 11V5.2M5.6 7.4L8 5l2.4 2.4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

static CLOUD_OFF: Icon = Icon {
    name: "bobba-cloud-off",
    view: 16.0,
    body: r#"<path d="M4.6 12.5h7a2.9 2.9 0 0 0 .7-5.7 4.2 4.2 0 0 0-7.6-1.4A3.6 3.6 0 0 0 4.6 12.5z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/><path d="M2.5 2.5l11 11" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
};

fn hue(title: &str) -> f32 {
    let hash = title
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    (hash % 360) as f32
}

pub fn tile(title: &str, size: f32) -> El {
    let hue = hue(title);
    let initials: String = title
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect();
    row()
        .size(size, size)
        .radius(size * 0.26)
        .fill(oklch(0.42, 0.10, hue))
        .border(1.0, oklch(0.55, 0.10, hue))
        .center()
        .child(
            text(initials)
                .font(TextStyle::new(size * 0.36, 650.0))
                .color(oklch(0.96, 0.03, hue)),
        )
}

fn rating(listing: &Listing) -> El {
    let p = paint();
    let k = &p.kit;
    row()
        .gap(3.0)
        .center_y()
        .child(icon(&STAR, 11.0).color(p.amber))
        .child(
            text(listing.rating.to_string())
                .font(k.ty.small)
                .color(k.c.text_muted),
        )
}

fn install(title: &str) -> impl Fn(&mut App) + Clone + 'static {
    let title = title.to_owned();
    move |s: &mut App| s.install_from_store(title.clone())
}

fn action(listing: &Listing, status: &ListingStatus) -> El {
    let k = &paint().kit;
    let key = format!("store-action-{}", listing.title);
    match status {
        ListingStatus::Available => Button::new("Install")
            .variant(ButtonVariant::Contrast)
            .on_press(install(&listing.title))
            .build(k),
        ListingStatus::Downloading => Button::new("Installing")
            .variant(ButtonVariant::Loading)
            .disabled(true)
            .build(k),
        ListingStatus::Installed => Button::new("Installed")
            .variant(ButtonVariant::GhostMuted)
            .disabled(true)
            .build(k),
        ListingStatus::Update { .. } => Button::new("Update")
            .variant(ButtonVariant::Outline)
            .on_press(install(&listing.title))
            .build(k),
        ListingStatus::NeedsRuntime {
            runtime,
            guide: Some(guide),
        } => {
            let guide = guide.clone();
            Button::new(format!("Get {runtime}"))
                .variant(ButtonVariant::Outline)
                .help(format!(
                    "{} needs {runtime}. Opens its install guide.",
                    listing.title
                ))
                .on_press_tx(move |_: &mut App, tx: &mut Tx| {
                    let _ = tx.open_url(&guide);
                })
                .build(k)
        }
        ListingStatus::NeedsRuntime {
            runtime,
            guide: None,
        } => Button::new("Install")
            .variant(ButtonVariant::Outline)
            .help(format!(
                "{} needs {runtime}, which Bobba could not find.",
                listing.title
            ))
            .on_press(install(&listing.title))
            .build(k),
        ListingStatus::Failed(_) => Button::new("Try again")
            .variant(ButtonVariant::Outline)
            .on_press(install(&listing.title))
            .build(k),
    }
    .key(key)
}

fn notice(listing: &Listing, status: &ListingStatus) -> Option<El> {
    let p = paint();
    let k = &p.kit;
    let (glyph, color, message) = match status {
        ListingStatus::Update { installed } => (
            &UPDATE,
            p.blue,
            format!(
                "You have {installed}. Version {} is available.",
                listing.version
            ),
        ),
        ListingStatus::Failed(reason) => (&ALERT, p.red, format!("Could not install: {reason}")),
        ListingStatus::NeedsRuntime { runtime, .. } => (
            &ALERT,
            p.amber,
            format!(
                "Needs {runtime}, which is not installed on this computer. Install it, then come back to install this extension."
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

fn listing_row(s: &App, listing: &Listing, selected: bool) -> El {
    let p = paint();
    let k = &p.kit;
    let status = s.store.status(listing, &s.installed);
    let subtitle = match &status {
        ListingStatus::Failed(reason) => text(format!("Could not install: {reason}")).color(p.red),
        ListingStatus::NeedsRuntime { runtime, .. } => {
            text(format!("Needs {runtime}, which is not installed")).color(p.amber)
        }
        _ => text(listing.description.clone()).color(k.c.text_muted),
    };
    let title = listing.title.clone();
    let action = match status {
        ListingStatus::Available => action(listing, &status).group_opacity(0.0, 1.0),
        _ => action(listing, &status),
    };
    row()
        .key(format!("store-row-{}", listing.title))
        .role("button")
        .label(listing.title.clone())
        .w_full()
        .padding(12.0)
        .gap(12.0)
        .center_y()
        .fill(if selected {
            p.panel_raised
        } else {
            p.panel.alpha(0.0)
        })
        .hover(p.panel_raised)
        .hover_group()
        .hover_motion(Anim::INSTANT, Anim::INSTANT)
        .child(tile(&listing.title, 36.0))
        .child(
            col()
                .gap(3.0)
                .grow(1.0)
                .shrink()
                .child(
                    row()
                        .gap(8.0)
                        .center_y()
                        .child(
                            text(listing.title.clone())
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .truncate(),
                        )
                        .child(rating(listing)),
                )
                .child(subtitle.font(k.ty.small).truncate()),
        )
        .child(action)
        .on_press(move |s: &mut App| s.toggle_store_detail(&title))
}

fn fact(label: &str, value: String) -> El {
    let k = &paint().kit;
    col()
        .gap(2.0)
        .w(120.0)
        .child(
            text(label.to_owned())
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
        .child(
            text(value)
                .font(k.ty.small_medium)
                .color(k.c.text_base)
                .truncate(),
        )
}

fn detail(s: &App, listing: &Listing) -> El {
    let p = paint();
    let k = &p.kit;
    let status = s.store.status(listing, &s.installed);
    let head = row()
        .w_full()
        .gap(14.0)
        .center_y()
        .child(tile(&listing.title, 44.0))
        .child(
            col()
                .gap(4.0)
                .grow(1.0)
                .shrink()
                .child(
                    text(listing.title.clone())
                        .key("store-detail-title")
                        .font(TextStyle::new(16.0, 650.0))
                        .color(k.c.text_base)
                        .truncate(),
                )
                .child(
                    text(format!("by {}", listing.authors.join(", ")))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .truncate(),
                ),
        )
        .child(action(listing, &status).key("store-detail-action"));
    let facts = row()
        .w_full()
        .gap(12.0)
        .flex_wrap()
        .child(fact("Version", listing.version.clone()))
        .child(fact("Framework", listing.framework.clone()))
        .child(fact("Category", listing.categories.join(", ")))
        .child(fact(
            "Updated",
            listing
                .updated
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned(),
        ));
    let source = listing.source.clone();
    let source_link = (!source.is_empty()).then(|| {
        Button::new("View source")
            .variant(ButtonVariant::GhostMuted)
            .on_press_tx(move |_: &mut App, tx: &mut Tx| {
                let _ = tx.open_url(&source);
            })
            .build(k)
            .key("store-detail-source")
    });
    col()
        .key(format!("store-detail-{}", listing.title))
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(20.0)
        .pt(0.0)
        .gap(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .transition(Transition::none())
        .child(head)
        .children(notice(listing, &status))
        .child(
            text(listing.description.clone())
                .font(k.ty.body)
                .color(k.c.text_base)
                .wrap(),
        )
        .child(el().w_full().h(1.0).fill(p.hairline))
        .child(facts)
        .child(
            row()
                .w_full()
                .gap(6.0)
                .center_y()
                .child(rating(listing))
                .child(
                    text("on the G-Earth Discord")
                        .font(k.ty.small)
                        .color(k.c.text_faint)
                        .grow(1.0),
                )
                .children(source_link),
        )
}

fn panel(s: &App, listing: &Listing) -> El {
    let p = paint();
    let k = &p.kit;
    col()
        .key("store-detail")
        .w(DETAIL_WIDTH)
        .h_full()
        .min_h(0.0)
        .fill(p.panel_raised)
        .border_sides([0.0, 0.0, 0.0, 1.0], p.hairline)
        .transition(
            Transition::offset(DETAIL_WIDTH, 0.0)
                .combined(Transition::opacity())
                .springs(SLIDE)
                .hold_layout(),
        )
        .child(
            row().w_full().pt(8.0).pr(8.0).justify_end().child(
                IconButton::new(k.glyph.close)
                    .variant(IconButtonVariant::Ghost)
                    .label("Close details")
                    .help("Close details (Esc)")
                    .on_press(|s: &mut App| s.store.detail = None)
                    .build(k)
                    .key("store-detail-close"),
            ),
        )
        .child(detail(s, listing))
}

fn chip(label: String, value: Option<String>, on: bool) -> El {
    let p = paint();
    let k = &p.kit;
    row()
        .key(format!("store-chip-{label}"))
        .role("button")
        .label(label.clone())
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
        .on_press(move |s: &mut App| s.store.category = value.clone())
}

fn toolbar(s: &App) -> El {
    let k = &paint().kit;
    let count = s.store.catalog().map_or(0, |c| c.listings.len());
    let current = s.store.category.clone();
    let categories = s
        .store
        .catalog()
        .map_or_else(Vec::new, |c| c.categories.clone());
    let chips = categories.into_iter().fold(
        row()
            .gap(6.0)
            .child(chip("All".into(), None, current.is_none())),
        |r, category| {
            let on = current.as_deref() == Some(category.name.as_str());
            r.child(chip(category.name.clone(), Some(category.name), on))
        },
    );
    row()
        .key("store-toolbar")
        .w_full()
        .gap(12.0)
        .center_y()
        .child(
            TextInput::new(&s.store.query)
                .label("Search extensions")
                .placeholder(format!("Search {count} extensions"))
                .leading_icon(k.glyph.search)
                .clear_button()
                .width(Some(220.0))
                .on_edit(|s: &mut App| &mut s.store.query)
                .build(k)
                .key("store-search"),
        )
        .child(
            col().grow(1.0).min_w(0.0).child(
                row()
                    .key("store-chips")
                    .w_full()
                    .min_w(0.0)
                    .scroll_x()
                    .scroll_edge_fade(16.0)
                    .child(chips),
            ),
        )
}

fn frame() -> El {
    let p = paint();
    col()
        .key("store-panel")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .radius(12.0)
        .fill(p.panel)
        .border(1.0, p.hairline)
        .clip_rounded(12.0)
}

fn message(key: &str, glyph: Option<&'static Icon>, title: &str, body: String) -> El {
    let k = &paint().kit;
    col()
        .key(key.to_owned())
        .w_full()
        .grow(1.0)
        .padding(32.0)
        .gap(10.0)
        .center()
        .children(glyph.map(|g| icon(g, 28.0).color(k.c.text_faint)))
        .child(
            text(title.to_owned())
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        )
        .child(
            text(body)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap()
                .max_w(380.0),
        )
}

fn listings(s: &App) -> El {
    let visible = s.store.visible();
    if visible.is_empty() {
        let query = s.store.query.text.trim();
        let body = if query.is_empty() {
            "Nothing in this category runs on this computer yet.".to_owned()
        } else {
            format!("Nothing matches “{query}”. Try another word or pick All.")
        };
        return frame().child(message("store-empty", None, "No extensions found", body));
    }
    let open = s.store.open_detail(&visible);
    let rows = visible
        .iter()
        .enumerate()
        .fold(col().w_full(), |c, (i, listing)| {
            let c = if i == 0 {
                c
            } else {
                c.child(el().w_full().h(1.0).fill(paint().hairline))
            };
            c.child(listing_row(
                s,
                listing,
                open.as_deref() == Some(&listing.title),
            ))
        });
    let detail = open
        .and_then(|title| visible.into_iter().find(|l| l.title == title))
        .map(|listing| panel(s, listing));
    frame().child(
        row()
            .w_full()
            .h_full()
            .min_h(0.0)
            .animate_layout()
            .child(
                col()
                    .key("store-list")
                    .grow(1.0)
                    .h_full()
                    .min_h(0.0)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(rows),
            )
            .children(detail),
    )
}

pub fn view(s: &App) -> El {
    let k = &paint().kit;
    let body = match &s.store.catalog {
        CatalogState::NotLoaded | CatalogState::Loading => frame().child(
            col()
                .key("store-loading")
                .w_full()
                .grow(1.0)
                .gap(10.0)
                .center()
                .child(Loader::new().build(k))
                .child(
                    text("Loading the G-ExtensionStore")
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                ),
        ),
        CatalogState::Unreachable(_) => frame().child(
            message(
                "store-offline",
                Some(&CLOUD_OFF),
                "The store is unreachable",
                "Bobba could not reach GitHub and has no saved copy of the catalog yet. Your installed extensions keep working.".into(),
            )
            .child(
                Button::new("Try again")
                    .variant(ButtonVariant::Outline)
                    .on_press(App::load_store)
                    .build(k)
                    .key("store-retry"),
            ),
        ),
        CatalogState::Loaded { origin, .. } => {
            let saved = (*origin == Origin::Saved).then(|| {
                text("GitHub is unreachable, so this is the last catalog Bobba saved.")
                    .key("store-saved")
                    .font(k.ty.small)
                    .color(k.c.text_faint)
            });
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .gap(12.0)
                .child(toolbar(s))
                .children(saved)
                .child(listings(s))
        }
    };
    col().key("store").w_full().grow(1.0).min_h(0.0).child(body)
}
