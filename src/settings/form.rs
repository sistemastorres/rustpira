//! The settings window's data, as the widgets hold it (numbers as `i32`,
//! times and colors as text), and its conversion to and from `Config`.
//! Pure: no Slint, no Windows, so every rule is unit tested.

use crate::domain::config::{
    BUILTIN_SOUNDS, Background, Config, DoubleClickAction, NoticeKind, Rgb, SoundConfig,
    SoundSource, Theme, TimeOfDay, TimeSlot, Weekday,
};

/// "Doble clic en el ícono" list, in display order.
pub const DOUBLE_CLICK_ACTIONS: [DoubleClickAction; 3] = [
    DoubleClickAction::OpenSettings,
    DoubleClickAction::StartBreak,
    DoubleClickAction::Pause30,
];

/// Index of "Archivo propio (.wav)" in the sound list; 0..BUILTIN_SOUNDS
/// are the built-in sounds.
pub const SOUND_CUSTOM: i32 = BUILTIN_SOUNDS as i32;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlotText {
    pub start: String,
    pub end: String,
}

impl SlotText {
    pub fn new(start: &str, end: &str) -> Self {
        Self {
            start: start.into(),
            end: end.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    // General
    pub interval_minutes: i32,
    pub break_minutes: i32,
    pub notification_only: bool,
    pub start_with_windows: bool,
    /// Index in `DOUBLE_CLICK_ACTIONS`.
    pub double_click: i32,
    // Mensaje
    pub title: String,
    pub body: String,
    // Horarios, Monday to Sunday (`Weekday::ALL` order)
    pub days: [Vec<SlotText>; 7],
    /// Per-day switch; a disabled day keeps its slots.
    pub enabled: [bool; 7],
    // Descanso
    pub allow_cancel: bool,
    pub limit_cancels: bool,
    pub cancel_limit: i32,
    pub postpone_minutes: i32,
    pub max_postpones: i32,
    pub finish_percent: i32,
    pub idle_enabled: bool,
    pub idle_minutes: i32,
    // Apariencia
    pub background_image: bool,
    pub color: String,
    pub image_path: String,
    pub opacity: i32,
    pub dark_theme: bool,
    // Sonido
    pub sound: i32,
    /// The built-in sound to keep while "Archivo propio" is selected.
    pub builtin_when_custom: u8,
    pub custom_sound_path: String,
    pub volume: i32,
    pub end_sound: bool,
}

/// Widget values are `i32`; out-of-range values saturate so that
/// `Config::validate` reports them with its usual message.
fn to_u8(value: i32) -> u8 {
    value.clamp(0, i32::from(u8::MAX)) as u8
}

fn to_u32(value: i32) -> u32 {
    value.max(0) as u32
}

fn slot_text(slot: &TimeSlot) -> SlotText {
    SlotText {
        start: slot.start.to_string(),
        end: slot.end.to_string(),
    }
}

fn parse_time(day: Weekday, text: &str) -> Result<TimeOfDay, String> {
    TimeOfDay::parse(text.trim()).ok_or_else(|| {
        format!(
            "{}: «{}» no es una hora válida. Use el formato HH:MM.",
            day.label(),
            text.trim()
        )
    })
}

/// The arrows next to a time: moves to the next (or previous) quarter hour,
/// within 00:00..23:59. Text that is not a time becomes 09:00.
pub fn step_time(text: &str, up: bool) -> String {
    const STEP: u16 = 15;
    const LAST: u16 = 23 * 60 + 59;
    let Some(time) = TimeOfDay::parse(text.trim()) else {
        return "09:00".into();
    };
    let minutes = time.minutes();
    let stepped = if up {
        ((minutes / STEP + 1) * STEP).min(LAST)
    } else if minutes % STEP == 0 {
        minutes.saturating_sub(STEP)
    } else {
        minutes - minutes % STEP
    };
    TimeOfDay::new((stepped / 60) as u8, (stepped % 60) as u8)
        .map(|t| t.to_string())
        .unwrap_or_else(|| "09:00".into())
}

impl Form {
    pub fn from_config(config: &Config) -> Self {
        let sound = &config.sound;
        let appearance = &config.appearance;
        Self {
            interval_minutes: config.interval_minutes as i32,
            break_minutes: config.break_minutes as i32,
            notification_only: config.notice == NoticeKind::Notification,
            start_with_windows: config.start_with_windows,
            double_click: DOUBLE_CLICK_ACTIONS
                .iter()
                .position(|action| *action == config.double_click)
                .unwrap_or(0) as i32,
            title: config.message.title.clone(),
            body: config.message.body.clone(),
            days: Weekday::ALL.map(|day| config.schedule.day(day).iter().map(slot_text).collect()),
            enabled: Weekday::ALL.map(|day| config.schedule.is_enabled(day)),
            allow_cancel: config.breaks.allow_cancel,
            limit_cancels: config.breaks.limit_cancels,
            cancel_limit: config.breaks.cancel_limit.into(),
            postpone_minutes: config.breaks.postpone_minutes as i32,
            max_postpones: config.breaks.max_postpones.into(),
            finish_percent: config.breaks.finish_percent.into(),
            idle_enabled: config.idle.enabled,
            idle_minutes: config.idle.minutes as i32,
            background_image: appearance.background == Background::Image,
            color: appearance.color.to_string(),
            image_path: appearance.image_path.clone(),
            opacity: appearance.opacity.into(),
            dark_theme: appearance.theme == Theme::Dark,
            sound: match sound.source {
                SoundSource::Custom => SOUND_CUSTOM,
                SoundSource::Builtin => i32::from(sound.builtin) - 1,
            },
            builtin_when_custom: sound.builtin,
            custom_sound_path: sound.custom_path.clone(),
            volume: sound.volume.into(),
            end_sound: sound.end_sound,
        }
    }

    /// The configuration the form describes, or the first problem found,
    /// as a Spanish message for the window.
    // Filled field by field because several steps can return early.
    #[allow(clippy::field_reassign_with_default)]
    pub fn to_config(&self) -> Result<Config, String> {
        let mut config = Config::default();
        config.interval_minutes = to_u32(self.interval_minutes);
        config.break_minutes = to_u32(self.break_minutes);
        config.notice = if self.notification_only {
            NoticeKind::Notification
        } else {
            NoticeKind::Lock
        };
        config.start_with_windows = self.start_with_windows;
        config.double_click = usize::try_from(self.double_click)
            .ok()
            .and_then(|index| DOUBLE_CLICK_ACTIONS.get(index).copied())
            .unwrap_or_default();
        config.message.title = self.title.clone();
        config.message.body = self.body.clone();

        for (day, slots) in Weekday::ALL.into_iter().zip(&self.days) {
            let parsed = slots
                .iter()
                .map(|slot| {
                    Ok(TimeSlot::new(
                        parse_time(day, &slot.start)?,
                        parse_time(day, &slot.end)?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            *config.schedule.day_mut(day) = parsed;
        }
        for (day, enabled) in Weekday::ALL.into_iter().zip(self.enabled) {
            config.schedule.set_enabled(day, enabled);
        }

        config.breaks.allow_cancel = self.allow_cancel;
        config.breaks.limit_cancels = self.limit_cancels;
        config.breaks.cancel_limit = to_u8(self.cancel_limit);
        config.breaks.postpone_minutes = to_u32(self.postpone_minutes);
        config.breaks.max_postpones = to_u8(self.max_postpones);
        config.breaks.finish_percent = to_u8(self.finish_percent);
        config.idle.enabled = self.idle_enabled;
        config.idle.minutes = to_u32(self.idle_minutes);

        let appearance = &mut config.appearance;
        appearance.color = Rgb::parse(self.color.trim()).ok_or_else(|| {
            format!(
                "El color «{}» no es válido. Use el formato #RRGGBB.",
                self.color.trim()
            )
        })?;
        appearance.image_path = self.image_path.trim().to_string();
        appearance.background = if self.background_image {
            if appearance.image_path.is_empty() {
                return Err("Elija una imagen de fondo o use un color.".into());
            }
            Background::Image
        } else {
            Background::Color
        };
        appearance.opacity = to_u8(self.opacity);
        appearance.theme = if self.dark_theme {
            Theme::Dark
        } else {
            Theme::Light
        };

        config.sound = self.sound_config();
        if config.sound.source == SoundSource::Custom && config.sound.custom_path.is_empty() {
            return Err("Elija un archivo .wav o un sonido incluido.".into());
        }

        config.validate().map_err(|e| e.to_string())?;
        // Checked here and not in `Config::validate`, so a hand-edited file
        // that was valid before keeps loading (L18).
        if config.idle.enabled && config.idle.minutes > config.interval_minutes {
            return Err(format!(
                "Los minutos sin actividad ({}) no pueden superar el intervalo entre descansos ({} min).",
                config.idle.minutes, config.interval_minutes
            ));
        }
        Ok(config)
    }

    /// The sound as currently chosen, for the "Probar" button (no need to
    /// save first).
    pub fn sound_config(&self) -> SoundConfig {
        let custom = self.sound == SOUND_CUSTOM;
        SoundConfig {
            source: if custom {
                SoundSource::Custom
            } else {
                SoundSource::Builtin
            },
            builtin: if custom {
                self.builtin_when_custom
            } else {
                to_u8(self.sound + 1)
            },
            custom_path: self.custom_sound_path.trim().to_string(),
            volume: to_u8(self.volume).min(100),
            end_sound: self.end_sound,
        }
    }

    /// Adds a slot to `day`: 09:00–18:00 on an empty day, otherwise one hour
    /// starting where the last slot ends (capped at 23:59).
    pub fn add_slot(&mut self, day: usize) {
        let Some(slots) = self.days.get_mut(day) else {
            return;
        };
        let slot = match slots.last() {
            None => SlotText::new("09:00", "18:00"),
            Some(last) => {
                let end = TimeOfDay::parse(last.end.trim())
                    .map(|t| {
                        let minutes = (t.minutes() + 60).min(23 * 60 + 59);
                        TimeOfDay::new((minutes / 60) as u8, (minutes % 60) as u8)
                            .map(|t| t.to_string())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                SlotText {
                    start: last.end.trim().to_string(),
                    end,
                }
            }
        };
        slots.push(slot);
    }

    /// One arrow click on the start (or end) time of a slot.
    pub fn step_slot(&mut self, day: usize, index: usize, start: bool, up: bool) {
        let Some(slot) = self
            .days
            .get_mut(day)
            .and_then(|slots| slots.get_mut(index))
        else {
            return;
        };
        let text = if start {
            &mut slot.start
        } else {
            &mut slot.end
        };
        *text = step_time(text, up);
    }

    pub fn remove_slot(&mut self, day: usize, index: usize) {
        if let Some(slots) = self.days.get_mut(day)
            && index < slots.len()
        {
            slots.remove(index);
        }
    }

    /// Copies the slots of `from` to Monday through Friday.
    pub fn copy_to_weekdays(&mut self, from: usize) {
        let Some(source) = self.days.get(from).cloned() else {
            return;
        };
        for (day, slots) in Weekday::ALL.into_iter().zip(self.days.iter_mut()) {
            if day.is_workday() {
                *slots = source.clone();
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    fn time(h: u8, m: u8) -> TimeOfDay {
        TimeOfDay::new(h, m).unwrap()
    }

    fn customized() -> Config {
        let mut c = Config::default();
        c.interval_minutes = 25;
        c.break_minutes = 3;
        c.notice = NoticeKind::Notification;
        c.start_with_windows = true;
        c.double_click = DoubleClickAction::Pause30;
        c.message.title = "Pausa".into();
        c.message.body = "Línea 1\nLínea 2".into();
        c.schedule.day_mut(Weekday::Monday).clear();
        c.schedule.day_mut(Weekday::Monday).extend([
            TimeSlot::new(time(8, 0), time(12, 0)),
            TimeSlot::new(time(13, 30), time(17, 45)),
        ]);
        c.schedule.day_mut(Weekday::Sunday).clear();
        c.schedule.set_enabled(Weekday::Tuesday, false);
        c.breaks.allow_cancel = false;
        c.breaks.limit_cancels = true;
        c.breaks.cancel_limit = 2;
        c.breaks.postpone_minutes = 10;
        c.breaks.max_postpones = 3;
        c.breaks.finish_percent = 90;
        c.idle.enabled = false;
        c.idle.minutes = 15;
        c.appearance.background = Background::Image;
        c.appearance.color = Rgb::parse("#AA3300").unwrap();
        c.appearance.image_path = "C:/fotos/mar.jpg".into();
        c.appearance.opacity = 65;
        c.appearance.theme = Theme::Dark;
        c.sound.source = SoundSource::Custom;
        c.sound.builtin = 3;
        c.sound.custom_path = "C:/sonidos/gong.wav".into();
        c.sound.volume = 30;
        c.sound.end_sound = true;
        c
    }

    #[test]
    fn default_config_round_trips_through_the_form() {
        let c = Config::default();
        assert_eq!(Form::from_config(&c).to_config(), Ok(c));
    }

    #[test]
    fn every_field_round_trips_through_the_form() {
        let c = customized();
        assert_eq!(Form::from_config(&c).to_config(), Ok(c));
    }

    #[test]
    fn form_shows_times_as_text_and_sounds_as_list_indexes() {
        let mut c = customized();
        let form = Form::from_config(&c);
        assert_eq!(
            form.days[0],
            vec![
                SlotText::new("08:00", "12:00"),
                SlotText::new("13:30", "17:45")
            ]
        );
        assert!(form.days[6].is_empty());
        assert_eq!(form.color, "#AA3300");
        assert_eq!(form.sound, SOUND_CUSTOM);
        c.sound.source = SoundSource::Builtin;
        assert_eq!(Form::from_config(&c).sound, 2);
    }

    #[test]
    fn invalid_time_names_the_day_and_the_text() {
        let mut form = Form::from_config(&Config::default());
        form.days[2] = vec![SlotText::new("09:00", "25:00")];
        assert_eq!(
            form.to_config(),
            Err("Miércoles: «25:00» no es una hora válida. Use el formato HH:MM.".into())
        );
    }

    #[test]
    fn times_with_surrounding_spaces_are_accepted() {
        let mut form = Form::from_config(&Config::default());
        form.days[0] = vec![SlotText::new(" 09:00", "18:00 ")];
        assert!(form.to_config().is_ok());
    }

    #[test]
    fn overlapping_slots_use_the_validation_message() {
        let mut form = Form::from_config(&Config::default());
        form.days[0] = vec![
            SlotText::new("09:00", "12:00"),
            SlotText::new("11:00", "13:00"),
        ];
        assert_eq!(
            form.to_config(),
            Err("Lunes: las franjas 09:00–12:00 y 11:00–13:00 se superponen.".into())
        );
    }

    #[test]
    fn invalid_color_is_reported() {
        let mut form = Form::from_config(&Config::default());
        form.color = "azul".into();
        assert_eq!(
            form.to_config(),
            Err("El color «azul» no es válido. Use el formato #RRGGBB.".into())
        );
    }

    #[test]
    fn image_background_needs_an_image() {
        let mut form = Form::from_config(&Config::default());
        form.background_image = true;
        form.image_path = "  ".into();
        assert_eq!(
            form.to_config(),
            Err("Elija una imagen de fondo o use un color.".into())
        );
        form.background_image = false;
        assert!(form.to_config().is_ok());
    }

    #[test]
    fn custom_sound_needs_a_file() {
        let mut form = Form::from_config(&Config::default());
        form.sound = SOUND_CUSTOM;
        form.custom_sound_path = String::new();
        assert_eq!(
            form.to_config(),
            Err("Elija un archivo .wav o un sonido incluido.".into())
        );
    }

    #[test]
    fn out_of_range_numbers_use_the_validation_message() {
        let mut form = Form::from_config(&Config::default());
        form.interval_minutes = 0;
        assert_eq!(
            form.to_config(),
            Err("«Intervalo (min)» debe estar entre 1 y 240 (valor actual: 0).".into())
        );
        form.interval_minutes = -5;
        assert_eq!(
            form.to_config(),
            Err("«Intervalo (min)» debe estar entre 1 y 240 (valor actual: 0).".into())
        );
    }

    #[test]
    fn idle_minutes_cannot_exceed_the_interval() {
        let mut form = Form::from_config(&Config::default());
        form.interval_minutes = 25;
        form.idle_enabled = true;
        form.idle_minutes = 30;
        assert_eq!(
            form.to_config(),
            Err(
                "Los minutos sin actividad (30) no pueden superar el intervalo entre descansos (25 min)."
                    .into()
            )
        );
        form.idle_minutes = 25;
        assert!(form.to_config().is_ok());
        form.idle_minutes = 30;
        form.idle_enabled = false;
        assert!(
            form.to_config().is_ok(),
            "only checked when idle pause is on"
        );
    }

    #[test]
    fn add_slot_fills_sensible_times() {
        let mut form = Form::from_config(&Config::default());
        form.days[5].clear();
        form.add_slot(5);
        assert_eq!(form.days[5], vec![SlotText::new("09:00", "18:00")]);

        form.days[5] = vec![SlotText::new("09:00", "12:00")];
        form.add_slot(5);
        assert_eq!(form.days[5][1], SlotText::new("12:00", "13:00"));

        form.days[5] = vec![SlotText::new("20:00", "23:30")];
        form.add_slot(5);
        assert_eq!(form.days[5][1], SlotText::new("23:30", "23:59"));
    }

    #[test]
    fn remove_slot_ignores_indexes_out_of_range() {
        let mut form = Form::from_config(&Config::default());
        form.days[0] = vec![
            SlotText::new("08:00", "09:00"),
            SlotText::new("10:00", "11:00"),
        ];
        form.remove_slot(0, 0);
        assert_eq!(form.days[0], vec![SlotText::new("10:00", "11:00")]);
        form.remove_slot(0, 7);
        form.remove_slot(9, 0);
        assert_eq!(form.days[0].len(), 1);
    }

    #[test]
    fn copy_to_weekdays_copies_monday_to_friday_only() {
        let mut form = Form::from_config(&Config::default());
        form.days[5] = vec![SlotText::new("10:00", "14:00")];
        form.days[6] = vec![];
        form.copy_to_weekdays(5);
        for (day, slots) in form.days.iter().enumerate().take(5) {
            assert_eq!(slots, &vec![SlotText::new("10:00", "14:00")], "day {day}");
        }
        assert_eq!(form.days[6], vec![]);
    }

    #[test]
    fn form_shows_which_days_are_enabled() {
        let form = Form::from_config(&customized());
        assert_eq!(form.enabled, [true, false, true, true, true, true, true]);
        assert_eq!(form.days[1].len(), 1, "a disabled day keeps its slots");
    }

    #[test]
    fn time_arrows_move_by_quarter_hours_and_stay_within_the_day() {
        assert_eq!(step_time("09:00", true), "09:15");
        assert_eq!(step_time("09:00", false), "08:45");
        assert_eq!(step_time("09:07", true), "09:15");
        assert_eq!(step_time("09:07", false), "09:00");
        assert_eq!(step_time(" 12:30 ", true), "12:45");
        assert_eq!(step_time("23:45", true), "23:59");
        assert_eq!(step_time("23:59", true), "23:59");
        assert_eq!(step_time("23:59", false), "23:45");
        assert_eq!(step_time("00:00", false), "00:00");
        assert_eq!(step_time("00:10", false), "00:00");
    }

    #[test]
    fn time_arrows_turn_invalid_text_into_a_valid_time() {
        assert_eq!(step_time("25:00", true), "09:00");
        assert_eq!(step_time("", false), "09:00");
    }

    #[test]
    fn step_slot_changes_only_the_chosen_time() {
        let mut form = Form::from_config(&Config::default());
        form.days[0] = vec![SlotText::new("09:00", "12:00")];
        form.step_slot(0, 0, true, true);
        assert_eq!(form.days[0][0], SlotText::new("09:15", "12:00"));
        form.step_slot(0, 0, false, false);
        assert_eq!(form.days[0][0], SlotText::new("09:15", "11:45"));
        form.step_slot(0, 5, true, true);
        form.step_slot(9, 0, true, true);
        assert_eq!(form.days[0][0], SlotText::new("09:15", "11:45"));
    }

    #[test]
    fn sound_config_reflects_the_current_choice() {
        let mut form = Form::from_config(&Config::default());
        form.sound = 1;
        form.volume = 70;
        let s = form.sound_config();
        assert_eq!(
            (s.source, s.builtin, s.volume),
            (SoundSource::Builtin, 2, 70)
        );

        form.sound = SOUND_CUSTOM;
        form.custom_sound_path = "C:/x.wav".into();
        form.volume = 150;
        let s = form.sound_config();
        assert_eq!(
            (s.source, s.custom_path.as_str(), s.volume),
            (SoundSource::Custom, "C:/x.wav", 100)
        );
    }
}
