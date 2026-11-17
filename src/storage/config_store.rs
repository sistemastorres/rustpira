//! Loads and saves the configuration file (`%APPDATA%\rustpira\config.toml`).
//!
//! Loading never fails: a missing file yields the defaults, and an unreadable
//! or invalid file yields the defaults plus a warning, after copying the bad
//! file to `config.toml.bak` so the user does not lose it.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::domain::config::Config;

const APP_DIR: &str = "rustpira";
const FILE_NAME: &str = "config.toml";

/// `%APPDATA%\rustpira\config.toml`, or `None` if `APPDATA` is not set.
pub fn default_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join(APP_DIR).join(FILE_NAME))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadWarning {
    /// The file exists but could not be read.
    Unreadable { reason: String },
    /// The file was read but is not a valid configuration.
    Invalid {
        reason: String,
        /// Where the bad file was copied, if the copy succeeded.
        backup: Option<PathBuf>,
    },
}

impl fmt::Display for LoadWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadWarning::Unreadable { reason } => write!(
                f,
                "No se pudo leer la configuración ({reason}). Se usan los valores por defecto."
            ),
            LoadWarning::Invalid {
                reason,
                backup: Some(backup),
            } => write!(
                f,
                "La configuración guardada no es válida: {reason} Se usan los valores por defecto; \
                 se guardó una copia en «{}».",
                backup.display()
            ),
            LoadWarning::Invalid {
                reason,
                backup: None,
            } => write!(
                f,
                "La configuración guardada no es válida: {reason} Se usan los valores por defecto."
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub config: Config,
    pub warning: Option<LoadWarning>,
}

/// Reads the configuration at `path`. See the module docs for fallbacks.
pub fn load(path: &Path) -> Loaded {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return defaults(None),
        Err(e) => {
            return defaults(Some(LoadWarning::Unreadable {
                reason: e.to_string(),
            }));
        }
    };

    let reason = match Config::from_toml(&text) {
        Ok(config) => match config.validate() {
            Ok(()) => {
                return Loaded {
                    config,
                    warning: None,
                };
            }
            Err(e) => e.to_string(),
        },
        Err(e) => match e.span() {
            Some(span) => format!(
                "error de formato en la línea {}.",
                text[..span.start].matches('\n').count() + 1
            ),
            None => "error de formato.".to_string(),
        },
    };

    let backup_path = sibling(path, "bak");
    let backup = std::fs::write(&backup_path, &text)
        .ok()
        .map(|()| backup_path);
    defaults(Some(LoadWarning::Invalid { reason, backup }))
}

/// Writes the configuration atomically (temp file + rename), creating the
/// parent directory if needed.
pub fn save(path: &Path, config: &Config) -> io::Result<()> {
    write_atomic(path, &config.to_toml())
}

/// Writes `contents` to a sibling temp file and renames it over `path`, so a
/// crash or power loss never leaves a half-written file. Creates the parent
/// directory if needed.
pub(crate) fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = sibling(path, "tmp");
    std::fs::write(&temp, contents)?;
    // On Windows, rename replaces an existing destination.
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

fn defaults(warning: Option<LoadWarning>) -> Loaded {
    Loaded {
        config: Config::default(),
        warning,
    }
}

/// `config.toml` -> `config.toml.<suffix>` in the same directory.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one field of the defaults
mod tests {
    use super::*;

    fn temp_config_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(FILE_NAME);
        (dir, path)
    }

    #[test]
    fn missing_file_yields_defaults_without_warning_or_side_effects() {
        let (_dir, path) = temp_config_path();
        let loaded = load(&path);
        assert_eq!(loaded.config, Config::default());
        assert_eq!(loaded.warning, None);
        assert!(!path.exists(), "load must not create the file");
    }

    #[test]
    fn save_creates_the_directory_and_load_reads_it_back() {
        let (_dir, path) = temp_config_path();
        let mut config = Config::default();
        config.interval_minutes = 25;
        config.message.title = "Pausa activa".into();

        save(&path, &config).unwrap();
        let loaded = load(&path);

        assert_eq!(loaded.config, config);
        assert_eq!(loaded.warning, None);
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(leftovers, vec![FILE_NAME.to_string()], "no temp files left");
    }

    #[test]
    fn syntax_error_yields_defaults_warning_and_backup() {
        let (_dir, path) = temp_config_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "break_minutes = 5\ninterval_minutes = = 50\n";
        std::fs::write(&path, original).unwrap();

        let loaded = load(&path);

        assert_eq!(loaded.config, Config::default());
        let backup = path.with_file_name("config.toml.bak");
        match &loaded.warning {
            Some(LoadWarning::Invalid {
                reason,
                backup: Some(b),
            }) => {
                assert_eq!(reason, "error de formato en la línea 2.");
                assert_eq!(b, &backup);
            }
            other => panic!("expected Invalid with backup, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "the original file is left untouched"
        );
    }

    #[test]
    fn out_of_range_value_is_treated_as_invalid_with_spanish_reason() {
        let (_dir, path) = temp_config_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[breaks]\nmax_postpones = 9\n").unwrap();

        let loaded = load(&path);

        assert_eq!(loaded.config, Config::default());
        let warning = loaded.warning.expect("warning");
        let text = warning.to_string();
        assert!(
            text.starts_with(
                "La configuración guardada no es válida: «Posponer hasta (veces)» debe estar entre 0 y 3 (valor actual: 9)."
            ),
            "{text}"
        );
        assert!(text.contains("config.toml.bak"), "{text}");
    }
}
