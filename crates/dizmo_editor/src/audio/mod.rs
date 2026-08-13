//! Sample audition: rodio playback on a background thread.
//!
//! The UI holds a [`PreviewPlayer`] handle; the audio device, mixer and player
//! live on a dedicated thread so WAV decoding and playback never block iced.
//! The output device is opened eagerly in [`PreviewPlayer::spawn`]; when that
//! fails, [`PreviewPlayer::audio_available`] reports it so the UI can say why
//! preview is silent instead of failing invisibly.
//!
//! When a playback ends on its own (it was not replaced by a newer play), the
//! audio thread reports its token through [`register_finished_listener`]; the
//! editor subscribes to those events so the Play/Stop button can un-toggle.

use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

/// How often the playback thread checks whether the current playback ended.
const COMPLETION_POLL: Duration = Duration::from_millis(50);

/// UI-side listeners for "playback finished" events (token-based).
static FINISHED_LISTENERS: OnceLock<Mutex<Vec<UnboundedSender<u64>>>> = OnceLock::new();

fn finished_listeners() -> &'static Mutex<Vec<UnboundedSender<u64>>> {
    FINISHED_LISTENERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Subscribes `tx` to playback-finished events. The editor calls this once
/// from its subscription; each event carries the token of the finished play.
pub fn register_finished_listener(tx: UnboundedSender<u64>) {
    finished_listeners().lock().unwrap().push(tx);
}

fn send_finished(token: u64) {
    let listeners = finished_listeners().lock().unwrap();
    for listener in listeners.iter() {
        let _ = listener.unbounded_send(token);
    }
}

enum Command {
    Play {
        path: PathBuf,
        channel: usize,
        volume: f32,
        /// Identifies this play; reported back when it finishes naturally.
        token: u64,
    },
    SetVolume(f32),
    Stop,
}

/// Handle to the preview playback thread.
#[derive(Clone)]
pub struct PreviewPlayer {
    sender: Sender<Command>,
    audio_available: Arc<AtomicBool>,
}

impl PreviewPlayer {
    /// Spawns the playback thread and opens the default output device. When no
    /// device is available the thread idles and [`Self::audio_available`]
    /// returns `false`.
    pub fn spawn() -> Self {
        let (sender, receiver) = mpsc::channel();
        let audio_available = Arc::new(AtomicBool::new(false));
        let sink = match DeviceSinkBuilder::open_default_sink() {
            Ok(sink) => {
                audio_available.store(true, Ordering::SeqCst);
                Some(sink)
            }
            Err(err) => {
                eprintln!("dizmo_editor: no audio output device available for preview: {err}");
                None
            }
        };
        let available = audio_available.clone();
        thread::spawn(move || Self::run(receiver, sink, available));
        Self {
            sender,
            audio_available,
        }
    }

    /// Whether a default output device was opened; `false` means preview is
    /// silent regardless of the sample.
    pub fn audio_available(&self) -> bool {
        self.audio_available.load(Ordering::SeqCst)
    }

    /// Auditions `path`, playing `channel` (0-based) at `volume` (linear gain).
    pub fn play(&self, path: &Path, channel: usize, volume: f32, token: u64) {
        let _ = self.sender.send(Command::Play {
            path: path.to_path_buf(),
            channel,
            volume,
            token,
        });
    }

    /// Stops whatever is playing.
    pub fn stop(&self) {
        let _ = self.sender.send(Command::Stop);
    }

    /// Sets the volume of the current playback.
    pub fn set_volume(&self, volume: f32) {
        let _ = self.sender.send(Command::SetVolume(volume));
    }

    fn run(
        receiver: mpsc::Receiver<Command>,
        sink: Option<MixerDeviceSink>,
        audio_available: Arc<AtomicBool>,
    ) {
        let Some(handle) = sink else {
            // Drain commands so the sender never blocks; nothing can play.
            while receiver.recv().is_ok() {}
            return;
        };
        let mut player: Option<Player> = None;
        // Token of the current playback; `None` once it has been reported.
        let mut current: Option<(u64, bool)> = None;
        loop {
            match receiver.recv_timeout(COMPLETION_POLL) {
                Ok(Command::Play {
                    path,
                    channel,
                    volume,
                    token,
                }) => {
                    if let Some(player) = &player {
                        player.stop();
                    }
                    match decode_channel(&path, channel) {
                        Ok(buffer) => {
                            let next = Player::connect_new(handle.mixer());
                            next.set_volume(volume);
                            next.append(buffer);
                            player = Some(next);
                            current = Some((token, false));
                        }
                        Err(err) => {
                            eprintln!("dizmo_editor: could not decode '{}': {err}", path.display());
                            current = None;
                        }
                    }
                }
                Ok(Command::Stop) => {
                    if let Some(player) = &player {
                        player.stop();
                    }
                }
                Ok(Command::SetVolume(volume)) => {
                    if let Some(player) = &player {
                        player.set_volume(volume);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // The playback finished on its own (queue drained): report
                    // it exactly once. A newer play replaces `current`, so a
                    // stopped-but-replaced play never reports.
                    if let Some((token, reported)) = current
                        && !reported
                        && player.as_ref().is_some_and(|player| player.len() == 0)
                    {
                        current = Some((token, true));
                        send_finished(token);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        drop(audio_available);
    }
}

/// Decodes `channel` (0-based) of a WAV into a mono `SamplesBuffer` (f32).
fn decode_channel(path: &Path, channel: usize) -> Result<SamplesBuffer, String> {
    let mut reader = hound::WavReader::open(path).map_err(|err| err.to_string())?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels).max(1);
    let channel = channel.min(channels.saturating_sub(1));

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())?,
        hound::SampleFormat::Int => {
            let scale =
                (1i64 << (u64::from(spec.bits_per_sample).saturating_sub(1)).min(31)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / scale))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| err.to_string())?
        }
    };

    let data: Vec<f32> = samples
        .iter()
        .skip(channel)
        .step_by(channels)
        .copied()
        .collect();
    if data.is_empty() {
        return Err("no samples in file".into());
    }

    let channels = NonZero::new(1u16).expect("1 is non-zero");
    let sample_rate = NonZero::new(spec.sample_rate).ok_or("sample rate is zero")?;
    Ok(SamplesBuffer::new(channels, sample_rate, data))
}

/// Resamples the WAV at `path` in place to `target_rate`, using the ARDFTSRC
/// algorithm (ardftsrc). A no-op when the file already is at the target rate.
/// The file is rewritten as 16-bit PCM, preserving the channel count.
pub fn resample_wav(path: &Path, target_rate: u32) -> Result<(), String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|err| format!("'{}' is not a readable WAV file: {err}", path.display()))?;
    let spec = reader.spec();
    if spec.sample_rate == target_rate {
        return Ok(());
    }
    let channels = usize::from(spec.channels).max(1);

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())?,
        hound::SampleFormat::Int => {
            let scale =
                (1i64 << (u64::from(spec.bits_per_sample).saturating_sub(1)).min(31)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / scale))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| err.to_string())?
        }
    };
    if samples.is_empty() {
        return Err("no samples in file".to_string());
    }

    // ardftsrc recommends f64 processing; PRESET_GOOD balances quality and
    // speed for offline conversion.
    let input: Vec<f64> = samples.iter().map(|sample| f64::from(*sample)).collect();
    let config = ardftsrc::PRESET_GOOD
        .with_input_rate(spec.sample_rate as usize)
        .with_output_rate(target_rate as usize)
        .with_channels(channels);
    let mut resampler =
        ardftsrc::InterleavedResampler::<f64>::new(config).map_err(|err| err.to_string())?;
    let output = resampler
        .process_all(&input)
        .map_err(|err| err.to_string())?;
    let resampled: Vec<f32> = output
        .interleave()
        .into_iter()
        .map(|sample| sample as f32)
        .collect();

    // Write to a temp file, then atomically replace the original.
    let tmp = path.with_extension("tmp");
    let mut writer = hound::WavWriter::create(
        &tmp,
        hound::WavSpec {
            channels: spec.channels,
            sample_rate: target_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .map_err(|err| format!("Could not create '{}': {err}", tmp.display()))?;
    for sample in resampled {
        writer
            .write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)
            .map_err(|err| err.to_string())?;
    }
    writer.finalize().map_err(|err| err.to_string())?;
    std::fs::rename(&tmp, path)
        .map_err(|err| format!("Could not replace '{}': {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_wav(path: &Path, channels: u16, samples: &[i16]) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn resamples_wav_to_target_rate_in_place() {
        let dir = std::env::temp_dir().join(format!("dizmo_resample_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // 8000 Hz, 100 frames of a ramp.
        let path = dir.join("down.wav");
        let input: Vec<i16> = (0..100).map(|i| (i as i16) * 100).collect();
        write_wav(&path, 1, &input);

        resample_wav(&path, 4000).unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 4000);
        let count = reader.samples::<i16>().count();
        // Roughly half the frames (ARDFTSRC adds a small tail).
        assert!(
            (40..=60).contains(&count),
            "expected ~50 frames, got {count}"
        );

        // Same rate is a no-op.
        resample_wav(&path, 4000).unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 4000);
        assert_eq!(reader.samples::<i16>().count(), count);

        // Stereo resampling preserves the channel count.
        let stereo = dir.join("stereo.wav");
        write_wav(&stereo, 2, &[1, -1, 2, -2, 3, -3, 4, -4]);
        resample_wav(&stereo, 16000).unwrap();
        let mut reader = hound::WavReader::open(&stereo).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 16000);
        let stereo_count = reader.samples::<i16>().count();
        assert!(
            stereo_count > 8,
            "expected more than 8 samples, got {stereo_count}"
        );

        // A non-WAV file errors.
        let not_wav = dir.join("readme.txt");
        std::fs::write(&not_wav, "not audio").unwrap();
        assert!(resample_wav(&not_wav, 4000).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn decodes_the_requested_channel_of_a_multichannel_wav() {
        let dir = std::env::temp_dir().join(format!("dizmo_audio_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stereo.wav");
        // Interleaved: left = 100, right = -100.
        write_wav(&path, 2, &[100, -100, 200, -200]);

        let left = decode_channel(&path, 0).unwrap();
        let left_samples: Vec<f32> = left.collect();
        assert_eq!(left_samples.len(), 2);
        assert!((left_samples[0] - 100.0 / 32768.0).abs() < 1e-5);
        assert!((left_samples[1] - 200.0 / 32768.0).abs() < 1e-5);

        let right = decode_channel(&path, 1).unwrap();
        let right_samples: Vec<f32> = right.collect();
        assert_eq!(right_samples.len(), 2);
        assert!((right_samples[0] + 100.0 / 32768.0).abs() < 1e-5);

        // An out-of-range channel clamps to the last one instead of failing.
        let clamped = decode_channel(&path, 7).unwrap();
        assert_eq!(clamped.len(), 2);

        // A missing file errors.
        assert!(decode_channel(&dir.join("missing.wav"), 0).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
