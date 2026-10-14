//! Device session: connect, draw the current page, dispatch key presses,
//! keep the connection alive and hot-reload the config.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use ajazz_hid::akp153e::{self, COLS, ROWS};
use ajazz_hid::{Device, HidApi};

use crate::actions::{self, Nav};
use crate::config::{Config, Key};
use crate::log;
use crate::render::Renderer;

const KEEP_ALIVE: Duration = Duration::from_secs(10);
const RELOAD_CHECK: Duration = Duration::from_secs(1);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

struct State {
    path: PathBuf,
    cfg: Config,
    renderer: Renderer,
    mtime: Option<SystemTime>,
    /// Page history; the last entry is the visible page.
    stack: Vec<String>,
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
        State {
            path: path.to_path_buf(),
            stack: vec![cfg.home.clone()],
            renderer,
            mtime: mtime(path),
            cfg,
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

/// Uploads every key of the current page and makes it visible.
fn draw_page(dev: &Device, st: &mut State) -> ajazz_hid::Result<()> {
    for row in 0..ROWS {
        for col in 0..COLS {
            let id = akp153e::key_id(row, col).expect("in range");
            match st.key_at(row, col).cloned() {
                Some(key) => dev.set_key_image(id, &st.renderer.jpeg(&key))?,
                None => dev.clear_key(id)?,
            }
        }
    }
    dev.flush()
}

fn on_press(st: &mut State, id: u8) -> bool {
    let Some((row, col)) = akp153e::position(id) else {
        return false;
    };
    let Some(action) = st.key_at(row, col).and_then(|k| k.action.clone()) else {
        return false;
    };
    let navs = actions::navigation(&action);
    thread::spawn(move || {
        if let Err(e) = actions::execute(&action) {
            log(&format!("key {},{}: {e}", row + 1, col + 1));
        }
    });
    let before = st.page().to_string();
    for n in &navs {
        st.navigate(n);
    }
    st.page() != before
}

fn session(dev: &Device, st: &mut State) -> ajazz_hid::Result<()> {
    dev.set_brightness(st.cfg.brightness)?;
    draw_page(dev, st)?;
    let mut last_ping = Instant::now();
    let mut last_check = Instant::now();
    loop {
        if let Some(report) = dev.read(Duration::from_millis(100))? {
            if let Some((id, 1)) = report.key {
                if on_press(st, id) {
                    draw_page(dev, st)?;
                }
            }
        }
        if last_check.elapsed() >= RELOAD_CHECK {
            last_check = Instant::now();
            if st.reload_if_changed() {
                dev.set_brightness(st.cfg.brightness)?;
                draw_page(dev, st)?;
            }
        }
        if last_ping.elapsed() >= KEEP_ALIVE {
            last_ping = Instant::now();
            dev.keep_alive()?;
        }
    }
}

/// Runs forever, reconnecting whenever the device goes away.
pub fn run(path: &Path, cfg: Config) -> ! {
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
                log(&format!(
                    "connected to {} (packet {} bytes)",
                    dev.serial,
                    dev.packet_size()
                ));
                if let Err(e) = session(&dev, &mut st) {
                    log(&format!("device disconnected: {e}"));
                }
            }
            Err(e) => {
                if !waiting_logged {
                    log(&format!("waiting for device ({e})"));
                    waiting_logged = true;
                }
            }
        }
        thread::sleep(RECONNECT_DELAY);
        if let Err(e) = api.refresh_devices() {
            log(&format!("HID refresh failed: {e}"));
        }
    }
}
