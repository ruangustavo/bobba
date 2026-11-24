use rune::tok::TextStyle;
use rune::{Color, El, FileDialogOptions, Tx, col, el, row, text};
use rune_kit::{
    Badge, BadgeVariant, Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant,
};

use crate::backend::Command;
use crate::state::App;
use crate::view::theme::{Paint, paint};

static PLAY: rune::Icon = rune::Icon {
    name: "bobba-play",
    view: 16.0,
    body: r#"<path d="M4.75 3.5L12.55 8L4.75 12.5Z" fill="currentColor" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"/>"#,
};

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

fn item(p: &Paint, key: String, title: String, detail: String, badge: El, actions: Vec<El>) -> El {
    let k = &p.kit;
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
    let mut items = vec![
        item(
            p,
            "ext-logger".into(),
            "Logger".into(),
            "Shows every packet after extensions decide, never slows traffic".into(),
            badge("built-in", BadgeVariant::Subtle),
            vec![
                IconButton::new(&PLAY)
                    .variant(IconButtonVariant::Ghost)
                    .control_size(ControlSize::Large)
                    .label("Open Logger")
                    .help("Open the Logger in its own window")
                    .on_press(App::open_logger)
                    .build(k)
                    .key("ext-logger-open"),
            ],
        ),
        item(
            p,
            "ext-injection".into(),
            "Injection".into(),
            "Sends packet expressions to the client or the server".into(),
            badge("built-in", BadgeVariant::Subtle),
            vec![],
        ),
    ];
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
        items.push(item(
            p,
            format!("installed-{}", state.file_name),
            title,
            detail,
            dot(status_color),
            vec![toggle, uninstall],
        ));
    }
    for (_, info) in s.extensions.iter().filter(|(_, info)| info.file.is_none()) {
        items.push(item(
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
                list.child(
                    el().key(format!("extension-divider-{}", i - 1))
                        .w_full()
                        .h(1.0)
                        .fill(p.hairline),
                )
            };
            list.child(item)
        },
    );
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
                    Button::new("Install")
                        .variant(ButtonVariant::Contrast)
                        .on_press_tx(install)
                        .build(k)
                        .key("install-extension"),
                ),
        )
        .child(list)
}
