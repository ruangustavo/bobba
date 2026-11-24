use bobba_extensions::{Permission, PermissionRequest};
use rune::tok::TextStyle;
use rune::{Color, Cx, El, el, row, text};
use rune_kit::{Button, ButtonVariant, SegmentedControl};

use crate::state::{App, Phase, Tab};
use crate::view::skull::skull;
use crate::view::theme::{Paint, paint};

pub const TITLEBAR_HEIGHT: f32 = 52.0;

fn status_color(p: &Paint, phase: &Phase) -> Color {
    match phase {
        Phase::Connected(_) => p.green,
        Phase::Failed(_) | Phase::SetupFailed { .. } => p.red,
        Phase::Idle | Phase::Ended { .. } => p.kit.c.text_faint,
        _ => p.amber,
    }
}

pub fn titlebar(s: &App, cx: &mut Cx<App>) -> El {
    let p = paint();
    let k = &p.kit;
    let tabs = SegmentedControl::new(&Tab::ALL.map(Tab::label), Some(s.tab.index()))
        .label("Section")
        .fit()
        .on_pick(|s: &mut App, index, _| {
            s.tab = Tab::ALL.get(index).copied().unwrap_or(Tab::Connection);
        })
        .build(k);
    let dot = el()
        .key("status-dot")
        .size(7.0, 7.0)
        .radius(3.5)
        .fill(status_color(p, &s.phase));
    let status = row()
        .key("title-status")
        .gap(8.0)
        .center_y()
        .px(10.0)
        .h(26.0)
        .radius(13.0)
        .fill(p.panel)
        .child(dot)
        .child(
            text(s.status_line())
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate(),
        );
    row()
        .key("titlebar")
        .w_full()
        .h(TITLEBAR_HEIGHT)
        .center_y()
        .px(14.0)
        .gap(12.0)
        .window_drag()
        .child(
            row()
                .key("brand")
                .grow(1.0)
                .gap(8.0)
                .center_y()
                .child(el().w(cx.titlebar_inset()))
                .child(skull(2.0, k.c.text_base))
                .child(
                    text("Bobba")
                        .font(TextStyle::new(13.5, 650.0))
                        .color(k.c.text_base),
                ),
        )
        .child(tabs)
        .child(
            row()
                .key("status-slot")
                .grow(1.0)
                .justify_end()
                .child(status),
        )
}

pub fn permission_banner(request: &PermissionRequest) -> El {
    let p = paint();
    let k = &p.kit;
    let info = &request.info;
    let author = if info.author.is_empty() {
        String::new()
    } else {
        format!(" by {}", info.author)
    };
    row()
        .key("permission-banner")
        .w_full()
        .px(18.0)
        .py(10.0)
        .gap(10.0)
        .center_y()
        .fill(p.amber.alpha(0.10))
        .child(
            text(format!(
                "“{}”{author} wants to connect and see your traffic.",
                info.title
            ))
            .font(k.ty.ui)
            .color(k.c.text_base)
            .grow(1.0)
            .truncate(),
        )
        .child(
            Button::new("Deny")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut App| s.answer_permission(Permission::Deny))
                .build(k)
                .key("permission-deny"),
        )
        .child(
            Button::new("Always allow")
                .variant(ButtonVariant::Outline)
                .on_press(|s: &mut App| s.answer_permission(Permission::AllowAlways))
                .build(k)
                .key("permission-always"),
        )
        .child(
            Button::new("Allow")
                .variant(ButtonVariant::Contrast)
                .on_press(|s: &mut App| s.answer_permission(Permission::Allow))
                .build(k)
                .key("permission-allow"),
        )
}

pub fn status_bar(s: &App) -> El {
    let p = paint();
    let k = &p.kit;
    let item = |label: String, ink: Color| text(label).font(k.ty.small).color(ink);
    let port = s
        .extension_port
        .map_or_else(|| "—".to_owned(), |port| port.to_string());
    row()
        .key("statusbar")
        .w_full()
        .h(28.0)
        .px(16.0)
        .gap(18.0)
        .center_y()
        .child(item(format!("↑ {}", s.traffic.to_server), k.c.text_muted).key("traffic-out"))
        .child(item(format!("↓ {}", s.traffic.to_client), k.c.text_muted).key("traffic-in"))
        .child(el().grow(1.0))
        .child(item(
            format!("{} extensions · port {port}", s.extensions.len()),
            k.c.text_faint,
        ))
}
