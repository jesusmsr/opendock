//! opendock - lightweight controller for AJAZZ Stream Dock devices.

// A GUI program: no console window when started from Explorer. CLI commands
// attach to the parent terminal's console instead (see `main`).
#![windows_subsystem = "windows"]

mod actions;
mod audio;
mod config;
mod deck;
mod gui;
mod import;
mod instance;
mod keys;
mod render;

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::exit;
use std::sync::OnceLock;

use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

const USAGE: &str = "\
opendock - lightweight controller for AJAZZ Stream Dock devices

USAGE: opendock [--config <file>] [command]

COMMANDS:
  (none)               Open the settings window
  run                  Run the engine that drives the device (no window)
  check                Validate the config and print a summary
  import [--profile <dir>] [--force]
                       Create the config from the vendor app's profile
                       (default: the one active on your device)
  preview <dir>        Render every key to PNG files for a quick look
  audio                List audio devices (names for audio_output/audio_input)
  audio output|input <name>...
                       Set the default device (several names: cycle)

The config defaults to %APPDATA%\\opendock\\config.toml.
The log is written next to it as opendock.log.
";

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Prints a timestamped line and appends it to the log file.
pub fn log(msg: &str) {
    // SAFETY: GetLocalTime only writes the struct we pass.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    let line = format!(
        "[{:04}-{:02}-{:02} {:02}:{:02}:{:02}] {msg}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    println!("{line}");
    if let Some(path) = LOG_PATH.get() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}

fn main() {
    // SAFETY: plain Win32 call; failure just means there is no parent console
    // (started from Explorer), and output then goes nowhere.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return;
    }
    let mut take_opt = |name: &str| -> Option<String> {
        let i = args.iter().position(|a| a == name)?;
        if i + 1 >= args.len() {
            fail(&format!("{name} needs a value"));
        }
        args.remove(i);
        Some(args.remove(i))
    };
    let config_path = take_opt("--config")
        .map(PathBuf::from)
        .unwrap_or_else(config::default_path);
    let profile = take_opt("--profile").map(PathBuf::from);
    let force = args
        .iter()
        .position(|a| a == "--force")
        .map(|i| args.remove(i))
        .is_some();
    let config_dir = config_path.parent().map(PathBuf::from).unwrap_or_default();

    match args.first().map(String::as_str).unwrap_or("settings") {
        "settings" => {
            if let Err(e) = gui::run(config_path) {
                fail(&format!("cannot open the settings window: {e}"));
            }
        }
        "run" => {
            let _ = LOG_PATH.set(config_dir.join("opendock.log"));
            let Some(lock) = instance::EngineLock::acquire() else {
                log("the engine is already running");
                exit(1);
            };
            let cfg = match config::Config::load(&config_path) {
                Ok(cfg) => cfg,
                Err(e) => {
                    log(&format!("cannot start: {e}"));
                    exit(1);
                }
            };
            log(&format!("using {}", config_path.display()));
            deck::run(&config_path, cfg, lock);
        }
        "check" => {
            let cfg = config::Config::load(&config_path).unwrap_or_else(|e| fail(&e));
            let mut pages: Vec<_> = cfg.pages.iter().collect();
            pages.sort_by_key(|(name, _)| name.as_str());
            println!("{} is valid. home = {:?}", config_path.display(), cfg.home);
            for (name, keys) in pages {
                println!("  page {name:?}: {} keys", keys.len());
            }
        }
        "import" => {
            if config_path.exists() && !force {
                fail(&format!(
                    "{} already exists (use --force to overwrite)",
                    config_path.display()
                ));
            }
            let profile = profile
                .map_or_else(import::active_profile, Ok)
                .unwrap_or_else(|e| fail(&e));
            println!("importing {}", profile.display());
            let report = import::import(&profile, &config_dir).unwrap_or_else(|e| fail(&e));
            std::fs::write(&config_path, report.config.to_toml())
                .unwrap_or_else(|e| fail(&e.to_string()));
            println!("wrote {}", config_path.display());
            for n in &report.notes {
                println!("  note: {n}");
            }
        }
        "preview" => {
            let dir = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| fail("preview needs a directory"));
            let cfg = config::Config::load(&config_path).unwrap_or_else(|e| fail(&e));
            let r = render::Renderer::new(cfg.font.as_deref(), &config_dir);
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| fail(&e.to_string()));
            for (page, keys) in &cfg.pages {
                for k in keys {
                    let path = dir.join(format!("{page}_{}-{}.png", k.pos.row + 1, k.pos.col + 1));
                    r.draw(k, false)
                        .save(&path)
                        .unwrap_or_else(|e| fail(&e.to_string()));
                    println!("wrote {}", path.display());
                }
            }
        }
        "audio" if args.len() > 1 => {
            let flow = match args[1].as_str() {
                "output" => audio::Flow::Output,
                "input" => audio::Flow::Input,
                other => fail(&format!("expected `output` or `input`, got {other:?}")),
            };
            let targets = &args[2..];
            if targets.is_empty() {
                fail("give a device name (or several to cycle)");
            }
            let _com = audio::ComGuard::new();
            let name = audio::switch(flow, targets).unwrap_or_else(|e| fail(&e));
            println!("default is now {name:?}");
        }
        "audio" => {
            let _com = audio::ComGuard::new();
            for (flow, label) in [
                (audio::Flow::Output, "Outputs"),
                (audio::Flow::Input, "Inputs"),
            ] {
                println!("{label} (* = default):");
                let default = audio::default_id(flow);
                for e in audio::list(flow).unwrap_or_else(|e| fail(&e)) {
                    let mark = if Some(&e.id) == default.as_ref() {
                        '*'
                    } else {
                        ' '
                    };
                    println!("  {mark} {}", e.name);
                }
            }
        }
        other => fail(&format!("unknown command {other:?}\n\n{USAGE}")),
    }
}
