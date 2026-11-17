//! Runtime state that must survive restarts (design §5.9): the
//! consecutive-cancel counter, so quitting and reopening the app is not a
//! way around the hard lock. Stored in `%APPDATA%\rustpira\state.toml`,
//! separate from the user preferences in `config.toml`.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage::config_store;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    consecutive_cancels: u8,
}

/// `%APPDATA%\rustpira\state.toml`, or `None` if `APPDATA` is not set.
pub fn default_path() -> Option<PathBuf> {
    config_store::default_path().map(|p| p.with_file_name("state.toml"))
}

/// Reads the counter. A missing or invalid file counts as zero.
pub fn load_cancels(path: &Path) -> u8 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<State>(&text).ok())
        .unwrap_or_default()
        .consecutive_cancels
}

pub fn save_cancels(path: &Path, consecutive_cancels: u8) -> io::Result<()> {
    let text = toml::to_string(&State {
        consecutive_cancels,
    })
    .expect("state is always serializable");
    config_store::write_atomic(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_counts_as_zero() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_cancels(&dir.path().join("state.toml")), 0);
    }

    #[test]
    fn saved_counter_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("state.toml");
        save_cancels(&path, 2).unwrap();
        assert_eq!(load_cancels(&path), 2);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap().trim(),
            "consecutive_cancels = 2"
        );
    }

    #[test]
    fn invalid_file_counts_as_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        std::fs::write(&path, "consecutive_cancels = \"muchas\"").unwrap();
        assert_eq!(load_cancels(&path), 0);
    }
}
