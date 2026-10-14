//! Device session: connect, draw the current page, dispatch key presses,
//! keep the connection alive and hot-reload the config.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use ajazz_hid::akp153e::{self, COLS, ROWS};
use ajazz_hid::{Device, HidApi};

use crate::actions::{self, Nav};
use crate::audio::{self, Endpoint, Flow};
use crate::config::{Action, Config, Key};
use crate::instance::EngineLock;
use crate::log;
use crate::render::Renderer;
use crate::tray::Status;

const KEEP_ALIVE: Duration = Duration::from_secs(10);
/// How often the config file and live key state (e.g. audio devices) are checked.
const POLL: Duration = Duration::from_secs(1);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

struct State {
    path: PathBuf,
    cfg: Config,
    renderer: Renderer,
    mtime: Option<SystemTime>,
    /// Page history; the last entry is the visible page.
    stack: Vec<String>,
    /// What each key id currently shows: hash of the uploaded JPEG, None if cleared.
    shown: HashMap<u8, Option<u64>>,
    live: Live,
    /// Signalled by action threads when they finish, so keys whose look
    /// depends on system state (e.g. the default audio device) update at once.
    done_tx: Sender<()>,
    done_rx: Receiver<()>,
}

/// System state that changes what keys look like.
#[derive(Default)]
struct Live {
    outputs: Vec<Endpoint>,
    inputs: Vec<Endpoint>,
    default_output: Option<String>,
    default_input: Option<String>,
}

impl Live {
    fn refresh(&mut self, output: bool, input: bool) {
        if output {
            self.outputs = audio::list(Flow::Output).unwrap_or_default();
            self.default_output = audio::default_id(Flow::Output);
        }
        if input {
            self.inputs = audio::list(Flow::Input).unwrap_or_default();
            self.default_input = audio::default_id(Flow::Input);
        }
    }

    /// A single-device audio key is active while its device is the default.
    fn is_active(&self, key: &Key) -> bool {
        let (list, default, target) = match &key.action {
            Some(Action::AudioOutput(t)) => (&self.outputs, &self.default_output, t),
            Some(Action::AudioInput(t)) => (&self.inputs, &self.default_input, t),
            _ => return false,
        };
        match target.targets().as_slice() {
            [one] => audio::resolve(list, one).is_some_and(|e| Some(&e.id) == default.as_ref()),
            _ => false,
        }
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn base_dir(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

impl State {
    fn new(path: &Path, cfg: Config) -> Self {
        let renderer = Renderer::new(cfg.font.as_deref(), &base_dir(path));
        let (done_tx, done_rx) = channel();
        State {
            path: path.to_path_buf(),
            stack: vec![cfg.home.clone()],
            renderer,
            mtime: mtime(path),
            cfg,
            shown: HashMap::new(),
            live: Live::default(),
            done_tx,
            done_rx,
        }
    }

    fn page(&self) -> &str {
        self.stack.last().expect("page stack is never empty")
    }

    fn key_at(&self, row: u8, col: u8) -> Option<&Key> {
        self.cfg
            .pages
            .get(self.page())?
            .iter()
            .find(|k| k.pos.row == row && k.pos.col == col)
    }

    /// Reloads the config if the file changed. Returns true if it did.
    fn reload_if_changed(&mut self) -> bool {
        let m = mtime(&self.path);
        if m == self.mtime {
            return false;
        }
        self.mtime = m;
        match Config::load(&self.path) {
            Ok(cfg) => {
                log("config reloaded");
                if cfg.font != self.cfg.font {
                    self.renderer = Renderer::new(cfg.font.as_deref(), &base_dir(&self.path));
                }
                self.stack.retain(|p| cfg.pages.contains_key(p));
                if self.stack.is_empty() {
                    self.stack.push(cfg.home.clone());
                }
                self.cfg = cfg;
                true
            }
            Err(e) => {
                log(&format!(
                    "config not reloaded, keeping the previous one: {e}"
                ));
                false
            }
        }
    }

    /// Re-reads live state needed by the visible page.
    fn refresh_live(&mut self) {
        let keys = self
            .cfg
            .pages
            .get(self.page())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let output = keys
            .iter()
            .any(|k| matches!(k.action, Some(Action::AudioOutput(_))));
        let input = keys
            .iter()
            .any(|k| matches!(k.action, Some(Action::AudioInput(_))));
        self.live.refresh(output, input);
    }

    fn navigate(&mut self, nav: &Nav) {
        match nav {
            Nav::Page(p) if p != self.page() => self.stack.push(p.clone()),
            Nav::Page(_) => {}
            Nav::Back if self.stack.len() > 1 => {
                self.stack.pop();
            }
            Nav::Back => {}
            Nav::Home => {
                self.stack.clear();
                self.stack.push(self.cfg.home.clone());
            }
        }
    }
}

/// Brings the device in line with the current page, uploading only keys
/// whose image changed.
fn draw_page(dev: &Device, st: &mut State) -> ajazz_hid::Result<()> {
    st.refresh_live();
    let mut changed = false;
    for row in 0..ROWS {
        for col in 0..COLS {
            let id = akp153e::key_id(row, col).expect("in range");
            let jpeg = st.key_at(row, col).cloned().map(|key| {
                let active = st.live.is_active(&key);
                st.renderer.jpeg(&key, active)
            });
            let hash = jpeg.as_ref().map(|j| {
                let mut h = DefaultHasher::new();
                j.hash(&mut h);
                h.finish()
            });
            if st.shown.get(&id) == Some(&hash) {
                continue;
            }
            match &jpeg {
                Some(j) => dev.set_key_image(id, j)?,
                None => dev.clear_key(id)?,
            }
            st.shown.insert(id, hash);
            changed = true;
        }
    }
    if changed {
        dev.flush()?;
    }
    Ok(())
}

fn on_press(st: &mut State, id: u8) -> bool {
    let Some((row, col)) = akp153e::position(id) else {
        return false;
    };
    let Some(action) = st.key_at(row, col).and_then(|k| k.action.clone()) else {
        return false;
    };
    let navs = actions::navigation(&action);
    let done = st.done_tx.clone();
    thread::spawn(move || {
        if let Err(e) = actions::execute(&action) {
            log(&format!("key {},{}: {e}", row + 1, col + 1));
        }
        let _ = done.send(());
    });
    let before = st.page().to_string();
    for n in &navs {
        st.navigate(n);
    }
    st.page() != before
}

/// Drives a connected device until it disconnects (Err) or a quit is
/// requested (Ok).
fn session(dev: &Device, st: &mut State, lock: &EngineLock) -> ajazz_hid::Result<()> {
    dev.set_brightness(st.cfg.brightness)?;
    // A fresh connection shows nothing we know of.
    st.shown.clear();
    draw_page(dev, st)?;
    let mut last_ping = Instant::now();
    let mut last_check = Instant::now();
    loop {
        if lock.quit_requested() {
            // Leave the keys blank rather than showing actions that no longer work.
            dev.clear_all()?;
            return dev.flush();
        }
        if let Some(report) = dev.read(Duration::from_millis(50))? {
            if let Some((id, 1)) = report.key {
                if on_press(st, id) {
                    draw_page(dev, st)?;
                }
            }
        }
        if st.done_rx.try_iter().count() > 0 {
            draw_page(dev, st)?;
        }
        if last_check.elapsed() >= POLL {
            last_check = Instant::now();
            if st.reload_if_changed() {
                dev.set_brightness(st.cfg.brightness)?;
            }
            // Cheap when nothing changed: only differing keys are uploaded.
            draw_page(dev, st)?;
        }
        if last_ping.elapsed() >= KEEP_ALIVE {
            last_ping = Instant::now();
            dev.keep_alive()?;
        }
    }
}

/// Runs until a quit is requested, reconnecting whenever the device goes away.
pub fn run(path: &Path, cfg: Config, lock: EngineLock, status: &Status) {
    let set_status = |s: &str| {
        if let Ok(mut st) = status.lock() {
            *st = s.to_string();
        }
    };
    let _com = audio::ComGuard::new();
    let mut st = State::new(path, cfg);
    let mut api = loop {
        match HidApi::new() {
            Ok(api) => break api,
            Err(e) => {
                log(&format!("HID init failed: {e}"));
                thread::sleep(RECONNECT_DELAY);
            }
        }
    };
    let mut waiting_logged = false;
    loop {
        st.reload_if_changed();
        match Device::open(&api, None) {
            Ok(dev) => {
                waiting_logged = false;
                set_status("deck connected");
                log(&format!(
                    "connected to {} (packet {} bytes)",
                    dev.serial,
                    dev.packet_size()
                ));
                match session(&dev, &mut st, &lock) {
                    Ok(()) => break,
                    Err(e) => log(&format!("device disconnected: {e}")),
                }
            }
            Err(e) => {
                if !waiting_logged {
                    set_status("waiting for the deck");
                    log(&format!("waiting for device ({e})"));
                    waiting_logged = true;
                }
            }
        }
        if lock.quit_requested() {
            break;
        }
        thread::sleep(RECONNECT_DELAY);
        if let Err(e) = api.refresh_devices() {
            log(&format!("HID refresh failed: {e}"));
        }
    }
    log("engine stopped");
}
