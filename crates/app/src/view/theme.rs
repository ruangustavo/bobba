use std::sync::OnceLock;

use rune::{Color, oklch};
use rune_kit::{Kit, PaletteSeed};

pub struct Paint {
    pub kit: Kit,
    pub mono_kit: Kit,
    pub amber: Color,
    pub green: Color,
    pub red: Color,
    pub blue: Color,
    pub panel: Color,
    pub panel_raised: Color,
    pub hairline: Color,
}

pub fn paint() -> &'static Paint {
    static PAINT: OnceLock<Paint> = OnceLock::new();
    PAINT.get_or_init(|| {
        let kit = Kit::from_palette(PaletteSeed {
            bg: oklch(0.165, 0.008, 275.0),
            fg: oklch(0.955, 0.006, 275.0),
            accent: oklch(0.82, 0.15, 80.0),
        });
        let mut mono_kit = kit.clone();
        mono_kit.ty.ui = kit.ty.mono;
        Paint {
            mono_kit,
            amber: oklch(0.82, 0.15, 80.0),
            green: oklch(0.78, 0.15, 155.0),
            red: oklch(0.68, 0.19, 22.0),
            blue: oklch(0.72, 0.13, 250.0),
            panel: oklch(0.20, 0.009, 275.0),
            panel_raised: oklch(0.235, 0.010, 275.0),
            hairline: oklch(0.30, 0.010, 275.0),
            kit,
        }
    })
}
