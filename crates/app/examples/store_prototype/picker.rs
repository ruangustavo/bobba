use rune::{Color, Curve, Easing, El, Family, TextStyle, el, oklcha, row, text};

use bobba::state::App;

use crate::{VARIANTS, active, replay, select};

const ITEM: f32 = 84.0;
const GAP: f32 = 2.0;
const FONT: TextStyle = TextStyle::new(13.0, 500.0).family(Family::Named("SF Pro Text, Segoe UI"));
const SLIDE: Curve = Curve::Tween {
    duration: 0.25,
    easing: Easing::cubic(0.23, 1.0, 0.32, 1.0),
};

fn white(alpha: f32) -> Color {
    oklcha(1.0, 0.0, 0.0, alpha)
}

fn item(index: usize, name: &'static str, on: bool) -> El {
    row()
        .key(format!("proto-picker-item-{index}"))
        .role("button")
        .label(name)
        .w(ITEM)
        .h(28.0)
        .radius(14.0)
        .center()
        .child(
            text(name)
                .font(FONT)
                .color(white(if on { 1.0 } else { 0.55 })),
        )
        .on_press(move |s: &mut App| select(s, index))
}

pub fn view() -> El {
    let highlight = el()
        .key("proto-picker-highlight")
        .absolute(4.0, 4.0)
        .size(ITEM, 28.0)
        .radius(14.0)
        .fill(white(0.12))
        .offset(active() as f32 * (ITEM + GAP), 0.0)
        .offset_motion(SLIDE);
    let pill = VARIANTS.iter().enumerate().fold(
        row()
            .key("proto-picker")
            .role("navigation")
            .label("Prototype variants")
            .gap(GAP)
            .padding(4.0)
            .center_y()
            .radius(18.0)
            .fill(oklcha(0.145, 0.0, 0.0, 0.82))
            .backdrop_blur(12.0)
            .border(1.0, white(0.08))
            .shadow(8.0, 24.0, oklcha(0.0, 0.0, 0.0, 0.24))
            .shadow(2.0, 6.0, oklcha(0.0, 0.0, 0.0, 0.12))
            .child(highlight),
        |pill, (i, variant)| pill.child(item(i, variant.name, i == active())),
    );
    let pill = pill.child(el().w(1.0).h(16.0).fill(white(0.12))).child(
        row()
            .key("proto-picker-replay")
            .role("button")
            .label("Replay animation (R)")
            .h(28.0)
            .px(10.0)
            .radius(14.0)
            .center()
            .child(
                text("↻")
                    .font(TextStyle::new(14.0, 500.0))
                    .color(white(0.55)),
            )
            .on_press(replay),
    );
    row()
        .key("proto-picker-dock")
        .pin(Some(0.0), None, Some(0.0), Some(24.0))
        .justify_center()
        .child(pill)
}
