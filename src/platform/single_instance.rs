//! Only one Rustpira runs per user session (design §5.1). A second launch
//! asks the running instance to open its settings window, then exits.

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, OpenEventW, SetEvent, WaitForSingleObject,
};
use windows_sys::w;

pub enum Instance {
    /// This is the only instance; keep the guard alive while running.
    Primary(SingleInstance),
    /// Another instance is running and was asked to open its settings.
    Secondary,
}

pub struct SingleInstance {
    mutex: HANDLE,
    open_request: HANDLE,
}

pub fn acquire() -> Instance {
    // SAFETY: null security attributes and a static, NUL-terminated name.
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, w!("Local\\Rustpira.Instance")) };
    // SAFETY: no preconditions; reads the error of the call above.
    let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

    if already_running {
        // SAFETY: static name; the handle is closed right after use.
        unsafe {
            let event = OpenEventW(EVENT_MODIFY_STATE, 0, w!("Local\\Rustpira.OpenSettings"));
            if !event.is_null() {
                SetEvent(event);
                CloseHandle(event);
            }
            if !mutex.is_null() {
                CloseHandle(mutex);
            }
        }
        return Instance::Secondary;
    }

    // Auto-reset event, initially not signaled.
    // SAFETY: null security attributes and a static, NUL-terminated name.
    let open_request =
        unsafe { CreateEventW(std::ptr::null(), 0, 0, w!("Local\\Rustpira.OpenSettings")) };
    Instance::Primary(SingleInstance {
        mutex,
        open_request,
    })
}

impl SingleInstance {
    /// True once per request from a second launch.
    pub fn take_open_request(&self) -> bool {
        // SAFETY: the handle is owned by `self` (or null, which just fails).
        !self.open_request.is_null()
            && unsafe { WaitForSingleObject(self.open_request, 0) } == WAIT_OBJECT_0
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: handles are owned by `self` and closed exactly once.
        unsafe {
            if !self.open_request.is_null() {
                CloseHandle(self.open_request);
            }
            if !self.mutex.is_null() {
                CloseHandle(self.mutex);
            }
        }
    }
}
