//! Monitor enumeration, so the break screen can cover every display.

use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows_sys::core::BOOL;

const MONITORINFOF_PRIMARY: u32 = 1;

/// A monitor rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

/// Lists the monitors. Coordinates are physical pixels because winit makes
/// the process per-monitor DPI aware before any window exists.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` is the `&mut Vec` passed below, alive for the whole
        // enumeration.
        let list = unsafe { &mut *(data as *mut Vec<Monitor>) };
        // SAFETY: plain data; zeroed is a valid initial state.
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = size_of::<MONITORINFO>() as u32;
        // SAFETY: `monitor` comes from the enumeration and `info` is sized.
        if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
            let r = info.rcMonitor;
            list.push(Monitor {
                x: r.left,
                y: r.top,
                width: (r.right - r.left).max(0) as u32,
                height: (r.bottom - r.top).max(0) as u32,
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        1
    }

    let mut list: Vec<Monitor> = Vec::new();
    // SAFETY: the callback only touches `list`, which outlives the call.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut list as *mut Vec<Monitor> as LPARAM,
        );
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_exactly_one_primary_monitor() {
        let all = monitors();
        assert!(!all.is_empty());
        assert_eq!(all.iter().filter(|m| m.primary).count(), 1);
    }
}
