use std::io::Cursor;

pub const ICON_SIDE: u32 = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub rgba: Vec<u8>,
}

impl Icon {
    pub fn decode(bytes: &[u8]) -> Option<Icon> {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(
            png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
        );
        let mut reader = decoder.read_info().ok()?;
        let mut buffer = vec![0; reader.output_buffer_size()?];
        let frame = reader.next_frame(&mut buffer).ok()?;
        let channels = match frame.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::GrayscaleAlpha => 2,
            _ => return None,
        };
        let (width, height) = (frame.width, frame.height);
        if width == 0 || height == 0 {
            return None;
        }
        let mut rgba = Vec::with_capacity((ICON_SIDE * ICON_SIDE * 4) as usize);
        for y in 0..ICON_SIDE {
            let row = (y * height / ICON_SIDE) as usize * frame.line_size;
            for x in 0..ICON_SIDE {
                let at = row + (x * width / ICON_SIDE) as usize * channels;
                let source = buffer.get(at..at + channels)?;
                match source {
                    [r, g, b, a] => rgba.extend_from_slice(&[*r, *g, *b, *a]),
                    [gray, a] => rgba.extend_from_slice(&[*gray, *gray, *gray, *a]),
                    _ => return None,
                }
            }
        }
        Some(Icon { rgba })
    }

    pub fn side(&self) -> u32 {
        ICON_SIDE
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn badge_png(side: u32) -> Vec<u8> {
        let mut rgba = Vec::new();
        for y in 0..side {
            for x in 0..side {
                let on = (x + y) % 2 == 0;
                rgba.extend_from_slice(if on {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 0]
                });
            }
        }
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, side, side);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(&rgba).expect("pixels");
        writer.finish().expect("finishes");
        bytes
    }

    fn pixel(icon: &Icon, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * ICON_SIDE + x) * 4) as usize;
        let mut out = [0; 4];
        out.copy_from_slice(&icon.rgba[at..at + 4]);
        out
    }

    #[test]
    fn a_badge_of_40_pixels_doubles_each_pixel() {
        let icon = Icon::decode(&badge_png(40)).expect("decodes");
        assert_eq!(icon.rgba.len(), (ICON_SIDE * ICON_SIDE * 4) as usize);
        assert_eq!(pixel(&icon, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&icon, 1, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(&icon, 2, 0), [0, 0, 255, 0]);
        assert_eq!(pixel(&icon, 3, 1), [0, 0, 255, 0]);
    }

    #[test]
    fn a_badge_of_80_pixels_keeps_its_pixels() {
        let icon = Icon::decode(&badge_png(80)).expect("decodes");
        assert_eq!(pixel(&icon, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&icon, 1, 0), [0, 0, 255, 0]);
    }

    #[test]
    fn bytes_that_are_not_a_png_have_no_icon() {
        assert_eq!(Icon::decode(b"<html>404</html>"), None);
    }
}
