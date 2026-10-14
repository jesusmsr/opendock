//! Engine process coordination through named Windows objects:
//! a mutex marks the running engine, an event asks it to quit.

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, OpenMutexW, SetEvent, WaitForSingleObject,
    EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE,
};

const MUTEX_NAME: &str = "Local\\opendock-engine";
const QUIT_EVENT_NAME: &str = "Local\\opendock-engine-quit";
const WAIT_OBJECT_0: u32 = 0;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Held by the running engine for its whole life.
pub struct EngineLock {
    quit_event: HANDLE,
}

impl EngineLock {
    /// Returns None if another engine is already running.
    pub fn acquire() -> Option<Self> {
        let name = wide(MUTEX_NAME);
        let quit = wide(QUIT_EVENT_NAME);
        // SAFETY: valid NUL-terminated names. The mutex handle is intentionally
        // never closed so it lives exactly as long as the process.
        unsafe {
            CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if GetLastError() == ERROR_ALREADY_EXISTS {
                return None;
            }
            // Auto-reset event, initially not signalled.
            let quit_event = CreateEventW(std::ptr::null(), 0, 0, quit.as_ptr());
            Some(EngineLock { quit_event })
        }
    }

    /// True once another process called [`request_quit`].
    pub fn quit_requested(&self) -> bool {
        // SAFETY: valid event handle owned by self; zero timeout never blocks.
        !self.quit_event.is_null()
            && unsafe { WaitForSingleObject(self.quit_event, 0) } == WAIT_OBJECT_0
    }
}

/// Whether an engine process is running.
pub fn engine_running() -> bool {
    let name = wide(MUTEX_NAME);
    // SAFETY: valid name; the handle is closed right away.
    unsafe {
        let h = OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr());
        if h.is_null() {
            return false;
        }
        CloseHandle(h);
        true
    }
}

/// Asks the running engine to exit. Returns false if none is running.
pub fn request_quit() -> bool {
    let name = wide(QUIT_EVENT_NAME);
    // SAFETY: valid name; the handle is closed right away.
    unsafe {
        let h = OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr());
        if h.is_null() {
            return false;
        }
        let ok = SetEvent(h) != 0;
        CloseHandle(h);
        ok
    }
}
