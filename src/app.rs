//! Wires the domain engine to Windows and the Slint UI: loads the files,
//! ticks the engine once per second, mirrors its snapshot into the tray and
//! the break screen, and routes menu and button actions back to it.

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use crate::about::AboutWindow;
use crate::break_screen::BreakScreen;
use crate::domain::config::{Config, Theme};
use crate::domain::engine::{BreakView, Effect, Engine};
use crate::platform::autostart;
use crate::platform::notify;
use crate::platform::single_instance::{self, Instance, SingleInstance};
use crate::platform::sound::Player;
use crate::platform::system;
use crate::settings::window::{Hooks, SettingsWindow};
use crate::storage::{config_store, state_store};
use crate::ui::Tray;

struct App {
    tray: Tray,
    engine: RefCell<Engine>,
    config: RefCell<Config>,
    state_path: PathBuf,
    instance: SingleInstance,
    session_locked: Cell<bool>,
    break_screen: RefCell<Option<BreakScreen>>,
    settings: RefCell<Option<SettingsWindow>>,
    about: RefCell<Option<AboutWindow>>,
    config_path: PathBuf,
    player: Player,
    this: Weak<App>,
}

/// Shows a Windows notification; if that is not possible, logs the text.
fn notify_or_log(title: &str, message: &str) {
    if !notify::show(title, message) {
        eprintln!("[rustpira] no se pudo mostrar la notificación: {title}: {message}");
    }
}

/// Runs the application until "Salir".
pub fn run() -> Result<(), Box<dyn Error>> {
    let instance = match single_instance::acquire() {
        Instance::Primary(guard) => guard,
        Instance::Secondary => return Ok(()),
    };

    let config_path = config_store::default_path().ok_or("la variable APPDATA no está definida")?;
    let state_path = state_store::default_path().ok_or("la variable APPDATA no está definida")?;
    let loaded = config_store::load(&config_path);
    if loaded.config.start_with_windows {
        // Keeps the Run entry pointing at this exe if the portable exe moved.
        if let Err(e) = autostart::set(true) {
            eprintln!("[rustpira] {e}");
        }
    }

    let tray = Tray::new()?;
    let app = Rc::new_cyclic(|this| App {
        tray,
        engine: RefCell::new(Engine::new(state_store::load_cancels(&state_path))),
        config: RefCell::new(loaded.config),
        state_path,
        instance,
        session_locked: Cell::new(false),
        break_screen: RefCell::new(None),
        settings: RefCell::new(None),
        about: RefCell::new(None),
        config_path,
        player: Player::new(),
        this: this.clone(),
    });
    wire_tray(&app);
    app.tick();
    app.tray.show()?;

    if let Some(warning) = loaded.warning {
        // Give the tray icon a moment to register before notifying.
        slint::Timer::single_shot(Duration::from_secs(1), move || {
            notify_or_log("Rustpira: configuración", &warning.to_string());
        });
    }

    let timer = slint::Timer::default();
    let weak = Rc::downgrade(&app);
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(1),
        move || {
            if let Some(app) = weak.upgrade() {
                app.tick();
            }
        },
    );

    slint::run_event_loop_until_quit()?;
    Ok(())
}

fn wire_tray(app: &Rc<App>) {
    let weak = Rc::downgrade(app);
    app.tray.on_disable_for(move |minutes| {
        if let Some(app) = weak.upgrade() {
            let minutes = u32::try_from(minutes).unwrap_or(0);
            app.engine.borrow_mut().disable_for(system::now(), minutes);
            app.refresh();
        }
    });

    let weak = Rc::downgrade(app);
    app.tray.on_reactivate(move || {
        if let Some(app) = weak.upgrade() {
            app.engine.borrow_mut().reactivate();
            app.tick();
        }
    });

    let weak = Rc::downgrade(app);
    app.tray.on_open_settings(move || {
        if let Some(app) = weak.upgrade() {
            app.open_settings();
        }
    });

    let weak = Rc::downgrade(app);
    app.tray.on_about(move || {
        if let Some(app) = weak.upgrade() {
            app.open_about();
        }
    });

    let weak = Rc::downgrade(app);
    app.tray.on_quit(move || {
        // The menu item is disabled during a break; double-check anyway.
        let allowed = weak.upgrade().is_some_and(|app| {
            let config = app.config.borrow();
            let engine = app.engine.borrow();
            engine.snapshot(&config, system::now()).menu.quit_enabled
        });
        if allowed {
            let _ = slint::quit_event_loop();
        }
    });
}

impl App {
    fn tick(&self) {
        let now = system::now();

        let locked = system::session_locked();
        if locked != self.session_locked.replace(locked) {
            let mut engine = self.engine.borrow_mut();
            if locked {
                engine.on_session_lock();
            } else {
                engine.on_session_unlock();
            }
        }

        let effects = {
            let config = self.config.borrow();
            self.engine
                .borrow_mut()
                .tick(&config, now, system::idle_secs())
        };
        self.apply(&effects);

        if self.instance.take_open_request() {
            self.open_settings();
        }
        self.refresh();
    }

    /// Mirrors the engine snapshot into the tray icon, the menu and the
    /// break screen (opened, updated or closed to match).
    fn refresh(&self) {
        let snapshot = {
            let config = self.config.borrow();
            self.engine.borrow().snapshot(&config, system::now())
        };
        self.tray.set_tip(snapshot.tooltip.into());
        self.tray
            .set_settings_enabled(snapshot.menu.settings_enabled);
        self.tray.set_disable_enabled(snapshot.menu.disable_enabled);
        self.tray.set_quit_enabled(snapshot.menu.quit_enabled);
        self.tray.set_show_reactivate(snapshot.menu.show_reactivate);
        self.sync_break_screen(snapshot.break_view);
    }

    fn sync_break_screen(&self, view: Option<BreakView>) {
        let mut screen = self.break_screen.borrow_mut();
        match (view, screen.as_ref()) {
            (Some(view), Some(open)) => open.update(&view),
            (Some(view), None) => {
                let config = self.config.borrow();
                let cancel = self.deferred(App::cancel_break);
                let postpone = self.deferred(App::postpone_break);
                match BreakScreen::open(&config, &view, cancel, postpone) {
                    Ok(opened) => *screen = Some(opened),
                    Err(e) => eprintln!("[rustpira] no se pudo abrir la pantalla de descanso: {e}"),
                }
            }
            // Dropping the screen releases its windows, image and hook.
            (None, Some(_)) => *screen = None,
            (None, None) => {}
        }
    }

    /// Wraps a button action so it runs on the next event-loop turn: the
    /// action may close the very window whose callback is running.
    fn deferred(&self, action: fn(&App)) -> Rc<dyn Fn()> {
        let weak = self.this.clone();
        Rc::new(move || {
            let weak = weak.clone();
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let Some(app) = weak.upgrade() {
                    action(&app);
                }
            });
        })
    }

    fn cancel_break(&self) {
        let effects = {
            let config = self.config.borrow();
            self.engine.borrow_mut().cancel(&config)
        };
        self.apply(&effects);
        self.refresh();
    }

    fn postpone_break(&self) {
        let effects = {
            let config = self.config.borrow();
            self.engine.borrow_mut().postpone(&config, system::now())
        };
        self.apply(&effects);
        self.refresh();
    }

    fn apply(&self, effects: &[Effect]) {
        for effect in effects {
            match effect {
                Effect::CancelCountChanged(count) => {
                    if let Err(e) = state_store::save_cancels(&self.state_path, *count) {
                        eprintln!("[rustpira] no se pudo guardar el contador: {e}");
                    }
                }
                // The break screen itself follows the snapshot (see `refresh`).
                Effect::BreakStarted => self.player.play(&self.config.borrow().sound),
                Effect::BreakEnded { completed } => {
                    let config = self.config.borrow();
                    if *completed && config.sound.end_sound {
                        self.player.play(&config.sound);
                    }
                }
                Effect::IdleEnded => {
                    let minutes = self.config.borrow().interval_minutes;
                    notify_or_log(
                        "Rustpira",
                        &format!(
                            "Se detectó actividad otra vez. El contador volvió a empezar: el próximo descanso es en {minutes} min."
                        ),
                    );
                }
                Effect::ShowNotification => {
                    let config = self.config.borrow();
                    notify_or_log(&config.message.title, &config.message.body);
                    self.player.play(&config.sound);
                }
            }
        }
    }

    /// Opens the settings window, or brings it to the front if it is open.
    /// Never during a break: that would be a way around the hard lock.
    fn open_settings(&self) {
        let allowed = {
            let config = self.config.borrow();
            let engine = self.engine.borrow();
            engine
                .snapshot(&config, system::now())
                .menu
                .settings_enabled
        };
        if !allowed {
            return;
        }
        if let Some(open) = self.settings.borrow().as_ref() {
            open.bring_to_front();
            return;
        }

        let (save, test, close) = (self.this.clone(), self.this.clone(), self.this.clone());
        let hooks = Hooks {
            save: Box::new(move |config| match save.upgrade() {
                Some(app) => app.save_settings(config),
                None => Ok(()),
            }),
            test_sound: Box::new(move |sound| {
                if let Some(app) = test.upgrade() {
                    app.player.play(sound);
                }
            }),
            close: Box::new(move || {
                let close = close.clone();
                // Dropped on the next turn: this runs inside the window's own callback.
                slint::Timer::single_shot(Duration::ZERO, move || {
                    if let Some(app) = close.upgrade() {
                        app.settings.borrow_mut().take();
                    }
                });
            }),
        };
        let opened = SettingsWindow::open(&self.config.borrow(), hooks);
        match opened {
            Ok(window) => *self.settings.borrow_mut() = Some(window),
            Err(e) => eprintln!("[rustpira] no se pudo abrir la configuración: {e}"),
        }
    }

    /// Opens "Acerca de", or brings it to the front if it is open. Allowed
    /// during a break (design §5.1): it changes nothing.
    fn open_about(&self) {
        if let Some(open) = self.about.borrow().as_ref() {
            open.bring_to_front();
            return;
        }
        let dark = self.config.borrow().appearance.theme == Theme::Dark;
        let weak = self.this.clone();
        let on_close = Box::new(move || {
            let weak = weak.clone();
            // Dropped on the next turn: this runs inside the window's own callback.
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let Some(app) = weak.upgrade() {
                    app.about.borrow_mut().take();
                }
            });
        });
        match AboutWindow::open(dark, on_close) {
            Ok(window) => *self.about.borrow_mut() = Some(window),
            Err(e) => eprintln!("[rustpira] no se pudo abrir «Acerca de»: {e}"),
        }
    }

    /// Persists and applies a validated configuration from the settings
    /// window. The counter restarts (design §5.2).
    fn save_settings(&self, config: Config) -> Result<(), String> {
        config_store::save(&self.config_path, &config)
            .map_err(|e| format!("No se pudo guardar la configuración: {e}"))?;
        let autostart = autostart::set(config.start_with_windows);
        *self.config.borrow_mut() = config;
        self.engine.borrow_mut().config_changed();
        self.refresh();
        autostart.map_err(|e| format!("Se guardó la configuración, pero {e}."))
    }
}
