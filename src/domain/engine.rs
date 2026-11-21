//! Break engine: combines the work timer with the break rules (design §5.3,
//! §5.4) and produces what the UI must show.
//!
//! Each call returns one-shot [`Effect`]s (play a sound, persist the cancel
//! counter...), and [`Engine::snapshot`] describes the steady state: tooltip
//! text, the break screen to display (if any) and which menu items are
//! enabled. The UI only reflects this; it makes no decisions.

use crate::domain::config::{Config, NoticeKind};
use crate::domain::timer::{Now, TimerStatus, WorkTimer};

/// What the break screen must show while a break is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakView {
    pub remaining_secs: u64,
    /// Length of the whole break, for the progress bar.
    pub total_secs: u64,
    /// Hard lock: no buttons at all until the countdown ends.
    pub hard: bool,
    pub can_cancel: bool,
    pub can_postpone: bool,
    /// "Terminar descanso": replaces "Cancelar" once enough of the break
    /// went by; it ends the break as completed.
    pub can_finish: bool,
    pub postpone_minutes: u32,
}

impl BreakView {
    /// Share of the break still ahead, from 1.0 (just started) to 0.0, for
    /// the descending progress bar.
    pub fn fraction_left(&self) -> f32 {
        if self.total_secs == 0 {
            return 0.0;
        }
        (self.remaining_secs.min(self.total_secs) as f32) / (self.total_secs as f32)
    }
}

/// Enabled state of the tray menu items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuState {
    pub settings_enabled: bool,
    pub disable_enabled: bool,
    pub quit_enabled: bool,
    /// "Reactivar ahora" is only shown while reminders are disabled.
    pub show_reactivate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub tooltip: String,
    pub break_view: Option<BreakView>,
    pub menu: MenuState,
}

/// One-shot consequences of a tick or a user action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// A break screen appeared (play the start sound).
    BreakStarted,
    /// The break screen went away; `completed` is true when the countdown
    /// finished (play the end sound if configured).
    BreakEnded { completed: bool },
    /// Notification-only mode: show a Windows notification with sound.
    ShowNotification,
    /// The consecutive-cancel counter changed and must be persisted.
    CancelCountChanged(u8),
    /// Activity came back after an idle pause; the interval started over.
    IdleEnded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Working,
    OnBreak {
        ends_at: u64,
        hard: bool,
        postpones_used: u8,
    },
    Postponed {
        until: u64,
        postpones_used: u8,
    },
}

#[derive(Debug)]
pub struct Engine {
    timer: WorkTimer,
    phase: Phase,
    consecutive_cancels: u8,
    last_status: TimerStatus,
}

impl Engine {
    /// `consecutive_cancels` is the persisted counter, so restarting the app
    /// does not reset it.
    pub fn new(consecutive_cancels: u8) -> Self {
        Self {
            timer: WorkTimer::new(),
            phase: Phase::Working,
            consecutive_cancels,
            last_status: TimerStatus::OutsideHours,
        }
    }

    pub fn consecutive_cancels(&self) -> u8 {
        self.consecutive_cancels
    }

    pub fn tick(&mut self, config: &Config, now: Now, idle_secs: u64) -> Vec<Effect> {
        match self.phase {
            // The countdown uses the monotonic clock, so time asleep counts
            // as rest (design §5.2).
            Phase::OnBreak { ends_at, .. } if now.mono_secs >= ends_at => {
                self.complete_break(config)
            }
            Phase::OnBreak { .. } => Vec::new(),
            Phase::Postponed {
                until,
                postpones_used,
            } if now.mono_secs >= until => self.start_break(config, now, postpones_used),
            Phase::Postponed { .. } => Vec::new(),
            Phase::Working => {
                let was_idle = self.last_status == TimerStatus::IdlePaused;
                self.last_status = self.timer.tick(config, now, idle_secs);
                let mut effects = Vec::new();
                if was_idle && matches!(self.last_status, TimerStatus::Counting { .. }) {
                    effects.push(Effect::IdleEnded);
                }
                effects.extend(self.count_rest(config));
                if self.last_status != TimerStatus::BreakDue {
                    return effects;
                }
                match config.notice {
                    NoticeKind::Lock => effects.extend(self.start_break(config, now, 0)),
                    NoticeKind::Notification => {
                        self.restart_interval(config);
                        effects.push(Effect::ShowNotification);
                    }
                }
                effects
            }
        }
    }

    pub fn snapshot(&self, config: &Config, now: Now) -> Snapshot {
        let rules = &config.breaks;
        let break_view = match self.phase {
            Phase::OnBreak {
                ends_at,
                hard,
                postpones_used,
            } => {
                let remaining_secs = ends_at.saturating_sub(now.mono_secs);
                let can_finish = can_finish(config, hard, remaining_secs);
                Some(BreakView {
                    remaining_secs,
                    total_secs: break_secs(config),
                    hard,
                    can_cancel: rules.allow_cancel && !hard && !can_finish,
                    can_postpone: !hard && !can_finish && postpones_used < rules.max_postpones,
                    can_finish,
                    postpone_minutes: rules.postpone_minutes,
                })
            }
            _ => None,
        };

        let tooltip = match self.phase {
            Phase::OnBreak { .. } => "Descanso en curso".to_string(),
            Phase::Postponed { until, .. } => format!(
                "Descanso pospuesto: vuelve en {}",
                mm_ss(until.saturating_sub(now.mono_secs))
            ),
            Phase::Working => match self.last_status {
                TimerStatus::Counting { remaining_secs } => {
                    format!("Siguiente descanso en {}", hh_mm_ss(remaining_secs))
                }
                TimerStatus::OutsideHours => "Fuera del horario de trabajo".to_string(),
                TimerStatus::IdlePaused => "En pausa por inactividad".to_string(),
                TimerStatus::Locked => "En pausa: sesión bloqueada".to_string(),
                TimerStatus::Disabled { until } => {
                    format!("Recordatorios desactivados hasta las {until}")
                }
                TimerStatus::BreakDue => "Descanso en curso".to_string(),
            },
        };

        let on_break = break_view.is_some();
        Snapshot {
            tooltip,
            break_view,
            menu: MenuState {
                settings_enabled: !on_break,
                disable_enabled: !on_break,
                quit_enabled: !on_break,
                show_reactivate: self.phase == Phase::Working
                    && matches!(self.last_status, TimerStatus::Disabled { .. }),
            },
        }
    }

    /// "Cancelar descanso". Ignored when not allowed.
    pub fn cancel(&mut self, config: &Config) -> Vec<Effect> {
        let Phase::OnBreak { hard: false, .. } = self.phase else {
            return Vec::new();
        };
        if !config.breaks.allow_cancel {
            return Vec::new();
        }
        self.consecutive_cancels = self.consecutive_cancels.saturating_add(1);
        self.restart_interval(config);
        vec![
            Effect::BreakEnded { completed: false },
            Effect::CancelCountChanged(self.consecutive_cancels),
        ]
    }

    /// "Posponer M min". Ignored when not allowed.
    pub fn postpone(&mut self, config: &Config, now: Now) -> Vec<Effect> {
        let Phase::OnBreak {
            hard: false,
            postpones_used,
            ..
        } = self.phase
        else {
            return Vec::new();
        };
        if postpones_used >= config.breaks.max_postpones {
            return Vec::new();
        }
        self.phase = Phase::Postponed {
            until: now.mono_secs + u64::from(config.breaks.postpone_minutes) * 60,
            postpones_used: postpones_used + 1,
        };
        vec![Effect::BreakEnded { completed: false }]
    }

    /// "Terminar descanso". Ignored when not offered.
    pub fn finish(&mut self, config: &Config, now: Now) -> Vec<Effect> {
        let Phase::OnBreak { ends_at, hard, .. } = self.phase else {
            return Vec::new();
        };
        if !can_finish(config, hard, ends_at.saturating_sub(now.mono_secs)) {
            return Vec::new();
        }
        self.complete_break(config)
    }

    /// "Desactivar recordatorios". Ignored while a break is on screen; drops
    /// a postponed break.
    pub fn disable_for(&mut self, now: Now, minutes: u32) {
        if matches!(self.phase, Phase::OnBreak { .. }) {
            return;
        }
        self.phase = Phase::Working;
        self.timer.disable_for(now, minutes);
    }

    /// "Reactivar ahora".
    pub fn reactivate(&mut self) {
        self.timer.reactivate();
    }

    /// Double click set to "Empezar un descanso": shows the break screen
    /// now, whatever the notice mode. Ignored while a break is on screen.
    pub fn start_break_now(&mut self, config: &Config, now: Now) -> Vec<Effect> {
        match self.phase {
            Phase::OnBreak { .. } => Vec::new(),
            Phase::Postponed { postpones_used, .. } => {
                self.start_break(config, now, postpones_used)
            }
            Phase::Working => self.start_break(config, now, 0),
        }
    }

    /// Double click set to "Pausar 30 minutos": pauses the reminders, or
    /// reactivates them if they are paused.
    pub fn toggle_pause(&mut self, now: Now, minutes: u32) {
        if self.timer.is_disabled(now) {
            self.reactivate();
        } else {
            self.disable_for(now, minutes);
        }
    }

    /// The configuration was saved: restart the interval.
    pub fn config_changed(&mut self) {
        self.timer.restart();
    }

    pub fn on_resume(&mut self) {
        self.timer.on_resume();
    }

    pub fn on_session_lock(&mut self) {
        self.timer.on_session_lock();
    }

    pub fn on_session_unlock(&mut self) {
        self.timer.on_session_unlock();
    }

    fn start_break(&mut self, config: &Config, now: Now, postpones_used: u8) -> Vec<Effect> {
        let rules = &config.breaks;
        self.phase = Phase::OnBreak {
            ends_at: now.mono_secs + u64::from(config.break_minutes) * 60,
            hard: rules.limit_cancels && self.consecutive_cancels >= rules.cancel_limit,
            postpones_used,
        };
        vec![Effect::BreakStarted]
    }

    /// Time away from the computer (sleep, locked session, idle pause) at
    /// least as long as a break counts as one: the cancels start over.
    fn count_rest(&mut self, config: &Config) -> Option<Effect> {
        let rested = self.timer.take_rested_secs();
        if rested < break_secs(config) || self.consecutive_cancels == 0 {
            return None;
        }
        self.consecutive_cancels = 0;
        Some(Effect::CancelCountChanged(0))
    }

    fn complete_break(&mut self, config: &Config) -> Vec<Effect> {
        self.restart_interval(config);
        let mut effects = vec![Effect::BreakEnded { completed: true }];
        if self.consecutive_cancels != 0 {
            self.consecutive_cancels = 0;
            effects.push(Effect::CancelCountChanged(0));
        }
        effects
    }

    /// Back to work with a full interval ahead.
    fn restart_interval(&mut self, config: &Config) {
        self.phase = Phase::Working;
        self.timer.restart();
        self.last_status = TimerStatus::Counting {
            remaining_secs: u64::from(config.interval_minutes) * 60,
        };
    }
}

fn break_secs(config: &Config) -> u64 {
    u64::from(config.break_minutes) * 60
}

/// "Terminar descanso" is offered once `finish_percent` of the break went by,
/// only where cancelling is allowed and never in a hard lock (S25).
fn can_finish(config: &Config, hard: bool, remaining_secs: u64) -> bool {
    let rules = &config.breaks;
    let total = break_secs(config);
    let elapsed = total.saturating_sub(remaining_secs);
    !hard
        && rules.allow_cancel
        && rules.finish_percent < 100
        && elapsed * 100 >= total * u64::from(rules.finish_percent)
}

fn hh_mm_ss(secs: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

fn mm_ss(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one field of the defaults
mod tests {
    use super::*;
    use crate::domain::config::{TimeOfDay, Weekday};

    /// The configuration these tests were written against, independent of
    /// the product defaults: 50-minute interval, 5-minute break, 9:00-18:00
    /// on weekdays, idle pause after 5 minutes, cancel allowed with no limit.
    fn test_config() -> Config {
        let mut c = Config::default();
        c.interval_minutes = 50;
        c.break_minutes = 5;
        c.notice = crate::domain::config::NoticeKind::Lock;
        c.breaks.allow_cancel = true;
        c.breaks.limit_cancels = false;
        c.breaks.cancel_limit = 3;
        c.breaks.postpone_minutes = 5;
        c.breaks.max_postpones = 1;
        c.breaks.finish_percent = 100;
        c.idle.enabled = true;
        c.idle.minutes = 5;
        c
    }

    const MIN: u64 = 60;

    struct Clock {
        mono: u64,
        secs_of_day: u64,
    }

    impl Clock {
        /// Monday 10:00, inside the default working hours.
        fn new() -> Self {
            Self {
                mono: 1_000,
                secs_of_day: 10 * 3600,
            }
        }

        fn now(&self) -> Now {
            let minutes = self.secs_of_day / 60;
            Now {
                mono_secs: self.mono,
                weekday: Weekday::Monday,
                time: TimeOfDay::new((minutes / 60) as u8, (minutes % 60) as u8).unwrap(),
            }
        }

        /// Moves time forward without ticking (e.g. sleep).
        fn jump(&mut self, secs: u64) {
            self.mono += secs;
            self.secs_of_day += secs;
        }
    }

    /// Ticks now and then once per second for `secs`, collecting effects.
    fn run(e: &mut Engine, c: &Config, clock: &mut Clock, secs: u64) -> Vec<Effect> {
        let mut effects = e.tick(c, clock.now(), 0);
        for _ in 0..secs {
            clock.jump(1);
            effects.extend(e.tick(c, clock.now(), 0));
        }
        effects
    }

    /// Works a full interval so the next break appears.
    fn reach_break(e: &mut Engine, c: &Config, clock: &mut Clock) -> Vec<Effect> {
        run(e, c, clock, u64::from(c.interval_minutes) * MIN)
    }

    fn view(e: &Engine, c: &Config, clock: &Clock) -> Option<BreakView> {
        e.snapshot(c, clock.now()).break_view
    }

    fn tooltip(e: &Engine, c: &Config, clock: &Clock) -> String {
        e.snapshot(c, clock.now()).tooltip
    }

    #[test]
    fn lock_mode_shows_the_break_when_the_interval_elapses() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);

        let effects = reach_break(&mut e, &c, &mut clock);

        assert_eq!(effects, vec![Effect::BreakStarted]);
        assert_eq!(
            view(&e, &c, &clock),
            Some(BreakView {
                remaining_secs: 5 * MIN,
                total_secs: 5 * MIN,
                hard: false,
                can_cancel: true,
                can_postpone: true,
                can_finish: false,
                postpone_minutes: 5,
            })
        );
    }

    #[test]
    fn progress_bar_goes_down_from_full_to_empty() {
        let c = test_config(); // 5-minute break
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);
        assert_eq!(view(&e, &c, &clock).unwrap().fraction_left(), 1.0);

        run(&mut e, &c, &mut clock, 150);
        assert_eq!(view(&e, &c, &clock).unwrap().fraction_left(), 0.5);

        run(&mut e, &c, &mut clock, 149);
        let last = view(&e, &c, &clock).unwrap();
        assert_eq!(last.remaining_secs, 1);
        assert!(last.fraction_left() > 0.0 && last.fraction_left() < 0.01);
    }

    #[test]
    fn progress_of_an_empty_break_is_zero() {
        let view = BreakView {
            remaining_secs: 0,
            total_secs: 0,
            hard: false,
            can_cancel: false,
            can_postpone: false,
            can_finish: false,
            postpone_minutes: 5,
        };
        assert_eq!(view.fraction_left(), 0.0);
    }

    #[test]
    fn the_break_ends_by_itself_and_the_next_interval_starts() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);

        let effects = run(&mut e, &c, &mut clock, 5 * MIN);

        assert_eq!(effects, vec![Effect::BreakEnded { completed: true }]);
        assert_eq!(view(&e, &c, &clock), None);
        assert_eq!(tooltip(&e, &c, &clock), "Siguiente descanso en 00:50:00");
    }

    #[test]
    fn the_countdown_follows_real_time_across_sleep() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);

        clock.jump(3 * MIN); // asleep, no ticks
        assert!(e.tick(&c, clock.now(), 0).is_empty());
        assert_eq!(view(&e, &c, &clock).unwrap().remaining_secs, 2 * MIN);

        clock.jump(10 * MIN);
        assert_eq!(
            e.tick(&c, clock.now(), 0),
            vec![Effect::BreakEnded { completed: true }]
        );
    }

    #[test]
    fn cancel_counts_hides_the_break_and_restarts_the_interval() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);

        let effects = e.cancel(&c);

        assert_eq!(
            effects,
            vec![
                Effect::BreakEnded { completed: false },
                Effect::CancelCountChanged(1)
            ]
        );
        assert_eq!(e.consecutive_cancels(), 1);
        e.tick(&c, clock.now(), 0);
        assert_eq!(view(&e, &c, &clock), None);
        assert_eq!(tooltip(&e, &c, &clock), "Siguiente descanso en 00:50:00");
    }

    #[test]
    fn cancel_is_not_offered_nor_accepted_when_disallowed() {
        let mut c = test_config();
        c.breaks.allow_cancel = false;
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);

        assert!(!view(&e, &c, &clock).unwrap().can_cancel);
        assert!(e.cancel(&c).is_empty());
        assert!(view(&e, &c, &clock).is_some(), "the break stays on screen");
        assert_eq!(e.consecutive_cancels(), 0);
    }

    #[test]
    fn reaching_the_cancel_limit_makes_the_next_break_hard() {
        let mut c = test_config();
        c.breaks.limit_cancels = true;
        c.breaks.cancel_limit = 2;
        let mut clock = Clock::new();
        let mut e = Engine::new(0);

        for _ in 0..2 {
            reach_break(&mut e, &c, &mut clock);
            assert!(!view(&e, &c, &clock).unwrap().hard);
            e.cancel(&c);
        }
        reach_break(&mut e, &c, &mut clock);

        let hard = view(&e, &c, &clock).unwrap();
        assert!(hard.hard);
        assert!(!hard.can_cancel);
        assert!(!hard.can_postpone);
        assert!(e.cancel(&c).is_empty(), "cancel is refused in a hard lock");
        assert!(e.postpone(&c, clock.now()).is_empty(), "postpone too");

        // Completing it resets the counter, so the next break is normal.
        let effects = run(&mut e, &c, &mut clock, 5 * MIN);
        assert_eq!(
            effects,
            vec![
                Effect::BreakEnded { completed: true },
                Effect::CancelCountChanged(0)
            ]
        );
        reach_break(&mut e, &c, &mut clock);
        assert!(!view(&e, &c, &clock).unwrap().hard);
    }

    #[test]
    fn without_a_limit_cancelling_never_hard_locks() {
        let c = test_config(); // limit_cancels = false
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        for _ in 0..6 {
            reach_break(&mut e, &c, &mut clock);
            assert!(view(&e, &c, &clock).unwrap().can_cancel);
            e.cancel(&c);
        }
    }

    /// S25 setup: 5-minute break that can be finished after 60% (180 s).
    fn finish_config() -> Config {
        let mut c = test_config();
        c.breaks.finish_percent = 60;
        c
    }

    #[test]
    fn cancel_becomes_finish_and_postpone_goes_away_after_the_percentage() {
        let c = finish_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);

        run(&mut e, &c, &mut clock, 179);
        let before = view(&e, &c, &clock).unwrap();
        assert!(before.can_cancel && before.can_postpone && !before.can_finish);

        run(&mut e, &c, &mut clock, 1);
        let after = view(&e, &c, &clock).unwrap();
        assert!(after.can_finish);
        assert!(!after.can_cancel, "Terminar replaces Cancelar");
        assert!(!after.can_postpone, "Posponer disappears");
    }

    #[test]
    fn finish_is_ignored_before_the_percentage() {
        let c = finish_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);
        run(&mut e, &c, &mut clock, 179);

        assert_eq!(e.finish(&c, clock.now()), vec![]);
        assert!(view(&e, &c, &clock).is_some());
    }

    #[test]
    fn finish_counts_as_a_completed_break_and_resets_the_cancel_counter() {
        let c = finish_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(1);
        reach_break(&mut e, &c, &mut clock);
        run(&mut e, &c, &mut clock, 180);

        assert_eq!(
            e.finish(&c, clock.now()),
            vec![
                Effect::BreakEnded { completed: true },
                Effect::CancelCountChanged(0)
            ]
        );
        assert_eq!(e.consecutive_cancels(), 0);
        assert!(view(&e, &c, &clock).is_none());
        assert_eq!(tooltip(&e, &c, &clock), "Siguiente descanso en 00:50:00");
    }

    #[test]
    fn a_hard_break_is_never_finished_early() {
        let mut c = finish_config();
        c.breaks.limit_cancels = true;
        c.breaks.cancel_limit = 1;
        let mut clock = Clock::new();
        let mut e = Engine::new(1);
        reach_break(&mut e, &c, &mut clock);
        run(&mut e, &c, &mut clock, 299);

        assert!(!view(&e, &c, &clock).unwrap().can_finish);
        assert_eq!(e.finish(&c, clock.now()), vec![]);
        assert!(view(&e, &c, &clock).is_some());
    }

    #[test]
    fn finish_is_not_offered_at_100_percent_nor_when_cancel_is_disallowed() {
        for (percent, allow_cancel) in [(100, true), (60, false)] {
            let mut c = finish_config();
            c.breaks.finish_percent = percent;
            c.breaks.allow_cancel = allow_cancel;
            let mut clock = Clock::new();
            let mut e = Engine::new(0);
            reach_break(&mut e, &c, &mut clock);
            run(&mut e, &c, &mut clock, 299);

            assert!(!view(&e, &c, &clock).unwrap().can_finish, "{percent}");
            assert_eq!(e.finish(&c, clock.now()), vec![], "{percent}");
        }
    }

    /// Two cancels with a limit of two: the next break would be hard.
    fn at_the_cancel_limit() -> (Config, Clock, Engine) {
        let mut c = test_config();
        c.breaks.limit_cancels = true;
        c.breaks.cancel_limit = 2;
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        for _ in 0..2 {
            reach_break(&mut e, &c, &mut clock);
            e.cancel(&c);
        }
        (c, clock, e)
    }

    #[test]
    fn sleeping_at_least_a_break_resets_the_cancel_counter() {
        // Reported bug: two cancels, hibernation overnight, and the first
        // break of the next session was hard.
        let (c, mut clock, mut e) = at_the_cancel_limit();
        run(&mut e, &c, &mut clock, 10);
        clock.jump(5 * MIN); // asleep for the length of a break

        assert_eq!(
            e.tick(&c, clock.now(), 0),
            vec![Effect::CancelCountChanged(0)]
        );
        assert_eq!(e.consecutive_cancels(), 0);
        reach_break(&mut e, &c, &mut clock);
        assert!(!view(&e, &c, &clock).unwrap().hard);
    }

    #[test]
    fn a_short_sleep_keeps_the_cancel_counter() {
        let (c, mut clock, mut e) = at_the_cancel_limit();
        run(&mut e, &c, &mut clock, 10);
        clock.jump(5 * MIN - 1);

        assert_eq!(e.tick(&c, clock.now(), 0), vec![]);
        reach_break(&mut e, &c, &mut clock);
        assert!(view(&e, &c, &clock).unwrap().hard);
    }

    #[test]
    fn a_locked_session_of_at_least_a_break_resets_the_cancel_counter() {
        let (c, mut clock, mut e) = at_the_cancel_limit();
        e.on_session_lock();
        run(&mut e, &c, &mut clock, 5 * MIN);
        e.on_session_unlock();

        assert_eq!(
            e.tick(&c, clock.now(), 0),
            vec![Effect::CancelCountChanged(0)]
        );
        reach_break(&mut e, &c, &mut clock);
        assert!(!view(&e, &c, &clock).unwrap().hard);
    }

    #[test]
    fn a_short_lock_keeps_the_cancel_counter() {
        let (c, mut clock, mut e) = at_the_cancel_limit();
        e.on_session_lock();
        run(&mut e, &c, &mut clock, 2 * MIN);
        e.on_session_unlock();

        assert_eq!(e.tick(&c, clock.now(), 0), vec![]);
        reach_break(&mut e, &c, &mut clock);
        assert!(view(&e, &c, &clock).unwrap().hard);
    }

    #[test]
    fn an_idle_pause_of_at_least_a_break_resets_the_cancel_counter() {
        let (c, mut clock, mut e) = at_the_cancel_limit();
        for idle in 1..=6 * MIN {
            clock.jump(1);
            e.tick(&c, clock.now(), idle);
        }
        clock.jump(1);

        assert_eq!(
            e.tick(&c, clock.now(), 0),
            vec![Effect::IdleEnded, Effect::CancelCountChanged(0)]
        );
        reach_break(&mut e, &c, &mut clock);
        assert!(!view(&e, &c, &clock).unwrap().hard);
    }

    #[test]
    fn a_postponed_break_is_not_rest() {
        let mut c = test_config();
        c.breaks.limit_cancels = true;
        c.breaks.cancel_limit = 2;
        let mut clock = Clock::new();
        let mut e = Engine::new(1);
        reach_break(&mut e, &c, &mut clock);
        e.postpone(&c, clock.now());
        run(&mut e, &c, &mut clock, 5 * MIN); // the break comes back
        e.cancel(&c);

        let effects = run(&mut e, &c, &mut clock, 10);
        assert!(!effects.contains(&Effect::CancelCountChanged(0)));
        reach_break(&mut e, &c, &mut clock);
        assert!(view(&e, &c, &clock).unwrap().hard);
    }

    #[test]
    fn a_persisted_counter_at_the_limit_hard_locks_the_first_break() {
        let mut c = test_config();
        c.breaks.limit_cancels = true;
        c.breaks.cancel_limit = 3;
        let mut clock = Clock::new();
        let mut e = Engine::new(3);
        reach_break(&mut e, &c, &mut clock);
        assert!(view(&e, &c, &clock).unwrap().hard);
    }

    #[test]
    fn postpone_brings_the_break_back_after_m_minutes_up_to_k_times() {
        let mut c = test_config();
        c.breaks.postpone_minutes = 5;
        c.breaks.max_postpones = 2;
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);

        for _ in 0..2 {
            assert!(view(&e, &c, &clock).unwrap().can_postpone);
            assert_eq!(
                e.postpone(&c, clock.now()),
                vec![Effect::BreakEnded { completed: false }]
            );
            assert_eq!(run(&mut e, &c, &mut clock, 5 * MIN - 1), vec![]);
            assert_eq!(view(&e, &c, &clock), None);
            assert_eq!(run(&mut e, &c, &mut clock, 1), vec![Effect::BreakStarted]);
            assert_eq!(view(&e, &c, &clock).unwrap().remaining_secs, 5 * MIN);
        }
        let third = view(&e, &c, &clock).unwrap();
        assert!(!third.can_postpone, "K = 2 postpones used");
        assert!(third.can_cancel);
        assert!(e.postpone(&c, clock.now()).is_empty());
        assert_eq!(e.consecutive_cancels(), 0, "postponing is not cancelling");
    }

    #[test]
    fn zero_postpones_hides_the_button() {
        let mut c = test_config();
        c.breaks.max_postpones = 0;
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);
        assert!(!view(&e, &c, &clock).unwrap().can_postpone);
    }

    #[test]
    fn the_postpone_allowance_is_per_break() {
        let c = test_config(); // max_postpones = 1
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);
        e.postpone(&c, clock.now());
        run(&mut e, &c, &mut clock, 5 * MIN);
        assert!(!view(&e, &c, &clock).unwrap().can_postpone);
        run(&mut e, &c, &mut clock, 5 * MIN); // completes

        reach_break(&mut e, &c, &mut clock);
        assert!(view(&e, &c, &clock).unwrap().can_postpone);
    }

    #[test]
    fn a_break_can_be_started_at_once_and_runs_like_any_other() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        run(&mut e, &c, &mut clock, 10);

        assert_eq!(
            e.start_break_now(&c, clock.now()),
            vec![Effect::BreakStarted]
        );
        let view = view(&e, &c, &clock).unwrap();
        assert_eq!(view.remaining_secs, 5 * MIN);
        assert!(view.can_cancel);
        // Already on screen: a second request changes nothing.
        assert_eq!(e.start_break_now(&c, clock.now()), vec![]);
    }

    #[test]
    fn a_break_started_at_once_shows_the_screen_even_in_notification_mode() {
        let mut c = test_config();
        c.notice = NoticeKind::Notification;
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        run(&mut e, &c, &mut clock, 1);
        assert_eq!(
            e.start_break_now(&c, clock.now()),
            vec![Effect::BreakStarted]
        );
        assert!(view(&e, &c, &clock).is_some());
    }

    #[test]
    fn toggle_pause_pauses_and_a_second_toggle_reactivates() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        run(&mut e, &c, &mut clock, 1);

        e.toggle_pause(clock.now(), 30);
        run(&mut e, &c, &mut clock, 1);
        assert_eq!(
            tooltip(&e, &c, &clock),
            "Recordatorios desactivados hasta las 10:30"
        );

        e.toggle_pause(clock.now(), 30);
        run(&mut e, &c, &mut clock, 0);
        assert_eq!(tooltip(&e, &c, &clock), "Siguiente descanso en 00:50:00");
    }

    #[test]
    fn toggle_pause_pauses_again_once_the_pause_is_over() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        e.toggle_pause(clock.now(), 1);
        run(&mut e, &c, &mut clock, MIN);
        assert!(tooltip(&e, &c, &clock).starts_with("Siguiente descanso"));

        e.toggle_pause(clock.now(), 1);
        run(&mut e, &c, &mut clock, 1);
        assert!(tooltip(&e, &c, &clock).starts_with("Recordatorios desactivados"));
    }

    #[test]
    fn notification_mode_notifies_without_a_break_screen_and_restarts() {
        let mut c = test_config();
        c.notice = NoticeKind::Notification;
        let mut clock = Clock::new();
        let mut e = Engine::new(0);

        let effects = reach_break(&mut e, &c, &mut clock);

        assert_eq!(effects, vec![Effect::ShowNotification]);
        assert_eq!(view(&e, &c, &clock), None);
        assert_eq!(tooltip(&e, &c, &clock), "Siguiente descanso en 00:50:00");
    }

    #[test]
    fn activity_after_an_idle_pause_is_announced_once() {
        let c = test_config(); // idle pause after 5 min
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        run(&mut e, &c, &mut clock, 60);

        clock.jump(1);
        assert_eq!(e.tick(&c, clock.now(), 5 * MIN), vec![]);
        clock.jump(1);
        assert_eq!(e.tick(&c, clock.now(), 5 * MIN + 1), vec![]);

        clock.jump(1);
        assert_eq!(e.tick(&c, clock.now(), 0), vec![Effect::IdleEnded]);
        assert_eq!(tooltip(&e, &c, &clock), "Siguiente descanso en 00:50:00");
        clock.jump(1);
        assert_eq!(e.tick(&c, clock.now(), 0), vec![]);
    }

    #[test]
    fn plain_counting_never_announces_idle_end() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        let effects = run(&mut e, &c, &mut clock, 10 * MIN);
        assert!(!effects.contains(&Effect::IdleEnded));
    }

    #[test]
    fn menu_items_are_disabled_while_the_break_is_on_screen() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        run(&mut e, &c, &mut clock, 1);
        let working = e.snapshot(&c, clock.now()).menu;
        assert!(working.settings_enabled && working.disable_enabled && working.quit_enabled);

        reach_break(&mut e, &c, &mut clock);
        let on_break = e.snapshot(&c, clock.now()).menu;
        assert!(!on_break.settings_enabled);
        assert!(!on_break.disable_enabled);
        assert!(!on_break.quit_enabled);
    }

    #[test]
    fn disable_is_ignored_while_the_break_is_on_screen() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);
        e.disable_for(clock.now(), 240);
        run(&mut e, &c, &mut clock, 1);
        assert!(view(&e, &c, &clock).is_some());
    }

    #[test]
    fn tooltip_and_reactivate_reflect_each_state() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);

        run(&mut e, &c, &mut clock, 10 * MIN);
        assert_eq!(tooltip(&e, &c, &clock), "Siguiente descanso en 00:40:00");

        e.disable_for(clock.now(), 30); // 10:10 + 30 min
        run(&mut e, &c, &mut clock, 1);
        let snapshot = e.snapshot(&c, clock.now());
        assert_eq!(
            snapshot.tooltip,
            "Recordatorios desactivados hasta las 10:40"
        );
        assert!(snapshot.menu.show_reactivate);

        e.reactivate();
        run(&mut e, &c, &mut clock, 1);
        assert!(!e.snapshot(&c, clock.now()).menu.show_reactivate);

        assert_eq!(e.tick(&c, clock.now(), 5 * MIN), vec![]);
        assert_eq!(tooltip(&e, &c, &clock), "En pausa por inactividad");

        e.on_session_lock();
        run(&mut e, &c, &mut clock, 1);
        assert_eq!(tooltip(&e, &c, &clock), "En pausa: sesión bloqueada");
        e.on_session_unlock();

        let mut evening = Clock::new();
        evening.secs_of_day = 20 * 3600;
        run(&mut e, &c, &mut evening, 1);
        assert_eq!(tooltip(&e, &c, &evening), "Fuera del horario de trabajo");
    }

    #[test]
    fn tooltip_during_and_after_postponing() {
        let c = test_config();
        let mut clock = Clock::new();
        let mut e = Engine::new(0);
        reach_break(&mut e, &c, &mut clock);
        assert_eq!(tooltip(&e, &c, &clock), "Descanso en curso");

        e.postpone(&c, clock.now());
        run(&mut e, &c, &mut clock, 90);
        assert_eq!(
            tooltip(&e, &c, &clock),
            "Descanso pospuesto: vuelve en 03:30"
        );
    }
}
