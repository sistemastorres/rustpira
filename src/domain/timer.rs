//! Work timer: counts working time toward the next break (design §5.2).
//!
//! Pure state machine. The caller ticks it about once per second with the
//! current [`Now`] and the seconds since the last user input. It only counts
//! inside the working-hour slots of the current day, pauses on inactivity,
//! and restarts from zero when a slot begins, when activity returns after an
//! idle pause, after a disable period, and after sleep or session unlock.

use crate::domain::config::{Config, TimeOfDay, Weekday};

/// A tick gap larger than this means the machine was asleep (or the process
/// was frozen): treated like a resume, so the counter restarts.
pub const MAX_TICK_GAP_SECS: u64 = 60;

/// The current moment, as seen by the domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    /// Monotonic seconds (never goes backwards, unaffected by clock changes).
    pub mono_secs: u64,
    /// Local weekday.
    pub weekday: Weekday,
    /// Local time of day.
    pub time: TimeOfDay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerStatus {
    /// Counting toward the next break.
    Counting { remaining_secs: u64 },
    /// No slot of the current day covers the current time.
    OutsideHours,
    /// Inactivity threshold reached; restarts from zero on activity.
    IdlePaused,
    /// The Windows session is locked.
    Locked,
    /// Reminders disabled by the user until the given local time.
    Disabled { until: TimeOfDay },
    /// The interval elapsed; stays here until [`WorkTimer::restart`].
    BreakDue,
}

#[derive(Debug, Clone, Copy)]
struct DisabledUntil {
    mono_secs: u64,
    label: TimeOfDay,
}

#[derive(Debug, Default)]
pub struct WorkTimer {
    elapsed_secs: u64,
    last_tick: Option<u64>,
    in_hours: bool,
    idle_paused: bool,
    locked: bool,
    disabled: Option<DisabledUntil>,
}

impl WorkTimer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick(&mut self, config: &Config, now: Now, idle_secs: u64) -> TimerStatus {
        let mut delta = match self.last_tick {
            Some(previous) => now.mono_secs.saturating_sub(previous),
            None => 0,
        };
        self.last_tick = Some(now.mono_secs);
        if delta > MAX_TICK_GAP_SECS {
            self.elapsed_secs = 0;
            delta = 0;
        }

        if let Some(disabled) = self.disabled {
            if now.mono_secs < disabled.mono_secs {
                return TimerStatus::Disabled {
                    until: disabled.label,
                };
            }
            self.disabled = None;
            self.elapsed_secs = 0;
            delta = 0;
        }

        if self.locked {
            return TimerStatus::Locked;
        }

        let in_hours = config
            .schedule
            .active(now.weekday)
            .iter()
            .any(|slot| slot.start <= now.time && now.time < slot.end);
        if !in_hours {
            self.in_hours = false;
            return TimerStatus::OutsideHours;
        }
        if !self.in_hours {
            self.in_hours = true;
            self.elapsed_secs = 0;
            delta = 0;
        }

        if config.idle.enabled && idle_secs >= u64::from(config.idle.minutes) * 60 {
            self.idle_paused = true;
            return TimerStatus::IdlePaused;
        }
        if self.idle_paused {
            self.idle_paused = false;
            self.elapsed_secs = 0;
            delta = 0;
        }

        let interval = u64::from(config.interval_minutes) * 60;
        self.elapsed_secs = (self.elapsed_secs + delta).min(interval);
        if self.elapsed_secs >= interval {
            TimerStatus::BreakDue
        } else {
            TimerStatus::Counting {
                remaining_secs: interval - self.elapsed_secs,
            }
        }
    }

    /// Starts counting the interval from zero (after a break, a cancel, or a
    /// configuration change).
    pub fn restart(&mut self) {
        self.elapsed_secs = 0;
    }

    /// "Desactivar recordatorios": stop counting for `minutes`.
    pub fn disable_for(&mut self, now: Now, minutes: u32) {
        let end_minute = (u32::from(now.time.minutes()) + minutes) % (24 * 60);
        let label = TimeOfDay::new((end_minute / 60) as u8, (end_minute % 60) as u8)
            .expect("minute of day is always a valid time");
        self.disabled = Some(DisabledUntil {
            mono_secs: now.mono_secs + u64::from(minutes) * 60,
            label,
        });
    }

    /// "Reactivar ahora": end a disable period and count from zero.
    pub fn reactivate(&mut self) {
        self.disabled = None;
        self.elapsed_secs = 0;
    }

    /// The machine resumed from sleep or hibernation.
    pub fn on_resume(&mut self) {
        self.elapsed_secs = 0;
    }

    pub fn on_session_lock(&mut self) {
        self.locked = true;
    }

    pub fn on_session_unlock(&mut self) {
        self.locked = false;
        self.elapsed_secs = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::TimeSlot;

    const MIN: u64 = 60;

    /// Simulated clock: monotonic seconds plus local weekday and time.
    struct Clock {
        mono: u64,
        weekday: Weekday,
        secs_of_day: u64,
    }

    impl Clock {
        fn at(weekday: Weekday, hour: u64, minute: u64) -> Self {
            Self {
                mono: 1_000,
                weekday,
                secs_of_day: hour * 3600 + minute * 60,
            }
        }

        fn now(&self) -> Now {
            let minutes = self.secs_of_day / 60;
            Now {
                mono_secs: self.mono,
                weekday: self.weekday,
                time: TimeOfDay::new((minutes / 60) as u8, (minutes % 60) as u8).unwrap(),
            }
        }

        /// Moves both clocks forward without ticking (e.g. sleep).
        fn jump(&mut self, secs: u64) {
            self.mono += secs;
            self.secs_of_day += secs;
        }
    }

    /// Ticks now, then once per second for `secs` seconds, with a constant
    /// idle time. Returns the last status.
    fn run(t: &mut WorkTimer, c: &Config, clock: &mut Clock, secs: u64, idle: u64) -> TimerStatus {
        let mut status = t.tick(c, clock.now(), idle);
        for _ in 0..secs {
            clock.jump(1);
            status = t.tick(c, clock.now(), idle);
        }
        status
    }

    fn time(h: u8, m: u8) -> TimeOfDay {
        TimeOfDay::new(h, m).unwrap()
    }

    fn slot(sh: u8, sm: u8, eh: u8, em: u8) -> TimeSlot {
        TimeSlot::new(time(sh, sm), time(eh, em))
    }

    fn counting(remaining_secs: u64) -> TimerStatus {
        TimerStatus::Counting { remaining_secs }
    }

    #[test]
    fn counts_down_during_work_hours() {
        let c = Config::default();
        let mut clock = Clock::at(Weekday::Monday, 10, 0);
        let mut t = WorkTimer::new();
        assert_eq!(run(&mut t, &c, &mut clock, 10 * MIN, 0), counting(40 * MIN));
    }

    #[test]
    fn break_is_due_after_the_interval_and_holds_until_restart() {
        let c = Config::default();
        let mut clock = Clock::at(Weekday::Monday, 10, 0);
        let mut t = WorkTimer::new();
        assert_eq!(
            run(&mut t, &c, &mut clock, 50 * MIN, 0),
            TimerStatus::BreakDue
        );
        assert_eq!(run(&mut t, &c, &mut clock, 5, 0), TimerStatus::BreakDue);

        t.restart();
        assert_eq!(t.tick(&c, clock.now(), 0), counting(50 * MIN));
    }

    #[test]
    fn does_not_count_outside_work_hours() {
        let c = Config::default();
        let mut t = WorkTimer::new();
        let mut evening = Clock::at(Weekday::Monday, 19, 0);
        assert_eq!(
            run(&mut t, &c, &mut evening, 60, 0),
            TimerStatus::OutsideHours
        );
        let mut saturday = Clock::at(Weekday::Saturday, 10, 0);
        assert_eq!(
            run(&mut t, &c, &mut saturday, 60, 0),
            TimerStatus::OutsideHours
        );
    }

    #[test]
    fn disabled_day_counts_as_outside_work_hours() {
        let mut c = Config::default();
        c.schedule.set_enabled(Weekday::Monday, false);
        let mut t = WorkTimer::new();
        let mut monday = Clock::at(Weekday::Monday, 10, 0);
        assert_eq!(
            run(&mut t, &c, &mut monday, 60, 0),
            TimerStatus::OutsideHours
        );
        let mut tuesday = Clock::at(Weekday::Tuesday, 10, 0);
        assert_eq!(run(&mut t, &c, &mut tuesday, 0, 0), counting(50 * MIN));
    }

    #[test]
    fn each_slot_start_restarts_the_counter() {
        let mut c = Config::default();
        c.schedule.monday = vec![slot(9, 0, 13, 0), slot(14, 0, 18, 0)];
        let mut clock = Clock::at(Weekday::Monday, 12, 30);
        let mut t = WorkTimer::new();
        assert_eq!(
            run(&mut t, &c, &mut clock, 30 * MIN - 1, 0), // until 12:59:59
            counting(20 * MIN + 1)
        );
        assert_eq!(
            run(&mut t, &c, &mut clock, 60 * MIN, 0), // until 13:59:59
            TimerStatus::OutsideHours
        );
        // 14:00:00 enters the afternoon slot, counting from zero.
        assert_eq!(run(&mut t, &c, &mut clock, 1, 0), counting(50 * MIN));
    }

    #[test]
    fn idle_threshold_pauses_and_activity_restarts_from_zero() {
        let c = Config::default(); // idle detection enabled, 5 min
        let mut clock = Clock::at(Weekday::Monday, 10, 0);
        let mut t = WorkTimer::new();
        run(&mut t, &c, &mut clock, 20 * MIN, 0);

        // Just below the threshold: still counting.
        assert_eq!(
            run(&mut t, &c, &mut clock, 1, 5 * MIN - 1),
            counting(30 * MIN - 1)
        );
        // At the threshold: stops counting.
        assert_eq!(
            run(&mut t, &c, &mut clock, 10 * MIN, 5 * MIN),
            TimerStatus::IdlePaused
        );
        // Activity again: counts from the beginning.
        assert_eq!(run(&mut t, &c, &mut clock, 0, 0), counting(50 * MIN));
    }

    #[test]
    fn idle_is_ignored_when_detection_is_disabled() {
        let mut c = Config::default();
        c.idle.enabled = false;
        let mut clock = Clock::at(Weekday::Monday, 10, 0);
        let mut t = WorkTimer::new();
        assert_eq!(
            run(&mut t, &c, &mut clock, 10 * MIN, 60 * MIN),
            counting(40 * MIN)
        );
    }

    #[test]
    fn disable_for_stops_counting_until_the_deadline_then_restarts() {
        let c = Config::default();
        let mut clock = Clock::at(Weekday::Monday, 10, 0);
        let mut t = WorkTimer::new();
        run(&mut t, &c, &mut clock, 20 * MIN, 0); // now 10:20

        t.disable_for(clock.now(), 15);
        assert_eq!(
            run(&mut t, &c, &mut clock, 15 * MIN - 1, 0),
            TimerStatus::Disabled {
                until: time(10, 35)
            }
        );
        assert_eq!(run(&mut t, &c, &mut clock, 1, 0), counting(50 * MIN));
    }

    #[test]
    fn disable_label_wraps_past_midnight() {
        let c = Config::default();
        let clock = Clock::at(Weekday::Monday, 23, 0);
        let mut t = WorkTimer::new();
        t.disable_for(clock.now(), 4 * 60);
        assert_eq!(
            t.tick(&c, clock.now(), 0),
            TimerStatus::Disabled { until: time(3, 0) }
        );
    }

    #[test]
    fn reactivate_ends_the_disable_period_and_counts_from_zero() {
        let c = Config::default();
        let mut clock = Clock::at(Weekday::Monday, 10, 0);
        let mut t = WorkTimer::new();
        run(&mut t, &c, &mut clock, 20 * MIN, 0);
        t.disable_for(clock.now(), 60);
        run(&mut t, &c, &mut clock, 5 * MIN, 0);

        t.reactivate();
        assert_eq!(t.tick(&c, clock.now(), 0), counting(50 * MIN));
    }

    #[test]
    fn resume_from_sleep_restarts_from_zero() {
        let c = Config::default();
        let mut clock = Clock::at(Weekday::Monday, 10, 0);
        let mut t = WorkTimer::new();
        run(&mut t, &c, &mut clock, 20 * MIN, 0);
        t.on_resume();
        assert_eq!(t.tick(&c, clock.now(), 0), counting(50 * MIN));
    }

    #[test]
    fn a_large_tick_gap_is_treated_as_sleep() {
        let c = Config::default();
        let mut clock = Clock::at(Weekday::Monday, 10, 0);
        let mut t = WorkTimer::new();
        run(&mut t, &c, &mut clock, 20 * MIN, 0);
        clock.jump(MAX_TICK_GAP_SECS + 1);
        assert_eq!(t.tick(&c, clock.now(), 0), counting(50 * MIN));
    }

    #[test]
    fn session_lock_pauses_and_unlock_restarts_from_zero() {
        let c = Config::default();
        let mut clock = Clock::at(Weekday::Monday, 10, 0);
        let mut t = WorkTimer::new();
        run(&mut t, &c, &mut clock, 20 * MIN, 0);

        t.on_session_lock();
        assert_eq!(run(&mut t, &c, &mut clock, 30, 0), TimerStatus::Locked);
        t.on_session_unlock();
        assert_eq!(run(&mut t, &c, &mut clock, 0, 0), counting(50 * MIN));
    }
}
