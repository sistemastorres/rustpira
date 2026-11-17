//! Swallows the shortcuts that would let the user leave the break screen
//! (design §5.3): Windows key, Alt+Tab, Alt+F4, Alt+Space and Esc
//! (including Ctrl+Esc, Alt+Esc and Ctrl+Shift+Esc). Ctrl+Alt+Del cannot be
//! intercepted by any user application; that limit is accepted.
//!
//! Windows calls a low-level hook on the thread that installed it and
//! silently removes it if that thread does not answer within ~1 s. The UI
//! thread can be busy painting full-screen windows, so the hook lives on a
//! dedicated thread whose only job is to pump its messages.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_ESCAPE, VK_F4, VK_LWIN, VK_RWIN, VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MSG, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_QUIT, WM_USER,
};

const HC_ACTION: i32 = 0;

/// Key events the hook has seen; lets tests prove the hook is alive.
#[cfg(test)]
static HOOK_CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

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

/// While alive, the blocked shortcuts do nothing. Dropping it stops the
/// hook thread, which uninstalls the hook.
pub struct KeyboardBlock {
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
}

impl KeyboardBlock {
    /// Installs the hook; `None` if Windows refused (the break screen still
    /// works, only the shortcuts stay active).
    pub fn install() -> Option<Self> {
        let (ready, started) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("rustpira-keyboard".into())
            .spawn(move || run_hook_thread(&ready))
            .ok()?;
        match started.recv() {
            Ok(Some(thread_id)) => Some(Self {
                thread_id,
                worker: Some(worker),
            }),
            _ => {
                let _ = worker.join();
                None
            }
        }
    }
}

impl Drop for KeyboardBlock {
    fn drop(&mut self) {
        // SAFETY: posting to a thread id we own; its queue exists because the
        // thread created it before reporting ready.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Body of the hook thread: install, report, pump until WM_QUIT, uninstall.
fn run_hook_thread(ready: &mpsc::Sender<Option<u32>>) {
    // SAFETY: plain data; zeroed is a valid initial state.
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: forces the creation of this thread's message queue, so a
    // WM_QUIT posted right after "ready" is never lost.
    unsafe {
        PeekMessageW(
            &mut msg,
            std::ptr::null_mut(),
            WM_USER,
            WM_USER,
            PM_NOREMOVE,
        )
    };

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
    if hook.is_null() {
        let _ = ready.send(None);
        return;
    }
    // SAFETY: no preconditions.
    let _ = ready.send(Some(unsafe { GetCurrentThreadId() }));

    // SAFETY: `msg` is a valid MSG; GetMessageW returns 0 on WM_QUIT and -1
    // on error, both of which end the loop.
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {}

    // SAFETY: `hook` was returned by SetWindowsHookExW and is released once.
    unsafe { UnhookWindowsHookEx(hook) };
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION {
        #[cfg(test)]
        HOOK_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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

    /// Injects F24, a key no application uses.
    fn press_f24() {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_F24,
        };
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_F24,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [key(0), key(KEYEVENTF_KEYUP)];
        // SAFETY: the array outlives the call and the size matches INPUT.
        unsafe { SendInput(2, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    }

    // Windows silently removes a low-level hook whose thread does not answer
    // within ~1 s. The app's thread is busy painting full-screen windows, so
    // the hook must not depend on it. This test thread never pumps messages.
    #[test]
    fn hook_keeps_answering_while_the_installing_thread_is_busy() {
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};

        let block = KeyboardBlock::install().expect("hook installed");
        let before = HOOK_CALLS.load(Ordering::SeqCst);
        std::thread::spawn(press_f24);

        let deadline = Instant::now() + Duration::from_secs(3);
        while HOOK_CALLS.load(Ordering::SeqCst) == before && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            HOOK_CALLS.load(Ordering::SeqCst) > before,
            "the hook never answered"
        );
        drop(block);
    }

    #[test]
    fn ordinary_keys_pass_through() {
        assert!(!should_block(u32::from(b'A'), false));
        assert!(!should_block(u32::from(b'A'), true));
    }
}
