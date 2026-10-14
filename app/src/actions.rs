//! Executes key actions through Win32: `SendInput` for keys and text,
//! `ShellExecuteW` for programs, files and URLs.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null;
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};

use crate::audio::{self, Flow};
use crate::config::{Action, Media, Run};
use crate::keys;

/// Navigation requested by an action; handled by the device loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nav {
    Page(String),
    Back,
    Home,
}

/// Splits an action into its navigation part (applied immediately by the
/// caller) and runs everything else.
pub fn navigation(action: &Action) -> Vec<Nav> {
    match action {
        Action::Page(p) => vec![Nav::Page(p.clone())],
        Action::Back => vec![Nav::Back],
        Action::Home => vec![Nav::Home],
        Action::Multi(list) => list.iter().flat_map(navigation).collect(),
        _ => Vec::new(),
    }
}

/// Runs the non-navigation parts of `action`. Blocking; call off the device thread.
pub fn execute(action: &Action) -> Result<(), String> {
    match action {
        Action::Run(Run::Path(path)) => shell_open(path, None, None, false),
        Action::Run(Run::Full {
            path,
            args,
            cwd,
            hidden,
        }) => shell_open(path, args.as_deref(), cwd.as_deref(), *hidden),
        Action::Url(url) => shell_open(url, None, None, false),
        Action::Hotkey(s) => send_hotkey(&keys::parse(s)?),
        Action::Media(m) => send_hotkey(&keys::Hotkey {
            modifiers: vec![],
            key: media_vk(*m),
        }),
        Action::Text(t) => send_text(t),
        Action::AudioOutput(t) => switch_audio(Flow::Output, &t.targets()),
        Action::AudioInput(t) => switch_audio(Flow::Input, &t.targets()),
        Action::Delay(ms) => {
            sleep(Duration::from_millis(*ms));
            Ok(())
        }
        Action::Multi(list) => list.iter().try_for_each(execute),
        Action::Page(_) | Action::Back | Action::Home => Ok(()),
    }
}

fn switch_audio(flow: Flow, targets: &[String]) -> Result<(), String> {
    let _com = audio::ComGuard::new();
    let name = audio::switch(flow, targets)?;
    let kind = if flow == Flow::Output {
        "output"
    } else {
        "input"
    };
    crate::log(&format!("default {kind} device is now {name:?}"));
    Ok(())
}

fn media_vk(m: Media) -> u16 {
    match m {
        Media::PlayPause => 0xB3,
        Media::Next => 0xB0,
        Media::Previous => 0xB1,
        Media::Stop => 0xB2,
        Media::Mute => 0xAD,
        Media::VolumeDown => 0xAE,
        Media::VolumeUp => 0xAF,
    }
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn shell_open(
    target: &str,
    args: Option<&str>,
    cwd: Option<&str>,
    hidden: bool,
) -> Result<(), String> {
    let verb = wide("open");
    let file = wide(target);
    let args = args.map(wide);
    let cwd = cwd.map(wide);
    let show = if hidden { SW_HIDE } else { SW_SHOWNORMAL };
    // SAFETY: all strings are NUL-terminated and outlive the call.
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            args.as_ref().map_or(null(), |a| a.as_ptr()),
            cwd.as_ref().map_or(null(), |c| c.as_ptr()),
            show,
        )
    };
    // ShellExecute returns a value > 32 on success.
    if r as isize > 32 {
        Ok(())
    } else {
        Err(format!(
            "could not open {target:?} (ShellExecute error {})",
            r as isize
        ))
    }
}

/// Keys that need KEYEVENTF_EXTENDEDKEY to be distinguished from numpad keys.
fn is_extended(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x5D | 0x6F | 0xAD..=0xB3)
}

fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    // SAFETY: `inputs` is a valid slice of INPUT structs.
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err("SendInput was blocked (is an elevated window focused?)".into())
    }
}

fn send_hotkey(h: &keys::Hotkey) -> Result<(), String> {
    let ext = |vk| {
        if is_extended(vk) {
            KEYEVENTF_EXTENDEDKEY
        } else {
            0
        }
    };
    let mut inputs = Vec::new();
    for &m in &h.modifiers {
        inputs.push(key_input(m, 0, ext(m)));
    }
    inputs.push(key_input(h.key, 0, ext(h.key)));
    inputs.push(key_input(h.key, 0, ext(h.key) | KEYEVENTF_KEYUP));
    for &m in h.modifiers.iter().rev() {
        inputs.push(key_input(m, 0, ext(m) | KEYEVENTF_KEYUP));
    }
    send(&inputs)
}

fn send_text(text: &str) -> Result<(), String> {
    let inputs: Vec<INPUT> = text
        .encode_utf16()
        .flat_map(|u| {
            [
                key_input(0, u, KEYEVENTF_UNICODE),
                key_input(0, u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect();
    send(&inputs)
}
