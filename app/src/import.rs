//! One-time import of a profile from the vendor "Stream Dock AJAZZ" app.
//!
//! Vendor profiles live in `%APPDATA%\HotSpot\StreamDock\profiles\<id>.sdProfile`,
//! with key positions written as `"col,row"` (0-based). Column 5 is the small
//! secondary screen, which this importer skips.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::{Action, AudioTarget, Config, Key, Media, Pos, Run};
use crate::keys;

pub struct Report {
    pub config: Config,
    /// Human-readable notes about keys that were not fully converted.
    pub notes: Vec<String>,
}

fn vendor_dir() -> PathBuf {
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_default();
    appdata.join("HotSpot").join("StreamDock")
}

/// The profile the vendor app currently shows on the connected device.
pub fn active_profile() -> Result<PathBuf, String> {
    let ini_path = vendor_dir().join("config").join("StreamDockConfig.ini");
    let ini =
        std::fs::read_to_string(&ini_path).map_err(|e| format!("{}: {e}", ini_path.display()))?;
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current = String::new();
    for line in ini.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.to_string();
        } else if let Some((k, v)) = line.split_once('=') {
            sections
                .entry(current.clone())
                .or_default()
                .insert(k.trim().into(), v.trim().into());
        }
    }
    let device = sections
        .get("StreamDockCurrentDevice")
        .and_then(|s| s.get("Device"))
        .ok_or("vendor config has no current device")?;
    sections
        .iter()
        .filter(|(name, _)| name.ends_with("DeviceConfig"))
        .find(|(_, kv)| kv.get("DeviceUUID") == Some(device))
        .and_then(|(_, kv)| kv.get("sdProfilePath"))
        .map(|p| PathBuf::from(p.replace("\\\\", "\\")))
        .ok_or_else(|| format!("no profile configured for device {device}"))
}

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Menu order of the vendor's "Multimedia" action, as listed in its UI strings.
/// Fast forward/backward (4, 5) have no standard Windows key.
fn media_from_index(i: i64) -> Option<Media> {
    Some(match i {
        0 => Media::Previous,
        1 => Media::PlayPause,
        2 => Media::Next,
        3 => Media::Stop,
        6 => Media::Mute,
        7 => Media::VolumeUp,
        8 => Media::VolumeDown,
        _ => return None,
    })
}

fn hotkeys(settings: &Value) -> Vec<Action> {
    let list = settings
        .get("Hotkeys")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.iter()
        .filter_map(|h| {
            let vk = h.get("VKeyCode")?.as_i64().filter(|&v| v > 0)?;
            let flag = |k| h.get(k).and_then(Value::as_bool).unwrap_or(false);
            Some(Action::Hotkey(keys::format(
                flag("KeyCtrl"),
                flag("KeyShift"),
                flag("KeyOption"),
                flag("KeyCmd"),
                vk as u16,
            )))
        })
        .collect()
}

/// Converts one vendor action; `Err` explains why it could not be.
fn convert(action: &Value) -> Result<Action, String> {
    let uuid = s(action, "UUID").unwrap_or("");
    let settings = action.get("Settings").cloned().unwrap_or(Value::Null);
    match uuid {
        "com.hotspot.streamdock.system.open" => {
            let path = s(&settings, "path")
                .filter(|p| !p.is_empty())
                .ok_or("open: no path")?;
            Ok(Action::Run(Run::Path(path.to_string())))
        }
        "com.hotspot.streamdock.system.website" => {
            let url = s(&settings, "path").ok_or("website: no URL")?;
            Ok(Action::Url(url.to_string()))
        }
        "com.hotspot.streamdock.system.text" => Ok(Action::Text(
            s(&settings, "pastedText").unwrap_or("").to_string(),
        )),
        "com.hotspot.streamdock.system.hotkey" => {
            let mut list = hotkeys(&settings);
            match list.len() {
                0 => Err("hotkey: no key set".into()),
                1 => Ok(list.remove(0)),
                _ => Ok(Action::Multi(list)),
            }
        }
        "com.hotspot.streamdock.system.multimedia" => {
            let idx = settings
                .get("actionIdx")
                .and_then(Value::as_i64)
                .unwrap_or(-1);
            media_from_index(idx).map(Action::Media).ok_or_else(|| {
                format!("multimedia #{idx} (fast forward/backward?) has no equivalent yet")
            })
        }
        "com.hotspot.streamdock.multiactions.routine" => {
            let mut out = Vec::new();
            for sub in action
                .get("Actions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                out.push(convert(sub)?);
                let delay = sub
                    .pointer("/MultiActionDelays/Delay2")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if delay > 0 {
                    out.push(Action::Delay(delay));
                }
            }
            if matches!(out.last(), Some(Action::Delay(_))) {
                out.pop();
            }
            Ok(Action::Multi(out))
        }
        "com.mirabox.streamdock.switchAudio.myaction1" => {
            let id = s(&settings, "select")
                .filter(|i| !i.is_empty())
                .ok_or("audio switch: no device")?;
            // Prefer the readable name; ids also work but are opaque.
            let name = settings
                .get("devices")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|d| s(d, "deviceId") == Some(id))
                .and_then(|d| s(d, "deviceName"))
                .unwrap_or(id);
            let target = AudioTarget::One(name.to_string());
            Ok(match s(&settings, "device") {
                Some("input") => Action::AudioInput(target),
                _ => Action::AudioOutput(target),
            })
        }
        other => Err(format!(
            "{} ({other}) is not supported",
            s(action, "Name").unwrap_or("?")
        )),
    }
}

/// Reads `profile_dir`, copying icons into `<config_dir>/icons`.
pub fn import(profile_dir: &Path, config_dir: &Path) -> Result<Report, String> {
    let manifest_path = profile_dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let manifest: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let icons_dir = config_dir.join("icons");
    std::fs::create_dir_all(&icons_dir).map_err(|e| format!("{}: {e}", icons_dir.display()))?;

    let mut keys_out = Vec::new();
    let mut notes = Vec::new();
    let actions = manifest
        .get("Actions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut positions: Vec<_> = actions.keys().cloned().collect();
    positions.sort();
    for vpos in positions {
        let a = &actions[&vpos];
        let parsed = vpos
            .split_once(',')
            .and_then(|(c, r)| Some((c.parse::<u8>().ok()?, r.parse::<u8>().ok()?)));
        let Some((col, row)) = parsed else {
            notes.push(format!("skipped key at {vpos:?}: unrecognised position"));
            continue;
        };
        let name = s(a, "Name").unwrap_or("?");
        if ajazz_hid::akp153e::key_id(row, col).is_none() {
            notes.push(format!(
                "skipped {name:?} on the small screen (col {col}, row {row}) - not supported yet"
            ));
            continue;
        }
        let pos = Pos { row, col };

        // Image and title of the state the key is currently in.
        let states = a
            .get("States")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let idx = a.get("State").and_then(Value::as_u64).unwrap_or(0) as usize;
        let state = states
            .get(idx)
            .or_else(|| states.first())
            .cloned()
            .unwrap_or(Value::Null);
        let title = s(&state, "Title")
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        let icon = s(&state, "Image")
            .filter(|i| !i.is_empty())
            .and_then(|img| {
                let src = profile_dir.join("Images").join(img);
                let ext = Path::new(img).extension()?.to_str()?.to_ascii_lowercase();
                let dest = format!("{}.{ext}", s(a, "ActionID").unwrap_or(img));
                std::fs::copy(&src, icons_dir.join(&dest)).ok()?;
                Some(PathBuf::from("icons").join(dest))
            });

        let action = match convert(a) {
            Ok(action) => Some(action),
            Err(why) => {
                notes.push(format!("key {pos}: {why}"));
                None
            }
        };
        let title = title.or_else(|| {
            icon.is_none().then(|| match &action {
                Some(Action::Media(m)) => m.label().to_string(),
                Some(_) => name.to_string(),
                None => format!("{name}?"),
            })
        });
        keys_out.push(Key {
            pos,
            icon,
            icon_active: None,
            title,
            color: None,
            title_size: None,
            title_align: None,
            action,
        });
    }

    let config = Config {
        brightness: 60,
        home: "main".into(),
        font: None,
        pages: HashMap::from([("main".to_string(), keys_out)]),
    };
    Ok(Report { config, notes })
}
