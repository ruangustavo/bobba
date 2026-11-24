use bobba::state::App;
use bobba::view::theme::paint;
use rune::tok::TextStyle;
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Badge, BadgeVariant, Button, ButtonVariant};

use crate::catalog::{self, LISTINGS, Status};

pub fn header(s: &App, trailing: Vec<El>) -> El {
    let k = &paint().kit;
    let port = s.extension_port.map_or_else(
        || "The extension port is not open yet.".to_owned(),
        |port| format!("Extensions in any language connect on port {port}."),
    );
    row()
        .key("extensions-header")
        .w_full()
        .center_y()
        .gap(10.0)
        .child(
            col()
                .gap(3.0)
                .grow(1.0)
                .shrink()
                .child(
                    text("Extensions")
                        .font(TextStyle::new(20.0, 650.0))
                        .color(k.c.text_base),
                )
                .child(text(port).font(k.ty.body).color(k.c.text_muted).truncate()),
        )
        .children(trailing)
}

pub fn install_from_file() -> El {
    Button::new("Install from file")
        .variant(ButtonVariant::Outline)
        .build(&paint().kit)
        .key("install-from-file")
}

fn item(key: String, title: String, detail: String, badge: El, actions: Vec<El>) -> El {
    let k = &paint().kit;
    row()
        .key(key)
        .w_full()
        .padding(14.0)
        .gap(12.0)
        .center_y()
        .child(
            col()
                .gap(3.0)
                .grow(1.0)
                .shrink()
                .child(
                    row()
                        .gap(8.0)
                        .center_y()
                        .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
                        .child(badge),
                )
                .child(
                    text(detail)
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .truncate(),
                ),
        )
        .children(actions)
}

pub fn installed_list() -> El {
    let p = paint();
    let k = &p.kit;
    let mut items = vec![
        item(
            "ext-logger".into(),
            "Logger".into(),
            "Shows every packet after extensions decide, never slows traffic".into(),
            Badge::new("built-in")
                .variant(BadgeVariant::Subtle)
                .build(k),
            vec![],
        ),
        item(
            "ext-injection".into(),
            "Injection".into(),
            "Sends packet expressions to the client or the server".into(),
            Badge::new("built-in")
                .variant(BadgeVariant::Subtle)
                .build(k),
            vec![],
        ),
    ];
    for index in catalog::installed() {
        let listing = &LISTINGS[index];
        let (version, update) = match catalog::status(index) {
            Status::Update { installed } => (installed, true),
            _ => (listing.version, false),
        };
        let mut actions = Vec::new();
        if update {
            actions.push(catalog::action(index, "installed"));
        }
        actions.push(
            Button::new("Stop")
                .variant(ButtonVariant::Outline)
                .build(k)
                .key(format!("installed-stop-{index}")),
        );
        items.push(item(
            format!("installed-{index}"),
            listing.title.into(),
            format!("{} · v{version} · connected", listing.framework),
            el().size(7.0, 7.0).radius(3.5).fill(p.green),
            actions,
        ));
    }
    let list = items.into_iter().enumerate().fold(
        col()
            .key("extension-list")
            .w_full()
            .radius(12.0)
            .fill(p.panel)
            .border(1.0, p.hairline),
        |list, (i, item)| {
            let list = if i == 0 {
                list
            } else {
                list.child(catalog::divider())
            };
            list.child(item)
        },
    );
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(list)
}
