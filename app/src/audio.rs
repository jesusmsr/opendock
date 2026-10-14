//! Windows audio endpoints: listing devices, reading and changing the default.
//!
//! Listing uses the documented MMDevice API. Changing the default device has
//! no public API; like every audio-switcher tool, we use the undocumented
//! `IPolicyConfig` interface that the Sound control panel itself uses.

// IPolicyConfig's methods keep their COM names.
#![allow(non_snake_case)]

use std::ffi::c_void;

use windows::core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Output,
    Input,
}

impl Flow {
    fn data_flow(self) -> EDataFlow {
        match self {
            Flow::Output => eRender,
            Flow::Input => eCapture,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
}

/// Initialises COM for the current thread for as long as it lives.
pub struct ComGuard(bool);

impl ComGuard {
    pub fn new() -> Self {
        // SAFETY: plain COM initialisation; S_FALSE (already initialised) also
        // needs a matching CoUninitialize, a mode mismatch does not.
        let ok = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        ComGuard(ok)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balanced with the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
    }
}

/// Undocumented; only the vtable slot order matters. Signatures of the methods
/// we never call are simplified to raw pointers.
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, a: *mut i64, b: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT;
    fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, id: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
    fn SetPropertyValue(&self, id: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
}

const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

fn enumerator() -> Result<IMMDeviceEnumerator, String> {
    // SAFETY: standard COM activation; requires COM to be initialised (ComGuard).
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
        .map_err(|e| format!("audio: cannot create device enumerator: {e}"))
}

/// Takes ownership of a COM-allocated string.
fn take_pwstr(p: PWSTR) -> String {
    // SAFETY: `p` was allocated by COM and is freed exactly once here.
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const c_void));
        s
    }
}

fn endpoint(device: &IMMDevice) -> windows::core::Result<Endpoint> {
    // SAFETY: COM calls on a valid device; returned strings are freed by take_pwstr.
    unsafe {
        let id = take_pwstr(device.GetId()?);
        let props = device.OpenPropertyStore(STGM_READ)?;
        let value = props.GetValue(&PKEY_Device_FriendlyName)?;
        let name = PropVariantToStringAlloc(&value)
            .map(take_pwstr)
            .unwrap_or_default();
        Ok(Endpoint { id, name })
    }
}

/// Active endpoints of the given flow.
pub fn list(flow: Flow) -> Result<Vec<Endpoint>, String> {
    let en = enumerator()?;
    // SAFETY: COM calls on valid interfaces.
    unsafe {
        let col = en
            .EnumAudioEndpoints(flow.data_flow(), DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("audio: cannot list devices: {e}"))?;
        let count = col.GetCount().map_err(|e| e.to_string())?;
        Ok((0..count)
            .filter_map(|i| col.Item(i).ok())
            .filter_map(|d| endpoint(&d).ok())
            .collect())
    }
}

/// Id of the current default endpoint.
pub fn default_id(flow: Flow) -> Option<String> {
    let en = enumerator().ok()?;
    // SAFETY: COM calls on valid interfaces.
    unsafe {
        let d = en
            .GetDefaultAudioEndpoint(flow.data_flow(), eMultimedia)
            .ok()?;
        d.GetId().ok().map(take_pwstr)
    }
}

/// Finds an endpoint by exact id, else by case-insensitive name substring.
pub fn resolve<'a>(endpoints: &'a [Endpoint], pattern: &str) -> Option<&'a Endpoint> {
    let p = pattern.to_lowercase();
    endpoints
        .iter()
        .find(|e| e.id.eq_ignore_ascii_case(pattern))
        .or_else(|| endpoints.iter().find(|e| e.name.to_lowercase() == p))
        .or_else(|| {
            endpoints
                .iter()
                .find(|e| e.name.to_lowercase().contains(&p))
        })
}

/// Makes `id` the default device for every role (games, media and calls).
pub fn set_default(id: &str) -> Result<(), String> {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    // SAFETY: COM activation of the policy client; `wide` outlives the calls.
    unsafe {
        let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
            .map_err(|e| format!("audio: cannot create policy client: {e}"))?;
        for role in [eConsole, eMultimedia, eCommunications] {
            policy
                .SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role)
                .ok()
                .map_err(|e| format!("audio: cannot set default device: {e}"))?;
        }
    }
    Ok(())
}

/// Switches to `targets[0]`, or with several targets to the one after the
/// current default (cycling).
pub fn switch(flow: Flow, targets: &[String]) -> Result<String, String> {
    let endpoints = list(flow)?;
    let resolved: Vec<&Endpoint> = targets
        .iter()
        .filter_map(|t| resolve(&endpoints, t))
        .collect();
    if resolved.is_empty() {
        let names: Vec<&str> = endpoints.iter().map(|e| e.name.as_str()).collect();
        return Err(format!(
            "audio: no device matches {targets:?}; available: {names:?}"
        ));
    }
    let current = default_id(flow);
    let next = match resolved
        .iter()
        .position(|e| Some(&e.id) == current.as_ref())
    {
        Some(i) => resolved[(i + 1) % resolved.len()],
        None => resolved[0],
    };
    set_default(&next.id)?;
    Ok(next.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(id: &str, name: &str) -> Endpoint {
        Endpoint {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn resolve_prefers_id_then_exact_then_substring() {
        let eps = [
            ep("{a}", "Speakers (Realtek)"),
            ep("{b}", "Headset Earphone (CORSAIR VIRTUOSO)"),
            ep("{c}", "Speakers"),
        ];
        assert_eq!(resolve(&eps, "{B}").unwrap().id, "{b}");
        assert_eq!(resolve(&eps, "speakers").unwrap().id, "{c}");
        assert_eq!(resolve(&eps, "corsair").unwrap().id, "{b}");
        assert!(resolve(&eps, "hdmi").is_none());
    }
}
