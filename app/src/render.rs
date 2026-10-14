//! Renders a key (background, icon, title) into the JPEG the device expects.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use ajazz_hid::akp153e::{IMAGE_ROTATION, IMAGE_SIZE};
use image::{imageops, Rgb, RgbImage, RgbaImage};

use crate::config::{Align, Key};

const DEFAULT_FONT: &str = r"C:\Windows\Fonts\segoeuib.ttf";
const DEFAULT_TITLE_SIZE: u16 = 16;

pub struct Renderer {
    font: Option<FontVec>,
    base_dir: PathBuf,
    /// JPEGs keyed by a hash of the key's look and its icon's mtime.
    cache: HashMap<u64, Vec<u8>>,
}

impl Renderer {
    pub fn new(font: Option<&Path>, base_dir: &Path) -> Self {
        let path = font
            .map(|f| base_dir.join(f))
            .unwrap_or_else(|| DEFAULT_FONT.into());
        let font = std::fs::read(&path)
            .ok()
            .and_then(|b| FontVec::try_from_vec(b).ok());
        if font.is_none() {
            crate::log(&format!(
                "font {} could not be loaded; titles are disabled",
                path.display()
            ));
        }
        Renderer {
            font,
            base_dir: base_dir.to_path_buf(),
            cache: HashMap::new(),
        }
    }

    fn icon_path(&self, key: &Key, active: bool) -> Option<PathBuf> {
        let icon = if active {
            key.icon_active.as_ref().or(key.icon.as_ref())
        } else {
            key.icon.as_ref()
        };
        icon.map(|i| self.base_dir.join(i))
    }

    /// Identifies what `key` looks like: only the visual fields (not its
    /// position or action) plus the icon file's modification time.
    pub fn font(&self) -> Option<&FontVec> {
        self.font.as_ref()
    }

    pub fn look_id(&self, key: &Key, active: bool) -> u64 {
        let mut h = DefaultHasher::new();
        (&key.icon, &key.icon_active, &key.title, &key.color).hash(&mut h);
        (key.title_size, key.title_align, active).hash(&mut h);
        let mtime = self
            .icon_path(key, active)
            .and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        mtime.hash(&mut h);
        h.finish()
    }

    /// Device-ready JPEG for `key`, cached until its look or icon file changes.
    pub fn jpeg(&mut self, key: &Key, active: bool) -> Vec<u8> {
        let id = self.look_id(key, active);
        if let Some(j) = self.cache.get(&id) {
            return j.clone();
        }
        let jpeg = encode(&self.draw(key, active));
        self.cache.insert(id, jpeg.clone());
        jpeg
    }

    /// Upright RGB image of the key.
    pub fn draw(&self, key: &Key, active: bool) -> RgbImage {
        let bg = key
            .color
            .as_deref()
            .and_then(parse_color)
            .unwrap_or(Rgb([0, 0, 0]));
        let mut img = RgbImage::from_pixel(IMAGE_SIZE, IMAGE_SIZE, bg);
        if let Some(path) = self.icon_path(key, active) {
            match image::open(&path) {
                Ok(icon) => {
                    let icon = icon.resize(IMAGE_SIZE, IMAGE_SIZE, imageops::FilterType::Lanczos3);
                    let (x, y) = (
                        (IMAGE_SIZE - icon.width()) / 2,
                        (IMAGE_SIZE - icon.height()) / 2,
                    );
                    blend(&mut img, &icon.to_rgba8(), x, y);
                }
                Err(e) => crate::log(&format!("key {}: icon {}: {e}", key.pos, path.display())),
            }
        }
        if active && key.icon_active.is_none() {
            draw_border(&mut img, 5, Rgb([40, 220, 90]));
        }
        if let (Some(font), Some(title)) =
            (&self.font, key.title.as_deref().filter(|t| !t.is_empty()))
        {
            let align = key.title_align.unwrap_or(if key.icon.is_some() {
                Align::Bottom
            } else {
                Align::Middle
            });
            draw_title(
                &mut img,
                font,
                title,
                key.title_size.unwrap_or(DEFAULT_TITLE_SIZE),
                align,
            );
        }
        img
    }
}

pub fn parse_color(s: &str) -> Option<Rgb<u8>> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8]))
}

fn draw_border(img: &mut RgbImage, width: u32, color: Rgb<u8>) {
    let (w, h) = img.dimensions();
    for (x, y, p) in img.enumerate_pixels_mut() {
        if x < width || y < width || x >= w - width || y >= h - width {
            *p = color;
        }
    }
}

fn blend(dst: &mut RgbImage, src: &RgbaImage, ox: u32, oy: u32) {
    for (x, y, p) in src.enumerate_pixels() {
        let (dx, dy) = (ox + x, oy + y);
        if dx >= dst.width() || dy >= dst.height() {
            continue;
        }
        let a = p[3] as u32;
        let d = dst.get_pixel_mut(dx, dy);
        for c in 0..3 {
            d[c] = ((p[c] as u32 * a + d[c] as u32 * (255 - a)) / 255) as u8;
        }
    }
}

/// Draws `text` (one line per `\n`), centred horizontally, with a dark outline.
fn draw_title(img: &mut RgbImage, font: &FontVec, text: &str, size: u16, align: Align) {
    let scale = PxScale::from(size as f32);
    let sf = font.as_scaled(scale);
    let line_h = sf.height() + sf.line_gap();
    let lines: Vec<&str> = text.lines().collect();
    let block_h = line_h * lines.len() as f32;
    let pad = 4.0;
    let top = match align {
        Align::Top => pad,
        Align::Middle => (IMAGE_SIZE as f32 - block_h) / 2.0,
        Align::Bottom => IMAGE_SIZE as f32 - block_h - pad,
    };

    // Coverage mask for the text, so the outline can be derived from it.
    let n = IMAGE_SIZE as usize;
    let mut mask = vec![0f32; n * n];
    for (i, line) in lines.iter().enumerate() {
        let width: f32 = line.chars().map(|c| sf.h_advance(font.glyph_id(c))).sum();
        let mut x = (IMAGE_SIZE as f32 - width) / 2.0;
        let baseline = top + i as f32 * line_h + sf.ascent();
        for c in line.chars() {
            let gid = font.glyph_id(c);
            let glyph = gid.with_scale_and_position(scale, point(x, baseline));
            x += sf.h_advance(gid);
            if let Some(og) = font.outline_glyph(glyph) {
                let b = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if (0..n as i32).contains(&px) && (0..n as i32).contains(&py) {
                        let m = &mut mask[py as usize * n + px as usize];
                        *m = m.max(cov);
                    }
                });
            }
        }
    }

    for y in 0..n {
        for x in 0..n {
            // Outline: max coverage in a 5x5 neighbourhood.
            let mut o = 0f32;
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let (sx, sy) = (x as i32 + dx, y as i32 + dy);
                    if (0..n as i32).contains(&sx) && (0..n as i32).contains(&sy) {
                        o = o.max(mask[sy as usize * n + sx as usize]);
                    }
                }
            }
            let fill = mask[y * n + x];
            let p = img.get_pixel_mut(x as u32, y as u32);
            for c in 0..3 {
                let v = p[c] as f32 * (1.0 - o * 0.85); // darken under the outline
                p[c] = (v * (1.0 - fill) + 255.0 * fill) as u8;
            }
        }
    }
}

/// Width in pixels of one line of `text`.
pub fn text_width(font: &FontVec, text: &str, size: f32) -> f32 {
    let sf = font.as_scaled(PxScale::from(size));
    text.chars().map(|c| sf.h_advance(font.glyph_id(c))).sum()
}

/// Draws one line of `text` horizontally centred on `cx`, with its top at `top`.
pub fn draw_text_line(
    img: &mut RgbImage,
    font: &FontVec,
    text: &str,
    size: f32,
    cx: f32,
    top: f32,
    color: Rgb<u8>,
) {
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let mut x = cx - text_width(font, text, size) / 2.0;
    let baseline = top + sf.ascent();
    for c in text.chars() {
        let gid = font.glyph_id(c);
        let glyph = gid.with_scale_and_position(scale, point(x, baseline));
        x += sf.h_advance(gid);
        if let Some(og) = font.outline_glyph(glyph) {
            let b = og.px_bounds();
            og.draw(|gx, gy, cov| {
                let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                    let p = img.get_pixel_mut(px as u32, py as u32);
                    for ch in 0..3 {
                        p[ch] = (p[ch] as f32 * (1.0 - cov) + color[ch] as f32 * cov) as u8;
                    }
                }
            });
        }
    }
}

/// Rotates into device orientation and encodes as JPEG.
pub fn encode(img: &RgbImage) -> Vec<u8> {
    let rotated = match IMAGE_ROTATION {
        90 => imageops::rotate90(img),
        180 => imageops::rotate180(img),
        270 => imageops::rotate270(img),
        _ => img.clone(),
    };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(&rotated)
        .expect("JPEG encoding into memory cannot fail");
    out
}
