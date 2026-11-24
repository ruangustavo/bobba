use rune::tok::TextStyle;
use rune::{Color, El, FileDialogOptions, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Badge, BadgeVariant, Button, ButtonVariant};

use crate::backend::Command;
use crate::state::App;
use crate::view::skull::skull;
use crate::view::theme::{Paint, paint};

fn install(_: &mut App, tx: &mut Tx) {
    let options = FileDialogOptions {
        title: "Install extension".into(),
        ..Default::default()
    }
    .filter("Extensions", &["jar", "py", "py3", "js", "sh", "exe"]);
    tx.open_file(options, |s: &mut App, result, _| match result {
        Ok(Some(path)) => s.send(Command::InstallExtension(path)),
        Ok(None) => {}
        Err(error) => s.log_console(format!("Could not open the file picker: {error}")),
    });
}

fn card(p: &Paint, key: String, title: String, detail: String, badge: El, actions: Vec<El>) -> El {
    let k = &p.kit;
    row()
        .key(key)
        .w_full()
        .padding(14.0)
        .gap(12.0)
        .center_y()
        .radius(12.0)
        .fill(p.panel)
        .border(1.0, p.hairline)
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

fn badge(label: &str, variant: BadgeVariant) -> El {
    Badge::new(label).variant(variant).build(&paint().kit)
}

fn dot(color: Color) -> El {
    el().size(7.0, 7.0).radius(3.5).fill(color)
}

pub fn view(s: &App) -> El {
    let p = paint();
    let k = &p.kit;
    let port = s.extension_port.map_or_else(
        || "The extension port is not open yet.".to_owned(),
        |port| format!("Extensions in any language connect on port {port}."),
    );
    let mut list = col()
        .key("extension-list")
        .gap(8.0)
        .child(card(
            p,
            "ext-logger".into(),
            "Logger".into(),
            "Shows every packet after extensions decide, never slows traffic".into(),
            badge("built-in", BadgeVariant::Subtle),
            vec![],
        ))
        .child(card(
            p,
            "ext-injection".into(),
            "Injection".into(),
            "Sends packet expressions to the client or the server".into(),
            badge("built-in", BadgeVariant::Subtle),
            vec![],
        ));
    for state in &s.installed {
        let connected = s
            .extensions
            .iter()
            .find(|(_, info)| info.file.as_deref() == Some(state.file_name.as_str()));
        let title =
            connected.map_or_else(|| state.file_name.clone(), |(_, info)| info.title.clone());
        let (status, status_color) = match (connected.is_some(), state.running) {
            (true, _) => ("connected", p.green),
            (false, true) => ("starting", p.amber),
            (false, false) => ("stopped", k.c.text_faint),
        };
        let detail = format!("{:?} · {} · {}", state.runtime, status, state.file_name);
        let file = state.file_name.clone();
        let toggle = if state.running {
            let file = file.clone();
            Button::new("Stop")
                .variant(ButtonVariant::Outline)
                .on_press(move |s: &mut App| s.send(Command::StopExtension(file.clone())))
                .build(k)
        } else {
            let file = file.clone();
            Button::new("Start")
                .variant(ButtonVariant::Contrast)
                .on_press(move |s: &mut App| s.send(Command::StartExtension(file.clone())))
                .build(k)
        };
        let uninstall = Button::new("Uninstall")
            .variant(ButtonVariant::GhostMuted)
            .on_press(move |s: &mut App| s.send(Command::UninstallExtension(file.clone())))
            .build(k);
        list = list.child(card(
            p,
            format!("installed-{}", state.file_name),
            title,
            detail,
            dot(status_color),
            vec![toggle, uninstall],
        ));
    }
    for (_, info) in s.extensions.iter().filter(|(_, info)| info.file.is_none()) {
        list = list.child(card(
            p,
            format!("manual-{}", info.title),
            info.title.clone(),
            format!(
                "{} · v{} · started outside Bobba",
                info.author, info.version
            ),
            dot(p.green),
            vec![],
        ));
    }
    let console = s.console.iter().fold(col().gap(3.0), |lines, line| {
        lines.child(
            text(line.clone())
                .font(k.ty.mono)
                .color(k.c.text_muted)
                .wrap_anywhere(),
        )
    });
    let console_panel = if s.console.is_empty() {
        col()
            .key("extension-console")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .center()
            .gap(10.0)
            .child(skull(3.0, k.c.text_faint))
            .child(
                text("Extension output shows up here")
                    .font(k.ty.small)
                    .color(k.c.text_faint),
            )
    } else {
        col()
            .key("extension-console")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .padding(14.0)
            .radius(12.0)
            .fill(p.panel)
            .scroll(ScrollSpec { stick_bottom: true })
            .child(console)
    };
    col()
        .key("extensions")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(24.0)
        .gap(16.0)
        .child(
            row()
                .center_y()
                .gap(12.0)
                .child(
                    col()
                        .gap(3.0)
                        .grow(1.0)
                        .child(
                            text("Extensions")
                                .font(TextStyle::new(20.0, 650.0))
                                .color(k.c.text_base),
                        )
                        .child(text(port).font(k.ty.body).color(k.c.text_muted)),
                )
                .child(
                    Button::new("Install…")
                        .variant(ButtonVariant::Contrast)
                        .on_press_tx(install)
                        .build(k)
                        .key("install-extension"),
                ),
        )
        .child(list)
        .child(console_panel)
}
