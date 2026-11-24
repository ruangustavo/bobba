use std::borrow::Cow;

use bobba_store::{Listing, Origin, Runtime};
use rune::tok::TextStyle;
use rune::{
    Anim, Cx, El, GrowthSpec, Icon, ScrollSpec, Spring, SpringPair, Transition, Tx, col, el, icon,
    oklch, row, text,
};
use rune_kit::{Button, ButtonVariant, IconButton, IconButtonVariant, Loader, TextInput};

use crate::store::{CatalogState, ListingStatus};
use crate::store_window::StoreWindow;
use crate::view::chrome::TITLEBAR_HEIGHT;
use crate::view::theme::paint;

const DETAIL_WIDTH: f32 = 340.0;
const ROW_HEIGHT: f32 = 65.0;
const BADGE: f32 = 40.0;
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
    let mut initials: String = title
        .split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if initials.is_empty() {
        initials = title.chars().take(1).collect();
    }
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

fn badge(s: &StoreWindow, listing: &Listing) -> El {
    match s.store.icon(&listing.title) {
        Some(pixels) => el()
            .key(format!("store-icon-{}", listing.title))
            .size(BADGE, BADGE)
            .pixels(pixels),
        None => tile(&listing.title, BADGE),
    }
}

fn rating(listing: &Listing) -> Option<El> {
    let p = paint();
    let k = &p.kit;
    let rating = listing.rating?;
    Some(
        row()
            .gap(3.0)
            .center_y()
            .child(icon(&STAR, 11.0).color(p.amber))
            .child(
                text(rating.to_string())
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            ),
    )
}

fn summary(description: &str) -> Option<&str> {
    description
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
}

fn search_placeholder(count: usize) -> String {
    let noun = if count == 1 {
        "extension"
    } else {
        "extensions"
    };
    format!("Search {count} {noun}")
}

fn install(title: &str) -> impl Fn(&mut StoreWindow) + Clone + 'static {
    let title = title.to_owned();
    move |s: &mut StoreWindow| s.install(title.clone())
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
        ListingStatus::NeedsRuntime { first, .. } => {
            let guide = first.guide();
            Button::new(format!("Get {}", first.name()))
                .variant(ButtonVariant::Outline)
                .help(format!(
                    "{} needs {}. Opens its install guide.",
                    listing.title,
                    first.requirement()
                ))
                .on_press_tx(move |_: &mut StoreWindow, tx: &mut Tx| {
                    let _ = tx.open_url(guide);
                })
                .build(k)
        }
        ListingStatus::Failed(_) => Button::new("Try again")
            .variant(ButtonVariant::Outline)
            .on_press(install(&listing.title))
            .build(k),
    }
    .key(key)
}

fn missing(first: Runtime, rest: &[Runtime]) -> (String, bool) {
    let names: Vec<&str> = std::iter::once(first)
        .chain(rest.iter().copied())
        .map(Runtime::requirement)
        .collect();
    let joined = match names.split_last() {
        Some((last, init)) if !init.is_empty() => format!("{} and {last}", init.join(", ")),
        _ => first.requirement().to_owned(),
    };
    (joined, !rest.is_empty())
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
        ListingStatus::NeedsRuntime { first, rest } => {
            let (names, plural) = missing(*first, rest);
            let (are, them) = if plural {
                ("are", "them")
            } else {
                ("is", "it")
            };
            (
                &ALERT,
                p.amber,
                format!(
                    "Needs {names}, which {are} not installed on this computer. Install {them}, then come back to install this extension."
                ),
            )
        }
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
                    .wrap_anywhere()
                    .grow(1.0)
                    .shrink(),
            ),
    )
}

fn listing_row(s: &StoreWindow, listing: &Listing, selected: bool) -> El {
    let p = paint();
    let k = &p.kit;
    let status = s.store.status(listing, &s.installed);
    let subtitle = match &status {
        ListingStatus::Failed(reason) => {
            let message = format!("Could not install: {reason}");
            Some(
                text(message.clone())
                    .color(p.red)
                    .help_when_truncated(message),
            )
        }
        ListingStatus::NeedsRuntime { first, rest } => {
            let (names, plural) = missing(*first, rest);
            let are = if plural { "are" } else { "is" };
            Some(text(format!("Needs {names}, which {are} not installed")).color(p.amber))
        }
        _ => summary(&listing.description).map(|line| {
            text(line.to_owned())
                .color(k.c.text_muted)
                .help_when_truncated(line.to_owned())
        }),
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
        .child(badge(s, listing))
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
                                .truncate()
                                .help_when_truncated(listing.title.clone()),
                        )
                        .children(rating(listing)),
                )
                .children(subtitle.map(|line| line.font(k.ty.small).truncate())),
        )
        .child(action)
        .on_press(move |s: &mut StoreWindow| s.toggle_detail(&title))
}

fn visible_row(s: &StoreWindow, i: usize) -> El {
    let visible = s.visible();
    let open = s.open_detail(&visible);
    let Some(listing) = visible.get(i) else {
        return el();
    };
    let line = if i == 0 { 0.0 } else { 1.0 };
    listing_row(s, listing, open.as_deref() == Some(&listing.title))
        .border_sides([line, 0.0, 0.0, 0.0], paint().hairline)
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
            text(value.clone())
                .font(k.ty.small_medium)
                .color(k.c.text_base)
                .truncate()
                .help_when_truncated(value),
        )
}

fn detail(s: &StoreWindow, listing: &Listing) -> El {
    let p = paint();
    let k = &p.kit;
    let status = s.store.status(listing, &s.installed);
    let facts = row()
        .w_full()
        .gap(12.0)
        .flex_wrap()
        .child(fact("Version", listing.version.clone()))
        .child(fact("Framework", listing.framework.clone()))
        .child(fact("Category", listing.categories.join(", ")))
        .children(
            listing
                .updated
                .split_whitespace()
                .next()
                .map(|day| fact("Updated", day.to_owned())),
        );
    let source = listing.source.clone();
    let source_link = (!source.is_empty()).then(|| {
        Button::new("View source")
            .variant(ButtonVariant::GhostMuted)
            .on_press_tx(move |_: &mut StoreWindow, tx: &mut Tx| {
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
        .children(notice(listing, &status))
        .children((!listing.description.trim().is_empty()).then(|| {
            text(listing.description.clone())
                .font(k.ty.body)
                .color(k.c.text_base)
                .wrap_anywhere()
        }))
        .child(el().w_full().h(1.0).fill(p.hairline))
        .child(facts)
        .child(
            row()
                .w_full()
                .gap(6.0)
                .center_y()
                .children(rating(listing).map(|stars| {
                    row().gap(6.0).center_y().child(stars).child(
                        text("on the G-Earth Discord")
                            .font(k.ty.small)
                            .color(k.c.text_faint),
                    )
                }))
                .child(el().grow(1.0))
                .children(source_link),
        )
}

fn head(s: &StoreWindow, listing: &Listing) -> El {
    let k = &paint().kit;
    let status = s.store.status(listing, &s.installed);
    row()
        .w_full()
        .pt(14.0)
        .pb(16.0)
        .pl(20.0)
        .pr(10.0)
        .gap(10.0)
        .items_start()
        .child(
            col()
                .gap(2.0)
                .grow(1.0)
                .shrink()
                .child(
                    text(listing.title.clone())
                        .key("store-detail-title")
                        .font(TextStyle::new(15.0, 650.0))
                        .color(k.c.text_base)
                        .wrap_anywhere()
                        .max_lines(2)
                        .help_when_truncated(listing.title.clone()),
                )
                .children((!listing.authors.is_empty()).then(|| {
                    text(format!("by {}", listing.authors.join(", ")))
                        .font(k.ty.small)
                        .color(k.c.text_faint)
                        .truncate()
                        .help_when_truncated(listing.authors.join(", "))
                })),
        )
        .child(action(listing, &status).key("store-detail-action"))
        .child(
            IconButton::new(k.glyph.close)
                .variant(IconButtonVariant::Ghost)
                .label("Close details")
                .help("Close details (Esc)")
                .on_press(|s: &mut StoreWindow| s.detail = None)
                .build(k)
                .key("store-detail-close"),
        )
}

fn panel(s: &StoreWindow, listing: &Listing) -> El {
    let p = paint();
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
        .child(head(s, listing))
        .child(detail(s, listing))
}

fn category_tab(label: String, value: Option<String>, on: bool) -> El {
    let k = &paint().kit;
    row()
        .key(format!("store-category-{label}"))
        .role("tab")
        .label(label.clone())
        .h(32.0)
        .px(8.0)
        .items_end()
        .child(
            col()
                .gap(7.0)
                .child(
                    text(label)
                        .font(k.ty.small_medium)
                        .color(if on { k.c.text_base } else { k.c.text_muted })
                        .group_color(k.c.text_base),
                )
                .child(el().w_full().h(2.0).radius(1.0).fill(if on {
                    k.c.text_base
                } else {
                    k.c.text_base.alpha(0.0)
                })),
        )
        .hover_group()
        .on_press(move |s: &mut StoreWindow| s.category = value.clone())
}

fn categories(s: &StoreWindow) -> Option<El> {
    let p = paint();
    let catalog = s.store.catalog()?;
    let tabs = catalog.categories.iter().fold(
        row()
            .key("store-categories")
            .role("tablist")
            .label("Category")
            .px(10.0)
            .child(category_tab("All".into(), None, s.category.is_none())),
        |r, category| {
            let on = s.category.as_deref() == Some(category.name.as_str());
            r.child(category_tab(
                category.name.clone(),
                Some(category.name.clone()),
                on,
            ))
        },
    );
    Some(
        col()
            .w_full()
            .child(
                row()
                    .w_full()
                    .min_w(0.0)
                    .scroll_x()
                    .scroll_edge_fade(16.0)
                    .child(tabs),
            )
            .child(el().w_full().h(1.0).fill(p.hairline)),
    )
}

fn titlebar(s: &StoreWindow, cx: &mut Cx<StoreWindow>) -> El {
    let k = &paint().kit;
    let bar = row()
        .key("store-toolbar")
        .w_full()
        .h(TITLEBAR_HEIGHT)
        .px(14.0)
        .gap(12.0)
        .center_y()
        .window_drag()
        .child(el().w(cx.titlebar_inset()));
    let Some(catalog) = s.store.catalog() else {
        return bar.child(text("Store").font(k.ty.ui_medium).color(k.c.text_base));
    };
    bar.child(
        col().grow(1.0).min_w(0.0).child(
            TextInput::new(&s.query)
                .label("Search extensions")
                .placeholder(search_placeholder(catalog.listings.len()))
                .leading_icon(k.glyph.search)
                .clear_button()
                .width(None)
                .on_edit(|s: &mut StoreWindow| &mut s.query)
                .build(k)
                .key("store-search"),
        ),
    )
}

fn frame() -> El {
    col().key("store-panel").w_full().grow(1.0).min_h(0.0)
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

fn listings(s: &StoreWindow) -> El {
    let visible = s.visible();
    if visible.is_empty() {
        let query = s.query.text.trim();
        let body = if query.is_empty() {
            match s.category {
                Some(_) => "Nothing in this category runs on this computer yet.".to_owned(),
                None => "No extension in the store runs on this computer yet.".to_owned(),
            }
        } else {
            format!("Nothing matches “{query}”. Try another word or pick All.")
        };
        return frame().child(message("store-empty", None, "No extensions found", body));
    }
    let open = s.open_detail(&visible);
    let rows = col()
        .key("store-rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            visible.len(),
            ROW_HEIGHT,
            |s: &StoreWindow, i| {
                Cow::Owned(s.visible().get(i).map_or_else(String::new, |listing| {
                    format!("store-row-{}", listing.title)
                }))
            },
            visible_row,
        );
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
                    .scroll_edge_fade(24.0)
                    .child(rows),
            )
            .children(detail),
    )
}

fn body(s: &StoreWindow) -> El {
    let k = &paint().kit;
    match &s.store.catalog {
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
                    .on_press(StoreWindow::load_catalog)
                    .build(k)
                    .key("store-retry"),
            ),
        ),
        CatalogState::Loaded { origin, .. } => {
            let saved = (*origin == Origin::Saved).then(|| {
                row().w_full().px(16.0).py(8.0).child(
                    text("GitHub is unreachable, so this is the last catalog Bobba saved.")
                        .key("store-saved")
                        .font(k.ty.small)
                        .color(k.c.text_faint),
                )
            });
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .children(saved)
                .child(listings(s))
        }
    }
}

pub fn root(s: &StoreWindow, cx: &mut Cx<StoreWindow>) -> El {
    let p = paint();
    let page = col()
        .key("store")
        .role("window")
        .w_full()
        .h_full()
        .fill(p.kit.c.bg_base)
        .child(titlebar(s, cx))
        .child(el().key("titlebar-rule").w_full().h(1.0).fill(p.hairline))
        .children(categories(s))
        .child(body(s));
    rune_kit::root(page, Vec::new()).on_window_focus(|s: &mut StoreWindow, focused, _| {
        if focused {
            s.check_runtimes();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_placeholder_counts_one_extension() {
        assert_eq!(search_placeholder(1), "Search 1 extension");
        assert_eq!(search_placeholder(61), "Search 61 extensions");
    }
}
