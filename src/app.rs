//! Wires the domain engine to Windows and the Slint UI: loads the files,
//! ticks the engine once per second, mirrors its snapshot into the tray and
//! the break screen, and routes menu and button actions back to it.

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use crate::break_screen::BreakScreen;
use crate::domain::config::Config;
use crate::domain::engine::{BreakView, Effect, Engine};
use crate::platform::notify;
use crate::platform::single_instance::{self, Instance, SingleInstance};
use crate::platform::sound::Player;
use crate::platform::system;
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

    let tray = Tray::new()?;
    let app = Rc::new_cyclic(|this| App {
        tray,
        engine: RefCell::new(Engine::new(state_store::load_cancels(&state_path))),
        config: RefCell::new(loaded.config),
        state_path,
        instance,
        session_locked: Cell::new(false),
        break_screen: RefCell::new(None),
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

    app.tray.on_about(|| {
        // T9: "Acerca de" window.
        eprintln!("[rustpira] acerca de: pendiente (T9)");
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
                Effect::ShowNotification => {
                    let config = self.config.borrow();
                    notify_or_log(&config.message.title, &config.message.body);
                    self.player.play(&config.sound);
                }
            }
        }
    }

    fn open_settings(&self) {
        // T8: settings window.
        eprintln!("[rustpira] configuración: pendiente (T8)");
    }
}
