pub mod chrome;
pub mod connection;
pub mod extensions;
pub mod injection;
pub mod logger;
pub mod session;
pub mod setup;
pub mod skull;
pub mod theme;

use rune::{Cx, El, col, el};

use crate::state::{App, Tab};

pub fn root(s: &App, cx: &mut Cx<App>) -> El {
    let p = theme::paint();
    let mut layer = Vec::new();
    let content = match s.tab {
        Tab::Connection => connection::view(s),
        Tab::Injection => injection::view(s, &mut layer),
        Tab::Extensions => extensions::view(s),
    };
    let banner = s.permission_requests.front().map(chrome::permission_banner);
    let page = col()
        .key("window")
        .role("window")
        .w_full()
        .h_full()
        .fill(p.kit.c.bg_base)
        .child(chrome::titlebar(s, cx))
        .children(banner)
        .child(el().key("titlebar-rule").w_full().h(1.0).fill(p.hairline))
        .child(
            col()
                .key("content")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(content),
        )
        .child(el().key("statusbar-rule").w_full().h(1.0).fill(p.hairline))
        .child(chrome::status_bar(s));
    rune_kit::root(page, layer)
}
