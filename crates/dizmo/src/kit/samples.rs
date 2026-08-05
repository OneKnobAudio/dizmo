//! Sample loading: decodes the WAV files referenced by a loaded [`Kit`] into
//! immutable mono buffers the engine can read.
//!
//! Loading happens off the audio thread (`Kit::load` -> `load_samples`); the
//! resulting [`SampleBank`] is read-only and shared with the engine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{AudioFile, Kit};

/// A fully decoded WAV file, split into one mono buffer per channel.
#[derive(Debug)]
pub struct DecodedFile {
    pub sample_rate: u32,
    /// One mono buffer per channel in the file.
    pub channels: Vec<Arc<[f32]>>,
}

impl DecodedFile {
    /// The number of frames (per-channel samples).
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, |channel| channel.len())
    }
}

/// Every WAV file referenced by a kit, keyed by its resolved path.
///
/// Files are decoded once and shared, since many samples often reference the
/// same file (e.g. a multi-channel kick WAV used by several hits).
#[derive(Debug, Default)]
pub struct SampleBank {
    files: HashMap<PathBuf, Arc<DecodedFile>>,
}

impl SampleBank {
    /// The decoded file for `path`, if it was loaded.
    pub fn file(&self, path: &Path) -> Option<&Arc<DecodedFile>> {
        self.files.get(path)
    }

    /// The mono buffer for one `AudioFile` reference, resolved against
    /// `base_dir` (the directory of the instrument XML that declared it).
    pub fn audio_file(&self, base_dir: &Path, audio: &AudioFile) -> Option<&Arc<[f32]>> {
        let file = self.files.get(&base_dir.join(&audio.file))?;
        file.channels.get(audio.file_channel)
    }

    /// The number of unique decoded files.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Decodes and caches every sample referenced by `kit`.
///
/// When `target_sample_rate` is set and differs from a file's own sample rate,
/// every channel is resampled to that rate so the engine can play back at the
/// host rate 1:1.
///
/// Returns the first error encountered (missing file, malformed WAV, or a
/// `filechannel` outside the file's channel count).
pub fn load_samples(kit: &Kit, target_sample_rate: Option<u32>) -> Result<SampleBank, SampleError> {
    load_samples_with_progress(kit, target_sample_rate, &mut |_, _| {})
}

/// Like [`load_samples`], but reports decoding progress via `progress(loaded, total)`.
pub fn load_samples_with_progress(
    kit: &Kit,
    target_sample_rate: Option<u32>,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<SampleBank, SampleError> {
    let total = unique_file_count(kit);
    let mut files = HashMap::new();
    let mut loaded = 0;

    for instrument in &kit.instruments {
        for sample in &instrument.samples {
            for audio in &sample.audio_files {
                let path = instrument.base_dir.join(&audio.file);
                if files.contains_key(&path) {
                    continue;
                }

                let decoded = decode_file(&path, target_sample_rate)?;
                if audio.file_channel >= decoded.channels.len() {
                    return Err(SampleError::ChannelOutOfRange {
                        sample: sample.name.clone(),
                        path,
                        channel: audio.file_channel,
                        num_channels: decoded.channels.len(),
                    });
                }
                files.insert(path, Arc::new(decoded));
                loaded += 1;
                progress(loaded, total);
            }
        }
    }

    Ok(SampleBank { files })
}

/// The number of unique sample files referenced by `kit`, in load order.
fn unique_file_count(kit: &Kit) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut count = 0;
    for instrument in &kit.instruments {
        for sample in &instrument.samples {
            for audio in &sample.audio_files {
                if seen.insert(instrument.base_dir.join(&audio.file)) {
                    count += 1;
                }
            }
        }
    }
    count
}

fn decode_file(path: &Path, target_sample_rate: Option<u32>) -> Result<DecodedFile, SampleError> {
    let mut reader = hound::WavReader::open(path).map_err(|error| match error {
        hound::Error::IoError(source) => SampleError::Io {
            path: path.to_path_buf(),
            source,
        },
        other => SampleError::Decode {
            path: path.to_path_buf(),
            source: other,
        },
    })?;
    let spec = reader.spec();
    let num_channels = spec.channels as usize;

    let mut channels: Vec<Vec<f32>> = vec![Vec::new(); num_channels];

    match spec.sample_format {
        // Normalize integer PCM to [-1, 1] by the full-scale divisor.
        hound::SampleFormat::Int => {
            let divisor = 2f32.powi((spec.bits_per_sample - 1) as i32);
            for (index, sample) in reader.samples::<i32>().enumerate() {
                let sample = sample.map_err(|source| SampleError::Decode {
                    path: path.to_path_buf(),
                    source,
                })?;
                channels[index % num_channels].push(sample as f32 / divisor);
            }
        }
        hound::SampleFormat::Float => {
            for (index, sample) in reader.samples::<f32>().enumerate() {
                let sample = sample.map_err(|source| SampleError::Decode {
                    path: path.to_path_buf(),
                    source,
                })?;
                channels[index % num_channels].push(sample);
            }
        }
    }

    let sample_rate = match target_sample_rate {
        Some(target) if target != spec.sample_rate => {
            for channel in channels.iter_mut() {
                *channel = resample(channel, spec.sample_rate, target);
            }
            target
        }
        _ => spec.sample_rate,
    };

    Ok(DecodedFile {
        sample_rate,
        channels: channels.into_iter().map(Into::into).collect(),
    })
}

/// Resamples `data` from `src_rate` to `dst_rate` using a Lanczos-3
/// windowed-sinc kernel. Downsampling is anti-aliased by scaling the kernel by
/// the ratio. The result is normalized by the summed kernel weights so signals
/// keep their amplitude at the buffer edges.
fn resample(data: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    const RADIUS: f64 = 3.0;
    let ratio = src_rate as f64 / dst_rate as f64;
    let scale = ratio.max(1.0);
    let support = (RADIUS / scale).ceil() as i64;
    let out_len = (data.len() as f64 / ratio).ceil() as usize;

    let mut out = Vec::with_capacity(out_len);
    for j in 0..out_len {
        let pos = j as f64 * ratio;
        let center = pos.floor() as i64;
        let mut sum = 0.0;
        let mut weight_sum = 0.0;
        for k in (center - support)..=(center + support) {
            if k < 0 || k as usize >= data.len() {
                continue;
            }
            let x = (pos - k as f64) * scale;
            let weight = lanczos(x, RADIUS);
            sum += data[k as usize] as f64 * weight;
            weight_sum += weight;
        }
        out.push(if weight_sum.abs() > 0.0 {
            (sum / weight_sum) as f32
        } else {
            0.0
        });
    }
    out
}

/// The Lanczos kernel: `sinc(x) * sinc(x / radius)`, zero outside `radius`.
fn lanczos(x: f64, radius: f64) -> f64 {
    if x.abs() < 1e-9 {
        return 1.0;
    }
    if x.abs() >= radius {
        return 0.0;
    }
    let p = std::f64::consts::PI * x;
    radius * p.sin() * (p / radius).sin() / (p * p)
}

/// Errors that can occur while loading and decoding samples.
#[derive(Debug, thiserror::Error)]
pub enum SampleError {
    #[error("failed to read '{path}': {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to decode WAV '{path}': {source}")]
    Decode { path: PathBuf, source: hound::Error },

    #[error(
        "sample '{sample}' references channel {channel} but '{path}' only has {num_channels} channel(s)"
    )]
    ChannelOutOfRange {
        sample: String,
        path: PathBuf,
        channel: usize,
        num_channels: usize,
    },
}
