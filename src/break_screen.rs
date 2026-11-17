//! The break screen: one frameless, always-on-top, translucent window per
//! monitor, plus the keyboard block. Created when a break appears and
//! dropped when it ends, which releases the windows, the background image
//! and the hook (design §5.3, RAM rule in CLAUDE.md).

use std::path::Path;
use std::rc::Rc;

use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;
use slint::winit_030::winit::window::WindowLevel;

use crate::domain::config::{Background, Config};
use crate::domain::engine::BreakView;
use crate::platform::keyboard_block::KeyboardBlock;
use crate::platform::monitors::monitors;
use crate::ui::BreakOverlay;

pub struct BreakScreen {
    windows: Vec<BreakOverlay>,
    _keyboard: Option<KeyboardBlock>,
}

impl BreakScreen {
    pub fn open(
        config: &Config,
        view: &BreakView,
        on_cancel: Rc<dyn Fn()>,
        on_postpone: Rc<dyn Fn()>,
    ) -> Result<Self, slint::PlatformError> {
        let appearance = &config.appearance;
        let image = match appearance.background {
            Background::Image => load_image(&appearance.image_path),
            Background::Color => None,
        };
        let color =
            slint::Color::from_rgb_u8(appearance.color.r, appearance.color.g, appearance.color.b);
        let opacity = f32::from(appearance.opacity) / 100.0;

        let mut windows = Vec::new();
        for monitor in monitors() {
            let window = BreakOverlay::new()?;
            window.set_primary(monitor.primary);
            window.set_bg_color(color);
            window.set_layer_opacity(opacity);
            if let Some(image) = &image {
                window.set_bg_image(image.clone());
                window.set_show_image(true);
            }
            window.set_heading(config.message.title.clone().into());
            window.set_message(config.message.body.clone().into());

            let cancel = on_cancel.clone();
            window.on_cancel(move || cancel());
            let postpone = on_postpone.clone();
            window.on_postpone(move || postpone());
            // Alt+F4 or any other close request must not end the break.
            window
                .window()
                .on_close_requested(|| slint::CloseRequestResponse::KeepWindowShown);

            window
                .window()
                .set_position(slint::PhysicalPosition::new(monitor.x, monitor.y));
            window
                .window()
                .set_size(slint::PhysicalSize::new(monitor.width, monitor.height));
            window.show()?;
            window.window().with_winit_window(|w| {
                use slint::winit_030::winit::platform::windows::WindowExtWindows;
                w.set_skip_taskbar(true);
            });
            windows.push(window);
        }

        let keyboard = KeyboardBlock::install();
        if keyboard.is_none() {
            eprintln!(
                "[rustpira] Windows rechazó el bloqueo de teclado; los atajos siguen activos"
            );
        }
        let screen = Self {
            windows,
            _keyboard: keyboard,
        };
        screen.update(view);
        Ok(screen)
    }

    /// Called every second: refreshes the countdown and buttons, and
    /// re-asserts that the windows stay on top with the focus.
    pub fn update(&self, view: &BreakView) {
        let countdown = format!(
            "{:02}:{:02}",
            view.remaining_secs / 60,
            view.remaining_secs % 60
        );
        for window in &self.windows {
            window.set_countdown(countdown.clone().into());
            window.set_hard(view.hard);
            window.set_can_cancel(view.can_cancel);
            window.set_can_postpone(view.can_postpone);
            window.set_postpone_label(format!("Posponer {} min", view.postpone_minutes).into());
            let primary = window.get_primary();
            window.window().with_winit_window(|w| {
                w.set_window_level(WindowLevel::AlwaysOnTop);
                if primary {
                    w.focus_window();
                }
            });
        }
    }
}

impl Drop for BreakScreen {
    fn drop(&mut self) {
        for window in &self.windows {
            let _ = window.hide();
        }
    }
}

/// Loads the user's background image; on failure the color is used.
fn load_image(path: &str) -> Option<slint::Image> {
    if path.is_empty() {
        return None;
    }
    match slint::Image::load_from_path(Path::new(path)) {
        Ok(image) => Some(image),
        Err(_) => {
            eprintln!("[rustpira] no se pudo cargar la imagen de fondo «{path}»; se usa el color");
            None
        }
    }
}
