//! Pure audio helpers (no Windows): the built-in sounds, synthesized in code
//! so they are original work (MIT-compatible) and add nothing to the binary,
//! and the volume applied to any PCM `.wav` by scaling its samples.

use std::f32::consts::TAU;
use std::fmt;

use crate::domain::config::BUILTIN_SOUNDS;

/// Number of built-in sounds; `SoundConfig::builtin` is 1-based.
pub const BUILTIN_COUNT: u8 = BUILTIN_SOUNDS;

/// Spanish names shown in the settings window (T8), in index order.
pub const BUILTIN_NAMES: [&str; BUILTIN_COUNT as usize] =
    ["Campana", "Dos tonos", "Acorde suave", "Tres pulsos"];

pub const SAMPLE_RATE: u32 = 22_050;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WavError {
    NotWav,
    Truncated,
    Unsupported { format_tag: u16, bits: u16 },
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWav => write!(f, "no es un archivo WAV"),
            Self::Truncated => write!(f, "el archivo WAV está incompleto"),
            Self::Unsupported { format_tag, bits } => write!(
                f,
                "formato WAV no soportado (formato {format_tag}, {bits} bits)"
            ),
        }
    }
}

impl std::error::Error for WavError {}

/// Loudest sample of a built-in sound at volume 100, as a fraction of full
/// scale; leaves headroom so the sounds are not harsh.
const PEAK: f32 = 0.8;

/// A complete WAV file (mono, 16-bit PCM) of built-in sound `index`
/// (1-based; out-of-range values use sound 1) at `volume` 0..=100.
pub fn builtin(index: u8, volume: u8) -> Vec<u8> {
    let wave = match index {
        2 => two_tones(),
        3 => soft_chord(),
        4 => three_pulses(),
        _ => bell(),
    };
    let gain = PEAK * f32::from(volume.min(100)) / 100.0 * f32::from(i16::MAX);
    let samples: Vec<i16> = wave.iter().map(|s| (s * gain) as i16).collect();
    encode_wav(&samples)
}

/// Samples `secs` seconds of `f(t)` and normalizes the peak to 1.0.
fn render(secs: f32, f: impl Fn(f32) -> f32) -> Vec<f32> {
    let count = (secs * SAMPLE_RATE as f32) as usize;
    let mut wave: Vec<f32> = (0..count)
        .map(|i| f(i as f32 / SAMPLE_RATE as f32))
        .collect();
    let max = wave.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if max > 0.0 {
        wave.iter_mut().for_each(|s| *s /= max);
    }
    wave
}

fn sine(freq: f32, t: f32) -> f32 {
    (TAU * freq * t).sin()
}

/// Linear fade-in over `secs`, to avoid clicks.
fn attack(t: f32, secs: f32) -> f32 {
    (t / secs).min(1.0)
}

fn bell() -> Vec<f32> {
    render(1.6, |t| {
        let partials = sine(880.0, t) + 0.5 * sine(880.0 * 2.76, t) + 0.25 * sine(880.0 * 5.4, t);
        partials * (-3.0 * t).exp() * attack(t, 0.005)
    })
}

fn two_tones() -> Vec<f32> {
    let note = |freq: f32, t: f32, len: f32| sine(freq, t) * attack(t, 0.01) * (1.0 - t / len);
    render(0.7, |t| {
        if t < 0.25 {
            note(660.0, t, 0.25)
        } else {
            note(880.0, t - 0.25, 0.45)
        }
    })
}

fn soft_chord() -> Vec<f32> {
    render(1.8, |t| {
        let chord = sine(523.25, t) + sine(659.25, t) + sine(783.99, t);
        chord * attack(t, 0.15) * (-1.8 * t).exp()
    })
}

fn three_pulses() -> Vec<f32> {
    const PERIOD: f32 = 0.22;
    const ON: f32 = 0.12;
    render(0.66, |t| {
        let local = t % PERIOD;
        if local < ON {
            sine(1000.0, t) * attack(local, 0.01) * attack(ON - local, 0.01)
        } else {
            0.0
        }
    })
}

/// A canonical 44-byte-header WAV file: mono, 16-bit PCM, `SAMPLE_RATE`.
fn encode_wav(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

const FORMAT_PCM: u16 = 1;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// A copy of a PCM `.wav` with its samples scaled to `volume` 0..=100.
/// Supports integer PCM of 8, 16, 24 and 32 bits (also as
/// WAVE_FORMAT_EXTENSIBLE); other encodings are reported as unsupported.
pub fn with_volume(wav: &[u8], volume: u8) -> Result<Vec<u8>, WavError> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(WavError::NotWav);
    }
    let u16_at = |at: usize| u16::from_le_bytes([wav[at], wav[at + 1]]);
    let mut format = None;
    let mut pos = 12;
    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let size =
            u32::from_le_bytes([wav[pos + 4], wav[pos + 5], wav[pos + 6], wav[pos + 7]]) as usize;
        let body = pos + 8;
        match id {
            b"fmt " => {
                if size < 16 || body + 16 > wav.len() {
                    return Err(WavError::Truncated);
                }
                let mut tag = u16_at(body);
                if tag == FORMAT_EXTENSIBLE && size >= 26 && body + 26 <= wav.len() {
                    // The sub-format GUID starts with the real format tag.
                    tag = u16_at(body + 24);
                }
                format = Some((tag, u16_at(body + 14)));
            }
            b"data" => {
                let (format_tag, bits) = format.ok_or(WavError::NotWav)?;
                if format_tag != FORMAT_PCM || !matches!(bits, 8 | 16 | 24 | 32) {
                    return Err(WavError::Unsupported { format_tag, bits });
                }
                if body + size > wav.len() {
                    return Err(WavError::Truncated);
                }
                let mut out = wav.to_vec();
                if volume < 100 {
                    scale(&mut out[body..body + size], bits, volume);
                }
                return Ok(out);
            }
            _ => {}
        }
        // Chunks are padded to an even size.
        pos = body + size + (size & 1);
    }
    Err(WavError::Truncated)
}

/// Scales PCM samples in place, truncating toward zero.
fn scale(data: &mut [u8], bits: u16, volume: u8) {
    let volume = i64::from(volume);
    let apply = |sample: i64| sample * volume / 100;
    match bits {
        8 => data
            .iter_mut()
            .for_each(|b| *b = (apply(i64::from(*b) - 128) + 128) as u8),
        16 => data.as_chunks_mut::<2>().0.iter_mut().for_each(|c| {
            let s = apply(i16::from_le_bytes([c[0], c[1]]).into()) as i16;
            c.copy_from_slice(&s.to_le_bytes());
        }),
        24 => data.as_chunks_mut::<3>().0.iter_mut().for_each(|c| {
            // Sign-extend the 24-bit value through the top byte of an i32.
            let s = i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8;
            let scaled = ((apply(s.into()) as i32) << 8).to_le_bytes();
            c.copy_from_slice(&scaled[1..]);
        }),
        32 => data.as_chunks_mut::<4>().0.iter_mut().for_each(|c| {
            let s = apply(i32::from_le_bytes([c[0], c[1], c[2], c[3]]).into()) as i32;
            c.copy_from_slice(&s.to_le_bytes());
        }),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    /// Samples of a canonical 44-byte-header 16-bit file.
    fn samples16(wav: &[u8]) -> Vec<i16> {
        wav[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    fn peak(samples: &[i16]) -> i32 {
        samples
            .iter()
            .map(|s| i32::from(*s).abs())
            .max()
            .unwrap_or(0)
    }

    /// Builds a WAV with an arbitrary format and data, plus an extra chunk
    /// of odd size before "data" to exercise chunk padding.
    fn wav(format_tag: u16, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&format_tag.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes()); // channels
        fmt.extend_from_slice(&8000u32.to_le_bytes());
        fmt.extend_from_slice(&(8000 * u32::from(bits / 8)).to_le_bytes());
        fmt.extend_from_slice(&(bits / 8).to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        let mut body = b"WAVE".to_vec();
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        body.extend_from_slice(&fmt);
        body.extend_from_slice(b"LIST");
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(b"abc\0"); // 3 bytes + 1 pad byte
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn builtin_sounds_are_valid_short_mono_16_bit_wav_files() {
        for index in 1..=BUILTIN_COUNT {
            let w = builtin(index, 100);
            assert_eq!(&w[0..4], b"RIFF", "sound {index}");
            assert_eq!(u32_at(&w, 4) as usize, w.len() - 8);
            assert_eq!(&w[8..16], b"WAVEfmt ");
            assert_eq!(u16_at(&w, 20), 1, "PCM");
            assert_eq!(u16_at(&w, 22), 1, "mono");
            assert_eq!(u32_at(&w, 24), SAMPLE_RATE);
            assert_eq!(u16_at(&w, 34), 16, "bits");
            assert_eq!(&w[36..40], b"data");
            let data_len = u32_at(&w, 40) as usize;
            assert_eq!(data_len, w.len() - 44);
            let secs = data_len as f64 / 2.0 / f64::from(SAMPLE_RATE);
            assert!((0.3..=2.5).contains(&secs), "sound {index}: {secs} s");
            assert!(peak(&samples16(&w)) > 20_000, "sound {index} is audible");
        }
    }

    #[test]
    fn builtin_sounds_are_all_different() {
        let all: Vec<_> = (1..=BUILTIN_COUNT).map(|i| builtin(i, 100)).collect();
        for a in 0..all.len() {
            for b in a + 1..all.len() {
                assert_ne!(all[a], all[b], "sounds {} and {}", a + 1, b + 1);
            }
        }
    }

    #[test]
    fn out_of_range_builtin_index_uses_the_first_sound() {
        assert_eq!(builtin(0, 80), builtin(1, 80));
        assert_eq!(builtin(99, 80), builtin(1, 80));
    }

    #[test]
    fn builtin_volume_scales_the_amplitude() {
        let full = peak(&samples16(&builtin(1, 100)));
        let half = peak(&samples16(&builtin(1, 50)));
        assert!((half - full / 2).abs() <= 2, "{half} vs {full}");
        assert_eq!(peak(&samples16(&builtin(1, 0))), 0);
    }

    #[test]
    fn volume_scales_16_bit_samples_and_keeps_the_rest_of_the_file() {
        let data: Vec<u8> = [1000i16, -2000, 32767, -32768]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let original = wav(1, 16, &data);
        let scaled = with_volume(&original, 50).unwrap();
        assert_eq!(scaled.len(), original.len());
        let data_at = original.len() - data.len();
        assert_eq!(scaled[..data_at], original[..data_at]);
        let out: Vec<i16> = scaled[data_at..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(out, vec![500, -1000, 16383, -16384]);
    }

    #[test]
    fn volume_scales_8_bit_samples_around_the_midpoint() {
        let original = wav(1, 8, &[128, 228, 28, 255]);
        let scaled = with_volume(&original, 50).unwrap();
        assert_eq!(scaled[scaled.len() - 4..], [128, 178, 78, 191]);
    }

    #[test]
    fn full_volume_returns_the_file_unchanged() {
        let original = wav(1, 16, &[0x10, 0x20, 0x30, 0x40]);
        assert_eq!(with_volume(&original, 100).unwrap(), original);
    }

    #[test]
    fn non_wav_and_unsupported_formats_are_errors() {
        assert_eq!(
            with_volume(b"hello world, not a wav", 50),
            Err(WavError::NotWav)
        );
        assert_eq!(
            with_volume(&wav(3, 32, &[0; 8]), 50),
            Err(WavError::Unsupported {
                format_tag: 3,
                bits: 32
            })
        );
        let mut cut = wav(1, 16, &[0; 8]);
        cut.truncate(cut.len() - 4);
        assert_eq!(with_volume(&cut, 50), Err(WavError::Truncated));
    }
}
