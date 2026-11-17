//! "Iniciar con Windows": a value in the user's Run key (design §5.7), so no
//! administrator rights are needed.

use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Rustpira";

/// The Run value for `exe`: the path in quotes, so spaces are safe.
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Registers (or unregisters) the running executable to start with
/// Windows. Enabling again refreshes the path, which matters for a portable
/// exe that was moved.
pub fn set(enabled: bool) -> Result<(), String> {
    let command = if enabled {
        let exe =
            std::env::current_exe().map_err(|e| format!("no se pudo ubicar el ejecutable: {e}"))?;
        Some(command_for(&exe))
    } else {
        None
    };
    write_value(RUN_KEY, VALUE_NAME, command.as_deref())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Writes `value` (REG_SZ) under `HKCU\subkey`, or deletes it when `None`.
fn write_value(subkey: &str, name: &str, value: Option<&str>) -> Result<(), String> {
    let failed = |code: u32| format!("no se pudo actualizar el inicio con Windows (error {code})");
    let (subkey, name) = (wide(subkey), wide(name));
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: valid null-terminated strings and out pointer.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(failed(status));
    }
    let status = match value {
        Some(value) => {
            let data = wide(value);
            // SAFETY: `data` holds `data.len() * 2` bytes, null terminator included.
            unsafe {
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            }
        }
        None => {
            // SAFETY: `key` is open with KEY_SET_VALUE.
            match unsafe { RegDeleteValueW(key, name.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                other => other,
            }
        }
    };
    // SAFETY: `key` was opened above and is closed once.
    unsafe { RegCloseKey(key) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(failed(status))
    }
}

/// Reads a REG_SZ value under `HKCU\subkey`.
#[cfg_attr(not(test), allow(dead_code))]
fn read_value(subkey: &str, name: &str) -> Option<String> {
    let (subkey, name) = (wide(subkey), wide(name));
    let mut bytes = 0u32;
    // SAFETY: a null buffer asks only for the size.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
    // SAFETY: `buffer` has room for `bytes` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};

    /// A throwaway key under HKCU, deleted on drop; tests never touch the
    /// real Run key.
    struct TestKey(String);

    impl TestKey {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let n = NEXT.fetch_add(1, Ordering::SeqCst);
            Self(format!(
                r"Software\rustpira-test-{}-{n}",
                std::process::id()
            ))
        }
    }

    impl Drop for TestKey {
        fn drop(&mut self) {
            let wide: Vec<u16> = self.0.encode_utf16().chain([0]).collect();
            // SAFETY: deletes only the throwaway key this test created.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide.as_ptr()) };
        }
    }

    #[test]
    fn command_quotes_paths_with_spaces() {
        assert_eq!(
            command_for(Path::new(r"C:\Program Files\Rustpira\rustpira.exe")),
            r#""C:\Program Files\Rustpira\rustpira.exe""#
        );
    }

    #[test]
    fn enabling_writes_the_value_and_disabling_removes_it() {
        let key = TestKey::new();
        write_value(&key.0, VALUE_NAME, Some(r#""C:\a b\rustpira.exe""#)).unwrap();
        assert_eq!(
            read_value(&key.0, VALUE_NAME).as_deref(),
            Some(r#""C:\a b\rustpira.exe""#)
        );
        write_value(&key.0, VALUE_NAME, None).unwrap();
        assert_eq!(read_value(&key.0, VALUE_NAME), None);
    }

    #[test]
    fn enabling_again_replaces_the_path() {
        let key = TestKey::new();
        write_value(&key.0, VALUE_NAME, Some("viejo")).unwrap();
        write_value(&key.0, VALUE_NAME, Some("nuevo")).unwrap();
        assert_eq!(read_value(&key.0, VALUE_NAME).as_deref(), Some("nuevo"));
    }

    #[test]
    fn disabling_when_nothing_is_registered_is_fine() {
        let key = TestKey::new();
        assert_eq!(write_value(&key.0, VALUE_NAME, None), Ok(()));
    }
}
