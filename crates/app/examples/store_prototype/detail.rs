use bobba::view::theme::paint;
use rune::tok::TextStyle;
use rune::{El, ScrollSpec, col, row, text};

use crate::catalog::{self, LISTINGS};
use crate::entering;

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

pub fn detail(index: usize, title_size: f32, stagger: bool) -> El {
    let enter = |e: El, delay: f32| if stagger { entering(e, delay) } else { e };
    let p = paint();
    let k = &p.kit;
    let entry = &LISTINGS[index];
    let head = row()
        .w_full()
        .gap(14.0)
        .center_y()
        .child(catalog::tile(index, 44.0))
        .child(
            col()
                .gap(4.0)
                .grow(1.0)
                .shrink()
                .child(
                    text(entry.title)
                        .font(TextStyle::new(title_size, 650.0))
                        .color(k.c.text_base)
                        .truncate(),
                )
                .child(
                    text(format!("by {}", entry.authors))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .truncate(),
                ),
        )
        .child(catalog::action(index, "detail"));
    let facts = row()
        .w_full()
        .gap(12.0)
        .flex_wrap()
        .child(fact("Version", entry.version.into()))
        .child(fact("Framework", entry.framework.into()))
        .child(fact("Category", entry.category.into()))
        .child(fact("Updated", entry.updated.into()));
    col()
        .key(format!("split-detail-{index}"))
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(20.0)
        .gap(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(enter(head, 0.0))
        .children(catalog::notice(index).map(|n| enter(n, 0.04)))
        .child(enter(
            text(entry.description)
                .font(k.ty.body)
                .color(k.c.text_base)
                .wrap(),
            0.04,
        ))
        .child(enter(catalog::divider(), 0.08))
        .child(enter(facts, 0.08))
        .child(enter(
            row()
                .gap(6.0)
                .center_y()
                .child(catalog::rating(index))
                .child(
                    text("players rated it on the G-Earth Discord")
                        .font(k.ty.small)
                        .color(k.c.text_faint),
                ),
            0.12,
        ))
}
