//! Realtime drum engine: turns MIDI note events into sample voices.
//!
//! The engine reads only immutable data: a loaded [`Kit`] and a [`SampleBank`]
//! built off the audio thread. It never allocates or takes locks while
//! processing.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use crate::kit::{Kit, MidiMap, SampleBank};

/// Maximum number of simultaneously playing voices; the oldest is dropped
/// first when exceeded.
const MAX_VOICES: usize = 64;

/// The drum engine: note in -> voice playback into per-kit-channel mono output.
pub struct Engine {
    kit: Arc<Kit>,
    bank: Arc<SampleBank>,
    midimap: MidiMap,
    /// Kit channel name -> index in the output buffer.
    output_index: HashMap<String, usize>,
    /// Instrument name -> index into `kit.instruments`.
    instrument_index: HashMap<String, usize>,
    /// Per instrument: sample indices sorted by ascending power (v2 ordering).
    sample_order: Vec<Vec<usize>>,
    /// The current host sample rate, used for choke fade times.
    sample_rate: f32,
    voices: Vec<Voice>,
    rng: XorShift,
}

/// One playing hit. All streams share a single read position.
struct Voice {
    instrument: usize,
    /// One stream per instrument channel, each feeding a kit output channel.
    streams: Vec<VoiceStream>,
    position: f32,
    gain: f32,
    /// Per-frame linear gain decrement while choking; `0` when not choking.
    fade_step: f32,
    finished: bool,
}

/// A mono buffer playing into one output channel.
struct VoiceStream {
    output: usize,
    data: Arc<[f32]>,
}

impl Engine {
    pub fn new(kit: Arc<Kit>, bank: Arc<SampleBank>, midimap: MidiMap) -> Self {
        let output_index = kit
            .channels
            .iter()
            .enumerate()
            .map(|(index, channel)| (channel.name.clone(), index))
            .collect();
        let instrument_index = kit
            .instruments
            .iter()
            .enumerate()
            .map(|(index, instrument)| (instrument.name.clone(), index))
            .collect();
        let sample_order = kit
            .instruments
            .iter()
            .map(|instrument| {
                let mut order: Vec<usize> = (0..instrument.samples.len()).collect();
                if instrument.is_v2() {
                    order.sort_by(|&a, &b| {
                        instrument.samples[a]
                            .power
                            .partial_cmp(&instrument.samples[b].power)
                            .unwrap_or(Ordering::Equal)
                    });
                }
                order
            })
            .collect();

        Self {
            sample_rate: kit.samplerate as f32,
            kit,
            bank,
            midimap,
            output_index,
            instrument_index,
            sample_order,
            voices: Vec::new(),
            rng: XorShift::new(),
        }
    }

    /// Updates the host sample rate used for choke fade times.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    /// Handles a MIDI note-on: cuts the instrument's choke targets (and its own
    /// previous voice), then starts a new voice.
    pub fn note_on(&mut self, note: u8, velocity: u8) {
        let Some(instrument_name) = self.midimap.instrument_for_note(note) else {
            return;
        };
        let Some(instrument_index) = self.instrument_index.get(instrument_name).copied() else {
            return;
        };

        let chokes = self.kit.instruments[instrument_index].chokes.clone();
        for choke in &chokes {
            if let Some(&victim) = self.instrument_index.get(&choke.instrument) {
                self.choke_instrument(victim, choke.choketime_ms);
            }
        }
        // Self-choke: a retrigger cuts the previous voice.
        self.choke_instrument(instrument_index, 0);

        self.trigger(instrument_index, velocity as f32 / 127.0);
    }

    /// Stops all ringing voices immediately (panic / all-notes-off).
    pub fn all_notes_off(&mut self) {
        self.voices.clear();
    }

    /// Mixes all voices into `out`, one mono buffer per kit channel.
    pub fn process(&mut self, out: &mut [&mut [f32]]) {
        let frames = out.first().map_or(0, |buffer| buffer.len());
        for buffer in out.iter_mut() {
            buffer.fill(0.0);
        }

        for voice in &mut self.voices {
            if voice.finished {
                continue;
            }

            let start = voice.position as usize;
            let gain_start = voice.gain;
            let gain_step = -voice.fade_step;
            let mut max_len = 0usize;
            for stream in &voice.streams {
                max_len = max_len.max(stream.data.len());
                if start >= stream.data.len() {
                    continue;
                }
                let data = &stream.data;
                let read_to = (start + frames).min(data.len());
                let out_buffer = &mut out[stream.output];
                for (frame, &value) in data[start..read_to].iter().enumerate() {
                    let gain = (gain_start + gain_step * frame as f32).max(0.0);
                    out_buffer[frame] += value * gain;
                }
            }

            voice.position += frames as f32;
            voice.gain = (gain_start + gain_step * frames as f32).max(0.0);
            if voice.fade_step > 0.0 && voice.gain <= 0.0 {
                voice.finished = true;
            }
            if start + frames >= max_len {
                voice.finished = true;
            }
        }

        self.voices.retain(|voice| !voice.finished);
    }

    /// The number of currently active voices (useful for debugging/tests).
    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }

    fn trigger(&mut self, instrument_index: usize, velocity: f32) {
        let Some(sample_index) = self.select_sample(instrument_index, velocity) else {
            return;
        };
        let streams = self.build_streams(instrument_index, sample_index);
        if streams.is_empty() {
            return;
        }

        if self.voices.len() >= MAX_VOICES {
            self.voices.remove(0);
        }
        self.voices.push(Voice {
            instrument: instrument_index,
            streams,
            position: 0.0,
            gain: 1.0,
            fade_step: 0.0,
            finished: false,
        });
    }

    /// Picks a sample for `instrument` based on the normalized velocity.
    fn select_sample(&mut self, instrument_index: usize, velocity: f32) -> Option<usize> {
        let instrument = &self.kit.instruments[instrument_index];
        let velocity = velocity.clamp(0.0, 1.0);

        if instrument.is_v2() {
            // Loudest sample whose power does not exceed the velocity.
            let order = &self.sample_order[instrument_index];
            order
                .iter()
                .rev()
                .copied()
                .find(|&index| instrument.samples[index].power <= velocity)
                .or_else(|| order.first().copied())
        } else {
            // v1.0: pick the velocity group containing the velocity, then a
            // sample reference weighted by its probability.
            let group = instrument
                .velocities
                .iter()
                .find(|group| velocity >= group.lower && velocity < group.upper)
                .or_else(|| instrument.velocities.last())?;
            let total: f32 = group.sample_refs.iter().map(|r| r.probability).sum();
            let roll = self.rng.next_f32() * total;
            let mut accumulated = 0.0;
            for reference in &group.sample_refs {
                accumulated += reference.probability;
                if roll < accumulated {
                    return instrument
                        .samples
                        .iter()
                        .position(|sample| sample.name == reference.name);
                }
            }
            let fallback = group
                .sample_refs
                .last()
                .map(|reference| reference.name.as_str());
            instrument
                .samples
                .iter()
                .position(|sample| Some(sample.name.as_str()) == fallback)
        }
    }

    /// Resolves the sample's audio files into output streams via the
    /// instrument's channel map.
    fn build_streams(&self, instrument_index: usize, sample_index: usize) -> Vec<VoiceStream> {
        let instrument = &self.kit.instruments[instrument_index];
        let sample = &instrument.samples[sample_index];

        sample
            .audio_files
            .iter()
            .filter_map(|audio| {
                let output = instrument
                    .channel_map
                    .iter()
                    .find(|map| map.in_name == audio.channel)
                    .and_then(|map| self.output_index.get(&map.out_name))
                    .copied()?;
                let data = self.bank.audio_file(&instrument.base_dir, audio)?.clone();
                Some(VoiceStream { output, data })
            })
            .collect()
    }

    /// Fades out (or cuts) every ringing voice of `instrument`.
    fn choke_instrument(&mut self, instrument_index: usize, choketime_ms: u32) {
        let fade_frames = choketime_ms as f32 / 1000.0 * self.sample_rate;
        for voice in &mut self.voices {
            if voice.instrument != instrument_index || voice.finished || voice.fade_step > 0.0 {
                continue;
            }
            if fade_frames <= 1.0 {
                voice.finished = true;
            } else {
                voice.fade_step = voice.gain / fade_frames;
            }
        }
    }
}

/// A tiny deterministic PRNG for probability-weighted sample selection (v1.0
/// kits). Not cryptographically strong, but cheap and good enough for this.
struct XorShift(u64);

impl XorShift {
    fn new() -> Self {
        Self(0x2545_F491_4F6C_DD1D)
    }

    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}
