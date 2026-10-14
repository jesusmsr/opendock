//! chechudock - lightweight controller for AJAZZ Stream Dock devices.

mod actions;
mod config;
mod deck;
mod import;
mod keys;
mod render;

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::exit;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::CreateMutexW;

const USAGE: &str = "\
chechudock - lightweight controller for AJAZZ Stream Dock devices

USAGE: chechudock [--config <file>] [command]

COMMANDS:
  run (default)        Drive the device until closed
  check                Validate the config and print a summary
  import [--profile <dir>] [--force]
                       Create the config from the vendor app's profile
                       (default: the one active on your device)
  preview <dir>        Render every key to PNG files for a quick look

The config defaults to %APPDATA%\\chechudock\\config.toml.
The log is written next to it as chechudock.log.
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

/// Returns false if another chechudock is already running.
fn single_instance() -> bool {
    let name: Vec<u16> = "Local\\chechudock-single-instance\0"
        .encode_utf16()
        .collect();
    // SAFETY: valid NUL-terminated name; the handle is intentionally leaked so
    // the mutex lives as long as the process.
    unsafe {
        CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

fn main() {
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

    match args.first().map(String::as_str).unwrap_or("run") {
        "run" => {
            let _ = LOG_PATH.set(config_dir.join("chechudock.log"));
            if !single_instance() {
                fail("chechudock is already running");
            }
            let cfg = config::Config::load(&config_path).unwrap_or_else(|e| {
                fail(&format!(
                    "{e}\n(run `chechudock import` to create a config from the vendor app)"
                ))
            });
            log(&format!("using {}", config_path.display()));
            deck::run(&config_path, cfg);
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
                    r.draw(k)
                        .save(&path)
                        .unwrap_or_else(|e| fail(&e.to_string()));
                    println!("wrote {}", path.display());
                }
            }
        }
        other => fail(&format!("unknown command {other:?}\n\n{USAGE}")),
    }
}
