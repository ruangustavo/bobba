use bobba::state::App;
use bobba::view::theme::paint;
use rune::{Edge, El, ScrollSpec, Spring, SpringPair, Transition, col, oklcha, row, text};
use rune_kit::{IconButton, IconButtonVariant};

use crate::catalog::{self, LISTINGS};
use crate::detail::detail;
use crate::in_place;

const DETAIL_WIDTH: f32 = 340.0;
const SLIDE: SpringPair = SpringPair::new(Spring::new(0.25, 0.0), Spring::new(0.2, 0.0));
const SLIDE_IN: SpringPair = SpringPair::new(Spring::new(0.22, 0.0), Spring::new(0.2, 0.0));
const QUICK: SpringPair = SpringPair::new(Spring::new(0.18, 0.0), Spring::new(0.12, 0.0));

fn entry(index: usize, selected: bool, compact: bool) -> El {
    let p = paint();
    let k = &p.kit;
    let listing = &LISTINGS[index];
    let mut r = row()
        .key(format!("peek-row-{index}"))
        .role("button")
        .label(listing.title)
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
        .child(catalog::tile(index, 36.0))
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
                            text(listing.title)
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .truncate(),
                        )
                        .child(catalog::rating(index)),
                )
                .child(
                    text(listing.summary)
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .truncate(),
                ),
        )
        .on_press(move |_: &mut App| {
            catalog::with(|s| {
                s.expanded = if s.expanded == Some(index) {
                    None
                } else {
                    Some(index)
                }
            })
        });
    if !compact {
        r = r.child(catalog::action(index, "peek"));
    }
    r
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Slide,
    Overlay,
    Reveal,
}

fn close_button() -> El {
    let k = &paint().kit;
    row().w_full().pt(8.0).pr(8.0).justify_end().child(
        IconButton::new(k.glyph.close)
            .variant(IconButtonVariant::Ghost)
            .label("Close details")
            .help("Close details (Esc)")
            .on_press(|_: &mut App| catalog::with(|s| s.expanded = None))
            .build(k)
            .key("peek-close"),
    )
}

fn content(index: usize, motion: Motion) -> El {
    let body = col()
        .key(format!("peek-content-{index}"))
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(detail(index, 16.0, motion == Motion::Reveal).mt(-20.0));
    match motion {
        Motion::Slide => body.transition(Transition::none()),
        Motion::Overlay => body.transition(Transition::blur_replace().springs(QUICK)),
        Motion::Reveal => body,
    }
}

fn panel(index: usize, motion: Motion) -> El {
    let p = paint();
    let base = col()
        .key("peek-panel")
        .w(DETAIL_WIDTH)
        .h_full()
        .min_h(0.0)
        .fill(p.panel_raised)
        .child(close_button())
        .child(content(index, motion));
    match motion {
        Motion::Slide => base
            .border_sides([0.0, 0.0, 0.0, 1.0], p.hairline)
            .transition(
                Transition::offset(DETAIL_WIDTH, 0.0)
                    .combined(Transition::opacity())
                    .springs(SLIDE_IN)
                    .hold_layout(),
            ),
        Motion::Overlay => base
            .pin(None, Some(0.0), Some(0.0), Some(0.0))
            .border_sides([0.0, 0.0, 0.0, 1.0], p.hairline)
            .shadow(0.0, 28.0, oklcha(0.0, 0.0, 0.0, 0.45))
            .transition(Transition::move_edge(Edge::Trailing).springs(SLIDE)),
        Motion::Reveal => base
            .border_sides([0.0, 0.0, 0.0, 1.0], p.hairline)
            .transition(
                Transition::reveal(Edge::Trailing)
                    .springs(SLIDE)
                    .hold_layout(),
            ),
    }
}

pub fn catalog_view(s: &App, motion: Motion) -> El {
    let visible = catalog::visible(s);
    if visible.is_empty() {
        return in_place::panel().child(catalog::empty(&catalog::query(s)));
    }
    let open = catalog::with(|s| s.expanded).filter(|i| visible.contains(i));
    let compact = open.is_some() && motion == Motion::Reveal;
    let list = visible
        .iter()
        .enumerate()
        .fold(col().w_full(), |c, (i, &index)| {
            let c = if i == 0 {
                c
            } else {
                c.child(catalog::divider())
            };
            c.child(entry(index, open == Some(index), compact))
        });
    let columns = row().w_full().h_full().min_h(0.0).child(
        col()
            .key("peek-list")
            .grow(1.0)
            .h_full()
            .min_h(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(list),
    );
    let columns = if motion == Motion::Slide {
        columns.animate_layout()
    } else {
        columns
    };
    let columns = match open {
        Some(index) => columns.child(panel(index, motion)),
        None => columns,
    };
    in_place::panel().child(columns)
}

pub fn slide(s: &App) -> El {
    in_place::shell(s, |s| catalog_view(s, Motion::Slide))
}

pub fn overlay(s: &App) -> El {
    in_place::shell(s, |s| catalog_view(s, Motion::Overlay))
}

pub fn reveal(s: &App) -> El {
    in_place::shell(s, |s| catalog_view(s, Motion::Reveal))
}
