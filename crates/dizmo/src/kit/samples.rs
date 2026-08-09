//! Sample loading: decodes the WAV files referenced by a loaded [`Kit`] into
//! immutable mono buffers the engine can read.
//!
//! Loading happens off the audio thread (`Kit::load` -> `load_samples`); the
//! resulting [`SampleBank`] is read-only and shared with the engine. Files are
//! decoded in parallel, and only the channels that samples actually reference
//! are decoded and resampled.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use ardftsrc::{PRESET_GOOD, PlanarResampler};

use super::{AudioFile, DizmoKit};

/// A fully decoded WAV file, split into one mono buffer per channel.
#[derive(Debug)]
pub struct DecodedFile {
    pub sample_rate: u32,
    /// One mono buffer per file channel, in file order. Channels that no
    /// sample references are `None` and were never decoded.
    pub channels: Vec<Option<Arc<[f32]>>>,
}

impl DecodedFile {
    /// The number of frames (per-channel samples).
    pub fn frames(&self) -> usize {
        self.channels
            .iter()
            .flatten()
            .next()
            .map_or(0, |channel| channel.len())
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
        file.channels.get(audio.file_channel)?.as_ref()
    }

    /// The number of unique decoded files.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// One unique sample file to decode: its resolved path, the name of the first
/// sample that references it (for error messages), and the 0-based channels it
/// references.
struct SampleTask {
    path: PathBuf,
    sample: String,
    channels: Vec<usize>,
}

/// Decodes and caches every sample referenced by `kit`.
///
/// When `target_sample_rate` is set and differs from a file's own sample rate,
/// every referenced channel is resampled to that rate so the engine can play
/// back at the host rate 1:1.
///
/// Returns the first error encountered (missing file, malformed WAV, or a
/// `filechannel` outside the file's channel count).
pub fn load_samples(
    kit: &DizmoKit,
    target_sample_rate: Option<u32>,
) -> Result<SampleBank, SampleError> {
    load_samples_with_progress(kit, target_sample_rate, &mut |_, _| {})
}

/// Like [`load_samples`], but reports decoding progress via `progress(loaded, total)`.
///
/// Files are decoded on a worker pool sized to the machine. Errors are still
/// reported in kit declaration order: the first file (by load order) that
/// fails determines the returned error, matching serial loading.
pub fn load_samples_with_progress(
    kit: &DizmoKit,
    target_sample_rate: Option<u32>,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<SampleBank, SampleError> {
    let tasks = unique_files(kit);
    if tasks.is_empty() {
        return Ok(SampleBank::default());
    }

    let workers = std::thread::available_parallelism()
        .map_or(1, |count| count.get())
        .min(tasks.len());
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel();
    let mut bank = None;

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let next = &next;
            let tasks = &tasks;
            scope.spawn(move || {
                let mut cache = ResamplerCache::new();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(task) = tasks.get(index) else {
                        break;
                    };
                    let decoded = decode_file(task, target_sample_rate, &mut cache);
                    let _ = tx.send((index, decoded));
                }
            });
        }
        drop(tx);

        // Results arrive out of order; collect them all and reduce in load
        // order so the first failing file decides the error.
        let mut results: Vec<Option<Result<DecodedFile, SampleError>>> =
            (0..tasks.len()).map(|_| None).collect();
        let mut received = 0;
        let mut loaded = 0;
        while received < tasks.len() {
            match rx.recv() {
                Ok((index, decoded)) => {
                    received += 1;
                    if decoded.is_ok() {
                        loaded += 1;
                        progress(loaded, tasks.len());
                    }
                    results[index] = Some(decoded);
                }
                Err(_) => break,
            }
        }
        bank = Some(build_bank(&tasks, results));
    });

    bank.expect("sample loading always completes and assigns the bank")
}

/// The unique sample files referenced by `kit`, in load order, each with the
/// name of its first referencing sample and the 0-based channels it uses.
fn unique_files(kit: &DizmoKit) -> Vec<SampleTask> {
    let mut indexes: HashMap<PathBuf, usize> = HashMap::new();
    let mut tasks: Vec<SampleTask> = Vec::new();
    for instrument in &kit.instruments {
        for sample in &instrument.samples {
            for audio in &sample.audio_files {
                let path = instrument.base_dir.join(&audio.file);
                if let Some(&index) = indexes.get(&path) {
                    tasks[index].channels.push(audio.file_channel);
                } else {
                    indexes.insert(path.clone(), tasks.len());
                    tasks.push(SampleTask {
                        path,
                        sample: sample.name.clone(),
                        channels: vec![audio.file_channel],
                    });
                }
            }
        }
    }
    tasks
}

/// Reduces parallel decode results into a bank in load order, returning the
/// first error and inserting every successfully decoded file.
fn build_bank(
    tasks: &[SampleTask],
    results: Vec<Option<Result<DecodedFile, SampleError>>>,
) -> Result<SampleBank, SampleError> {
    let mut files = HashMap::with_capacity(tasks.len());
    for (task, result) in tasks.iter().zip(results) {
        let decoded = match result {
            Some(Ok(decoded)) => decoded,
            Some(Err(error)) => return Err(error),
            None => {
                return Err(SampleError::Internal(
                    "a decode worker finished without reporting a result".to_string(),
                ));
            }
        };
        files.insert(task.path.clone(), Arc::new(decoded));
    }
    Ok(SampleBank { files })
}

/// Decodes the channels of `task.path` that samples reference, resampling them
/// to `target_sample_rate` when it differs from the file's own rate.
fn decode_file(
    task: &SampleTask,
    target_sample_rate: Option<u32>,
    cache: &mut ResamplerCache,
) -> Result<DecodedFile, SampleError> {
    let path = &task.path;
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
    let frame_count = reader.duration() as usize;

    for &channel in &task.channels {
        if channel >= num_channels {
            return Err(SampleError::ChannelOutOfRange {
                sample: task.sample.clone(),
                path: path.to_path_buf(),
                channel,
                num_channels,
            });
        }
    }

    let mut wanted = vec![false; num_channels];
    for &channel in &task.channels {
        wanted[channel] = true;
    }

    // Preallocate only the referenced channels from the frame count, avoiding
    // growth reallocs for long samples.
    let mut channels: Vec<Vec<f32>> = wanted
        .iter()
        .map(|&keep| {
            if keep {
                Vec::with_capacity(frame_count)
            } else {
                Vec::new()
            }
        })
        .collect();

    let mut channel = 0usize;
    match spec.sample_format {
        // Normalize integer PCM to [-1, 1] by the full-scale divisor.
        hound::SampleFormat::Int => {
            let divisor = 2f32.powi((spec.bits_per_sample - 1) as i32);
            for sample in reader.samples::<i32>() {
                let sample = sample.map_err(|source| SampleError::Decode {
                    path: path.to_path_buf(),
                    source,
                })?;
                if wanted[channel] {
                    channels[channel].push(sample as f32 / divisor);
                }
                channel += 1;
                if channel == num_channels {
                    channel = 0;
                }
            }
        }
        hound::SampleFormat::Float => {
            for sample in reader.samples::<f32>() {
                let sample = sample.map_err(|source| SampleError::Decode {
                    path: path.to_path_buf(),
                    source,
                })?;
                if wanted[channel] {
                    channels[channel].push(sample);
                }
                channel += 1;
                if channel == num_channels {
                    channel = 0;
                }
            }
        }
    }

    let sample_rate = match target_sample_rate {
        Some(target) if target != spec.sample_rate => {
            cache.resample(&mut channels, &wanted, spec.sample_rate, target)?;
            target
        }
        _ => spec.sample_rate,
    };

    Ok(DecodedFile {
        sample_rate,
        channels: channels
            .into_iter()
            .zip(wanted)
            .map(|(buffer, keep)| if keep { Some(buffer.into()) } else { None })
            .collect(),
    })
}

/// Per-worker-thread resamplers, reused across the files that worker decodes.
/// Building a resampler constructs FFT plans and scratch buffers, so caching
/// one instance per `(source rate, target rate, channel count)` keeps that
/// cost out of the per-file hot path. `process_all` resets its stream state
/// between calls, so instances can be reused without explicit resets.
struct ResamplerCache {
    resamplers: HashMap<(u32, u32, usize), PlanarResampler<f32>>,
}

impl ResamplerCache {
    fn new() -> Self {
        Self {
            resamplers: HashMap::new(),
        }
    }

    /// Resamples every referenced channel of one file in a single call. The
    /// data is already planar (one `Vec` per channel), so the planar API avoids
    /// the interleave/deinterleave copies the interleaved wrapper would do.
    fn resample(
        &mut self,
        channels: &mut [Vec<f32>],
        wanted: &[bool],
        src_rate: u32,
        dst_rate: u32,
    ) -> Result<(), SampleError> {
        let refs: Vec<&[f32]> = wanted
            .iter()
            .zip(channels.iter())
            .filter(|(keep, _)| **keep)
            .map(|(_, buffer)| buffer.as_slice())
            .collect();
        if refs.is_empty() {
            return Ok(());
        }

        let key = (src_rate, dst_rate, refs.len());
        let config = PRESET_GOOD
            .with_input_rate(src_rate as usize)
            .with_output_rate(dst_rate as usize)
            .with_channels(refs.len());
        let resampler = self.resamplers.entry(key).or_insert_with(|| {
            PlanarResampler::<f32>::new(config).expect(
                "an ardftsrc configuration built from valid rates and channel counts is always valid",
            )
        });

        let mut output = resampler
            .process_all(&refs)
            .map_err(|error| SampleError::Resampling(error.to_string()))?
            .into_iter();
        for (buffer, keep) in channels.iter_mut().zip(wanted) {
            if *keep {
                *buffer = output
                    .next()
                    .expect("the resampler emits one channel per input channel");
            }
        }
        Ok(())
    }
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

    #[error("internal error while loading samples: {0}")]
    Internal(String),
    #[error("resampling error: {0}")]
    Resampling(String),
}
