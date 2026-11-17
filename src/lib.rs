//! Rustpira: break reminder for Windows 10/11.
//!
//! `domain` holds pure logic (no Windows, no Slint, no I/O); `storage`
//! persists it to disk.

pub mod domain {
    pub mod config;
    pub mod timer;
}

pub mod storage {
    pub mod config_store;
}
