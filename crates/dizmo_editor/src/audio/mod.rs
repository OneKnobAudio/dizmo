//! Sample audition: rodio playback on a background thread.
//!
//! The UI holds a [`PreviewPlayer`] handle; the audio device, mixer and player
//! live on a dedicated thread so WAV decoding and playback never block iced.
//! The output device is opened eagerly in [`PreviewPlayer::spawn`]; when that
//! fails, [`PreviewPlayer::audio_available`] reports it so the UI can say why
//! preview is silent instead of failing invisibly.

use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;

use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

enum Command {
    Play {
        path: PathBuf,
        channel: usize,
        volume: f32,
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
    pub fn play(&self, path: &Path, channel: usize, volume: f32) {
        let _ = self.sender.send(Command::Play {
            path: path.to_path_buf(),
            channel,
            volume,
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
        while let Ok(command) = receiver.recv() {
            match command {
                Command::Play {
                    path,
                    channel,
                    volume,
                } => {
                    if let Some(player) = &player {
                        player.stop();
                    }
                    match decode_channel(&path, channel) {
                        Ok(buffer) => {
                            let next = Player::connect_new(handle.mixer());
                            next.set_volume(volume);
                            next.append(buffer);
                            player = Some(next);
                        }
                        Err(err) => {
                            eprintln!("dizmo_editor: could not decode '{}': {err}", path.display());
                        }
                    }
                }
                Command::Stop => {
                    if let Some(player) = &player {
                        player.stop();
                    }
                }
                Command::SetVolume(volume) => {
                    if let Some(player) = &player {
                        player.set_volume(volume);
                    }
                }
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
