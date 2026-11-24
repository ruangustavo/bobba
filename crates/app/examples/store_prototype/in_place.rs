use bobba::state::App;
use bobba::view::theme::paint;
use rune::{El, col, row};
use rune_kit::SegmentedControl;

use crate::{catalog, tab};

fn switcher(open: bool) -> El {
    SegmentedControl::new(&["Installed", "Store"], Some(usize::from(open)))
        .label("Show")
        .fit()
        .on_pick(|_: &mut App, i, _| catalog::with(|s| s.open = i == 1))
        .build(&paint().kit)
        .key("in-place-switch")
}

pub fn panel() -> El {
    let p = paint();
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .radius(12.0)
        .fill(p.panel)
        .border(1.0, p.hairline)
        .clip_rounded(12.0)
}

fn store(s: &App, catalog_view: fn(&App) -> El) -> El {
    let body = if catalog::offline() {
        panel().child(catalog::offline_state())
    } else {
        catalog_view(s)
    };
    col()
        .key("in-place-store")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(12.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(catalog::search(s, Some(220.0)))
                .child(col().grow(1.0).min_w(0.0).child(catalog::chips())),
        )
        .child(body)
}

pub fn shell(s: &App, catalog_view: fn(&App) -> El) -> El {
    let open = catalog::with(|s| s.open);
    let body = if open {
        store(s, catalog_view)
    } else {
        tab::installed_list()
    };
    let mut trailing = Vec::new();
    if !open {
        trailing.push(tab::install_from_file());
    }
    trailing.push(switcher(open));
    col()
        .key("extensions")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(24.0)
        .gap(16.0)
        .child(tab::header(s, trailing))
        .child(body)
}
