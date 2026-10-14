//! Test-pattern images that make cropping, rotation and mirroring obvious.

use image::{imageops, Rgb, RgbImage};

/// 5x7 bitmap glyphs; each row's low 5 bits are pixels, MSB on the left.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        _ => [0; 7],
    }
}

fn fill_rect(img: &mut RgbImage, x0: u32, y0: u32, w: u32, h: u32, c: Rgb<u8>) {
    for y in y0..(y0 + h).min(img.height()) {
        for x in x0..(x0 + w).min(img.width()) {
            img.put_pixel(x, y, c);
        }
    }
}

/// Draws `text` centered, as large as fits in ~60% of the image width.
fn draw_text(img: &mut RgbImage, text: &str, c: Rgb<u8>) {
    let n = text.chars().count().max(1) as u32;
    let (w, h) = img.dimensions();
    // Each glyph is 5 px wide plus 1 px spacing.
    let scale = ((w * 6 / 10) / (n * 6 - 1)).clamp(1, h * 5 / 10 / 7);
    let tw = (n * 6 - 1) * scale;
    let (ox, oy) = ((w - tw) / 2, (h - 7 * scale) / 2);
    for (i, ch) in text.chars().enumerate() {
        let gx = ox + i as u32 * 6 * scale;
        for (row, bits) in glyph(ch).iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) != 0 {
                    fill_rect(
                        img,
                        gx + col * scale,
                        oy + row as u32 * scale,
                        scale,
                        scale,
                        c,
                    );
                }
            }
        }
    }
}

/// How the upright pattern is transformed before upload.
#[derive(Clone, Copy, Debug)]
pub struct Orientation {
    /// Clockwise rotation in degrees: 0, 90, 180 or 270.
    pub rotate: u16,
    pub flip_h: bool,
}

impl std::fmt::Display for Orientation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rot{}{}",
            self.rotate,
            if self.flip_h { "+flipH" } else { "" }
        )
    }
}

fn hue_color(i: u32) -> Rgb<u8> {
    const PALETTE: [[u8; 3]; 6] = [
        [30, 70, 160],
        [20, 120, 70],
        [130, 40, 130],
        [150, 90, 20],
        [20, 110, 130],
        [90, 90, 90],
    ];
    Rgb(PALETTE[i as usize % PALETTE.len()])
}

/// An upright `size`x`size` pattern: colored background, white border,
/// a red block in the top-left corner and a centered label.
pub fn test_pattern(size: u32, label: &str, color_index: u32) -> RgbImage {
    let mut img = RgbImage::from_pixel(size, size, hue_color(color_index));
    let white = Rgb([255, 255, 255]);
    let b = (size / 30).max(2);
    fill_rect(&mut img, 0, 0, size, b, white);
    fill_rect(&mut img, 0, size - b, size, b, white);
    fill_rect(&mut img, 0, 0, b, size, white);
    fill_rect(&mut img, size - b, 0, b, size, white);
    let m = size / 5;
    fill_rect(&mut img, b, b, m, m, Rgb([230, 30, 30]));
    draw_text(&mut img, label, white);
    img
}

pub fn orient(img: RgbImage, o: Orientation) -> RgbImage {
    let img = match o.rotate {
        90 => imageops::rotate90(&img),
        180 => imageops::rotate180(&img),
        270 => imageops::rotate270(&img),
        _ => img,
    };
    if o.flip_h {
        imageops::flip_horizontal(&img)
    } else {
        img
    }
}

pub fn to_jpeg(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(img)
        .expect("JPEG encoding into memory cannot fail");
    out
}

/// Draws `text` with its top-left corner at (x, y), `scale` pixels per font pixel.
fn draw_text_at(img: &mut RgbImage, text: &str, x: u32, y: u32, scale: u32, c: Rgb<u8>) {
    for (i, ch) in text.chars().enumerate() {
        let gx = x + i as u32 * 6 * scale;
        for (row, bits) in glyph(ch).iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) != 0 {
                    fill_rect(
                        img,
                        gx + col * scale,
                        y + row as u32 * scale,
                        scale,
                        scale,
                        c,
                    );
                }
            }
        }
    }
}

/// Calibration image: grid lines every 10 px (brighter every 50), coordinate
/// labels every 20 px along the top and every 10 px down the left, and a 2 px coloured edge on
/// each side (top red, right green, bottom blue, left yellow) so it is obvious
/// which edges the screen shows. `y_offset` is added to the vertical labels,
/// so stacked slices of one tall canvas keep continuous coordinates.
pub fn ruler(w: u32, h: u32, y_offset: u32, label: &str) -> RgbImage {
    let mut img = RgbImage::from_pixel(w, h, Rgb([20, 24, 32]));
    for x in (0..w).step_by(10) {
        let c = if x.is_multiple_of(50) {
            Rgb([110, 110, 130])
        } else {
            Rgb([50, 52, 64])
        };
        fill_rect(&mut img, x, 0, 1, h, c);
    }
    for y in 0..h {
        if (y + y_offset).is_multiple_of(10) {
            let c = if (y + y_offset).is_multiple_of(50) {
                Rgb([110, 110, 130])
            } else {
                Rgb([50, 52, 64])
            };
            fill_rect(&mut img, 0, y, w, 1, c);
        }
    }
    let white = Rgb([235, 235, 235]);
    for x in (20..w.saturating_sub(10)).step_by(20) {
        draw_text_at(&mut img, &x.to_string(), x + 2, 4, 1, white);
    }
    for y in 0..h.saturating_sub(10) {
        let v = y + y_offset;
        if v.is_multiple_of(10) && y > 12 {
            draw_text_at(&mut img, &v.to_string(), 4, y + 2, 1, white);
        }
    }
    draw_text_at(&mut img, label, w / 2 - 6, h / 2 - 7, 2, Rgb([255, 220, 0]));
    let t = 2;
    fill_rect(&mut img, 0, 0, w, t, Rgb([230, 40, 40])); // top: red
    fill_rect(&mut img, 0, h - t, w, t, Rgb([60, 110, 255])); // bottom: blue
    fill_rect(&mut img, 0, 0, t, h, Rgb([250, 220, 0])); // left: yellow
    fill_rect(&mut img, w - t, 0, t, h, Rgb([40, 200, 80])); // right: green
    img
}

/// One slice of a tall seamless canvas: a 10 px grid with continuous y labels
/// every 10 px, no borders and a lighter background, so gaps (dark bands) or
/// overlaps (short grid squares) at the joins between slots stand out.
pub fn canvas_slice(w: u32, h: u32, y_offset: u32) -> RgbImage {
    let mut img = RgbImage::from_pixel(w, h, Rgb([40, 64, 96]));
    for x in (0..w).step_by(10) {
        let c = if x.is_multiple_of(50) {
            Rgb([170, 180, 200])
        } else {
            Rgb([80, 104, 136])
        };
        fill_rect(&mut img, x, 0, 1, h, c);
    }
    for y in 0..h {
        let v = y + y_offset;
        if v.is_multiple_of(10) {
            let c = if v.is_multiple_of(50) {
                Rgb([170, 180, 200])
            } else {
                Rgb([80, 104, 136])
            };
            fill_rect(&mut img, 0, y, w, 1, c);
            if y + 9 < h {
                draw_text_at(&mut img, &v.to_string(), 30, y + 2, 1, Rgb([255, 255, 255]));
            }
        }
    }
    img
}
