//! Native Windows notification: a tray balloon, which Windows 10/11 shows as
//! a regular toast with the app's name and icon. No extra dependency and no
//! extra memory (design §5.3, "Solo notificación").
//!
//! Slint owns the tray icon; its hidden message-only window has the class
//! `SlintSystemTrayWindow` and the icon id 1 (`i-slint-core` 1.19,
//! `items/system_tray/windows.rs`). If a future Slint changes that, `show`
//! returns `false` and the caller logs the text instead.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Shell::{
    NIF_INFO, NIIF_NOSOUND, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowThreadProcessId, HWND_MESSAGE,
};

const SLINT_TRAY_CLASS: &str = "SlintSystemTrayWindow";
const SLINT_TRAY_ICON_ID: u32 = 1;

/// Copies `text` into a fixed, null-terminated UTF-16 buffer as the
/// notification fields require, cutting it with "…" when it does not fit.
pub fn fixed_wide<const N: usize>(text: &str) -> [u16; N] {
    let mut buffer = [0u16; N];
    let capacity = N.saturating_sub(1); // the last unit stays 0
    if text.encode_utf16().count() <= capacity {
        for (slot, unit) in buffer.iter_mut().zip(text.encode_utf16()) {
            *slot = unit;
        }
        return buffer;
    }
    // Keep whole characters only, leaving one unit for the ellipsis.
    let mut used = 0;
    for ch in text.chars() {
        let mut units = [0u16; 2];
        let encoded = ch.encode_utf16(&mut units);
        if used + encoded.len() + 1 > capacity {
            break;
        }
        buffer[used..used + encoded.len()].copy_from_slice(encoded);
        used += encoded.len();
    }
    if used < capacity {
        buffer[used] = '…' as u16;
    }
    buffer
}

/// Whether Windows plays its own notification sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// No system sound: the app plays its configured one with its volume.
    Silent,
    /// The system notification sound, following the user's Windows settings.
    Windows,
}

/// The balloon flags for `sound`.
pub fn info_flags(sound: Sound) -> u32 {
    match sound {
        Sound::Silent => NIIF_NOSOUND,
        Sound::Windows => 0,
    }
}

/// Shows `title` and `message` as a Windows notification. `false` if the
/// tray icon could not be found or Windows refused.
pub fn show(title: &str, message: &str, sound: Sound) -> bool {
    let Some(window) = tray_window() else {
        return false;
    };
    // SAFETY: plain data; zeroed is a valid initial state.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = window;
    data.uID = SLINT_TRAY_ICON_ID;
    data.uFlags = NIF_INFO;
    data.szInfoTitle = fixed_wide(title);
    data.szInfo = fixed_wide(message);
    data.dwInfoFlags = info_flags(sound);
    // SAFETY: `data` is fully initialized and identifies an existing icon.
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) != 0 }
}

/// Slint's tray window in this process (other Slint apps may have one too).
fn tray_window() -> Option<HWND> {
    let class: Vec<u16> = SLINT_TRAY_CLASS.encode_utf16().chain([0]).collect();
    // SAFETY: no preconditions.
    let own_process = unsafe { GetCurrentProcessId() };
    let mut after: HWND = std::ptr::null_mut();
    loop {
        // SAFETY: `class` is null-terminated; `after` is null or a window
        // returned by the previous call.
        after = unsafe { FindWindowExW(HWND_MESSAGE, after, class.as_ptr(), std::ptr::null()) };
        if after.is_null() {
            return None;
        }
        let mut process = 0u32;
        // SAFETY: `after` is a window handle and `process` a valid out pointer.
        unsafe { GetWindowThreadProcessId(after, &mut process) };
        if process == own_process {
            return Some(after);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|c| *c == 0).unwrap();
        String::from_utf16(&buffer[..end]).unwrap()
    }

    #[test]
    fn a_silent_notification_asks_windows_for_no_sound() {
        assert_eq!(info_flags(Sound::Silent), NIIF_NOSOUND);
    }

    #[test]
    fn a_windows_sound_notification_keeps_the_system_sound() {
        assert_eq!(info_flags(Sound::Windows) & NIIF_NOSOUND, 0);
    }

    #[test]
    fn short_text_is_copied_and_null_terminated() {
        let buffer: [u16; 8] = fixed_wide("Hola");
        assert_eq!(text(&buffer), "Hola");
        assert_eq!(buffer[4], 0);
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis_and_still_terminated() {
        let buffer: [u16; 8] = fixed_wide("Descanso largo");
        assert_eq!(text(&buffer), "Descan…");
        assert_eq!(buffer[7], 0);
    }

    #[test]
    fn text_that_fits_exactly_is_not_cut() {
        let buffer: [u16; 8] = fixed_wide("Pausa!!");
        assert_eq!(text(&buffer), "Pausa!!");
    }

    #[test]
    fn a_character_outside_the_basic_plane_is_never_split() {
        // "😀" takes two UTF-16 units; cutting between them would corrupt it.
        let buffer: [u16; 5] = fixed_wide("ab😀cd");
        assert_eq!(text(&buffer), "ab…");
    }
}
