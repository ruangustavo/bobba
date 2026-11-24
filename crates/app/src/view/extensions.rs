use rune::tok::TextStyle;
use std::path::Path;

use rune::{Color, El, FileDialogOptions, Key, KeyEvent, ScrollSpec, Tx, col, el, icon, row, text};
use rune_kit::{
    Badge, BadgeVariant, Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Loader,
};

use bobba_extensions::{ExtensionId, ProcessStatus};

use crate::backend::{Command, InstalledKind};
use crate::state::{App, GEarthOffer};
use crate::view::theme::{Paint, paint};

static PLAY: rune::Icon = rune::Icon {
    name: "bobba-play",
    view: 16.0,
    body: r#"<path d="M4.75 3.5L12.55 8L4.75 12.5Z" fill="currentColor" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"/>"#,
};

static CLOSE: rune::Icon = rune::Icon {
    name: "bobba-close",
    view: 16.0,
    body: r#"<path d="M4.5 4.5l7 7M11.5 4.5l-7 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>"#,
};

static RESTART: rune::Icon = rune::Icon {
    name: "bobba-restart",
    view: 16.0,
    body: r#"<path d="M12.6 8.6A4.7 4.7 0 1 1 11.2 4.7" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><path d="M11.6 2.4v2.8H8.8" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

static TRASH: rune::Icon = rune::Icon {
    name: "bobba-trash",
    view: 16.0,
    body: r#"<path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

const ACTION_SLOT: f32 = 28.0;

static INFO: rune::Icon = rune::Icon {
    name: "bobba-info",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="6.3" fill="none" stroke="currentColor" stroke-width="1.3"/><path d="M8 7.2v4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><circle cx="8" cy="4.9" r="0.85" fill="currentColor"/>"#,
};

#[cfg(target_os = "macos")]
fn shown_path(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

#[cfg(not(target_os = "macos"))]
fn shown_path(path: &Path) -> String {
    path.display().to_string()
}

fn choose_gearth(_: &mut App, tx: &mut Tx) {
    let options = FileDialogOptions {
        title: "Choose the G-Earth folder".into(),
        ..Default::default()
    };
    tx.open_folder(options, |s: &mut App, result, _| match result {
        Ok(Some(folder)) => s.import_from_gearth(folder),
        Ok(None) => {}
        Err(error) => s.log_console(format!("Could not open the folder picker: {error}")),
    });
}

fn gearth_banner(s: &App) -> Option<El> {
    let p = paint();
    let k = &p.kit;
    let (title, detail, actions) = match &s.gearth {
        GEarthOffer::Found { folder, count } => {
            let folder = folder.clone();
            (
                format!("Found {count} extensions in G-Earth"),
                shown_path(&folder),
                vec![
                    Button::new("Not now")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut App| s.gearth = GEarthOffer::Dismissed)
                        .build(k)
                        .key("gearth-import-dismiss"),
                    Button::new("Import")
                        .variant(ButtonVariant::Contrast)
                        .on_press(move |s: &mut App| s.import_from_gearth(folder.clone()))
                        .build(k)
                        .key("gearth-import-accept"),
                ],
            )
        }
        GEarthOffer::Importing => (
            "Importing extensions from G-Earth".to_owned(),
            "They start as soon as they are copied".to_owned(),
            vec![Loader::new().build(k)],
        ),
        GEarthOffer::None | GEarthOffer::Dismissed => return None,
    };
    Some(
        row()
            .key("gearth-import")
            .w_full()
            .padding(14.0)
            .gap(12.0)
            .center_y()
            .radius(12.0)
            .fill(p.blue.alpha(0.12))
            .border(1.0, p.blue.alpha(0.35))
            .child(icon(&INFO, 16.0).color(p.blue))
            .child(
                col()
                    .gap(3.0)
                    .grow(1.0)
                    .shrink()
                    .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
                    .child(
                        text(detail)
                            .font(k.ty.small)
                            .color(k.c.text_muted)
                            .truncate(),
                    ),
            )
            .children(actions),
    )
}

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

fn item(
    p: &Paint,
    key: String,
    title: String,
    detail: String,
    badge: El,
    dimmed: bool,
    actions: Vec<El>,
) -> El {
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
                )
                .opacity(if dimmed { 0.5 } else { 1.0 }),
        )
        .children(actions)
}

fn icon_button(
    icon: &'static rune::Icon,
    label: String,
    key: String,
    act: impl Fn(&mut App) + 'static,
) -> El {
    IconButton::new(icon)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label(label.clone())
        .help(label)
        .on_press(act)
        .build(&paint().kit)
        .key(key)
}

fn slot() -> El {
    el().size(ACTION_SLOT, ACTION_SLOT)
}

fn open_button(id: ExtensionId, title: &str, key: String) -> El {
    icon_button(&PLAY, format!("Open {title}"), key, move |s: &mut App| {
        s.open_extension(id)
    })
}

fn uninstall_button(s: &App, title: &str, id: &str) -> El {
    let file = id.to_owned();
    if s.confirm_uninstall.as_deref() == Some(id) {
        return Button::new("Uninstall")
            .variant(ButtonVariant::Danger)
            .on_press(move |s: &mut App| {
                s.confirm_uninstall = None;
                s.send(Command::UninstallExtension(file.clone()));
            })
            .build(&paint().kit)
            .key(format!("installed-confirm-{id}"));
    }
    icon_button(
        &TRASH,
        format!("Uninstall {title}"),
        format!("installed-uninstall-{id}"),
        move |s: &mut App| s.confirm_uninstall = Some(file.clone()),
    )
}

fn update_link(title: &str, version: &str, id: &str) -> El {
    let p = paint();
    let title = title.to_owned();
    row()
        .key(format!("installed-update-{id}"))
        .role("button")
        .label(format!("See {title} {version} in the store"))
        .px(6.0)
        .h(24.0)
        .center_y()
        .radius(6.0)
        .child(
            text(format!("{version} available"))
                .font(p.kit.ty.small_medium)
                .color(p.blue),
        )
        .on_press(move |s: &mut App| s.open_store_at(title.clone()))
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
    let mut items = vec![
        item(
            p,
            "ext-logger".into(),
            "Logger".into(),
            "Shows every packet after extensions decide, never slows traffic".into(),
            badge("built-in", BadgeVariant::Subtle),
            false,
            vec![
                icon_button(
                    &PLAY,
                    "Open Logger".into(),
                    "ext-logger-open".into(),
                    App::open_logger,
                ),
                slot(),
            ],
        ),
        item(
            p,
            "ext-store".into(),
            "Store".into(),
            "Ready-made extensions from the G-ExtensionStore".into(),
            badge("built-in", BadgeVariant::Subtle),
            false,
            vec![
                icon_button(
                    &PLAY,
                    "Open Store".into(),
                    "ext-store-open".into(),
                    App::open_store,
                ),
                slot(),
            ],
        ),
        item(
            p,
            "ext-injection".into(),
            "Injection".into(),
            "Sends packet expressions to the client or the server".into(),
            badge("built-in", BadgeVariant::Subtle),
            false,
            vec![],
        ),
    ];
    for state in &s.installed {
        let connected = s
            .extensions
            .iter()
            .find(|(_, info)| info.file.as_deref() == Some(state.id.as_str()));
        let fallback = match &state.kind {
            InstalledKind::Package { title, .. } => title.clone(),
            InstalledKind::Single(_) => state.id.clone(),
        };
        let title = connected.map_or(fallback, |(_, info)| info.title.clone());
        let (status, status_color) = match (connected.is_some(), state.status) {
            (true, _) => ("connected".to_owned(), p.green),
            (false, ProcessStatus::Running) => ("starting".to_owned(), p.amber),
            (false, ProcessStatus::Stopped) => ("stopped".to_owned(), k.c.text_faint),
            (false, ProcessStatus::Exited { code }) => {
                let reason = match (s.last_output.get(&state.id), code) {
                    (Some(line), _) => format!("exited · {line}"),
                    (None, Some(code)) => format!("exited with code {code}"),
                    (None, None) => "exited".to_owned(),
                };
                (reason, p.red)
            }
        };
        let detail = match &state.kind {
            InstalledKind::Single(runtime) => format!("{runtime:?} · {status} · {}", state.id),
            InstalledKind::Package { version, .. } => format!("v{version} · {status}"),
        };
        let open = connected
            .filter(|(_, info)| info.on_click_used)
            .map(|(id, info)| {
                open_button(*id, &info.title, format!("installed-open-{}", state.id))
            });
        let update = match &state.kind {
            InstalledKind::Package { title, version } => s
                .store
                .update_for(title, version)
                .map(|listing| update_link(title, &listing.version, &state.id)),
            InstalledKind::Single(_) => None,
        };
        let (first, second) = if state.status == ProcessStatus::Running {
            let file = state.id.clone();
            (
                open.unwrap_or_else(slot),
                icon_button(
                    &CLOSE,
                    format!("Close {title}"),
                    format!("installed-close-{}", state.id),
                    move |s: &mut App| s.send(Command::StopExtension(file.clone())),
                ),
            )
        } else {
            let file = state.id.clone();
            (
                icon_button(
                    &RESTART,
                    format!("Run {title} again"),
                    format!("installed-restart-{}", state.id),
                    move |s: &mut App| s.start_extension(file.clone()),
                ),
                uninstall_button(s, &title, &state.id),
            )
        };
        items.push(item(
            p,
            format!("installed-{}", state.id),
            title,
            detail,
            dot(status_color),
            state.status == ProcessStatus::Stopped,
            update.into_iter().chain([first, second]).collect(),
        ));
    }
    let empty = s.installed.is_empty() && !s.extensions.iter().any(|(_, info)| info.file.is_none());
    if empty {
        items.push(item(
            p,
            "extensions-empty".into(),
            "No extensions yet".into(),
            "The G-ExtensionStore has ready-made extensions for Flash".into(),
            el(),
            false,
            vec![
                Button::new("Import from G-Earth")
                    .variant(ButtonVariant::Outline)
                    .on_press_tx(choose_gearth)
                    .build(k)
                    .key("extensions-empty-gearth"),
                Button::new("Browse store")
                    .variant(ButtonVariant::Contrast)
                    .on_press(App::open_store)
                    .build(k)
                    .key("extensions-empty-browse"),
            ],
        ));
    }
    for (id, info) in s.extensions.iter().filter(|(_, info)| info.file.is_none()) {
        items.push(item(
            p,
            format!("manual-{}", info.title),
            info.title.clone(),
            format!(
                "{} · v{} · started outside Bobba",
                info.author, info.version
            ),
            dot(p.green),
            false,
            vec![
                if info.on_click_used {
                    open_button(*id, &info.title, format!("manual-open-{}", info.title))
                } else {
                    slot()
                },
                slot(),
            ],
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
    let install_from_file = Button::new("Install from file")
        .variant(ButtonVariant::Outline)
        .on_press_tx(install)
        .build(k)
        .key("install-from-file");
    let banner = gearth_banner(s);
    let import_offered = empty || banner.is_some();
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
                    text("Extensions")
                        .font(TextStyle::new(20.0, 650.0))
                        .color(k.c.text_base)
                        .grow(1.0)
                        .shrink()
                        .truncate(),
                )
                .child(install_from_file),
        )
        .child(
            col()
                .key("installed")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .gap(12.0)
                .children(banner)
                .child(
                    col()
                        .w_full()
                        .grow(1.0)
                        .min_h(0.0)
                        .scroll(ScrollSpec {
                            stick_bottom: false,
                        })
                        .scroll_edge_fade(24.0)
                        .child(list)
                        .children((!import_offered).then(|| {
                            row().w_full().pt(8.0).justify_end().child(
                                Button::new("Import from G-Earth…")
                                    .variant(ButtonVariant::GhostMuted)
                                    .on_press_tx(choose_gearth)
                                    .build(k)
                                    .key("gearth-import-choose"),
                            )
                        })),
                ),
        )
}

pub fn cancel_uninstall(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if e.key != Key::Escape || s.confirm_uninstall.is_none() {
        return false;
    }
    s.confirm_uninstall = None;
    tx.instant();
    true
}
