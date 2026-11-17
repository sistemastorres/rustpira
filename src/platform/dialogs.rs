//! Native Windows dialogs for the settings window: open a file (background
//! image, custom sound) and pick a color. Slint has neither. They block the
//! calling thread, so callers run them on a worker thread (see
//! `settings::window`).

use std::path::{Path, PathBuf};

use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::UI::Controls::Dialogs::{
    CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW, GetOpenFileNameW, OFN_EXPLORER,
    OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

use crate::domain::config::Rgb;

/// Long enough for any path Windows can return through this dialog.
const PATH_CAPACITY: usize = 32_768;

/// COLORREF layout: 0x00BBGGRR.
pub fn colorref_from_rgb(color: Rgb) -> u32 {
    u32::from(color.r) | u32::from(color.g) << 8 | u32::from(color.b) << 16
}

pub fn rgb_from_colorref(value: u32) -> Rgb {
    Rgb {
        r: (value & 0xFF) as u8,
        g: (value >> 8 & 0xFF) as u8,
        b: (value >> 16 & 0xFF) as u8,
    }
}

/// The double-null-terminated filter `GetOpenFileNameW` expects, e.g.
/// "Imágenes (*.png;*.jpg)\0*.png;*.jpg\0\0".
pub fn filter_string(name: &str, patterns: &[&str]) -> Vec<u16> {
    let joined = patterns.join(";");
    format!("{name} ({joined})\0{joined}\0\0")
        .encode_utf16()
        .collect()
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// COM must be initialized on the thread that shows a shell dialog.
struct ComGuard;

impl ComGuard {
    fn new() -> Self {
        // SAFETY: balanced by CoUninitialize in Drop on the same thread.
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        Self
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: matches the CoInitializeEx in `new`.
        unsafe { CoUninitialize() };
    }
}

/// Shows the "Open" dialog; `None` if the user cancels. Starts in the
/// folder of `current` when it exists.
pub fn pick_file(
    title: &str,
    filter_name: &str,
    patterns: &[&str],
    current: &Path,
) -> Option<PathBuf> {
    let _com = ComGuard::new();
    let filter = filter_string(filter_name, patterns);
    let title = wide(title);
    let initial_dir = current
        .parent()
        .filter(|dir| dir.is_dir())
        .map(|dir| wide(&dir.to_string_lossy()));
    let mut file = vec![0u16; PATH_CAPACITY];

    // SAFETY: plain data; zeroed is a valid initial state.
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.nFilterIndex = 1;
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.lpstrTitle = title.as_ptr();
    dialog.lpstrInitialDir = initial_dir
        .as_ref()
        .map_or(std::ptr::null(), |d| d.as_ptr());
    dialog.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;

    // SAFETY: every pointer in `dialog` refers to a buffer alive until the
    // call returns.
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return None;
    }
    let end = file.iter().position(|c| *c == 0).unwrap_or(file.len());
    Some(PathBuf::from(String::from_utf16_lossy(&file[..end])))
}

/// Shows the color dialog starting at `initial`; `None` if cancelled.
pub fn pick_color(initial: Rgb) -> Option<Rgb> {
    let _com = ComGuard::new();
    // The dialog's 16 "custom colors" slots; white until the user sets them.
    let mut custom = [0x00FF_FFFFu32; 16];
    // SAFETY: plain data; zeroed is a valid initial state.
    let mut dialog: CHOOSECOLORW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = size_of::<CHOOSECOLORW>() as u32;
    dialog.rgbResult = colorref_from_rgb(initial);
    dialog.lpCustColors = custom.as_mut_ptr();
    dialog.Flags = CC_RGBINIT | CC_FULLOPEN;
    // SAFETY: `custom` holds the 16 entries the dialog requires.
    if unsafe { ChooseColorW(&mut dialog) } == 0 {
        return None;
    }
    Some(rgb_from_colorref(dialog.rgbResult))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_stores_red_in_the_low_byte() {
        let color = Rgb::parse("#123456").unwrap();
        assert_eq!(colorref_from_rgb(color), 0x0056_3412);
        assert_eq!(rgb_from_colorref(0x0056_3412), color);
    }

    #[test]
    fn colorref_ignores_the_high_byte() {
        assert_eq!(
            rgb_from_colorref(0xFF00_00FF),
            Rgb::parse("#FF0000").unwrap()
        );
    }

    #[test]
    fn filter_string_lists_name_then_patterns_with_double_null() {
        let filter = filter_string("Imágenes", &["*.png", "*.jpg"]);
        let expected: Vec<u16> = "Imágenes (*.png;*.jpg)\0*.png;*.jpg\0\0"
            .encode_utf16()
            .collect();
        assert_eq!(filter, expected);
    }
}
