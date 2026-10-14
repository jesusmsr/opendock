//! The engine's tray icon. The device loop runs on a worker thread; this
//! thread pumps Windows messages for the icon and handles its menu.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

use crate::config::Config;
use crate::instance::{self, EngineLock};
use crate::{autostart, deck, icon, log};

/// Shared, human-readable engine state shown in the tray tooltip.
pub type Status = Arc<Mutex<String>>;

fn pump_messages() {
    // SAFETY: standard Win32 message loop on the thread that owns the tray window.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn open_settings(config: &Path) {
    let result = std::env::current_exe().and_then(|exe| {
        std::process::Command::new(exe)
            .arg("--config")
            .arg(config)
            .spawn()
    });
    if let Err(e) = result {
        log(&format!("cannot open settings: {e}"));
    }
}

/// Runs the engine with a tray icon until it is asked to quit.
pub fn run(config_path: PathBuf, cfg: Config, lock: EngineLock) {
    let status: Status = Arc::new(Mutex::new("starting".into()));
    let engine = {
        let (path, status) = (config_path.clone(), status.clone());
        thread::spawn(move || deck::run(&path, cfg, lock, &status))
    };
    autostart::refresh(&config_path);

    let open = MenuItem::new("Open settings", true, None);
    let startup = CheckMenuItem::new("Start with Windows", true, autostart::is_enabled(), None);
    let quit = MenuItem::new("Quit opendock", true, None);
    let menu = Menu::new();
    let built = menu.append_items(&[
        &open,
        &PredefinedMenuItem::separator(),
        &startup,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);
    let tray = built
        .map_err(|e| e.to_string())
        .and_then(|()| {
            Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE).map_err(|e| e.to_string())
        })
        .and_then(|icon| {
            TrayIconBuilder::new()
                .with_icon(icon)
                .with_tooltip("opendock")
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
                .map_err(|e| e.to_string())
        });
    let tray = match tray {
        Ok(t) => Some(t),
        Err(e) => {
            // The deck still works without an icon; it can be stopped from the settings window.
            log(&format!("no tray icon: {e}"));
            None
        }
    };

    let mut shown_status = String::new();
    let mut last_sync = Instant::now();
    while !engine.is_finished() {
        pump_messages();
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == *open.id() {
                open_settings(&config_path);
            } else if ev.id == *startup.id() {
                // The menu toggles the check mark itself; mirror it in the registry.
                let on = startup.is_checked();
                if let Err(e) = autostart::set(on, &config_path) {
                    log(&e);
                    startup.set_checked(!on);
                }
            } else if ev.id == *quit.id() {
                instance::request_quit();
            }
        }
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = ev
            {
                open_settings(&config_path);
            }
        }
        if let Some(tray) = &tray {
            let now = status.lock().map(|s| s.clone()).unwrap_or_default();
            if now != shown_status {
                let _ = tray.set_tooltip(Some(format!("opendock: {now}")));
                shown_status = now;
            }
        }
        // The settings window can change start-up too; keep the check mark in sync.
        if last_sync.elapsed() >= Duration::from_secs(2) {
            last_sync = Instant::now();
            startup.set_checked(autostart::is_enabled());
        }
        thread::sleep(Duration::from_millis(30));
    }
    drop(tray);
}
