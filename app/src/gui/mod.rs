//! Settings window. It edits `config.toml`; the running engine notices a
//! saved file by itself, so the deck updates without restarting anything.

mod editor;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use ajazz_hid::akp153e::{COLS, IMAGE_SIZE, ROWS};
use eframe::egui::{
    self, Color32, ColorImage, ComboBox, Id, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    TextureHandle, TextureOptions, Vec2,
};

use crate::audio::{self, Endpoint, Flow};
use crate::config::{Action, Config, Key, Pos, Run};
use crate::render::Renderer;
use crate::sensors::{Readings, Sensors};
use crate::side;
use crate::{actions, autostart, icon, import, instance, lhm};
use editor::EditorCtx;

const ACCENT: Color32 = Color32::from_rgb(80, 160, 255);
const OK: Color32 = Color32::from_rgb(90, 200, 120);
const ERR: Color32 = Color32::from_rgb(230, 90, 80);
const FULL_UV: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));

fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 5.0, color);
}

pub fn run(config_path: PathBuf) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(instance::SETTINGS_TITLE)
            .with_icon(egui::IconData {
                rgba: icon::rgba(),
                width: icon::SIZE,
                height: icon::SIZE,
            })
            .with_inner_size([1040.0, 660.0])
            .with_min_inner_size([800.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "opendock",
        options,
        Box::new(|_cc| Ok(Box::new(SettingsApp::new(config_path)))),
    )
}

/// Messages from the background thread that checks, installs or removes LHM.
enum LhmEvent {
    Status(lhm::Status),
    Progress(String),
    Done(Result<(), String>),
}

enum Dialog {
    Settings,
    NewPage(String),
    RenamePage(String),
    DeletePage,
    Import,
    ImportNotes(Vec<String>),
    Unsaved,
}

struct Status {
    text: String,
    error: bool,
    at: Instant,
}

struct SettingsApp {
    path: PathBuf,
    base_dir: PathBuf,
    cfg: Config,
    /// `to_toml()` of what is on disk, to detect unsaved changes.
    saved_toml: String,
    page: String,
    selected: Option<Pos>,
    /// The small screen is selected in the inspector (instead of a key).
    side_selected: bool,
    sensors: Sensors,
    readings: Readings,
    /// Preview texture of the small screen and what it was drawn from.
    side_tex: Option<(u64, TextureHandle)>,
    /// LibreHardwareMonitor status (None while it is being checked).
    lhm_status: Option<lhm::Status>,
    lhm_rx: Option<Receiver<LhmEvent>>,
    lhm_progress: Option<String>,
    renderer: Renderer,
    textures: HashMap<u64, TextureHandle>,
    /// Font the renderer was built with; rebuilt when the config's font changes.
    renderer_font: Option<PathBuf>,
    drag_from: Option<Pos>,
    outputs: Vec<Endpoint>,
    inputs: Vec<Endpoint>,
    status: Option<Status>,
    dialog: Option<Dialog>,
    engine_running: bool,
    start_with_windows: bool,
    engine_checked: Instant,
    test_tx: Sender<Result<(), String>>,
    test_rx: Receiver<Result<(), String>>,
    allow_close: bool,
}

fn is_blank(k: &Key) -> bool {
    k.icon.is_none()
        && k.icon_active.is_none()
        && k.title.is_none()
        && k.color.is_none()
        && k.action.is_none()
}

/// Pages whose keys link to `page`.
fn pages_linking_to(cfg: &Config, page: &str) -> Vec<String> {
    fn links(a: &Action, page: &str) -> bool {
        match a {
            Action::Page(p) => p == page,
            Action::Multi(list) => list.iter().any(|a| links(a, page)),
            _ => false,
        }
    }
    let mut out: Vec<String> = cfg
        .pages
        .iter()
        .filter(|(name, keys)| {
            name.as_str() != page
                && keys
                    .iter()
                    .any(|k| k.action.as_ref().is_some_and(|a| links(a, page)))
        })
        .map(|(name, _)| name.clone())
        .collect();
    out.sort();
    out
}

impl SettingsApp {
    fn new(path: PathBuf) -> Self {
        let base_dir = path.parent().map(PathBuf::from).unwrap_or_default();
        let mut status = None;
        let (cfg, saved_toml) = if path.exists() {
            match Config::load(&path) {
                Ok(cfg) => {
                    let t = cfg.to_toml();
                    (cfg, t)
                }
                Err(e) => {
                    status = Some(Status {
                        text: format!("Could not read the config ({e}). Saving will replace it."),
                        error: true,
                        at: Instant::now(),
                    });
                    (Config::default(), String::new())
                }
            }
        } else {
            let cfg = Config::default();
            let t = cfg.to_toml();
            (cfg, t)
        };
        let (test_tx, test_rx) = channel();
        let mut app = SettingsApp {
            renderer: Renderer::new(cfg.font.as_deref(), &base_dir),
            page: cfg.home.clone(),
            path,
            base_dir,
            cfg,
            saved_toml,
            selected: None,
            side_selected: false,
            sensors: Sensors::new(Duration::from_secs(1)),
            readings: Readings::default(),
            side_tex: None,
            lhm_status: None,
            lhm_rx: None,
            lhm_progress: None,
            textures: HashMap::new(),
            renderer_font: None,
            drag_from: None,
            outputs: Vec::new(),
            inputs: Vec::new(),
            status,
            dialog: None,
            engine_running: instance::engine_running(),
            start_with_windows: autostart::is_enabled(),
            engine_checked: Instant::now(),
            test_tx,
            test_rx,
            allow_close: false,
        };
        app.refresh_audio();
        app
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status {
            text: text.into(),
            error,
            at: Instant::now(),
        });
    }

    fn refresh_audio(&mut self) {
        let _com = audio::ComGuard::new();
        self.outputs = audio::list(Flow::Output).unwrap_or_default();
        self.inputs = audio::list(Flow::Input).unwrap_or_default();
    }

    fn dirty(&self) -> bool {
        self.cfg.to_toml() != self.saved_toml
    }

    fn save(&mut self) -> bool {
        match self.cfg.save(&self.path) {
            Ok(()) => {
                self.saved_toml = self.cfg.to_toml();
                let msg = if self.engine_running {
                    "Saved. The deck is updated."
                } else {
                    "Saved."
                };
                self.set_status(msg, false);
                true
            }
            Err(e) => {
                self.set_status(format!("Not saved: {e}"), true);
                false
            }
        }
    }

    fn sorted_pages(&self) -> Vec<String> {
        let mut pages: Vec<String> = self.cfg.pages.keys().cloned().collect();
        pages.sort_by_key(|p| (p != &self.cfg.home, p.to_lowercase()));
        pages
    }

    fn key_at(&self, pos: Pos) -> Option<&Key> {
        self.cfg
            .pages
            .get(&self.page)?
            .iter()
            .find(|k| k.pos == pos)
    }

    fn texture(&mut self, ctx: &egui::Context, key: &Key) -> TextureHandle {
        let id = self.renderer.look_id(key, false);
        if self.textures.len() > 256 {
            self.textures.clear();
        }
        let renderer = &self.renderer;
        self.textures
            .entry(id)
            .or_insert_with(|| {
                let img = renderer.draw(key, false);
                let size = [img.width() as usize, img.height() as usize];
                ctx.load_texture(
                    format!("key-{id}"),
                    ColorImage::from_rgb(size, img.as_raw()),
                    TextureOptions::LINEAR,
                )
            })
            .clone()
    }

    /// The Sensors section of the Settings dialog.
    fn sensors_settings(&mut self, ui: &mut egui::Ui) {
        ui.weak("GPU, CPU usage and RAM are read by opendock directly.");
        let cpu = self.readings.cpu_temp;
        ui.horizontal(|ui| {
            status_dot(ui, if cpu.is_some() { OK } else { Color32::GRAY });
            ui.label(match cpu {
                Some(t) => format!("CPU temperature: {t:.0}°"),
                None => "CPU temperature: not available".to_string(),
            });
        });
        if let Some(p) = &self.lhm_progress {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(p.as_str());
            });
            return;
        }
        let Some(st) = self.lhm_status.clone() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Checking LibreHardwareMonitor…");
            });
            return;
        };
        if st.installed {
            let mut parts = vec!["LibreHardwareMonitor installed"];
            parts.push(if st.autostart {
                "starts at sign-in"
            } else {
                "start-up task missing"
            });
            parts.push(if st.running { "running" } else { "not running" });
            ui.label(parts.join(" · "));
            if st.running && !st.cpu_temp {
                ui.colored_label(
                    Color32::from_rgb(240, 190, 80),
                    "It runs, but reports no CPU temperature yet.",
                );
            }
            ui.horizontal(|ui| {
                if !st.autostart
                    && ui
                        .button("Repair")
                        .on_hover_text("Reinstall and re-register the start-up task")
                        .clicked()
                {
                    self.run_lhm_job(true);
                }
                if ui.button("Uninstall").clicked() {
                    self.run_lhm_job(false);
                }
            });
        } else if st.running {
            ui.label("A LibreHardwareMonitor you run yourself is providing sensors.");
        } else {
            ui.label("CPU temperature needs LibreHardwareMonitor (free, open source).");
            if ui.button("Install LibreHardwareMonitor").clicked() {
                self.run_lhm_job(true);
            }
            ui.weak(
                "Downloads LHM and the PawnIO driver it uses from their official GitHub releases, and starts LHM minimized at sign-in. Windows asks for administrator permission once.",
            );
        }
    }

    /// Texture of the small screen as the deck would show it right now.
    fn side_texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        use std::hash::{Hash, Hasher};
        let now = side::Now::local();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            &self.cfg.side,
            now,
            format!("{:?}", self.readings),
            &self.renderer_font,
        )
            .hash(&mut h);
        let id = h.finish();
        if let Some((tid, tex)) = &self.side_tex {
            if *tid == id {
                return tex.clone();
            }
        }
        let img = side::draw(self.renderer.font(), &self.cfg.side, &self.readings, now);
        let size = [img.width() as usize, img.height() as usize];
        let tex = ctx.load_texture(
            "side",
            ColorImage::from_rgb(size, img.as_raw()),
            TextureOptions::LINEAR,
        );
        self.side_tex = Some((id, tex.clone()));
        tex
    }

    fn move_key(&mut self, from: Pos, to: Pos) {
        if from == to {
            return;
        }
        if let Some(keys) = self.cfg.pages.get_mut(&self.page) {
            for k in keys.iter_mut() {
                if k.pos == from {
                    k.pos = to;
                } else if k.pos == to {
                    k.pos = from;
                }
            }
        }
        self.selected = Some(to);
    }

    fn start_engine(&mut self) {
        if (!self.path.exists() || self.dirty()) && !self.save() {
            return;
        }
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => return self.set_status(format!("Cannot start the engine: {e}"), true),
        };
        match std::process::Command::new(exe)
            .arg("--config")
            .arg(&self.path)
            .arg("run")
            .spawn()
        {
            Ok(_) => self.set_status("Engine started.", false),
            Err(e) => self.set_status(format!("Cannot start the engine: {e}"), true),
        }
        // Re-check soon; the engine needs a moment to take its lock.
        self.engine_checked = Instant::now() - Duration::from_millis(700);
    }

    /// Checks LibreHardwareMonitor's status in the background.
    fn check_lhm(&mut self) {
        if self.lhm_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.lhm_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(LhmEvent::Status(lhm::status()));
        });
    }

    /// Installs (or removes) LibreHardwareMonitor in the background.
    fn run_lhm_job(&mut self, install: bool) {
        let (tx, rx) = channel();
        self.lhm_rx = Some(rx);
        self.lhm_progress = Some(
            if install {
                "Starting…"
            } else {
                "Removing…"
            }
            .into(),
        );
        std::thread::spawn(move || {
            let progress = |m: &str| {
                let _ = tx.send(LhmEvent::Progress(m.to_string()));
            };
            let result = if install {
                lhm::install(&progress)
            } else {
                lhm::uninstall(&progress)
            };
            let _ = tx.send(LhmEvent::Done(result));
            let _ = tx.send(LhmEvent::Status(lhm::status()));
        });
    }

    /// Background bookkeeping done every frame.
    fn poll(&mut self, ctx: &egui::Context) {
        let mut finished = false;
        if let Some(rx) = &self.lhm_rx {
            let mut events = Vec::new();
            while let Ok(e) = rx.try_recv() {
                events.push(e);
            }
            for e in events {
                match e {
                    LhmEvent::Status(st) => {
                        self.lhm_status = Some(st);
                        finished = true;
                    }
                    LhmEvent::Progress(m) => self.lhm_progress = Some(m),
                    LhmEvent::Done(Ok(())) => {
                        self.lhm_progress = None;
                        self.set_status("LibreHardwareMonitor updated.", false);
                    }
                    LhmEvent::Done(Err(e)) => {
                        self.lhm_progress = None;
                        self.set_status(e, true);
                    }
                }
            }
        }
        if finished && self.lhm_progress.is_none() {
            self.lhm_rx = None;
        }
        self.readings = self.sensors.read().clone();
        if self.cfg.font != self.renderer_font {
            self.renderer = Renderer::new(self.cfg.font.as_deref(), &self.base_dir);
            self.renderer_font = self.cfg.font.clone();
            self.textures.clear();
        }
        if self.engine_checked.elapsed() >= Duration::from_secs(1) {
            self.engine_running = instance::engine_running();
            self.start_with_windows = autostart::is_enabled();
            self.engine_checked = Instant::now();
        }
        while let Ok(result) = self.test_rx.try_recv() {
            match result {
                Ok(()) => self.set_status("Action ran.", false),
                Err(e) => self.set_status(format!("Action failed: {e}"), true),
            }
        }
        if self
            .status
            .as_ref()
            .is_some_and(|s| !s.error && s.at.elapsed() > Duration::from_secs(6))
        {
            self.status = None;
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::Unsaved);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading(RichText::new("opendock").strong());
            ui.separator();
            ui.label("Page");
            let pages = self.sorted_pages();
            let label = |p: &str, home: &str| {
                if p == home {
                    format!("🏠 {p}")
                } else {
                    p.to_string()
                }
            };
            ComboBox::from_id_salt("page")
                .width(160.0)
                .selected_text(label(&self.page, &self.cfg.home))
                .show_ui(ui, |ui| {
                    for p in &pages {
                        if ui
                            .selectable_label(*p == self.page, label(p, &self.cfg.home))
                            .clicked()
                        {
                            self.page = p.clone();
                            self.selected = None;
                        }
                    }
                });
            if ui.button("➕").on_hover_text("New page").clicked() {
                self.dialog = Some(Dialog::NewPage(String::new()));
            }
            if ui.button("✏").on_hover_text("Rename page").clicked() {
                self.dialog = Some(Dialog::RenamePage(self.page.clone()));
            }
            let not_home = self.page != self.cfg.home;
            if ui
                .add_enabled(not_home, egui::Button::new("🗑"))
                .on_hover_text("Delete page")
                .clicked()
            {
                self.dialog = Some(Dialog::DeletePage);
            }
            if ui
                .add_enabled(not_home, egui::Button::new("🏠"))
                .on_hover_text("Make this the home page (shown at start)")
                .clicked()
            {
                self.cfg.home = self.page.clone();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("☰", |ui| {
                    if ui.button("Import from Stream Dock AJAZZ…").clicked() {
                        self.dialog = Some(Dialog::Import);
                        ui.close();
                    }
                    if ui.button("Reload from disk").clicked() {
                        match Config::load(&self.path) {
                            Ok(cfg) => {
                                self.saved_toml = cfg.to_toml();
                                self.page = cfg.home.clone();
                                self.cfg = cfg;
                                self.selected = None;
                                self.set_status("Reloaded.", false);
                            }
                            Err(e) => self.set_status(e, true),
                        }
                        ui.close();
                    }
                    if ui.button("Open config folder").clicked() {
                        let dir = self.base_dir.to_string_lossy().into_owned();
                        if let Err(e) = actions::execute(&Action::Run(Run::Path(dir))) {
                            self.set_status(e, true);
                        }
                        ui.close();
                    }
                });
                if ui.button("⚙ Settings").clicked() {
                    self.dialog = Some(Dialog::Settings);
                    self.check_lhm();
                }
            });
        });
        ui.add_space(4.0);
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            status_dot(
                ui,
                if self.engine_running {
                    OK
                } else {
                    Color32::GRAY
                },
            );
            if self.engine_running {
                ui.label("Engine running");
                if ui
                    .button("Stop")
                    .on_hover_text("Stop driving the deck")
                    .clicked()
                {
                    instance::request_quit();
                    self.engine_checked = Instant::now() - Duration::from_millis(500);
                }
            } else {
                ui.label("Engine stopped");
                if ui
                    .button("Start")
                    .on_hover_text("Start driving the deck")
                    .clicked()
                {
                    self.start_engine();
                }
            }
            ui.separator();
            if let Some(s) = &self.status {
                ui.colored_label(
                    if s.error {
                        ERR
                    } else {
                        ui.visuals().text_color()
                    },
                    &s.text,
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dirty = self.dirty();
                let save = egui::Button::new(RichText::new("💾 Save").strong());
                if ui
                    .add_enabled(dirty, save)
                    .on_hover_text("Ctrl+S")
                    .clicked()
                {
                    self.save();
                }
                if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                    if let Ok(cfg) = toml::from_str::<Config>(&self.saved_toml) {
                        if !cfg.pages.contains_key(&self.page) {
                            self.page = cfg.home.clone();
                        }
                        self.cfg = cfg;
                    }
                }
                if dirty {
                    ui.colored_label(Color32::from_rgb(240, 190, 80), "Unsaved changes");
                }
            });
        });
        ui.add_space(4.0);
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        if self.side_selected {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    editor::side_ui(ui, &mut self.cfg.side, self.readings.cpu_temp.is_some())
                });
            return;
        }
        let Some(pos) = self.selected else {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.weak("Select a key to edit it.");
                ui.weak("Drag keys to move or swap them.");
            });
            return;
        };
        let pages = self.sorted_pages();
        let mut req = editor::Requests::default();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let ctx = EditorCtx {
                    pages: &pages,
                    outputs: &self.outputs,
                    inputs: &self.inputs,
                    base_dir: &self.base_dir,
                };
                let keys = self.cfg.pages.entry(self.page.clone()).or_default();
                let idx = keys.iter().position(|k| k.pos == pos);
                let mut key = idx.map_or_else(|| Key::empty(pos), |i| keys[i].clone());
                req = editor::key_ui(ui, &mut key, &ctx);
                if req.clear {
                    key = Key::empty(pos);
                }
                match (idx, is_blank(&key)) {
                    (Some(i), true) => {
                        keys.remove(i);
                    }
                    (Some(i), false) => keys[i] = key,
                    (None, false) => keys.push(key),
                    (None, true) => {}
                }
            });
        if let Some(action) = req.test {
            let tx = self.test_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(actions::execute(&action));
            });
        }
        if req.refresh_audio {
            self.refresh_audio();
        }
        if let Some(e) = req.error {
            self.set_status(e, true);
        }
    }

    fn grid(&mut self, ui: &mut egui::Ui) {
        // Keys are drawn at device size when there is room, smaller otherwise.
        // The small screen sits to the right, as tall as the key grid.
        let aspect = side::ASPECT;
        let avail_w = ui.available_width() - 2.0 * 46.0;
        let gap = 14.0;
        let size = ((avail_w - (COLS + 1) as f32 * gap - 2.0 * gap * aspect)
            / (COLS as f32 + ROWS as f32 * aspect))
            .clamp(48.0, IMAGE_SIZE as f32);
        let keys_w = COLS as f32 * size + (COLS - 1) as f32 * gap;
        let keys_h = ROWS as f32 * size + (ROWS - 1) as f32 * gap;
        let side_w = keys_h * aspect;
        let total = Vec2::new(keys_w + 2.0 * gap + side_w, keys_h);
        let avail = ui.available_rect_before_wrap();
        let origin = Pos2::new(
            (avail.center().x - total.x / 2.0).max(avail.left() + 30.0),
            (avail.center().y - total.y / 2.0 - 20.0).max(avail.top() + 30.0),
        );
        let grid_rect = Rect::from_min_size(origin, total);
        ui.allocate_rect(avail, Sense::hover());
        let painter = ui.painter().clone();
        painter.rect_filled(grid_rect.expand(22.0), 16.0, Color32::from_gray(24));
        painter.rect_stroke(
            grid_rect.expand(22.0),
            16.0,
            Stroke::new(1.0, Color32::from_gray(60)),
            StrokeKind::Inside,
        );

        let rect_of = |p: Pos| {
            Rect::from_min_size(
                origin + Vec2::new(p.col as f32 * (size + gap), p.row as f32 * (size + gap)),
                Vec2::splat(size),
            )
        };
        let pointer = ui.ctx().pointer_hover_pos();
        let released = ui.input(|i| i.pointer.any_released());
        let ctx = ui.ctx().clone();
        let mut dragged_tex = None;

        // Small screen preview with live readings.
        let side_rect = Rect::from_min_size(
            origin + Vec2::new(keys_w + 2.0 * gap, 0.0),
            Vec2::new(side_w, keys_h),
        );
        let side_resp = ui.interact(side_rect, Id::new("side"), Sense::click());
        let tex = self.side_texture(&ctx);
        painter.add(
            egui::epaint::RectShape::filled(side_rect, 8.0, Color32::WHITE)
                .with_texture(tex.id(), FULL_UV),
        );
        let stroke = if self.side_selected {
            Stroke::new(3.0, ACCENT)
        } else if side_resp.hovered() {
            Stroke::new(1.5, Color32::from_gray(140))
        } else {
            Stroke::new(1.0, Color32::from_gray(55))
        };
        painter.rect_stroke(side_rect, 8.0, stroke, StrokeKind::Outside);
        if side_resp
            .on_hover_text("Small screen: click to choose what it shows")
            .clicked()
        {
            self.side_selected = true;
            self.selected = None;
        }

        for row in 0..ROWS {
            for col in 0..COLS {
                let pos = Pos { row, col };
                let rect = rect_of(pos);
                let resp = ui.interact(rect, Id::new(("key", row, col)), Sense::click_and_drag());
                let key = self.key_at(pos).cloned();
                let being_dragged = self.drag_from == Some(pos);
                match &key {
                    Some(k) => {
                        let tex = self.texture(&ctx, k);
                        let tint = if being_dragged {
                            Color32::from_gray(70)
                        } else {
                            Color32::WHITE
                        };
                        painter.add(
                            egui::epaint::RectShape::filled(rect, 10.0, tint)
                                .with_texture(tex.id(), FULL_UV),
                        );
                        if being_dragged {
                            dragged_tex = Some(tex);
                        }
                    }
                    None => {
                        painter.rect_filled(rect, 10.0, Color32::from_gray(38));
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "+",
                            egui::FontId::proportional(26.0),
                            Color32::from_gray(80),
                        );
                    }
                }
                let drop_target = self.drag_from.is_some_and(|f| f != pos)
                    && pointer.is_some_and(|p| rect.contains(p));
                let stroke = if drop_target {
                    Stroke::new(3.0, Color32::from_rgb(240, 190, 80))
                } else if self.selected == Some(pos) {
                    Stroke::new(3.0, ACCENT)
                } else if resp.hovered() {
                    Stroke::new(1.5, Color32::from_gray(140))
                } else {
                    Stroke::new(1.0, Color32::from_gray(55))
                };
                painter.rect_stroke(rect, 10.0, stroke, StrokeKind::Outside);

                if resp.drag_started() && key.is_some() {
                    self.drag_from = Some(pos);
                    self.selected = Some(pos);
                    self.side_selected = false;
                }
                if resp.clicked() {
                    self.selected = Some(pos);
                    self.side_selected = false;
                }
            }
        }

        if let (Some(tex), Some(p)) = (dragged_tex, pointer) {
            let layer = ui
                .ctx()
                .layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("drag")));
            let r = Rect::from_center_size(p, Vec2::splat(size * 0.8));
            layer.add(
                egui::epaint::RectShape::filled(r, 10.0, Color32::from_white_alpha(220))
                    .with_texture(tex.id(), FULL_UV),
            );
        }
        if released {
            if let (Some(from), Some(p)) = (self.drag_from, pointer) {
                let target = (0..ROWS)
                    .flat_map(|row| (0..COLS).map(move |col| Pos { row, col }))
                    .find(|&q| rect_of(q).contains(p));
                if let Some(to) = target {
                    self.move_key(from, to);
                }
            }
            self.drag_from = None;
        }

        painter.text(
            Pos2::new(grid_rect.center().x, grid_rect.bottom() + 44.0),
            egui::Align2::CENTER_CENTER,
            "Click a key or the small screen to edit it · drag keys to move or swap · save to update the deck",
            egui::FontId::proportional(13.0),
            Color32::from_gray(120),
        );
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let creating = matches!(dialog, Dialog::NewPage(_));
        let modal = egui::Modal::new(Id::new("dialog")).show(ctx, |ui| {
            ui.set_width(380.0);
            match &mut dialog {
                Dialog::Settings => {
                    ui.heading("⚙ Settings");
                    ui.add_space(8.0);

                    ui.label(RichText::new("General").strong());
                    ui.indent("general", |ui| {
                        if ui
                            .checkbox(&mut self.start_with_windows, "Start opendock with Windows")
                            .changed()
                        {
                            if let Err(e) = autostart::set(self.start_with_windows, &self.path) {
                                self.start_with_windows = !self.start_with_windows;
                                self.set_status(e, true);
                            }
                        }
                        ui.weak("Runs the engine and its tray icon when you sign in. Applies right away.");
                    });
                    ui.add_space(10.0);

                    ui.label(RichText::new("Sensors").strong());
                    ui.indent("sensors", |ui| self.sensors_settings(ui));
                    ui.add_space(10.0);

                    ui.label(RichText::new("Deck").strong());
                    ui.indent("deck", |ui| {
                        egui::Grid::new("deck_settings").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                            ui.label("Brightness");
                            ui.add(egui::Slider::new(&mut self.cfg.brightness, 0..=100).suffix(" %"));
                            ui.end_row();

                            ui.label("Title font");
                            ui.horizontal(|ui| {
                                let name = self.cfg.font.as_ref().map_or("Segoe UI Bold (default)".to_string(), |f| {
                                    f.file_name().map_or(f.display().to_string(), |n| n.to_string_lossy().into_owned())
                                });
                                ui.label(name);
                                if ui.button("Choose…").clicked() {
                                    let picked = rfd::FileDialog::new()
                                        .set_title("Choose a font for key titles")
                                        .add_filter("Fonts", &["ttf", "otf"])
                                        .set_directory(r"C:\Windows\Fonts")
                                        .pick_file();
                                    if let Some(f) = picked {
                                        let rel = f.strip_prefix(&self.base_dir).map(PathBuf::from).unwrap_or(f);
                                        self.cfg.font = Some(rel);
                                    }
                                }
                                if self.cfg.font.is_some() && ui.button("Default").clicked() {
                                    self.cfg.font = None;
                                }
                            });
                            ui.end_row();
                        });
                        ui.weak("Deck settings are saved with your keys (Save / Ctrl+S).");
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    if ui.button("Close").clicked() {
                        keep = false;
                    }
                }
                Dialog::NewPage(name) | Dialog::RenamePage(name) => {
                    ui.heading(if creating { "New page" } else { "Rename page" });
                    let edit = ui.add(egui::TextEdit::singleline(name).hint_text("Page name"));
                    edit.request_focus();
                    let name = name.trim().to_string();
                    let taken = self.cfg.pages.contains_key(&name) && (creating || name != self.page);
                    if taken {
                        ui.colored_label(ERR, "A page with that name already exists.");
                    }
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        let ok = !name.is_empty() && !taken;
                        let go = ui.add_enabled(ok, egui::Button::new(if creating { "Create" } else { "Rename" })).clicked()
                            || (ok && enter);
                        if go {
                            if creating {
                                self.cfg.pages.insert(name.clone(), Vec::new());
                            } else {
                                let from = self.page.clone();
                                self.cfg.rename_page(&from, &name);
                            }
                            self.page = name;
                            self.selected = None;
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                }
                Dialog::DeletePage => {
                    ui.heading("Delete page");
                    let linked = pages_linking_to(&self.cfg, &self.page);
                    if linked.is_empty() {
                        let n = self.cfg.pages.get(&self.page).map_or(0, Vec::len);
                        ui.label(format!("Delete page \"{}\" and its {n} keys?", self.page));
                        ui.horizontal(|ui| {
                            if ui.button(RichText::new("Delete").color(ERR)).clicked() {
                                self.cfg.pages.remove(&self.page);
                                self.page = self.cfg.home.clone();
                                self.selected = None;
                                keep = false;
                            }
                            if ui.button("Cancel").clicked() {
                                keep = false;
                            }
                        });
                    } else {
                        ui.label(format!(
                            "Keys on {} still open this page. Change those keys first.",
                            linked.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(", ")
                        ));
                        if ui.button("OK").clicked() {
                            keep = false;
                        }
                    }
                }
                Dialog::Import => {
                    ui.heading("Import from Stream Dock AJAZZ");
                    ui.label(
                        "Replace the current setup with the profile active in the vendor app? \
                         Icons are copied into the config folder. Nothing is saved until you click Save.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Import").clicked() {
                            keep = false;
                            match import::active_profile().and_then(|p| import::import(&p, &self.base_dir)) {
                                Ok(report) => {
                                    self.page = report.config.home.clone();
                                    self.cfg = report.config;
                                    self.selected = None;
                                    if !report.notes.is_empty() {
                                        self.dialog = Some(Dialog::ImportNotes(report.notes));
                                    }
                                    self.set_status("Imported. Review the keys, then save.", false);
                                }
                                Err(e) => self.set_status(format!("Import failed: {e}"), true),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                }
                Dialog::ImportNotes(notes) => {
                    ui.heading("Imported with notes");
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for n in notes.iter() {
                            ui.label(format!("• {n}"));
                        }
                    });
                    if ui.button("OK").clicked() {
                        keep = false;
                    }
                }
                Dialog::Unsaved => {
                    ui.heading("Unsaved changes");
                    ui.label("Save your changes before closing?");
                    ui.horizontal(|ui| {
                        let mut close = false;
                        if ui.button(RichText::new("Save and close").strong()).clicked() && self.save() {
                            close = true;
                        }
                        if ui.button("Discard").clicked() {
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                        if close {
                            keep = false;
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                }
            }
        });
        if modal.should_close() {
            keep = false;
        }
        // An action inside the modal may have opened a follow-up dialog.
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_close(&ctx);
        self.poll(&ctx);
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) && self.dirty() {
            self.save();
        }
        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("bottom").show(ui, |ui| self.bottom_bar(ui));
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(400.0)
            .size_range(340.0..=560.0)
            .show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::default().show(ui, |ui| self.grid(ui));
        self.dialogs(&ctx);
    }
}
