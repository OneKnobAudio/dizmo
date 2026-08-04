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
/// Returns the first error encountered (missing file, malformed WAV, or a
/// `filechannel` outside the file's channel count).
pub fn load_samples(kit: &Kit) -> Result<SampleBank, SampleError> {
    let mut files = HashMap::new();

    for instrument in &kit.instruments {
        for sample in &instrument.samples {
            for audio in &sample.audio_files {
                let path = instrument.base_dir.join(&audio.file);
                if files.contains_key(&path) {
                    continue;
                }

                let decoded = decode_file(&path)?;
                if audio.file_channel >= decoded.channels.len() {
                    return Err(SampleError::ChannelOutOfRange {
                        sample: sample.name.clone(),
                        path,
                        channel: audio.file_channel,
                        num_channels: decoded.channels.len(),
                    });
                }
                files.insert(path, Arc::new(decoded));
            }
        }
    }

    Ok(SampleBank { files })
}

fn decode_file(path: &Path) -> Result<DecodedFile, SampleError> {
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

    Ok(DecodedFile {
        sample_rate: spec.sample_rate,
        channels: channels.into_iter().map(Into::into).collect(),
    })
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
