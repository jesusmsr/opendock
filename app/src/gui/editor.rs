//! Inspector for one key: appearance and (possibly nested) action.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, ComboBox, Id, RichText, Ui};

use crate::audio::{self, Endpoint};
use crate::config::{Action, Align, AudioTarget, Key, Media, Run};
use crate::keys;

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
    (Kind::Delay, "Wait"),
    (Kind::Multi, "Multi-step"),
];

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
    ui.horizontal(|ui| {
        ui.heading(format!("Key {}", key.pos));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("🗑 Clear")
                .on_hover_text("Remove everything from this key")
                .clicked()
            {
                req.clear = true;
            }
            let test = ui.add_enabled(key.action.is_some(), egui::Button::new("▶ Test"));
            if test.on_hover_text("Run this key's action now").clicked() {
                req.test = key.action.clone();
            }
        });
    });
    ui.separator();

    egui::CollapsingHeader::new(RichText::new("Appearance").strong())
        .default_open(true)
        .show(ui, |ui| appearance_ui(ui, key, ctx, &mut req));
    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("Action").strong())
        .default_open(true)
        .show(ui, |ui| {
            let id = Id::new(("action", key.pos.row, key.pos.col));
            top_action_ui(ui, &mut key.action, ctx, id, &mut req);
        });
    req
}

fn appearance_ui(ui: &mut Ui, key: &mut Key, ctx: &EditorCtx, req: &mut Requests) {
    egui::Grid::new("appearance")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("Icon");
            icon_field(ui, &mut key.icon, ctx, req);
            ui.end_row();

            ui.label("Active icon").on_hover_text(
                "Shown while the key is active, e.g. when its audio device is the default. \
             Without it, active keys get a green border.",
            );
            icon_field(ui, &mut key.icon_active, ctx, req);
            ui.end_row();

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
                let mut on = key.color.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    key.color = on.then(|| "#202020".to_string());
                }
                if let Some(c) = &mut key.color {
                    let mut rgb = crate::render::parse_color(c).map_or([32, 32, 32], |p| p.0);
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        *c = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                    }
                } else {
                    ui.weak("black");
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

fn icon_field(ui: &mut Ui, icon: &mut Option<PathBuf>, ctx: &EditorCtx, req: &mut Requests) {
    ui.horizontal(|ui| {
        let text = icon.as_ref().map_or("none".to_string(), |p| {
            p.file_name().map_or(p.display().to_string(), |f| {
                f.to_string_lossy().into_owned()
            })
        });
        let hover = icon
            .as_ref()
            .map_or(String::new(), |p| p.display().to_string());
        ui.allocate_ui(egui::vec2(150.0, 18.0), |ui| {
            ui.set_width(150.0);
            ui.add(egui::Label::new(RichText::new(text).monospace()).truncate())
                .on_hover_text(hover);
        });
        if ui.button("Browse…").clicked() {
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
        if icon.is_some() && ui.small_button("🗙").on_hover_text("Remove").clicked() {
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
        .width(220.0)
        .selected_text(current.map_or("Nothing", label))
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "Nothing").clicked() {
                *action = None;
            }
            for (k, l) in KINDS {
                if k == Kind::Delay {
                    continue; // only meaningful inside a multi-step action
                }
                if ui.selectable_label(current == Some(k), l).clicked() && current != Some(k) {
                    *action = Some(new_action(k, ctx));
                }
            }
        });
    if let Some(a) = action {
        ui.add_space(4.0);
        action_ui(ui, a, ctx, id, req);
    }
}

fn kind_combo(ui: &mut Ui, a: &mut Action, ctx: &EditorCtx, id: Id) {
    let current = kind(a);
    ComboBox::from_id_salt(id.with("kind"))
        .width(180.0)
        .selected_text(label(current))
        .show_ui(ui, |ui| {
            for (k, l) in KINDS {
                if k == Kind::Multi {
                    continue; // keep steps flat
                }
                if ui.selectable_label(current == k, l).clicked() && current != k {
                    *a = new_action(k, ctx);
                }
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
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}.", i + 1)).strong());
                kind_combo(ui, step, ctx, sid);
                if ui
                    .add_enabled(i > 0, egui::Button::new("⬆").small())
                    .clicked()
                {
                    swap = Some((i, i - 1));
                }
                if ui
                    .add_enabled(i + 1 < n, egui::Button::new("⬇").small())
                    .clicked()
                {
                    swap = Some((i, i + 1));
                }
                if ui.small_button("🗙").clicked() {
                    remove = Some(i);
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
        ui.colored_label(Color32::from_rgb(230, 90, 80), e);
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
                    .desired_width(200.0),
            );
            ui.menu_button("⏷", |ui| {
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
            if i > 0 && ui.small_button("🗙").clicked() {
                remove = Some(i);
            }
        });
        match audio::resolve(devices, entry) {
            Some(d) if !entry.is_empty() => {
                ui.colored_label(Color32::from_rgb(90, 200, 120), format!("✔ {}", d.name));
            }
            _ => {
                ui.colored_label(
                    Color32::from_rgb(230, 90, 80),
                    "No matching device connected",
                );
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
