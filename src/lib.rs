//! Rustpira: break reminder for Windows 10/11.
//!
//! Dependencies point inward: `domain` holds pure logic (no Windows, no
//! Slint, no I/O); `storage` persists it; `platform` talks to Windows; `ui`
//! is the Slint code generated from `ui/*.slint`; `app` wires them together.

pub mod domain {
    pub mod config;
    pub mod engine;
    pub mod timer;
}

pub mod storage {
    pub mod config_store;
    pub mod state_store;
}

pub mod platform {
    pub mod autostart;
    pub mod dialogs;
    pub mod keyboard_block;
    pub mod monitors;
    pub mod notify;
    pub mod single_instance;
    pub mod sound;
    pub mod system;
}

#[allow(clippy::all)] // generated code
pub mod ui {
    slint::include_modules!();
}

pub mod app;
pub mod audio;
pub mod break_screen;

pub mod settings {
    pub mod form;
}
