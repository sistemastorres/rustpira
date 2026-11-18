//! Application configuration: model, defaults (design §5.10), validation and
//! TOML (de)serialization. Pure: no file system access here.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A time of day with minute precision, serialized as `"HH:MM"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TimeOfDay {
    minutes: u16,
}

impl TimeOfDay {
    pub fn new(hour: u8, minute: u8) -> Option<Self> {
        (hour < 24 && minute < 60).then(|| Self {
            minutes: u16::from(hour) * 60 + u16::from(minute),
        })
    }

    /// Parses `"HH:MM"` (two digits each, 00:00..=23:59).
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 5 || bytes[2] != b':' {
            return None;
        }
        let two_digits = |hi: u8, lo: u8| {
            (hi.is_ascii_digit() && lo.is_ascii_digit()).then(|| (hi - b'0') * 10 + (lo - b'0'))
        };
        Self::new(
            two_digits(bytes[0], bytes[1])?,
            two_digits(bytes[3], bytes[4])?,
        )
    }

    /// Minutes since midnight.
    pub fn minutes(self) -> u16 {
        self.minutes
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.minutes / 60, self.minutes % 60)
    }
}

impl TryFrom<String> for TimeOfDay {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text).ok_or_else(|| format!("hora inválida «{text}» (se espera HH:MM)"))
    }
}

impl From<TimeOfDay> for String {
    fn from(time: TimeOfDay) -> Self {
        time.to_string()
    }
}

/// An RGB color, serialized as `"#RRGGBB"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#')?;
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Self {
            r: channel(0)?,
            g: channel(2)?,
            b: channel(4)?,
        })
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl TryFrom<String> for Rgb {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text).ok_or_else(|| format!("color inválido «{text}» (se espera #RRGGBB)"))
    }
}

impl From<Rgb> for String {
    fn from(color: Rgb) -> Self {
        color.to_string()
    }
}

/// A working-hours slot within one day. `start` must be before `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSlot {
    pub start: TimeOfDay,
    pub end: TimeOfDay,
}

impl TimeSlot {
    pub fn new(start: TimeOfDay, end: TimeOfDay) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub const ALL: [Weekday; 7] = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];

    pub fn is_workday(self) -> bool {
        !matches!(self, Weekday::Saturday | Weekday::Sunday)
    }

    /// Spanish name, used in UI and validation messages.
    pub fn label(self) -> &'static str {
        match self {
            Weekday::Monday => "Lunes",
            Weekday::Tuesday => "Martes",
            Weekday::Wednesday => "Miércoles",
            Weekday::Thursday => "Jueves",
            Weekday::Friday => "Viernes",
            Weekday::Saturday => "Sábado",
            Weekday::Sunday => "Domingo",
        }
    }
}

/// Working hours per weekday. A day without slots means no reminders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Schedule {
    pub monday: Vec<TimeSlot>,
    pub tuesday: Vec<TimeSlot>,
    pub wednesday: Vec<TimeSlot>,
    pub thursday: Vec<TimeSlot>,
    pub friday: Vec<TimeSlot>,
    pub saturday: Vec<TimeSlot>,
    pub sunday: Vec<TimeSlot>,
    /// Days switched off in "Horarios": they keep their slots but get no
    /// reminders. Omitted from the file when empty, so older files load
    /// with every day enabled.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<Weekday>,
}

impl Schedule {
    pub fn is_enabled(&self, day: Weekday) -> bool {
        !self.disabled.contains(&day)
    }

    pub fn set_enabled(&mut self, day: Weekday, enabled: bool) {
        self.disabled.retain(|d| *d != day);
        if !enabled {
            self.disabled.push(day);
            self.disabled.sort();
        }
    }

    /// The slots that count today: none on a disabled day.
    pub fn active(&self, day: Weekday) -> &[TimeSlot] {
        if self.is_enabled(day) {
            self.day(day)
        } else {
            &[]
        }
    }

    pub fn day(&self, day: Weekday) -> &[TimeSlot] {
        match day {
            Weekday::Monday => &self.monday,
            Weekday::Tuesday => &self.tuesday,
            Weekday::Wednesday => &self.wednesday,
            Weekday::Thursday => &self.thursday,
            Weekday::Friday => &self.friday,
            Weekday::Saturday => &self.saturday,
            Weekday::Sunday => &self.sunday,
        }
    }

    pub fn day_mut(&mut self, day: Weekday) -> &mut Vec<TimeSlot> {
        match day {
            Weekday::Monday => &mut self.monday,
            Weekday::Tuesday => &mut self.tuesday,
            Weekday::Wednesday => &mut self.wednesday,
            Weekday::Thursday => &mut self.thursday,
            Weekday::Friday => &mut self.friday,
            Weekday::Saturday => &mut self.saturday,
            Weekday::Sunday => &mut self.sunday,
        }
    }

    /// "Copiar a todos los días hábiles": copies the slots of `from` to
    /// Monday..Friday.
    pub fn copy_to_weekdays(&mut self, from: Weekday) {
        let slots = self.day(from).to_vec();
        for day in Weekday::ALL.into_iter().filter(|d| d.is_workday()) {
            *self.day_mut(day) = slots.clone();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// Full-screen lock on every monitor.
    Lock,
    /// Native Windows notification only.
    Notification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageConfig {
    pub title: String,
    /// Multi-line message (lines separated by `\n`).
    pub body: String,
}

impl Default for MessageConfig {
    fn default() -> Self {
        Self {
            title: "Tomate un descanso".into(),
            body: "Levantate, estirá las piernas y mirá a lo lejos.\nRelajá los hombros y respirá profundo."
                .into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BreakRules {
    pub allow_cancel: bool,
    /// When true, reaching `cancel_limit` consecutive cancels triggers a hard lock.
    pub limit_cancels: bool,
    pub cancel_limit: u8,
    pub postpone_minutes: u32,
    /// Postpones allowed per break, 0..=3. Zero hides the button.
    pub max_postpones: u8,
}

impl Default for BreakRules {
    fn default() -> Self {
        Self {
            allow_cancel: true,
            limit_cancels: false,
            cancel_limit: 3,
            postpone_minutes: 5,
            max_postpones: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IdleConfig {
    pub enabled: bool,
    pub minutes: u32,
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            minutes: 5,
        }
    }
}

/// How many built-in sounds exist (`SoundConfig::builtin` is 1-based).
pub const BUILTIN_SOUNDS: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundSource {
    Builtin,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundConfig {
    pub source: SoundSource,
    /// 1-based index of the built-in sound.
    pub builtin: u8,
    /// Path to a user `.wav`, used when `source = "custom"`.
    pub custom_path: String,
    /// 0..=100.
    pub volume: u8,
    /// Also play a sound when the break ends.
    pub end_sound: bool,
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self {
            source: SoundSource::Builtin,
            builtin: 1,
            custom_path: String::new(),
            volume: 50,
            end_sound: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    Color,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub background: Background,
    pub color: Rgb,
    /// Path to a PNG/JPG, used when `background = "image"`.
    pub image_path: String,
    /// Opacity of the color or image, 0..=100. Single value for both.
    pub opacity: u8,
    pub theme: Theme,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            background: Background::Color,
            color: Rgb {
                r: 0x1E,
                g: 0x3A,
                b: 0x5F,
            },
            image_path: String::new(),
            opacity: 80,
            theme: Theme::Light,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub interval_minutes: u32,
    pub break_minutes: u32,
    pub notice: NoticeKind,
    pub start_with_windows: bool,
    pub message: MessageConfig,
    pub schedule: Schedule,
    pub breaks: BreakRules,
    pub idle: IdleConfig,
    pub sound: SoundConfig,
    pub appearance: AppearanceConfig,
}

impl Default for Config {
    fn default() -> Self {
        let office = vec![TimeSlot::new(
            TimeOfDay::new(9, 0).expect("valid time"),
            TimeOfDay::new(18, 0).expect("valid time"),
        )];
        let mut schedule = Schedule::default();
        for day in Weekday::ALL.into_iter().filter(|d| d.is_workday()) {
            *schedule.day_mut(day) = office.clone();
        }
        Self {
            interval_minutes: 50,
            break_minutes: 5,
            notice: NoticeKind::Lock,
            start_with_windows: false,
            message: MessageConfig::default(),
            schedule,
            breaks: BreakRules::default(),
            idle: IdleConfig::default(),
            sound: SoundConfig::default(),
            appearance: AppearanceConfig::default(),
        }
    }
}

/// A validation failure, with a Spanish message suitable for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    OutOfRange {
        field: &'static str,
        min: u32,
        max: u32,
        value: u32,
    },
    EmptySlot {
        day: Weekday,
        slot: TimeSlot,
    },
    OverlappingSlots {
        day: Weekday,
        first: TimeSlot,
        second: TimeSlot,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::OutOfRange {
                field,
                min,
                max,
                value,
            } => write!(
                f,
                "«{field}» debe estar entre {min} y {max} (valor actual: {value})."
            ),
            ConfigError::EmptySlot { day, slot } => write!(
                f,
                "{}: la franja {}–{} debe empezar antes de terminar.",
                day.label(),
                slot.start,
                slot.end
            ),
            ConfigError::OverlappingSlots { day, first, second } => write!(
                f,
                "{}: las franjas {}–{} y {}–{} se superponen.",
                day.label(),
                first.start,
                first.end,
                second.start,
                second.end
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Checks every rule; returns the first violation found.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let ranges: [(&'static str, u32, u32, u32); 9] = [
            ("Intervalo (min)", self.interval_minutes, 1, 240),
            ("Duración del descanso (min)", self.break_minutes, 1, 60),
            (
                "Cancelaciones seguidas",
                self.breaks.cancel_limit.into(),
                1,
                10,
            ),
            ("Posponer (min)", self.breaks.postpone_minutes, 1, 30),
            (
                "Posponer hasta (veces)",
                self.breaks.max_postpones.into(),
                0,
                3,
            ),
            ("Inactividad (min)", self.idle.minutes, 1, 120),
            (
                "Sonido incluido",
                self.sound.builtin.into(),
                1,
                BUILTIN_SOUNDS.into(),
            ),
            ("Volumen", self.sound.volume.into(), 0, 100),
            ("Opacidad", self.appearance.opacity.into(), 0, 100),
        ];
        for (field, value, min, max) in ranges {
            if !(min..=max).contains(&value) {
                return Err(ConfigError::OutOfRange {
                    field,
                    min,
                    max,
                    value,
                });
            }
        }

        for day in Weekday::ALL {
            let mut slots = self.schedule.day(day).to_vec();
            if let Some(&slot) = slots.iter().find(|s| s.start >= s.end) {
                return Err(ConfigError::EmptySlot { day, slot });
            }
            slots.sort_by_key(|s| s.start);
            if let Some(pair) = slots.windows(2).find(|w| w[1].start < w[0].end) {
                return Err(ConfigError::OverlappingSlots {
                    day,
                    first: pair[0],
                    second: pair[1],
                });
            }
        }
        Ok(())
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("config is always serializable")
    }

    /// Parses TOML. Missing fields take their default value; the result is
    /// not validated (call [`Config::validate`]).
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one field of the defaults
mod tests {
    use super::*;

    fn t(hour: u8, minute: u8) -> TimeOfDay {
        TimeOfDay::new(hour, minute).unwrap()
    }

    fn slot(start: (u8, u8), end: (u8, u8)) -> TimeSlot {
        TimeSlot::new(t(start.0, start.1), t(end.0, end.1))
    }

    #[test]
    fn defaults_match_the_approved_design() {
        let c = Config::default();
        assert_eq!(c.interval_minutes, 50);
        assert_eq!(c.break_minutes, 5);
        assert_eq!(c.notice, NoticeKind::Lock);
        assert!(!c.start_with_windows);
        assert!(c.breaks.allow_cancel);
        assert!(!c.breaks.limit_cancels);
        assert_eq!(c.breaks.postpone_minutes, 5);
        assert_eq!(c.breaks.max_postpones, 1);
        assert!(c.idle.enabled);
        assert_eq!(c.idle.minutes, 5);
        assert_eq!(c.appearance.background, Background::Color);
        assert_eq!(c.appearance.opacity, 80);
        assert_eq!(c.appearance.theme, Theme::Light);
        assert_eq!(c.sound.source, SoundSource::Builtin);
        assert_eq!(c.sound.builtin, 1);
        assert_eq!(c.sound.volume, 50);
        for day in Weekday::ALL {
            let expected = if day.is_workday() {
                vec![slot((9, 0), (18, 0))]
            } else {
                vec![]
            };
            assert_eq!(c.schedule.day(day), expected.as_slice(), "{day:?}");
        }
    }

    #[test]
    fn defaults_are_valid() {
        assert_eq!(Config::default().validate(), Ok(()));
    }

    #[test]
    fn time_of_day_parses_and_prints_hh_mm() {
        assert_eq!(TimeOfDay::parse("09:30"), Some(t(9, 30)));
        assert_eq!(TimeOfDay::parse("00:00"), Some(t(0, 0)));
        assert_eq!(TimeOfDay::parse("23:59"), Some(t(23, 59)));
        for bad in [
            "24:00", "12:60", "9:30", "09:3", "0930", "aa:bb", "", "09:30 ",
        ] {
            assert_eq!(TimeOfDay::parse(bad), None, "{bad:?}");
        }
        assert_eq!(t(7, 5).to_string(), "07:05");
    }

    #[test]
    fn slot_must_start_before_it_ends() {
        let mut c = Config::default();
        let bad = slot((18, 0), (9, 0));
        c.schedule.monday = vec![bad];
        let err = c.validate().unwrap_err();
        assert_eq!(
            err,
            ConfigError::EmptySlot {
                day: Weekday::Monday,
                slot: bad
            }
        );
        assert_eq!(
            err.to_string(),
            "Lunes: la franja 18:00–09:00 debe empezar antes de terminar."
        );

        c.schedule.monday = vec![slot((9, 0), (9, 0))];
        assert!(c.validate().is_err(), "zero-length slot is rejected");
    }

    #[test]
    fn slots_of_the_same_day_must_not_overlap() {
        let mut c = Config::default();
        c.schedule.tuesday = vec![slot((12, 0), (18, 0)), slot((9, 0), (13, 0))];
        let err = c.validate().unwrap_err();
        assert_eq!(
            err.to_string(),
            "Martes: las franjas 09:00–13:00 y 12:00–18:00 se superponen."
        );

        c.schedule.tuesday = vec![slot((9, 0), (13, 0)), slot((13, 0), (18, 0))];
        assert_eq!(c.validate(), Ok(()), "adjacent slots are allowed");
    }

    #[test]
    fn max_postpones_is_limited_to_0_through_3() {
        let mut c = Config::default();
        for ok in 0..=3 {
            c.breaks.max_postpones = ok;
            assert_eq!(c.validate(), Ok(()), "{ok}");
        }
        c.breaks.max_postpones = 4;
        assert_eq!(
            c.validate().unwrap_err().to_string(),
            "«Posponer hasta (veces)» debe estar entre 0 y 3 (valor actual: 4)."
        );
    }

    #[test]
    fn numeric_settings_are_range_checked() {
        type Mutation = fn(&mut Config);
        let cases: [(Mutation, &str); 10] = [
            (|c| c.interval_minutes = 0, "Intervalo (min)"),
            (|c| c.break_minutes = 0, "Duración del descanso (min)"),
            (|c| c.breaks.cancel_limit = 0, "Cancelaciones seguidas"),
            (|c| c.breaks.postpone_minutes = 0, "Posponer (min)"),
            (|c| c.idle.minutes = 0, "Inactividad (min)"),
            (|c| c.sound.volume = 101, "Volumen"),
            (|c| c.appearance.opacity = 101, "Opacidad"),
            (|c| c.interval_minutes = 241, "Intervalo (min)"),
            (|c| c.sound.builtin = 0, "Sonido incluido"),
            (|c| c.sound.builtin = 5, "Sonido incluido"),
        ];
        for (break_it, field) in cases {
            let mut c = Config::default();
            break_it(&mut c);
            match c.validate() {
                Err(ConfigError::OutOfRange { field: f, .. }) => assert_eq!(f, field),
                other => panic!("{field}: expected OutOfRange, got {other:?}"),
            }
        }
    }

    #[test]
    fn copy_to_weekdays_replaces_monday_to_friday_only() {
        let mut c = Config::default();
        let custom = vec![slot((8, 0), (12, 0)), slot((13, 0), (17, 0))];
        c.schedule.wednesday = custom.clone();
        c.schedule.saturday = vec![slot((10, 0), (12, 0))];

        c.schedule.copy_to_weekdays(Weekday::Wednesday);

        for day in Weekday::ALL.into_iter().filter(|d| d.is_workday()) {
            assert_eq!(c.schedule.day(day), custom.as_slice(), "{day:?}");
        }
        assert_eq!(c.schedule.saturday, vec![slot((10, 0), (12, 0))]);
        assert!(c.schedule.sunday.is_empty());
    }

    #[test]
    fn disabled_day_keeps_its_slots_but_has_no_active_hours() {
        let mut c = Config::default();
        let office = c.schedule.monday.clone();
        assert!(c.schedule.is_enabled(Weekday::Monday));

        c.schedule.set_enabled(Weekday::Monday, false);
        assert!(!c.schedule.is_enabled(Weekday::Monday));
        assert_eq!(c.schedule.day(Weekday::Monday), office.as_slice());
        assert!(c.schedule.active(Weekday::Monday).is_empty());
        assert_eq!(c.schedule.active(Weekday::Tuesday), office.as_slice());

        c.schedule.set_enabled(Weekday::Monday, false);
        assert_eq!(c.schedule.disabled, vec![Weekday::Monday]);
        c.schedule.set_enabled(Weekday::Monday, true);
        assert!(c.schedule.is_enabled(Weekday::Monday));
        assert_eq!(c.schedule.active(Weekday::Monday), office.as_slice());
    }

    #[test]
    fn disabled_days_round_trip_and_old_files_load_with_every_day_enabled() {
        let mut c = Config::default();
        c.schedule.set_enabled(Weekday::Sunday, false);
        c.schedule.set_enabled(Weekday::Wednesday, false);
        let text = c.to_toml();
        assert!(text.contains("\"wednesday\""), "{text}");
        assert_eq!(Config::from_toml(&text).unwrap(), c);

        let old =
            Config::from_toml("[schedule]\nmonday = [{ start = \"09:00\", end = \"12:00\" }]")
                .unwrap();
        assert!(Weekday::ALL.iter().all(|d| old.schedule.is_enabled(*d)));
        assert!(!Config::default().to_toml().contains("disabled"));
    }

    #[test]
    fn toml_round_trip_preserves_everything() {
        let mut c = Config::default();
        c.notice = NoticeKind::Notification;
        c.message.body = "Línea 1\nLínea 2".into();
        c.appearance.background = Background::Image;
        c.appearance.image_path = r"C:\Fotos\playa.jpg".into();
        c.appearance.theme = Theme::Dark;
        c.schedule.saturday = vec![slot((10, 0), (12, 30))];

        let text = c.to_toml();
        assert!(text.contains("interval_minutes = 50"), "{text}");
        assert!(text.contains("\"09:00\""), "{text}");
        assert_eq!(Config::from_toml(&text).unwrap(), c);
    }

    #[test]
    fn partial_toml_fills_missing_fields_with_defaults() {
        let c = Config::from_toml("interval_minutes = 30\n[breaks]\nmax_postpones = 2\n").unwrap();
        let mut expected = Config::default();
        expected.interval_minutes = 30;
        expected.breaks.max_postpones = 2;
        assert_eq!(c, expected);
    }

    #[test]
    fn malformed_values_fail_to_parse() {
        assert!(
            Config::from_toml("[schedule]\nmonday = [{ start = \"25:00\", end = \"26:00\" }]")
                .is_err()
        );
        assert!(Config::from_toml("[appearance]\ncolor = \"azul\"").is_err());
        assert!(Config::from_toml("notice = \"popup\"").is_err());
    }
}
