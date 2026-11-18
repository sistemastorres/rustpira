//! "Acerca de" (design §5.8, S21): icon, version, license, developer and
//! links. Version and repository come from `Cargo.toml`. The Slint
//! attribution lives in the README instead (clause 2(b) of its
//! royalty-free license). The owner drops the window when `on_close` is
//! called, which frees it.

use std::rc::Rc;

use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;

use crate::platform::system;
use crate::ui::About;

pub const DEVELOPER: &str = "Sistemas Torres";
pub const DEVELOPER_SITE: &str = "https://www.sistemastorres.com/";
pub const DEVELOPER_MAIL: &str = "sistemastorres.info@gmail.com";

pub struct AboutWindow {
    window: About,
}

fn open_or_log(link: &str) {
    if !system::open_link(link) {
        eprintln!("[rustpira] no se pudo abrir el enlace «{link}»");
    }
}

impl AboutWindow {
    /// `on_close` runs when the window must close ("Cerrar" or the X); the
    /// owner should drop the `AboutWindow` on the next event-loop turn.
    pub fn open(dark_theme: bool, on_close: Box<dyn Fn()>) -> Result<Self, slint::PlatformError> {
        let window = About::new()?;
        window.set_version(env!("CARGO_PKG_VERSION").into());
        window.set_developer(DEVELOPER.into());
        window.set_site(DEVELOPER_SITE.into());
        window.set_mail(DEVELOPER_MAIL.into());
        window.set_repository(env!("CARGO_PKG_REPOSITORY").into());
        window.set_theme_index(i32::from(dark_theme));

        window.on_open_site(|| open_or_log(DEVELOPER_SITE));
        window.on_open_mail(|| open_or_log(&format!("mailto:{DEVELOPER_MAIL}")));
        window.on_open_repository(|| open_or_log(env!("CARGO_PKG_REPOSITORY")));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_about_link_is_a_link_the_app_will_open() {
        assert!(system::is_safe_link(DEVELOPER_SITE));
        assert!(system::is_safe_link(&format!("mailto:{DEVELOPER_MAIL}")));
        assert!(system::is_safe_link(env!("CARGO_PKG_REPOSITORY")));
    }
}
