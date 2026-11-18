//! "Acerca de" (design §5.8): icon, version, license, repository link and
//! the Slint attribution its royalty-free license asks for. Version and
//! repository come from `Cargo.toml`. The owner drops the window when
//! `on_close` is called, which frees it.

use std::rc::Rc;

use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;

use crate::platform::system;
use crate::ui::About;

pub struct AboutWindow {
    window: About,
}

impl AboutWindow {
    /// `on_close` runs when the window must close ("Cerrar" or the X); the
    /// owner should drop the `AboutWindow` on the next event-loop turn.
    pub fn open(dark_theme: bool, on_close: Box<dyn Fn()>) -> Result<Self, slint::PlatformError> {
        let window = About::new()?;
        window.set_version(env!("CARGO_PKG_VERSION").into());
        window.set_repository(env!("CARGO_PKG_REPOSITORY").into());
        window.set_theme_index(i32::from(dark_theme));

        window.on_open_repository(|| {
            let url = env!("CARGO_PKG_REPOSITORY");
            if !system::open_url(url) {
                eprintln!("[rustpira] no se pudo abrir el enlace «{url}»");
            }
        });
        let on_close = Rc::new(on_close);
        let close = on_close.clone();
        window.on_close_clicked(move || close());
        window.window().on_close_requested(move || {
            on_close();
            slint::CloseRequestResponse::HideWindow
        });

        window.show()?;
        Ok(Self { window })
    }

    pub fn bring_to_front(&self) {
        let _ = self.window.show();
        self.window.window().with_winit_window(|w| w.focus_window());
    }
}

impl Drop for AboutWindow {
    fn drop(&mut self) {
        let _ = self.window.hide();
    }
}
