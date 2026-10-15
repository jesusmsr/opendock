//! Inspector for one key: appearance and (possibly nested) action.

use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Color32, ColorImage, ComboBox, Id, Rect, RichText, Sense, Stroke, StrokeKind,
    TextureHandle, TextureOptions, Ui, Vec2,
};

use crate::audio::{self, Endpoint};
use crate::config::{
    Action, Align, AudioTarget, Key, Media, Run, Side, Widget, WidgetKind, MAX_SIDE_WIDGETS,
};
use crate::keys;

use super::theme::{self, ERR, OK, WARN};

/// Read-only data the editor needs from the app.
pub struct EditorCtx<'a> {
    pub pages: &'a [String],
    pub outputs: &'a [Endpoint],
    pub inputs: &'a [Endpoint],
    /// Folder of the config file; icons are stored under `icons/` here.
    pub base_dir: &'a Path,
}

/// What the user asked for besides editing fields.
#[derive(Default)]
pub struct Requests {
    pub test: Option<Action>,
    pub clear: bool,
    pub refresh_audio: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Run,
    Url,
    Hotkey,
    Media,
    Text,
    AudioOutput,
    AudioInput,
    Page,
    Back,
    Home,
    Delay,
    Multi,
}

/// Action kinds in menu order; the menu draws a separator between groups.
const KINDS: [(Kind, &str); 12] = [
    (Kind::Run, "Open program / file"),
    (Kind::Url, "Open website"),
    (Kind::Hotkey, "Hotkey"),
    (Kind::Media, "Media key"),
    (Kind::Text, "Type text"),
    (Kind::AudioOutput, "Audio output device"),
    (Kind::AudioInput, "Microphone device"),
    (Kind::Page, "Go to page"),
    (Kind::Back, "Previous page"),
    (Kind::Home, "Home page"),
    (Kind::Multi, "Multi-step"),
    (Kind::Delay, "Wait"),
];

/// Kinds that start a new group in the menu.
const GROUP_STARTS: [Kind; 4] = [Kind::Hotkey, Kind::AudioOutput, Kind::Page, Kind::Multi];

/// Lists the kinds in `ui` (minus `skip`); returns the one clicked.
fn kind_menu(ui: &mut Ui, current: Option<Kind>, skip: Kind) -> Option<Kind> {
    let mut picked = None;
    for (k, l) in KINDS {
        if k == skip {
            continue;
        }
        if GROUP_STARTS.contains(&k) {
            ui.separator();
        }
        if ui.selectable_label(current == Some(k), l).clicked() && current != Some(k) {
            picked = Some(k);
        }
    }
    picked
}

/// Short human description of an action, e.g. "Hotkey Ctrl+Shift+M".
pub fn describe(a: &Action) -> String {
    let file = |p: &str| {
        Path::new(p)
            .file_name()
            .map_or(p.to_string(), |n| n.to_string_lossy().into_owned())
    };
    let audio = |what: &str, t: &AudioTarget| {
        let list = t.targets();
        match list.as_slice() {
            [one] if one.is_empty() => format!("{what}: not set"),
            [one] => format!("{what}: {one}"),
            many => format!("{what}: cycle {}", many.join(" / ")),
        }
    };
    match a {
        Action::Run(Run::Path(p)) | Action::Run(Run::Full { path: p, .. }) if p.is_empty() => {
            "Open: not set".into()
        }
        Action::Run(Run::Path(p)) | Action::Run(Run::Full { path: p, .. }) => {
            format!("Open {}", file(p))
        }
        Action::Url(u) => format!(
            "Open {}",
            u.trim_start_matches("https://")
                .trim_start_matches("http://")
        ),
        Action::Hotkey(h) if h.is_empty() => "Hotkey: not set".into(),
        Action::Hotkey(h) => {
            let pretty: Vec<String> = h
                .split('+')
                .map(|p| {
                    let mut c = p.chars();
                    c.next()
                        .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
                })
                .collect();
            format!("Hotkey {}", pretty.join("+"))
        }
        Action::Media(m) => MEDIA
            .iter()
            .find(|(x, _)| x == m)
            .map_or("Media key".into(), |(_, l)| l.to_string()),
        Action::Text(_) => "Type text".into(),
        Action::AudioOutput(t) => audio("Output", t),
        Action::AudioInput(t) => audio("Microphone", t),
        Action::Page(p) => format!("Go to page \"{p}\""),
        Action::Back => "Previous page".into(),
        Action::Home => "Home page".into(),
        Action::Delay(ms) => format!("Wait {ms} ms"),
        Action::Multi(steps) => match steps.len() {
            1 => "1 step".into(),
            n => format!("{n} steps"),
        },
    }
}

const MEDIA: [(Media, &str); 7] = [
    (Media::PlayPause, "Play / Pause"),
    (Media::Next, "Next track"),
    (Media::Previous, "Previous track"),
    (Media::Stop, "Stop"),
    (Media::Mute, "Mute"),
    (Media::VolumeUp, "Volume up"),
    (Media::VolumeDown, "Volume down"),
];

fn kind(a: &Action) -> Kind {
    match a {
        Action::Run(_) => Kind::Run,
        Action::Url(_) => Kind::Url,
        Action::Hotkey(_) => Kind::Hotkey,
        Action::Media(_) => Kind::Media,
        Action::Text(_) => Kind::Text,
        Action::AudioOutput(_) => Kind::AudioOutput,
        Action::AudioInput(_) => Kind::AudioInput,
        Action::Page(_) => Kind::Page,
        Action::Back => Kind::Back,
        Action::Home => Kind::Home,
        Action::Delay(_) => Kind::Delay,
        Action::Multi(_) => Kind::Multi,
    }
}

fn label(k: Kind) -> &'static str {
    KINDS.iter().find(|(x, _)| *x == k).map_or("?", |(_, l)| l)
}

fn new_action(k: Kind, ctx: &EditorCtx) -> Action {
    match k {
        Kind::Run => Action::Run(Run::Path(String::new())),
        Kind::Url => Action::Url("https://".into()),
        Kind::Hotkey => Action::Hotkey(String::new()),
        Kind::Media => Action::Media(Media::PlayPause),
        Kind::Text => Action::Text(String::new()),
        Kind::AudioOutput => Action::AudioOutput(AudioTarget::One(String::new())),
        Kind::AudioInput => Action::AudioInput(AudioTarget::One(String::new())),
        Kind::Page => Action::Page(ctx.pages.first().cloned().unwrap_or_default()),
        Kind::Back => Action::Back,
        Kind::Home => Action::Home,
        Kind::Delay => Action::Delay(200),
        Kind::Multi => Action::Multi(Vec::new()),
    }
}

/// The whole inspector for `key`.
pub fn key_ui(ui: &mut Ui, key: &mut Key, ctx: &EditorCtx) -> Requests {
    let mut req = Requests::default();
    // Header: what the key does, with the key-wide commands next to it.
    ui.horizontal(|ui| {
        let title = match &key.action {
            Some(a) => describe(a),
            None if key.icon.is_none() && key.title.is_none() => "Empty key".into(),
            None => "No action".into(),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::icon_button(ui, true, "🗑", "Clear this key (Del)").clicked() {
                req.clear = true;
            }
            let test = ui.add_enabled(key.action.is_some(), egui::Button::new("▶ Test"));
            if test
                .on_hover_text("Run this key's action now")
                .on_disabled_hover_text("Choose an action first")
                .clicked()
            {
                req.test = key.action.clone();
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(RichText::new(title).family(theme::semibold()).size(16.0))
                        .truncate(),
                );
            });
        });
    });
    ui.add_space(12.0);

    theme::section(ui, "Action");
    theme::card(ui, |ui| {
        let id = Id::new(("action", key.pos.row, key.pos.col));
        top_action_ui(ui, &mut key.action, ctx, id, &mut req);
    });
    ui.add_space(14.0);
    theme::section(ui, "Appearance");
    theme::card(ui, |ui| appearance_ui(ui, key, ctx, &mut req));
    req
}

fn appearance_ui(ui: &mut Ui, key: &mut Key, ctx: &EditorCtx, req: &mut Requests) {
    // Normal and active icon side by side, so the two looks are easy to compare.
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("Icon").color(theme::TEXT_WEAK));
            icon_field(ui, &mut key.icon, ctx, req);
        });
        ui.add_space(18.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Active icon  ⓘ").color(theme::TEXT_WEAK))
                .on_hover_text(
                    "Shown while the key is active, e.g. when its audio device is the default. \
                 Without it, active keys get a green border.",
                );
            icon_field(ui, &mut key.icon_active, ctx, req);
        });
    });
    ui.add_space(8.0);
    egui::Grid::new("appearance")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Title");
            let mut title = key.title.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::multiline(&mut title)
                        .desired_rows(2)
                        .desired_width(220.0),
                )
                .changed()
            {
                key.title = (!title.is_empty()).then_some(title);
            }
            ui.end_row();

            ui.label("Title style");
            ui.horizontal(|ui| {
                let mut size = key.title_size.unwrap_or(16);
                if ui
                    .add(egui::DragValue::new(&mut size).range(8..=48).suffix(" px"))
                    .changed()
                {
                    key.title_size = Some(size);
                }
                let align = key.title_align;
                ComboBox::from_id_salt("title_align")
                    .selected_text(match align {
                        None => "Auto",
                        Some(Align::Top) => "Top",
                        Some(Align::Middle) => "Middle",
                        Some(Align::Bottom) => "Bottom",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut key.title_align, None, "Auto");
                        ui.selectable_value(&mut key.title_align, Some(Align::Top), "Top");
                        ui.selectable_value(&mut key.title_align, Some(Align::Middle), "Middle");
                        ui.selectable_value(&mut key.title_align, Some(Align::Bottom), "Bottom");
                    });
            });
            ui.end_row();

            ui.label("Background");
            ui.horizontal(|ui| {
                let mut rgb = key
                    .color
                    .as_deref()
                    .and_then(crate::render::parse_color)
                    .map_or([0, 0, 0], |p| p.0);
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    key.color = Some(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
                }
                if key.color.is_some() {
                    if ui
                        .small_button("Reset")
                        .on_hover_text("Back to black")
                        .clicked()
                    {
                        key.color = None;
                    }
                } else {
                    ui.weak("Black");
                }
            });
            ui.end_row();
        });
}

/// Copies an image into `<base>/icons` (unless it is already under `base`)
/// and returns the path to store in the config.
fn import_icon(src: &Path, base: &Path) -> Result<PathBuf, String> {
    if let Ok(rel) = src.strip_prefix(base) {
        return Ok(rel.to_path_buf());
    }
    let dir = base.join("icons");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("icon");
    let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("png");
    let bytes = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
    for n in 0.. {
        let name = if n == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        let dest = dir.join(&name);
        match std::fs::read(&dest) {
            Ok(existing) if existing == bytes => return Ok(PathBuf::from("icons").join(name)),
            Ok(_) => continue,
            Err(_) => {
                std::fs::write(&dest, &bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
                return Ok(PathBuf::from("icons").join(name));
            }
        }
    }
    unreachable!("the loop only ends by returning")
}

/// Texture of an icon file for the editor, cached per file version (a file
/// that fails to load is cached as `None` so it is not retried every frame).
fn thumbnail(ui: &Ui, path: &Path) -> Option<TextureHandle> {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let id = Id::new(("thumb", path, mtime));
    if let Some(cached) = ui.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return cached;
    }
    let tex = image::open(path).ok().map(|img| {
        let img = img.thumbnail(112, 112).to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let image = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        ui.ctx().load_texture(
            format!("thumb-{}", path.display()),
            image,
            TextureOptions::LINEAR,
        )
    });
    ui.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// A clickable thumbnail of the icon (click to choose another) and a remove button.
fn icon_field(ui: &mut Ui, icon: &mut Option<PathBuf>, ctx: &EditorCtx, req: &mut Requests) {
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::click());
        let painter = ui.painter();
        painter.rect_filled(rect, 8.0, theme::FIELD);
        let path = icon.as_ref().map(|p| ctx.base_dir.join(p));
        match path.as_deref().map(|p| thumbnail(ui, p)) {
            Some(Some(tex)) => {
                // Fit the image inside the box, keeping its aspect ratio.
                let [w, h] = tex.size().map(|v| v as f32);
                let inner = rect.shrink(4.0);
                let scale = (inner.width() / w).min(inner.height() / h);
                let img_rect = Rect::from_center_size(inner.center(), Vec2::new(w, h) * scale);
                painter.add(
                    egui::epaint::RectShape::filled(img_rect, 6.0, Color32::WHITE).with_texture(
                        tex.id(),
                        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    ),
                );
            }
            Some(None) => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "⚠",
                    egui::FontId::proportional(20.0),
                    ERR,
                );
            }
            None => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "+",
                    egui::FontId::proportional(24.0),
                    theme::TEXT_FAINT,
                );
            }
        }
        let stroke = if resp.hovered() {
            Stroke::new(1.5, Color32::from_gray(150))
        } else {
            Stroke::new(1.0, theme::BORDER)
        };
        painter.rect_stroke(rect, 8.0, stroke, StrokeKind::Inside);
        let hover = match &path {
            Some(p) if p.exists() => format!(
                "{}
Click to choose another image",
                p.display()
            ),
            Some(p) => format!(
                "Missing file: {}
Click to choose another image",
                p.display()
            ),
            None => "Click to choose an image".to_string(),
        };
        if resp
            .on_hover_text(hover)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            let picked = rfd::FileDialog::new()
                .set_title("Choose an icon")
                .add_filter(
                    "Images",
                    &["png", "jpg", "jpeg", "bmp", "gif", "ico", "webp"],
                )
                .set_directory(ctx.base_dir.join("icons"))
                .pick_file();
            if let Some(src) = picked {
                match import_icon(&src, ctx.base_dir) {
                    Ok(p) => *icon = Some(p),
                    Err(e) => req.error = Some(e),
                }
            }
        }
        if icon.is_some() && theme::icon_button(ui, true, "🗙", "Remove the image").clicked() {
            *icon = None;
        }
    });
}

fn top_action_ui(
    ui: &mut Ui,
    action: &mut Option<Action>,
    ctx: &EditorCtx,
    id: Id,
    req: &mut Requests,
) {
    let current = action.as_ref().map(kind);
    ComboBox::from_id_salt(id.with("kind"))
        .width(ui.available_width().min(260.0))
        .height(600.0)
        .selected_text(current.map_or("Nothing", label))
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "Nothing").clicked() {
                *action = None;
            }
            // Wait is only meaningful inside a multi-step action.
            if let Some(k) = kind_menu(ui, current, Kind::Delay) {
                *action = Some(new_action(k, ctx));
            }
        });
    if let Some(a) = action {
        ui.add_space(8.0);
        action_ui(ui, a, ctx, id, req);
    }
}

fn kind_combo(ui: &mut Ui, a: &mut Action, ctx: &EditorCtx, id: Id) {
    let current = kind(a);
    ComboBox::from_id_salt(id.with("kind"))
        .width(180.0)
        .selected_text(label(current))
        .height(600.0)
        .show_ui(ui, |ui| {
            // Keep steps flat: no multi-step inside a multi-step.
            if let Some(k) = kind_menu(ui, Some(current), Kind::Multi) {
                *a = new_action(k, ctx);
            }
        });
}

fn action_ui(ui: &mut Ui, a: &mut Action, ctx: &EditorCtx, id: Id, req: &mut Requests) {
    match a {
        Action::Run(run) => run_ui(ui, run),
        Action::Url(url) => {
            ui.add(
                egui::TextEdit::singleline(url)
                    .hint_text("https://…")
                    .desired_width(280.0),
            );
        }
        Action::Hotkey(s) => hotkey_ui(ui, s, id),
        Action::Media(m) => {
            ComboBox::from_id_salt(id.with("media"))
                .selected_text(MEDIA.iter().find(|(x, _)| x == m).map_or("?", |(_, l)| l))
                .show_ui(ui, |ui| {
                    for (v, l) in MEDIA {
                        ui.selectable_value(m, v, l);
                    }
                });
        }
        Action::Text(t) => {
            ui.add(
                egui::TextEdit::multiline(t)
                    .hint_text("Text to type")
                    .desired_width(280.0),
            );
        }
        Action::AudioOutput(t) => audio_ui(ui, t, ctx.outputs, req),
        Action::AudioInput(t) => audio_ui(ui, t, ctx.inputs, req),
        Action::Page(p) => {
            ComboBox::from_id_salt(id.with("page"))
                .selected_text(p.as_str())
                .show_ui(ui, |ui| {
                    for name in ctx.pages {
                        ui.selectable_value(p, name.clone(), name);
                    }
                });
        }
        Action::Back => {
            ui.weak("Returns to the page you came from.");
        }
        Action::Home => {
            ui.weak("Jumps to the home page.");
        }
        Action::Delay(ms) => {
            ui.add(
                egui::DragValue::new(ms)
                    .range(0..=60_000)
                    .speed(10)
                    .suffix(" ms"),
            );
        }
        Action::Multi(steps) => multi_ui(ui, steps, ctx, id, req),
    }
}

fn multi_ui(ui: &mut Ui, steps: &mut Vec<Action>, ctx: &EditorCtx, id: Id, req: &mut Requests) {
    let mut swap = None;
    let mut remove = None;
    let n = steps.len();
    for (i, step) in steps.iter_mut().enumerate() {
        let sid = id.with(("step", i));
        step_frame(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}", i + 1)).color(theme::TEXT_FAINT));
                kind_combo(ui, step, ctx, sid);
                match reorder_buttons(ui, i, n, "step") {
                    Some(Reorder::Swap(a, b)) => swap = Some((a, b)),
                    Some(Reorder::Remove(a)) => remove = Some(a),
                    None => {}
                }
            });
            action_ui(ui, step, ctx, sid, req);
        });
    }
    if let Some((a, b)) = swap {
        steps.swap(a, b);
    }
    if let Some(i) = remove {
        steps.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button("➕ Add step").clicked() {
            steps.push(Action::Hotkey(String::new()));
        }
        if ui.button("➕ Add wait").clicked() {
            steps.push(Action::Delay(200));
        }
    });
}

enum Reorder {
    Swap(usize, usize),
    Remove(usize),
}

/// Up / down / remove buttons for item `i` of `n` in a list.
fn reorder_buttons(ui: &mut Ui, i: usize, n: usize, what: &str) -> Option<Reorder> {
    let mut out = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if theme::icon_button(ui, true, "🗙", &format!("Remove this {what}")).clicked() {
            out = Some(Reorder::Remove(i));
        }
        if theme::icon_button(ui, i + 1 < n, "⬇", "Move down").clicked() {
            out = Some(Reorder::Swap(i, i + 1));
        }
        if theme::icon_button(ui, i > 0, "⬆", "Move up").clicked() {
            out = Some(Reorder::Swap(i, i - 1));
        }
    });
    out
}

/// The box around one step of a multi-step action or one widget.
fn step_frame(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::FIELD)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(6)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    ui.add_space(6.0);
}

fn run_ui(ui: &mut Ui, run: &mut Run) {
    let (mut path, mut args, mut cwd, mut hidden) = match run {
        Run::Path(p) => (p.clone(), String::new(), String::new(), false),
        Run::Full {
            path,
            args,
            cwd,
            hidden,
        } => (
            path.clone(),
            args.clone().unwrap_or_default(),
            cwd.clone().unwrap_or_default(),
            *hidden,
        ),
    };
    egui::Grid::new(ui.id().with("run"))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Program");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut path)
                        .hint_text("C:/path/to/app.exe")
                        .desired_width(200.0),
                );
                if ui.button("Browse…").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_title("Choose a program or file")
                        .pick_file()
                    {
                        path = p.to_string_lossy().replace('\\', "/");
                    }
                }
            });
            ui.end_row();
            ui.label("Arguments");
            ui.add(
                egui::TextEdit::singleline(&mut args)
                    .hint_text("optional")
                    .desired_width(200.0),
            );
            ui.end_row();
            ui.label("Start in");
            ui.add(
                egui::TextEdit::singleline(&mut cwd)
                    .hint_text("optional folder")
                    .desired_width(200.0),
            );
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut hidden, "Hide window (for scripts)");
            ui.end_row();
        });
    *run = if args.is_empty() && cwd.is_empty() && !hidden {
        Run::Path(path)
    } else {
        Run::Full {
            path,
            args: (!args.is_empty()).then_some(args),
            cwd: (!cwd.is_empty()).then_some(cwd),
            hidden,
        }
    };
}

/// Splits `"ctrl+shift+k"` into modifier flags and the remaining key text,
/// without requiring the key to be valid (so it can be typed gradually).
fn split_hotkey(s: &str) -> ([bool; 4], String) {
    let mut mods = [false; 4];
    let mut rest = Vec::new();
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods[0] = true,
            "shift" => mods[1] = true,
            "alt" => mods[2] = true,
            "win" | "meta" | "super" => mods[3] = true,
            _ => rest.push(part),
        }
    }
    (mods, rest.join("+"))
}

fn join_hotkey(mods: [bool; 4], key: &str) -> String {
    let mut parts: Vec<&str> = ["ctrl", "shift", "alt", "win"]
        .iter()
        .zip(mods)
        .filter_map(|(n, on)| on.then_some(*n))
        .collect();
    if !key.is_empty() {
        parts.push(key);
    }
    parts.join("+")
}

/// Our key name for a key pressed in the window, if we support it.
fn egui_key_name(k: egui::Key) -> Option<String> {
    let name = match k.name().to_ascii_lowercase().as_str() {
        "escape" => "esc".to_string(),
        "equals" => "plus".to_string(),
        "openbracket" => "lbracket".to_string(),
        "closebracket" => "rbracket".to_string(),
        "backtick" => "backquote".to_string(),
        other => other.to_string(),
    };
    keys::vk_from_name(&name).map(|_| name)
}

fn hotkey_ui(ui: &mut Ui, s: &mut String, id: Id) {
    let (mut mods, mut key) = split_hotkey(s);
    let rec_id = id.with("recording");
    let mut recording = ui.data(|d| d.get_temp::<bool>(rec_id).unwrap_or(false));

    ui.horizontal(|ui| {
        for (i, n) in ["Ctrl", "Shift", "Alt", "Win"].iter().enumerate() {
            ui.checkbox(&mut mods[i], *n);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Key");
        ui.add(
            egui::TextEdit::singleline(&mut key)
                .hint_text("e.g. d, f5, esc")
                .desired_width(90.0),
        );
        let text = if recording {
            "⏺ Press a key…"
        } else {
            "⏺ Record"
        };
        let btn = ui.add(egui::Button::new(text).selected(recording));
        if btn
            .on_hover_text("Press the combination on your keyboard. Windows-key combos: tick Win.")
            .clicked()
        {
            recording = !recording;
        }
    });
    if recording {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => egui_key_name(*key).map(|n| (n, *modifiers)),
                _ => None,
            })
        });
        if let Some((name, m)) = pressed {
            mods = [m.ctrl, m.shift, m.alt, mods[3]];
            key = name;
            recording = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(rec_id, recording));

    *s = join_hotkey(mods, &key);
    if let Err(e) = keys::parse(s) {
        ui.colored_label(ERR, e);
    }
}

fn audio_ui(ui: &mut Ui, target: &mut AudioTarget, devices: &[Endpoint], req: &mut Requests) {
    let mut list = target.targets();
    let mut remove = None;
    for (i, entry) in list.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(entry)
                    .hint_text("device name or part of it")
                    .desired_width(ui.available_width() - 70.0),
            );
            ui.menu_button("⏷", |ui| {
                ui.set_min_width(220.0);
                for d in devices {
                    if ui.button(&d.name).clicked() {
                        *entry = d.name.clone();
                        ui.close();
                    }
                }
                if devices.is_empty() {
                    ui.weak("No devices found");
                }
            });
            if i > 0 && theme::icon_button(ui, true, "🗙", "Remove from the cycle").clicked() {
                remove = Some(i);
            }
        });
        match audio::resolve(devices, entry) {
            Some(d) if !entry.is_empty() => {
                ui.colored_label(OK, format!("✔ {}", d.name));
            }
            _ => {
                ui.colored_label(ERR, "No matching device connected");
            }
        }
    }
    if let Some(i) = remove {
        list.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button("➕ Add device to cycle").clicked() {
            list.push(String::new());
        }
        if ui
            .button("⟳")
            .on_hover_text("Refresh the device list")
            .clicked()
        {
            req.refresh_audio = true;
        }
    });
    if list.len() > 1 {
        ui.weak("Each press switches to the next device in the list.");
    }
    *target = if list.len() == 1 {
        AudioTarget::One(list.remove(0))
    } else {
        AudioTarget::Cycle(list)
    };
}

const WIDGETS: [(WidgetKind, &str); 9] = [
    (WidgetKind::CpuTemp, "CPU temperature"),
    (WidgetKind::CpuUsage, "CPU usage"),
    (WidgetKind::GpuTemp, "GPU temperature"),
    (WidgetKind::GpuUsage, "GPU usage"),
    (WidgetKind::GpuMemory, "GPU memory (VRAM)"),
    (WidgetKind::Ram, "RAM"),
    (WidgetKind::Clock, "Clock"),
    (WidgetKind::Date, "Date"),
    (WidgetKind::Text, "Text"),
];

fn widget_label(k: WidgetKind) -> &'static str {
    WIDGETS
        .iter()
        .find(|(x, _)| *x == k)
        .map_or("?", |(_, l)| l)
}

/// Editor for the small screen: an ordered list of up to four widgets.
pub fn side_ui(ui: &mut Ui, side: &mut Side, cpu_temp_available: bool) {
    ui.label(
        RichText::new("Small screen")
            .family(theme::semibold())
            .size(16.0),
    );
    theme::hint(
        ui,
        "Up to four widgets, stacked top to bottom. They share the screen equally.",
    );
    ui.add_space(12.0);
    theme::section(ui, "Widgets");

    let n = side.widgets.len();
    let mut swap = None;
    let mut remove = None;
    for (i, w) in side.widgets.iter_mut().enumerate() {
        step_frame(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}", i + 1)).color(theme::TEXT_FAINT));
                ComboBox::from_id_salt(("widget", i))
                    .width(170.0)
                    .selected_text(widget_label(w.kind))
                    .show_ui(ui, |ui| {
                        for (k, l) in WIDGETS {
                            if ui.selectable_label(w.kind == k, l).clicked() {
                                w.kind = k;
                            }
                        }
                    });
                match reorder_buttons(ui, i, n, "widget") {
                    Some(Reorder::Swap(a, b)) => swap = Some((a, b)),
                    Some(Reorder::Remove(a)) => remove = Some(a),
                    None => {}
                }
            });
            egui::Grid::new(("widget_fields", i))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    if w.kind == WidgetKind::Text {
                        ui.label("Text");
                        let mut text = w.text.clone().unwrap_or_default();
                        if ui
                            .add(
                                egui::TextEdit::multiline(&mut text)
                                    .desired_rows(2)
                                    .desired_width(180.0),
                            )
                            .changed()
                        {
                            w.text = (!text.is_empty()).then_some(text);
                        }
                        ui.end_row();
                    }
                    ui.label("Caption");
                    let mut label = w.label.clone().unwrap_or_default();
                    let hint = if w.kind == WidgetKind::Text {
                        "none"
                    } else {
                        "default"
                    };
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut label)
                                .hint_text(hint)
                                .desired_width(180.0),
                        )
                        .changed()
                    {
                        w.label = (!label.is_empty()).then_some(label);
                    }
                    ui.end_row();
                });
            if w.kind == WidgetKind::CpuTemp && !cpu_temp_available {
                ui.colored_label(
                    WARN,
                    "CPU temperature needs LibreHardwareMonitor: set it up in ⚙ Settings > Sensors.",
                );
            }
        });
    }
    if let Some((a, b)) = swap {
        side.widgets.swap(a, b);
    }
    if let Some(i) = remove {
        side.widgets.remove(i);
    }
    ui.add_space(4.0);
    let can_add = side.widgets.len() < MAX_SIDE_WIDGETS;
    if ui
        .add_enabled(can_add, egui::Button::new("➕ Add widget"))
        .clicked()
    {
        side.widgets.push(Widget::new(WidgetKind::Clock));
    }
    if side.widgets.is_empty() {
        theme::hint(
            ui,
            "No widgets yet, so the small screen stays dark. Add a clock or a sensor.",
        );
    }
}
