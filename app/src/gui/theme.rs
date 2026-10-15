//! Look of the settings window: colours, fonts, spacing and a few small
//! building blocks (sections, cards, primary buttons) so every screen shares them.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin,
    RichText, Stroke, TextStyle, Ui, Vec2,
};

// Surfaces, darkest to lightest.
pub const CANVAS: Color32 = Color32::from_rgb(17, 18, 21);
pub const PANEL: Color32 = Color32::from_rgb(26, 27, 31);
pub const FIELD: Color32 = Color32::from_rgb(19, 20, 23);
pub const SURFACE: Color32 = Color32::from_rgb(36, 38, 43);
pub const SURFACE_HI: Color32 = Color32::from_rgb(46, 48, 54);
pub const BORDER: Color32 = Color32::from_rgb(52, 54, 61);

pub const TEXT: Color32 = Color32::from_rgb(230, 231, 234);
/// Secondary text; about 6:1 against `PANEL`, so it stays readable.
pub const TEXT_WEAK: Color32 = Color32::from_rgb(154, 157, 166);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(110, 113, 122);

pub const ACCENT: Color32 = Color32::from_rgb(79, 157, 255);
pub const OK: Color32 = Color32::from_rgb(90, 200, 120);
pub const WARN: Color32 = Color32::from_rgb(240, 190, 80);
pub const ERR: Color32 = Color32::from_rgb(235, 95, 85);

pub const RADIUS: u8 = 6;

/// Family for headings and emphasis (Segoe UI Semibold when available).
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

fn load(fonts: &mut FontDefinitions, name: &str, file: &str) -> bool {
    match std::fs::read(format!(r"C:\Windows\Fonts\{file}")) {
        Ok(bytes) => {
            fonts
                .font_data
                .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
            true
        }
        Err(_) => false,
    }
}

/// Segoe UI (the Windows UI font) first, egui's bundled fonts after it for
/// icons, and Segoe UI Symbol last for symbols neither of them has.
fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let defaults = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut regular = Vec::new();
    if load(&mut fonts, "segoe", "segoeui.ttf") {
        regular.push("segoe".to_string());
    }
    let mut bold = Vec::new();
    if load(&mut fonts, "segoe-sb", "seguisb.ttf") {
        bold.push("segoe-sb".to_string());
    } else {
        bold.extend(regular.iter().cloned());
    }
    let mut tail = defaults;
    if load(&mut fonts, "segoe-symbol", "seguisym.ttf") {
        tail.push("segoe-symbol".to_string());
    }
    regular.extend(tail.iter().cloned());
    bold.extend(tail);
    fonts.families.insert(FontFamily::Proportional, regular);
    fonts.families.insert(semibold(), bold);
    fonts
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        use TextStyle::*;
        style.text_styles = [
            (Heading, FontId::new(18.0, semibold())),
            (Body, FontId::proportional(14.0)),
            (Button, FontId::proportional(14.0)),
            (Small, FontId::proportional(12.0)),
            (Monospace, FontId::monospace(13.0)),
        ]
        .into();

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 6.0);
        s.button_padding = Vec2::new(10.0, 4.0);
        s.interact_size = Vec2::new(40.0, 26.0);
        s.window_margin = Margin::same(18);
        s.menu_margin = Margin::same(6);
        s.combo_width = 160.0;

        let v = &mut style.visuals;
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = FIELD;
        v.faint_bg_color = SURFACE;
        v.code_bg_color = SURFACE;
        v.weak_text_color = Some(TEXT_WEAK);
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = WARN;
        v.error_fg_color = ERR;
        v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.slider_trailing_fill = true;
        v.indent_has_left_vline = false;

        let r = CornerRadius::same(RADIUS);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = PANEL;
        w.noninteractive.weak_bg_fill = PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        for (state, fill, text) in [
            (&mut w.inactive, SURFACE, TEXT),
            (&mut w.hovered, SURFACE_HI, Color32::WHITE),
            (&mut w.active, Color32::from_rgb(56, 59, 66), Color32::WHITE),
            (&mut w.open, SURFACE_HI, TEXT),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.corner_radius = r;
            state.fg_stroke = Stroke::new(1.0, text);
            state.expansion = 0.0;
        }
        // Buttons use `weak_bg_fill`; check boxes, sliders and radio buttons use
        // `bg_fill`, which is darker so their boxes stand out on cards.
        w.inactive.bg_fill = Color32::from_rgb(14, 15, 18);
        w.inactive.bg_stroke = Stroke::NONE;
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(78, 81, 90));
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.open.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.corner_radius = r;
    });
}

/// A small uppercase heading that starts a group of settings.
pub fn section(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.5)
            .family(semibold())
            .extra_letter_spacing(0.8)
            .color(TEXT_FAINT),
    );
    ui.add_space(2.0);
}

/// A slightly raised box that groups related controls.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(Color32::from_rgb(31, 32, 37))
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// The one button on a screen that does the main thing.
pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).family(semibold()).color(Color32::WHITE))
        .fill(Color32::from_rgb(45, 112, 210))
}

/// A button whose action removes something.
pub fn danger(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).family(semibold()).color(Color32::WHITE))
        .fill(Color32::from_rgb(170, 55, 50))
}

/// A frameless icon button; `tip` is required so every icon has a name.
pub fn icon_button(ui: &mut Ui, enabled: bool, icon: &str, tip: &str) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(icon).frame_when_inactive(false))
        .on_hover_text(tip)
        .on_disabled_hover_text(tip)
}

/// Grey helper text that wraps to the available width.
pub fn hint(ui: &mut Ui, text: impl Into<String>) {
    ui.add(egui::Label::new(RichText::new(text.into()).color(TEXT_WEAK).size(12.5)).wrap());
}
