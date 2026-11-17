//! Reads the clock, local time, user inactivity and session lock state
//! from Windows, in the shape the domain expects.

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSFreeMemory, WTSINFOEXW,
    WTSQuerySessionInformationW, WTSSessionInfoEx,
};
use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetTickCount, GetTickCount64};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

use crate::domain::config::{TimeOfDay, Weekday};
use crate::domain::timer::Now;

/// `SessionFlags` value meaning the session is locked (Windows 10+).
const WTS_SESSIONSTATE_LOCK: i32 = 0;

/// The current moment. The monotonic part comes from `GetTickCount64`, which
/// keeps counting while the machine sleeps, so a resume shows up as a gap.
pub fn now() -> Now {
    // SAFETY: plain data out-parameter.
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    // SAFETY: `local` is a valid, writable SYSTEMTIME.
    unsafe { GetLocalTime(&mut local) };
    // SAFETY: no preconditions.
    let ticks_ms = unsafe { GetTickCount64() };
    Now {
        mono_secs: ticks_ms / 1000,
        weekday: weekday_from_system(local.wDayOfWeek),
        time: TimeOfDay::new(local.wHour as u8, local.wMinute as u8)
            .expect("Windows always reports a valid time of day"),
    }
}

/// Seconds since the last keyboard or mouse input in this session.
pub fn idle_secs() -> u64 {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is sized and writable.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return 0;
    }
    // SAFETY: no preconditions.
    let now_ms = unsafe { GetTickCount() };
    idle_secs_between(now_ms, info.dwTime)
}

/// Whether the Windows session is locked (Win+L, lock screen).
pub fn session_locked() -> bool {
    let mut buffer: *mut u16 = std::ptr::null_mut();
    let mut bytes: u32 = 0;
    // SAFETY: out-parameters are valid; the buffer is freed below.
    let ok = unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        )
    };
    if ok == 0 || buffer.is_null() {
        return false;
    }
    let locked = if bytes as usize >= size_of::<WTSINFOEXW>() {
        // SAFETY: Windows returned at least one WTSINFOEXW in `buffer`.
        let info = unsafe { &*(buffer as *const WTSINFOEXW) };
        // SAFETY: level 1 is the only level Windows returns for this class.
        info.Level == 1
            && unsafe { info.Data.WTSInfoExLevel1.SessionFlags } == WTS_SESSIONSTATE_LOCK
    } else {
        false
    };
    // SAFETY: `buffer` was allocated by WTSQuerySessionInformationW.
    unsafe { WTSFreeMemory(buffer.cast()) };
    locked
}

/// `SYSTEMTIME::wDayOfWeek` counts from Sunday = 0.
fn weekday_from_system(day_of_week: u16) -> Weekday {
    match day_of_week {
        0 => Weekday::Sunday,
        1 => Weekday::Monday,
        2 => Weekday::Tuesday,
        3 => Weekday::Wednesday,
        4 => Weekday::Thursday,
        5 => Weekday::Friday,
        _ => Weekday::Saturday,
    }
}

/// Both values are 32-bit millisecond tick counts that wrap every ~49 days.
fn idle_secs_between(now_ms: u32, last_input_ms: u32) -> u64 {
    u64::from(now_ms.wrapping_sub(last_input_ms)) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_day_of_week_starts_on_sunday() {
        assert_eq!(weekday_from_system(0), Weekday::Sunday);
        assert_eq!(weekday_from_system(1), Weekday::Monday);
        assert_eq!(weekday_from_system(5), Weekday::Friday);
        assert_eq!(weekday_from_system(6), Weekday::Saturday);
    }

    #[test]
    fn idle_time_survives_the_32_bit_tick_wraparound() {
        assert_eq!(idle_secs_between(10_500, 500), 10);
        assert_eq!(idle_secs_between(1_000, u32::MAX - 999), 2);
    }

    #[test]
    fn live_readings_are_sane() {
        let first = now();
        assert!(first.mono_secs > 0);
        assert!(idle_secs() < 365 * 24 * 3600);
        assert!(!session_locked(), "tests run in an unlocked session");
    }
}
