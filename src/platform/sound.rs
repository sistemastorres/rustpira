//! Plays the break sounds with `PlaySoundW` from memory, so the volume can
//! be applied to the samples (`crate::audio`) without a mixer dependency.

use std::cell::RefCell;
use std::path::Path;

use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

use crate::audio;
use crate::domain::config::{SoundConfig, SoundSource};

/// Biggest custom `.wav` accepted; it is held in memory while it plays.
pub const MAX_CUSTOM_BYTES: u64 = 20 * 1024 * 1024;

/// Reads the user's `.wav` and applies `volume`. The error is a Spanish
/// message for the log or the settings window.
pub fn load_custom(path: &Path, volume: u8) -> Result<Vec<u8>, String> {
    let open_error =
        |e: std::io::Error| format!("no se pudo abrir el sonido «{}»: {e}", path.display());
    let size = std::fs::metadata(path).map_err(open_error)?.len();
    if size > MAX_CUSTOM_BYTES {
        return Err("el sonido propio pesa más de 20 MB".into());
    }
    let bytes = std::fs::read(path).map_err(open_error)?;
    audio::with_volume(&bytes, volume)
        .map_err(|e| format!("el sonido propio no se puede usar: {e}"))
}

/// Owns the sound being played: `PlaySoundW` with `SND_MEMORY | SND_ASYNC`
/// reads the buffer while it plays, so it must outlive the playback.
#[derive(Default)]
pub struct Player {
    playing: RefCell<Option<Vec<u8>>>,
}

impl Player {
    pub fn new() -> Self {
        Self::default()
    }

    /// Plays the configured sound. A custom sound that cannot be used falls
    /// back to the first built-in one (and is logged). Volume 0 is silence.
    pub fn play(&self, sound: &SoundConfig) {
        self.stop();
        if sound.volume == 0 {
            return;
        }
        let wav = match sound.source {
            SoundSource::Builtin => audio::builtin(sound.builtin, sound.volume),
            SoundSource::Custom => load_custom(Path::new(&sound.custom_path), sound.volume)
                .unwrap_or_else(|error| {
                    eprintln!("[rustpira] {error}; se usa el sonido incluido");
                    audio::builtin(1, sound.volume)
                }),
        };
        // SAFETY: with SND_MEMORY the first argument points to a complete
        // WAV image, kept alive in `self.playing` until the next play/stop.
        let started = unsafe {
            PlaySoundW(
                wav.as_ptr().cast(),
                std::ptr::null_mut(),
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            )
        };
        if started == 0 {
            eprintln!("[rustpira] Windows no pudo reproducir el sonido");
            return;
        }
        *self.playing.borrow_mut() = Some(wav);
    }

    /// Stops the current sound and releases its buffer.
    pub fn stop(&self) {
        // SAFETY: a null sound stops whatever this process is playing.
        unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
        self.playing.borrow_mut().take();
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_wav_is_loaded_with_the_volume_applied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("propio.wav");
        std::fs::write(&path, audio::builtin(2, 100)).unwrap();
        let loaded = load_custom(&path, 40).unwrap();
        assert_eq!(
            loaded,
            audio::with_volume(&audio::builtin(2, 100), 40).unwrap()
        );
    }

    #[test]
    fn missing_file_is_a_spanish_error() {
        let dir = tempfile::tempdir().unwrap();
        let error = load_custom(&dir.path().join("no.wav"), 50).unwrap_err();
        assert!(error.starts_with("no se pudo abrir el sonido"), "{error}");
    }

    #[test]
    fn a_file_that_is_not_a_wav_is_a_spanish_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("texto.wav");
        std::fs::write(&path, "esto no es audio").unwrap();
        assert_eq!(
            load_custom(&path, 50).unwrap_err(),
            "el sonido propio no se puede usar: no es un archivo WAV"
        );
    }

    #[test]
    fn an_oversized_file_is_rejected_before_reading_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("enorme.wav");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_CUSTOM_BYTES + 1).unwrap();
        assert_eq!(
            load_custom(&path, 50).unwrap_err(),
            "el sonido propio pesa más de 20 MB"
        );
    }
}
