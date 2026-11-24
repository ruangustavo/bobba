use rune::tok::TextStyle;
use std::path::Path;

use rune::{Color, El, FileDialogOptions, Key, KeyEvent, ScrollSpec, Tx, col, el, icon, row, text};
use rune_kit::{
    Badge, BadgeVariant, Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Loader,
};

use crate::backend::{Command, InstalledKind};
use crate::state::{App, GEarthOffer};
use crate::store::Showing;
use crate::view::store;
use crate::view::theme::{Paint, paint};

static PLAY: rune::Icon = rune::Icon {
    name: "bobba-play",
    view: 16.0,
    body: r#"<path d="M4.75 3.5L12.55 8L4.75 12.5Z" fill="currentColor" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"/>"#,
};

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

fn showing_segment(s: &App, showing: Showing, count: Option<usize>) -> El {
    let p = paint();
    let k = &p.kit;
    let on = s.store.showing == showing;
    let (label, index, name) = match showing {
        Showing::Installed => ("Installed", 0, "installed"),
        Showing::Store => ("Store", 1, "store"),
    };
    let segment = row()
        .key(format!("segment{index}"))
        .role("tab")
        .label(label)
        .h(26.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(7.0)
        .child(
            text(label)
                .font(k.ty.ui)
                .color(if on { k.c.text_base } else { k.c.text_muted })
                .group_color(k.c.text_base),
        )
        .children(count.map(|n| {
            text(n.to_string())
                .key(format!("extensions-count-{name}"))
                .font(k.ty.small)
                .color(if on { k.c.text_muted } else { k.c.text_faint })
        }));
    if on {
        segment.fill(p.panel_raised)
    } else {
        k.interactive(segment.hover_group())
            .on_press(move |s: &mut App| s.show(showing))
    }
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
            .find(|(_, info)| info.file.as_deref() == Some(state.id.as_str()));
        let fallback = match &state.kind {
            InstalledKind::Package { title, .. } => title.clone(),
            InstalledKind::Single(_) => state.id.clone(),
        };
        let title = connected.map_or(fallback, |(_, info)| info.title.clone());
        let (status, status_color) = match (connected.is_some(), state.running) {
            (true, _) => ("connected", p.green),
            (false, true) => ("starting", p.amber),
            (false, false) => ("stopped", k.c.text_faint),
        };
        let detail = match &state.kind {
            InstalledKind::Single(runtime) => format!("{runtime:?} · {status} · {}", state.id),
            InstalledKind::Package { version, .. } => format!("v{version} · {status}"),
        };
        let file = state.id.clone();
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
        let update = match &state.kind {
            InstalledKind::Package { title, version } => {
                s.store.update_for(title, version).map(|listing| {
                    let title = title.clone();
                    Button::new(format!("Update to {}", listing.version))
                        .variant(ButtonVariant::Outline)
                        .on_press(move |s: &mut App| s.install_from_store(title.clone()))
                        .build(k)
                        .key(format!("installed-update-{}", state.id))
                })
            }
            InstalledKind::Single(_) => None,
        };
        items.push(item(
            p,
            format!("installed-{}", state.id),
            title,
            detail,
            dot(status_color),
            update.into_iter().chain([toggle, uninstall]).collect(),
        ));
    }
    if s.installed.is_empty() && !s.extensions.iter().any(|(_, info)| info.file.is_none()) {
        items.push(item(
            p,
            "extensions-empty".into(),
            "No extensions yet".into(),
            "The G-ExtensionStore has ready-made extensions for Flash".into(),
            el(),
            vec![
                Button::new("Import from G-Earth")
                    .variant(ButtonVariant::Outline)
                    .on_press_tx(choose_gearth)
                    .build(k)
                    .key("extensions-empty-gearth"),
                Button::new("Browse store")
                    .variant(ButtonVariant::Contrast)
                    .on_press(|s: &mut App| s.show(Showing::Store))
                    .build(k)
                    .key("extensions-empty-browse"),
            ],
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
    let showing = s.store.showing;
    let manual = s
        .extensions
        .iter()
        .filter(|(_, info)| info.file.is_none())
        .count();
    let switch = row()
        .key("extensions-showing")
        .role("tablist")
        .label("Show")
        .gap(2.0)
        .child(showing_segment(
            s,
            Showing::Installed,
            Some(s.installed.len() + manual),
        ))
        .child(showing_segment(
            s,
            Showing::Store,
            s.store.catalog().map(|catalog| catalog.listings.len()),
        ));
    let install_from_file = (showing == Showing::Installed).then(|| {
        Button::new("Install from file")
            .variant(ButtonVariant::Outline)
            .on_press_tx(install)
            .build(k)
            .key("install-from-file")
    });
    let body = match showing {
        Showing::Installed => col()
            .key("installed")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .gap(12.0)
            .children(gearth_banner(s))
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
                    .child(
                        row().w_full().pt(8.0).justify_end().child(
                            Button::new("Import from G-Earth…")
                                .variant(ButtonVariant::GhostMuted)
                                .on_press_tx(choose_gearth)
                                .build(k)
                                .key("gearth-import-choose"),
                        ),
                    ),
            ),
        Showing::Store => store::view(s),
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
                    text("Extensions")
                        .font(TextStyle::new(20.0, 650.0))
                        .color(k.c.text_base)
                        .grow(1.0)
                        .shrink()
                        .truncate(),
                )
                .children(install_from_file)
                .child(switch),
        )
        .child(body)
}

pub fn close_detail(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if e.key != Key::Escape || s.store.detail.is_none() {
        return false;
    }
    s.store.detail = None;
    tx.instant();
    true
}
