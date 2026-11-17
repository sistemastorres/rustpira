//! The settings window: fills `ui/settings.slint` from a `Form`, keeps the
//! schedule draft, runs the native pickers on a worker thread and hands a
//! validated `Config` to the owner on "Guardar". The owner drops it when
//! `Hooks::close` is called, which frees the window (design §5.7).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::audio::BUILTIN_NAMES;
use crate::domain::config::{Config, Rgb, SoundConfig, Weekday};
use crate::platform::dialogs;
use crate::settings::form::{Form, SOUND_CUSTOM};
use crate::ui::{DayRow, Settings, SlotRow};

/// What the owner does with the window's results.
pub struct Hooks {
    /// Applies a validated configuration; an `Err` is shown in the window.
    pub save: Box<dyn Fn(Config) -> Result<(), String>>,
    /// "Probar": plays the sound as currently chosen.
    pub test_sound: Box<dyn Fn(&SoundConfig)>,
    /// The window must close (saved, cancelled or closed with the X). The
    /// owner should drop the `SettingsWindow` on the next event-loop turn.
    pub close: Box<dyn Fn()>,
}

pub struct SettingsWindow {
    window: Settings,
}

/// Only one native picker at a time.
static PICKER_OPEN: AtomicBool = AtomicBool::new(false);

impl SettingsWindow {
    pub fn open(config: &Config, hooks: Hooks) -> Result<Self, slint::PlatformError> {
        let window = Settings::new()?;
        let form = Rc::new(RefCell::new(Form::from_config(config)));
        load(&window, &form.borrow());
        wire(&window, &form, Rc::new(hooks));
        window.show()?;
        Ok(Self { window })
    }

    pub fn bring_to_front(&self) {
        let _ = self.window.show();
        self.window.window().with_winit_window(|w| w.focus_window());
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        let _ = self.window.hide();
    }
}

fn color_of(text: &str) -> slint::Color {
    Rgb::parse(text.trim()).map_or(slint::Color::from_argb_u8(0, 0, 0, 0), |c| {
        slint::Color::from_rgb_u8(c.r, c.g, c.b)
    })
}

fn days_model(form: &Form) -> ModelRc<DayRow> {
    let rows: Vec<DayRow> = Weekday::ALL
        .iter()
        .zip(&form.days)
        .map(|(day, slots)| {
            let slots: Vec<SlotRow> = slots
                .iter()
                .map(|slot| SlotRow {
                    start: slot.start.as_str().into(),
                    end: slot.end.as_str().into(),
                })
                .collect();
            DayRow {
                name: day.label().into(),
                slots: ModelRc::new(VecModel::from(slots)),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Form → widgets.
fn load(window: &Settings, form: &Form) {
    window.set_interval_minutes(form.interval_minutes);
    window.set_break_minutes(form.break_minutes);
    window.set_notice_index(i32::from(form.notification_only));
    window.set_start_with_windows(form.start_with_windows);
    window.set_heading(form.title.as_str().into());
    window.set_message(form.body.as_str().into());
    window.set_days(days_model(form));
    window.set_allow_cancel(form.allow_cancel);
    window.set_limit_cancels(form.limit_cancels);
    window.set_cancel_limit(form.cancel_limit);
    window.set_postpone_minutes(form.postpone_minutes);
    window.set_max_postpones(form.max_postpones);
    window.set_idle_enabled(form.idle_enabled);
    window.set_idle_minutes(form.idle_minutes);
    window.set_background_index(i32::from(form.background_image));
    window.set_color_text(form.color.as_str().into());
    window.set_color_preview(color_of(&form.color));
    window.set_image_path(form.image_path.as_str().into());
    window.set_layer_opacity(form.opacity as f32);
    window.set_theme_index(i32::from(form.dark_theme));
    let names: Vec<SharedString> = BUILTIN_NAMES
        .iter()
        .map(|name| SharedString::from(*name))
        .chain([SharedString::from("Archivo propio (.wav)")])
        .collect();
    window.set_sound_names(ModelRc::new(VecModel::from(names)));
    window.set_custom_sound_index(SOUND_CUSTOM);
    window.set_sound_index(form.sound);
    window.set_custom_sound_path(form.custom_sound_path.as_str().into());
    window.set_volume(form.volume as f32);
    window.set_end_sound(form.end_sound);
}

/// Widgets → form. The schedule is kept in the form as it is edited.
fn read(window: &Settings, form: &mut Form) {
    form.interval_minutes = window.get_interval_minutes();
    form.break_minutes = window.get_break_minutes();
    form.notification_only = window.get_notice_index() == 1;
    form.start_with_windows = window.get_start_with_windows();
    form.title = window.get_heading().into();
    form.body = window.get_message().into();
    form.allow_cancel = window.get_allow_cancel();
    form.limit_cancels = window.get_limit_cancels();
    form.cancel_limit = window.get_cancel_limit();
    form.postpone_minutes = window.get_postpone_minutes();
    form.max_postpones = window.get_max_postpones();
    form.idle_enabled = window.get_idle_enabled();
    form.idle_minutes = window.get_idle_minutes();
    form.background_image = window.get_background_index() == 1;
    form.color = window.get_color_text().into();
    form.image_path = window.get_image_path().into();
    form.opacity = window.get_layer_opacity().round() as i32;
    form.dark_theme = window.get_theme_index() == 1;
    form.sound = window.get_sound_index();
    form.custom_sound_path = window.get_custom_sound_path().into();
    form.volume = window.get_volume().round() as i32;
    form.end_sound = window.get_end_sound();
}

/// Runs a blocking native dialog on a worker thread and hands its result
/// to `apply` on the UI thread, if the window still exists.
fn pick_in_background<T: Send + 'static>(
    window: &Settings,
    pick: impl FnOnce() -> Option<T> + Send + 'static,
    apply: impl FnOnce(&Settings, T) + Send + 'static,
) {
    if PICKER_OPEN.swap(true, Ordering::SeqCst) {
        return;
    }
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let result = pick();
        PICKER_OPEN.store(false, Ordering::SeqCst);
        if let Some(value) = result {
            let _ = weak.upgrade_in_event_loop(move |window| apply(&window, value));
        }
    });
}

fn wire(window: &Settings, form: &Rc<RefCell<Form>>, hooks: Rc<Hooks>) {
    let edit_slot = |form: &Rc<RefCell<Form>>, is_start: bool| {
        let form = form.clone();
        move |day: i32, index: i32, text: SharedString| {
            let mut form = form.borrow_mut();
            let slot = form
                .days
                .get_mut(day as usize)
                .and_then(|slots| slots.get_mut(index as usize));
            if let Some(slot) = slot {
                if is_start {
                    slot.start = text.into();
                } else {
                    slot.end = text.into();
                }
            }
        }
    };
    window.on_start_edited(edit_slot(form, true));
    window.on_end_edited(edit_slot(form, false));

    let schedule_action = |action: fn(&mut Form, usize, usize)| {
        let form = form.clone();
        let weak = window.as_weak();
        move |day: i32, index: i32| {
            let mut form = form.borrow_mut();
            action(&mut form, day as usize, index as usize);
            if let Some(window) = weak.upgrade() {
                window.set_days(days_model(&form));
            }
        }
    };
    let add = schedule_action(|form, day, _| form.add_slot(day));
    window.on_add_slot(move |day| add(day, 0));
    let copy = schedule_action(|form, day, _| form.copy_to_weekdays(day));
    window.on_copy_to_weekdays(move |day| copy(day, 0));
    window.on_remove_slot(schedule_action(Form::remove_slot));

    let weak = window.as_weak();
    window.on_color_edited(move |text| {
        if let Some(window) = weak.upgrade() {
            window.set_color_preview(color_of(&text));
        }
    });

    let weak = window.as_weak();
    window.on_pick_color(move || {
        let Some(window) = weak.upgrade() else { return };
        let initial = Rgb::parse(window.get_color_text().trim())
            .unwrap_or(Config::default().appearance.color);
        pick_in_background(
            &window,
            move || dialogs::pick_color(initial),
            |window, color| {
                let text = color.to_string();
                window.set_color_preview(color_of(&text));
                window.set_color_text(text.into());
            },
        );
    });

    let weak = window.as_weak();
    window.on_pick_image(move || {
        let Some(window) = weak.upgrade() else { return };
        let current = window.get_image_path().to_string();
        pick_in_background(
            &window,
            move || {
                dialogs::pick_file(
                    "Elegir la imagen de fondo",
                    "Imágenes",
                    &["*.png", "*.jpg", "*.jpeg"],
                    Path::new(&current),
                )
            },
            |window, path| window.set_image_path(path.to_string_lossy().as_ref().into()),
        );
    });

    let weak = window.as_weak();
    window.on_pick_sound(move || {
        let Some(window) = weak.upgrade() else { return };
        let current = window.get_custom_sound_path().to_string();
        pick_in_background(
            &window,
            move || {
                dialogs::pick_file(
                    "Elegir un sonido",
                    "Sonidos WAV",
                    &["*.wav"],
                    Path::new(&current),
                )
            },
            |window, path| window.set_custom_sound_path(path.to_string_lossy().as_ref().into()),
        );
    });

    let (weak, form_ref, hooks_ref) = (window.as_weak(), form.clone(), hooks.clone());
    window.on_test_sound(move || {
        let Some(window) = weak.upgrade() else { return };
        let mut form = form_ref.borrow_mut();
        read(&window, &mut form);
        (hooks_ref.test_sound)(&form.sound_config());
    });

    let (weak, form_ref, hooks_ref) = (window.as_weak(), form.clone(), hooks.clone());
    window.on_save(move || {
        let Some(window) = weak.upgrade() else { return };
        let result = {
            let mut form = form_ref.borrow_mut();
            read(&window, &mut form);
            form.to_config()
        };
        match result.and_then(|config| (hooks_ref.save)(config)) {
            Ok(()) => (hooks_ref.close)(),
            Err(message) => window.set_error(message.into()),
        }
    });

    let hooks_ref = hooks.clone();
    window.on_cancel(move || (hooks_ref.close)());

    window.window().on_close_requested(move || {
        (hooks.close)();
        slint::CloseRequestResponse::HideWindow
    });
}
