use rune::tok::TextStyle;
use rune::{Color, El, Spring, SpringPair, Transition, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Loader};

use crate::setup::{Mark, Progress, Step};
use crate::state::{App, Mode, Phase};
use crate::view::theme::{Paint, paint};

const TITLE: TextStyle = TextStyle::new(20.0, 650.0);
const WIDTH: f32 = 468.0;
const MARKER: f32 = 22.0;
const FADE: Spring = Spring::new(0.18, 0.0);
const ENTER: SpringPair = SpringPair::new(Spring::new(0.25, 0.0), Spring::new(0.2, 0.0));

fn title(step: Step) -> &'static str {
    match step {
        Step::Prepare => "Prepare Habbo",
        Step::Allow => "Give permission",
        Step::Open => "Open Habbo",
    }
}

fn detail(step: Step) -> &'static str {
    match step {
        Step::Prepare => "Re-signs Habbo.app so Bobba can read its memory",
        Step::Allow => "Your Mac password, to edit /etc/hosts",
        Step::Open => "Bobba waits for Habbo, then undoes /etc/hosts",
    }
}

fn header(progress: &Progress) -> (&'static str, &'static str) {
    match progress {
        Progress::NotStarted => (
            "Connect to Habbo",
            "Bobba re-signs Habbo.app once, and redirects Habbo through /etc/hosts.",
        ),
        Progress::At {
            mark: Mark::Failed(_),
            ..
        } => (
            "Setup stopped",
            "The step below failed. Nothing after it ran.",
        ),
        Progress::At { .. } => (
            "Getting set up",
            "Re-signing happens once. The /etc/hosts redirect asks for your password every time.",
        ),
    }
}

fn button(label: &str, variant: ButtonVariant, key: &str, on: impl Fn(&mut App) + 'static) -> El {
    Button::new(label)
        .variant(variant)
        .on_press(on)
        .build(&paint().kit)
        .key(key.to_owned())
}

fn connect_button(label: &str) -> El {
    button(label, ButtonVariant::Contrast, "connect", App::connect)
}

fn abort_button(label: &str) -> El {
    button(label, ButtonVariant::Ghost, "abort", App::abort)
}

fn resign_button() -> El {
    button(
        "Prepare Habbo",
        ButtonVariant::Contrast,
        "resign",
        |s: &mut App| {
            if let Phase::NeedsResign(app) = &s.phase {
                let app = app.clone();
                s.resign(app);
            }
        },
    )
}

fn crossfading_marker(p: &Paint, step: Step, mark: &Mark) -> El {
    let k = &p.kit;
    let index = step.index();
    let layer = |name: &str, on: bool| {
        el().key(format!("setup-marker-{index}-{name}"))
            .absolute(0.0, 0.0)
            .size(MARKER, MARKER)
            .radius(MARKER / 2.0)
            .center()
            .opacity(if on { 1.0 } else { 0.0 })
            .opacity_motion(FADE)
    };
    let number = |ink: Color| {
        text((index + 1).to_string())
            .font(k.ty.text_xsmall_medium)
            .color(ink)
    };
    el().key(format!("setup-marker-{index}"))
        .size(MARKER, MARKER)
        .child(
            layer("todo", *mark == Mark::Todo)
                .border(1.0, p.hairline)
                .child(number(k.c.text_faint)),
        )
        .child(
            layer("working", *mark == Mark::Working)
                .child(Loader::new().size(MARKER).color(p.amber).build(k)),
        )
        .child(
            layer("yours", *mark == Mark::WaitingOnPerson)
                .border(1.5, p.amber)
                .child(number(p.amber)),
        )
        .child(
            layer("done", *mark == Mark::Done)
                .fill(p.green.alpha(0.14))
                .child(icon(k.glyph.check_small, 14.0).color(p.green)),
        )
        .child(
            layer("failed", matches!(mark, Mark::Failed(_)))
                .border(1.5, p.red)
                .child(icon(k.glyph.close, 11.0).color(p.red)),
        )
}

fn failure(step: Step, reason: &str) -> String {
    let what = match step {
        Step::Prepare => "Couldn't re-sign Habbo.app",
        Step::Allow => "Couldn't edit /etc/hosts",
        Step::Open => "The session with Habbo failed",
    };
    format!("{what}: {reason}.")
}

fn prompt(s: &App, step: Step, mark: &Mark) -> Option<(String, Vec<El>)> {
    match (step, mark, &s.phase) {
        (_, Mark::Failed(reason), _) => {
            Some((failure(step, reason), vec![connect_button("Try again")]))
        }
        (Step::Prepare, Mark::WaitingOnPerson, _) => Some((
            "Bobba re-signs the Habbo.app the Launcher installed, so Bobba can read its memory. The original stays next to it as Habbo.bobba-original.app."
                .into(),
            vec![resign_button(), abort_button("Not now")],
        )),
        (Step::Prepare, Mark::Done, Phase::Prepared(_)) => Some((
            "Habbo.app is re-signed. The original is Habbo.bobba-original.app.".into(),
            vec![connect_button("Continue")],
        )),
        (Step::Allow, Mark::WaitingOnPerson, _) => Some((
            "macOS is asking for your password so Bobba can point Habbo's game servers at itself in /etc/hosts. The entries go away when Habbo connects."
                .into(),
            vec![abort_button("Cancel")],
        )),
        (Step::Open, Mark::WaitingOnPerson, Phase::WaitingForClient(hotels)) => Some((
            format!(
                "Open the Habbo Launcher and press Play. Bobba is listening for {hotels} hotels."
            ),
            vec![abort_button("Cancel")],
        )),
        _ => None,
    }
}

fn line(p: &Paint, s: &App, progress: &Progress, step: Step) -> El {
    let k = &p.kit;
    let index = step.index();
    let mark = progress.mark_of(step);
    let ink = if mark == Mark::Todo {
        k.c.text_muted
    } else {
        k.c.text_base
    };
    let trailing = match &mark {
        Mark::Done => Some(("Done", k.c.text_faint)),
        Mark::WaitingOnPerson => Some(("Your turn", p.amber)),
        _ => None,
    };
    let head = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(crossfading_marker(p, step, &mark))
        .child(
            col()
                .grow(1.0)
                .gap(2.0)
                .child(text(title(step)).font(k.ty.ui_medium).color(ink))
                .child(
                    text(detail(step))
                        .font(k.ty.small)
                        .color(k.c.text_faint)
                        .truncate(),
                ),
        )
        .children(trailing.map(|(label, ink)| text(label).font(k.ty.small).color(ink)));
    let mut item = col()
        .key(format!("setup-step-{index}"))
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .child(head);
    if progress.step() == Some(step)
        && let Some((body, actions)) = prompt(s, step, &mark)
    {
        let open = col()
            .key(format!("setup-open-{index}"))
            .w_full()
            .pl(MARKER + 12.0)
            .gap(12.0)
            .child(
                text(body)
                    .key("setup-said")
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap(),
            )
            .child(actions.into_iter().fold(row().gap(8.0), El::child))
            .transition(
                Transition::offset(0.0, 8.0)
                    .combined(Transition::opacity())
                    .springs(ENTER),
            );
        item = item.fill(p.panel_raised).child(open);
    }
    item
}

pub fn view(s: &App) -> El {
    let p = paint();
    let k = &p.kit;
    let progress = Progress::of(&s.phase);
    let (heading, body) = header(&progress);
    let list = Step::ALL.iter().enumerate().fold(
        col()
            .key("setup-steps")
            .w_full()
            .radius(12.0)
            .fill(p.panel)
            .border(1.0, p.hairline)
            .clip_rounded(12.0),
        |list, (i, step)| {
            let list = if i == 0 {
                list
            } else {
                list.child(el().w_full().h(1.0).fill(p.hairline))
            };
            list.child(line(p, s, &progress, *step))
        },
    );
    let footer = match &progress {
        Progress::NotStarted => Some(
            row()
                .w_full()
                .center_y()
                .child(connect_button("Connect"))
                .child(el().grow(1.0))
                .child(button(
                    "Use a custom host and port",
                    ButtonVariant::GhostMuted,
                    "switch-mode",
                    |s: &mut App| {
                        s.mode = Mode::Manual;
                        s.phase = Phase::Idle;
                    },
                )),
        ),
        Progress::At {
            mark: Mark::Working,
            ..
        } => Some(row().child(abort_button("Cancel"))),
        Progress::At { .. } => None,
    };
    col()
        .key("connection")
        .w_full()
        .h_full()
        .pt(36.0)
        .items_center()
        .child(
            col()
                .key("setup")
                .w(WIDTH)
                .gap(18.0)
                .child(
                    col()
                        .gap(6.0)
                        .child(
                            text(heading)
                                .key("hero-title")
                                .font(TITLE)
                                .color(k.c.text_base),
                        )
                        .child(
                            text(body)
                                .key("setup-subtitle")
                                .font(k.ty.body)
                                .color(k.c.text_muted)
                                .wrap(),
                        ),
                )
                .child(list)
                .children(footer),
        )
}
