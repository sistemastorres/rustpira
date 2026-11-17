//! Swallows the shortcuts that would let the user leave the break screen
//! (design §5.3): Windows key, Alt+Tab, Alt+F4, Alt+Space and Esc
//! (including Ctrl+Esc, Alt+Esc and Ctrl+Shift+Esc). Ctrl+Alt+Del cannot be
//! intercepted by any user application; that limit is accepted.
//!
//! A low-level hook runs on the installing thread's message loop, which is
//! the Slint/winit event loop here.

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_ESCAPE, VK_F4, VK_LWIN, VK_RWIN, VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL,
};

const HC_ACTION: i32 = 0;

/// Whether a key press must be swallowed while a break is on screen.
pub fn should_block(virtual_key: u32, alt_down: bool) -> bool {
    let key = |vk: u16| u32::from(vk);
    match virtual_key {
        vk if vk == key(VK_LWIN) || vk == key(VK_RWIN) => true,
        vk if vk == key(VK_ESCAPE) => true,
        vk if vk == key(VK_TAB) || vk == key(VK_F4) || vk == key(VK_SPACE) => alt_down,
        _ => false,
    }
}

/// While alive, the blocked shortcuts do nothing. Dropping it uninstalls
/// the hook.
pub struct KeyboardBlock {
    hook: HHOOK,
}

impl KeyboardBlock {
    /// Installs the hook; `None` if Windows refused (the break screen still
    /// works, only the shortcuts stay active).
    pub fn install() -> Option<Self> {
        // SAFETY: the callback is a valid `extern "system"` fn for the whole
        // program, and the module handle of the executable is valid.
        let hook = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(hook_proc),
                GetModuleHandleW(std::ptr::null()),
                0,
            )
        };
        (!hook.is_null()).then_some(Self { hook })
    }
}

impl Drop for KeyboardBlock {
    fn drop(&mut self) {
        // SAFETY: `hook` was returned by SetWindowsHookExW and is released once.
        unsafe { UnhookWindowsHookEx(self.hook) };
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a
        // KBDLLHOOKSTRUCT owned by Windows for the duration of the call.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if should_block(info.vkCode, info.flags & LLKHF_ALTDOWN != 0) {
            return 1;
        }
    }
    // SAFETY: forwarding the unmodified arguments to the next hook.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vk(key: u16) -> u32 {
        u32::from(key)
    }

    #[test]
    fn windows_keys_and_escape_are_always_blocked() {
        for key in [VK_LWIN, VK_RWIN, VK_ESCAPE] {
            assert!(should_block(vk(key), false), "{key}");
            assert!(should_block(vk(key), true), "{key}");
        }
    }

    #[test]
    fn tab_f4_and_space_are_blocked_only_with_alt() {
        for key in [VK_TAB, VK_F4, VK_SPACE] {
            assert!(should_block(vk(key), true), "Alt+{key}");
            assert!(!should_block(vk(key), false), "{key} alone");
        }
    }

    #[test]
    fn ordinary_keys_pass_through() {
        assert!(!should_block(u32::from(b'A'), false));
        assert!(!should_block(u32::from(b'A'), true));
    }
}
