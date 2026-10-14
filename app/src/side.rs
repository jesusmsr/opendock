//! The small screen: widgets drawn on one upright canvas, then cut into the
//! three slot images the device expects.

use ab_glyph::FontVec;
use ajazz_hid::akp153e::side::{HEIGHT, SLICE_HEIGHT, SLOTS, WIDTH};
use image::{imageops, Rgb, RgbImage};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

use crate::config::{Side, Widget, WidgetKind};
use crate::render::{draw_text_line, text_width};
use crate::sensors::Readings;

/// Width / height of the small screen.
pub const ASPECT: f32 = WIDTH as f32 / HEIGHT as f32;

const BG: Rgb<u8> = Rgb([0, 0, 0]);
const LABEL: Rgb<u8> = Rgb([150, 155, 165]);
const VALUE: Rgb<u8> = Rgb([240, 240, 240]);
const MISSING: Rgb<u8> = Rgb([90, 90, 95]);
const GOOD: Rgb<u8> = Rgb([90, 200, 120]);
const WARN: Rgb<u8> = Rgb([240, 190, 80]);
const HOT: Rgb<u8> = Rgb([230, 90, 80]);
const TRACK: Rgb<u8> = Rgb([45, 48, 56]);

/// Local wall-clock time, enough for the clock and date widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Now {
    pub hour: u16,
    pub minute: u16,
    pub day: u16,
    pub month: u16,
    pub weekday: u16,
}

impl Now {
    pub fn local() -> Self {
        // SAFETY: GetLocalTime only writes the struct we pass.
        let t = unsafe {
            let mut t = std::mem::zeroed();
            GetLocalTime(&mut t);
            t
        };
        Now {
            hour: t.wHour,
            minute: t.wMinute,
            day: t.wDay,
            month: t.wMonth,
            weekday: t.wDayOfWeek,
        }
    }
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Colour for a value where higher is worse.
fn level(v: f32, warn: f32, hot: f32) -> Rgb<u8> {
    if v >= hot {
        HOT
    } else if v >= warn {
        WARN
    } else {
        GOOD
    }
}

/// What one widget shows: caption, main text and colour, optional 0-100 bar,
/// and an optional hint shown instead of the bar.
struct Face {
    caption: String,
    value: String,
    color: Rgb<u8>,
    bar: Option<f32>,
    hint: Option<&'static str>,
}

fn face(w: &Widget, r: &Readings, now: Now) -> Face {
    let pct = |v: Option<f32>, caption: &str, warn: f32, hot: f32| match v {
        Some(v) => Face {
            caption: caption.into(),
            value: format!("{:.0}%", v),
            color: level(v, warn, hot),
            bar: Some(v),
            hint: None,
        },
        None => Face {
            caption: caption.into(),
            value: "–".into(),
            color: MISSING,
            bar: None,
            hint: None,
        },
    };
    let temp = |v: Option<f32>, caption: &str, hint: Option<&'static str>| match v {
        Some(v) => Face {
            caption: caption.into(),
            value: format!("{:.0}°", v),
            color: level(v, 65.0, 80.0),
            bar: Some(v.clamp(0.0, 100.0)),
            hint: None,
        },
        None => Face {
            caption: caption.into(),
            value: "–".into(),
            color: MISSING,
            bar: None,
            hint,
        },
    };
    let gb = |used: Option<f32>, pct: Option<f32>, caption: &str| match (used, pct) {
        (Some(u), Some(p)) => Face {
            caption: caption.into(),
            value: format!("{u:.1}G"),
            color: level(p, 75.0, 90.0),
            bar: Some(p),
            hint: None,
        },
        _ => Face {
            caption: caption.into(),
            value: "–".into(),
            color: MISSING,
            bar: None,
            hint: None,
        },
    };
    let mut f = match w.kind {
        WidgetKind::Clock => Face {
            caption: format!("{} {}", WEEKDAYS[now.weekday as usize % 7], now.day),
            value: format!("{:02}:{:02}", now.hour, now.minute),
            color: VALUE,
            bar: None,
            hint: None,
        },
        WidgetKind::Date => Face {
            caption: WEEKDAYS[now.weekday as usize % 7].into(),
            value: format!("{} {}", now.day, MONTHS[(now.month as usize + 11) % 12]),
            color: VALUE,
            bar: None,
            hint: None,
        },
        WidgetKind::CpuUsage => pct(r.cpu_usage, "CPU", 60.0, 85.0),
        WidgetKind::CpuTemp => temp(r.cpu_temp, "CPU", Some("needs LHM")),
        WidgetKind::Ram => gb(r.ram_used_gb, r.ram_usage(), "RAM"),
        WidgetKind::GpuTemp => temp(r.gpu_temp, "GPU", None),
        WidgetKind::GpuUsage => pct(r.gpu_usage, "GPU LOAD", 60.0, 85.0),
        WidgetKind::GpuMemory => gb(r.gpu_mem_used_gb, r.gpu_mem_usage(), "VRAM"),
        WidgetKind::Text => Face {
            caption: String::new(),
            value: w.text.clone().unwrap_or_default(),
            color: VALUE,
            bar: None,
            hint: None,
        },
    };
    if let Some(l) = &w.label {
        f.caption = l.clone();
    }
    f
}

fn fill(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, c: Rgb<u8>) {
    for py in y..(y + h).min(img.height()) {
        for px in x..(x + w).min(img.width()) {
            img.put_pixel(px, py, c);
        }
    }
}

/// Largest font size (up to `max`) at which `text` fits in `width`.
fn fit(font: &FontVec, text: &str, max: f32, width: f32) -> f32 {
    let w = text_width(font, text, max);
    if w <= width {
        max
    } else {
        (max * width / w).max(10.0)
    }
}

/// The whole small screen, upright, `WIDTH` x `HEIGHT`.
pub fn draw(font: Option<&FontVec>, side: &Side, r: &Readings, now: Now) -> RgbImage {
    let mut img = RgbImage::from_pixel(WIDTH, HEIGHT, BG);
    let (Some(font), n) = (font, side.widgets.len()) else {
        return img;
    };
    if n == 0 {
        return img;
    }
    let band = HEIGHT as f32 / n as f32;
    let cx = WIDTH as f32 / 2.0;
    let inner = WIDTH as f32 - 10.0;
    for (i, w) in side.widgets.iter().enumerate() {
        let top = i as f32 * band;
        let f = face(w, r, now);
        let has_caption = !f.caption.is_empty();
        let has_footer = f.bar.is_some() || f.hint.is_some();
        // Vertical layout inside the band: caption, value, bar/hint.
        let value_size = fit(font, &f.value, (band * 0.28).min(34.0), inner);
        let caption_size = (band * 0.11).clamp(11.0, 15.0);
        let block = value_size
            + if has_caption { caption_size + 6.0 } else { 0.0 }
            + if has_footer { 14.0 } else { 0.0 };
        let mut y = top + (band - block) / 2.0;
        if has_caption {
            let size = fit(font, &f.caption, caption_size, inner);
            draw_text_line(&mut img, font, &f.caption, size, cx, y, LABEL);
            y += caption_size + 6.0;
        }
        draw_text_line(&mut img, font, &f.value, value_size, cx, y, f.color);
        y += value_size + 6.0;
        if let Some(v) = f.bar {
            let (x0, bw) = (12u32, WIDTH - 24);
            fill(&mut img, x0, y as u32, bw, 5, TRACK);
            let filled = (bw as f32 * (v / 100.0).clamp(0.0, 1.0)).round() as u32;
            fill(&mut img, x0, y as u32, filled, 5, f.color);
        } else if let Some(h) = f.hint {
            draw_text_line(&mut img, font, h, 11.0, cx, y - 2.0, MISSING);
        }
        if i + 1 < n {
            fill(&mut img, 14, (top + band) as u32, WIDTH - 28, 1, TRACK);
        }
    }
    img
}

/// Cuts the canvas into the images for slots 16-18 (upright, not yet rotated).
pub fn slices(canvas: &RgbImage) -> Vec<(u8, RgbImage)> {
    SLOTS
        .iter()
        .map(|&(id, y0)| {
            let mut slice = RgbImage::from_pixel(WIDTH, SLICE_HEIGHT, BG);
            let h = SLICE_HEIGHT.min(HEIGHT.saturating_sub(y0));
            let part = imageops::crop_imm(canvas, 0, y0, WIDTH, h).to_image();
            imageops::replace(&mut slice, &part, 0, 0);
            (id, slice)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_cover_the_canvas_in_order() {
        let mut canvas = RgbImage::from_pixel(WIDTH, HEIGHT, BG);
        // Mark the first row each slot should show.
        for &(_, y0) in &SLOTS {
            canvas.put_pixel(0, y0, Rgb([255, 0, 0]));
        }
        let s = slices(&canvas);
        assert_eq!(
            s.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [16, 17, 18]
        );
        for (_, img) in &s {
            assert_eq!(img.dimensions(), (WIDTH, SLICE_HEIGHT));
            assert_eq!(img.get_pixel(0, 0), &Rgb([255, 0, 0]));
        }
    }
}
