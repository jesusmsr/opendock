//! "Start with Windows": a per-user `Run` registry value that launches the
//! engine at sign-in. No admin rights needed.

use std::path::Path;

use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "opendock";
const ERROR_FILE_NOT_FOUND: u32 = 2;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The command line registered to run at sign-in.
pub fn command(config: &Path) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate opendock.exe: {e}"))?;
    let mut cmd = format!("\"{}\"", exe.display());
    if config != crate::config::default_path() {
        cmd += &format!(" --config \"{}\"", config.display());
    }
    Ok(cmd + " run")
}

/// The registered command, if any.
fn registered() -> Option<String> {
    let (key, name) = (wide(RUN_KEY), wide(VALUE_NAME));
    let mut buf = vec![0u16; 2048];
    let mut bytes = (buf.len() * 2) as u32;
    // SAFETY: valid NUL-terminated strings; `bytes` is the buffer size in bytes.
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if err != 0 {
        return None;
    }
    buf.truncate((bytes as usize / 2).saturating_sub(1)); // drop the trailing NUL
    Some(String::from_utf16_lossy(&buf))
}

pub fn is_enabled() -> bool {
    registered().is_some()
}

pub fn set(enabled: bool, config: &Path) -> Result<(), String> {
    let (key, name) = (wide(RUN_KEY), wide(VALUE_NAME));
    // SAFETY: valid NUL-terminated strings; data length is given in bytes.
    let err = unsafe {
        if enabled {
            let data = wide(&command(config)?);
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        } else {
            match RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) {
                ERROR_FILE_NOT_FOUND => 0,
                e => e,
            }
        }
    };
    if err == 0 {
        Ok(())
    } else {
        Err(format!(
            "could not update the Windows startup entry (error {err})"
        ))
    }
}

/// If start-up is enabled but points somewhere else (e.g. opendock.exe was
/// moved), re-register it with the current location.
pub fn refresh(config: &Path) {
    if let (Some(current), Ok(wanted)) = (registered(), command(config)) {
        if current != wanted {
            let _ = set(true, config);
        }
    }
}
