//! Sample audition: rodio playback on a background thread.
//!
//! The UI holds a [`PreviewPlayer`] handle; the audio device, mixer and player
//! live on a dedicated thread so WAV decoding and playback never block iced.

use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;

use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, Player};

enum Command {
    Play { path: PathBuf, channel: usize, volume: f32 },
    SetVolume(f32),
    Stop,
}

/// Handle to the preview playback thread.
#[derive(Clone)]
pub struct PreviewPlayer {
    sender: Sender<Command>,
}

impl PreviewPlayer {
    /// Spawns the playback thread; it grabs the default output device lazily.
    pub fn spawn() -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || Self::run(receiver));
        Self { sender }
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

    fn run(receiver: mpsc::Receiver<Command>) {
        let Ok(handle) = DeviceSinkBuilder::open_default_sink() else {
            eprintln!("dizmo_editor: no audio output device available for preview");
            return;
        };
        let mut player: Option<Player> = None;
        while let Ok(command) = receiver.recv() {
            match command {
                Command::Play { path, channel, volume } => {
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
