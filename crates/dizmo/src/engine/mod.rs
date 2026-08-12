//! Realtime drum engine: turns MIDI note events into sample voices.
//!
//! The engine reads only immutable data: a loaded [`Kit`] and a [`SampleBank`]
//! built off the audio thread. It never allocates or takes locks while
//! processing.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::kit::{DizmoKit, KitError, MidiMap};
use crate::params::NUM_CHANNELS;
use crate::samples::{SampleBank, SampleError, load_samples_with_progress};

/// Maximum number of simultaneously playing voices; the oldest is faded out
/// first when exceeded.
pub const MAX_VOICES: usize = 64;

/// Minimum sample-set size for the v2 power-list spread: with fewer samples
/// the velocity layers get wider. Matches DrumGizmo's `MIN_SAMPLE_SET_SIZE`.
const MIN_SAMPLE_SET_SIZE: usize = 26;

/// Redraws allowed when a v2 power-list draw repeats the previously chosen
/// sample (DrumGizmo's `PowerList::get` retry count).
const MAX_RETRIES: usize = 3;

/// Length of the click-prevention fade applied when a ringing voice is cut
/// early by voice stealing at `MAX_VOICES` or by all-notes-off. No new hit
/// overlaps these, so a longer fade is safe.
const CUT_FADE_MS: f32 = 5.0;

/// Length of the fade applied to the previous voice of the same instrument on
/// retrigger. Much shorter than [`CUT_FADE_MS`]: a fresh hit starts
/// immediately, so the old voice must vanish almost instantly to avoid
/// doubling the drum (e.g. two kicks at once), while still ramping to silence
/// to avoid a click. The residual 1 ms overlap is masked by the new attack.
const RETRIGGER_FADE_MS: f32 = 1.0;

/// Length of the fade-in applied to every new voice. A sample may not start at
/// a zero crossing, so an instantly full-gain voice would step from whatever
/// the previous output was to the sample's first value, popping. Ramping the
/// gain from 0 over ~1 ms hides that discontinuity; the transient attack of a
/// drum hit is not audibly dulled by such a short ramp.
const ATTACK_FADE_MS: f32 = 1.0;

/// The drum engine: note in -> voice playback into per-kit-channel mono output.
pub struct Engine {
    kit: Arc<DizmoKit>,
    bank: Arc<SampleBank>,
    midimap: MidiMap,
    /// Kit channel name -> index in the output buffer.
    output_index: HashMap<String, usize>,
    /// Instrument name -> index into `kit.instruments`.
    instrument_index: HashMap<String, usize>,
    /// Per instrument: the smallest and largest sample power (v2 power list).
    power_min: Vec<f32>,
    power_max: Vec<f32>,
    /// Per instrument: the sample chosen by the last v2 power-list draw, used
    /// to avoid repeating the same sample twice in a row.
    last_v2_sample: Vec<Option<usize>>,
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
    /// Frames left in the initial fade-in from silence; `0` once the voice is
    /// at full gain. The attack ramps `gain` up at [`ATTACK_FADE_MS`].
    attack_remaining: f32,
    /// Per-frame linear gain increment while in the attack ramp; `0` otherwise.
    attack_step: f32,
    /// Per-frame linear gain decrement while fading out (choke or
    /// click-prevention cut); `0` when not fading.
    fade_step: f32,
    finished: bool,
}

/// A mono buffer playing into one output channel.
struct VoiceStream {
    output: usize,
    data: Arc<[f32]>,
}

/// One row of the editor's Mappings dialog: an instrument with the MIDI notes
/// that trigger it and its channel assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentMapping {
    pub instrument: String,
    /// MIDI notes from the midimap that trigger this instrument.
    pub notes: Vec<u8>,
    /// The instrument's primary channel mapping (at most one entry).
    pub channel_map: Vec<ChannelAssignment>,
}

/// A single `<channelmap>` entry of an instrument.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelAssignment {
    pub in_name: String,
    pub out_name: String,
    pub is_main: bool,
}

impl Engine {
    pub fn new(kit: Arc<DizmoKit>, bank: Arc<SampleBank>, midimap: MidiMap) -> Self {
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
        let (power_min, power_max) = kit
            .instruments
            .iter()
            .map(|instrument| {
                let mut min = f32::INFINITY;
                let mut max = f32::NEG_INFINITY;
                for sample in &instrument.samples {
                    min = min.min(sample.power);
                    max = max.max(sample.power);
                }
                (min, max)
            })
            .unzip();
        let last_v2_sample = vec![None; kit.instruments.len()];

        Self {
            sample_rate: kit.samplerate as f32,
            kit,
            bank,
            midimap,
            output_index,
            instrument_index,
            power_min,
            power_max,
            last_v2_sample,
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
        // Self-choke: a retrigger fades out the previous voice almost
        // instantly so the drum is not heard twice, without clicking.
        self.choke_instrument(instrument_index, RETRIGGER_FADE_MS as u32);

        self.trigger(instrument_index, velocity as f32 / 127.0);
    }

    /// Handles a MIDI note-off. For drum samplers, note-offs are typically
    /// ignored (samples play to completion), but we provide this for completeness.
    pub fn note_off(&mut self, _note: u8, _velocity: u8) {
        // Drums typically ignore note-off and let samples ring out naturally.
        // This is a no-op for standard drum behavior.
    }

    /// Fades out all ringing voices (panic / all-notes-off) so they stop
    /// click-free instead of being cut.
    pub fn all_notes_off(&mut self) {
        let fade_frames = cut_fade_frames(self.sample_rate);
        for voice in &mut self.voices {
            if voice.finished || voice.fade_step > 0.0 {
                continue;
            }
            begin_cut_fade(fade_frames, voice);
        }
    }

    /// Mixes all voices into `out`, one mono `Vec` per kit channel. The buffers
    /// must be at least `offset + frames` samples long; only `frames` samples
    /// starting at `offset` are written. The caller is responsible for clearing
    /// the buffers before processing a block. No allocations happen here.
    pub fn process(&mut self, offset: usize, frames: usize, out: &mut [Vec<f32>]) {
        if frames == 0 {
            return;
        }

        for voice in &mut self.voices {
            if voice.finished {
                continue;
            }

            let start = voice.position as usize;
            let gain_start = voice.gain;
            let gain_step = voice.attack_step - voice.fade_step;
            let mut max_len = 0usize;
            for stream in &voice.streams {
                max_len = max_len.max(stream.data.len());
                if start >= stream.data.len() || stream.output >= out.len() {
                    continue;
                }
                let data = &stream.data;
                let read_to = (start + frames).min(data.len());
                let out_buffer = &mut out[stream.output];
                for (frame, &value) in data[start..read_to].iter().enumerate() {
                    let gain = (gain_start + gain_step * frame as f32).clamp(0.0, 1.0);
                    out_buffer[offset + frame] += value * gain;
                }
            }

            voice.position += frames as f32;
            voice.gain = (gain_start + gain_step * frames as f32).clamp(0.0, 1.0);
            if voice.attack_remaining > 0.0 {
                voice.attack_remaining -= frames as f32;
                if voice.attack_remaining <= 0.0 {
                    // The ramp reached full gain; stop advancing it.
                    voice.attack_step = 0.0;
                    voice.gain = 1.0;
                }
            }
            if voice.fade_step > 0.0 && voice.gain <= 0.0 {
                voice.finished = true;
            }
            if start + frames >= max_len {
                voice.finished = true;
            }
        }

        self.voices.retain(|voice| !voice.finished);
    }

    /// The number of output channels this engine writes (one per kit channel,
    /// capped at the number of plugin outputs).
    pub fn kit_channels(&self) -> usize {
        self.kit.channels.len().min(NUM_CHANNELS)
    }

    /// The kit's display name from drumkit.xml.
    pub fn kit_name(&self) -> &str {
        &self.kit.name
    }

    /// The instrument assigned to each kit output channel via its channelmap,
    /// considering only `main` channelmap entries. `None` for channels no
    /// instrument routes its main channel to. Shown in the editor strips when a
    /// kit is loaded.
    pub fn instruments_per_channel(&self) -> Vec<Option<String>> {
        let mut main = vec![None; self.kit.channels.len()];
        for instrument in &self.kit.instruments {
            for map in instrument.channel_map.iter().filter(|map| map.is_main) {
                let Some(&output) = self.output_index.get(&map.out_name) else {
                    continue;
                };
                main[output].get_or_insert_with(|| instrument.name.clone());
            }
        }
        main
    }

    /// The kit's channel names, in output order, exactly as declared in the
    /// drumkit's `<channels>` section. Shown in the editor strips.
    pub fn channel_names(&self) -> Vec<String> {
        self.kit
            .channels
            .iter()
            .map(|channel| channel.name.clone())
            .collect()
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
            // Fade out the oldest voice instead of cutting it, so stealing a
            // ringing voice does not click. The faded voice expires within
            // CUT_FADE_MS and frees its slot.
            let fade_frames = cut_fade_frames(self.sample_rate);
            if let Some(oldest) = self.voices.first_mut() {
                begin_cut_fade(fade_frames, oldest);
            }
        }

        let attack_frames = attack_fade_frames(self.sample_rate);
        let attack_step = 1.0 / attack_frames.max(1.0);
        self.voices.push(Voice {
            instrument: instrument_index,
            streams,
            position: 0.0,
            // The ramp starts just above silence (frame 0 is `attack_step`) so
            // a sample that begins at a non-zero crossing steps in softly.
            gain: attack_step,
            attack_remaining: attack_frames,
            attack_step,
            fade_step: 0.0,
            finished: false,
        });
    }

    /// The per-instrument MIDI and channel mappings, shown in the editor's
    /// Mappings dialog. Computed once at load time. Each instrument gets a
    /// primary channels mapping from its `main` channelmap entries: the
    /// entry whose input channel shares the instrument's name, falling back to
    /// the first `main` entry. Instruments without a `main` entry expose no
    /// channel mapping.
    pub fn mappings(&self) -> Vec<InstrumentMapping> {
        self.kit
            .instruments
            .iter()
            .map(|instrument| {
                let notes = self
                    .midimap
                    .entries
                    .iter()
                    .filter(|entry| entry.instrument == instrument.name)
                    .map(|entry| entry.note)
                    .collect();
                let channel_map = {
                    let primary = instrument
                        .channel_map
                        .iter()
                        .find(|map| map.is_main && map.in_name == instrument.name)
                        .or_else(|| instrument.channel_map.iter().find(|map| map.is_main));
                    primary
                        .map(|map| ChannelAssignment {
                            in_name: map.in_name.clone(),
                            out_name: map.out_name.clone(),
                            is_main: map.is_main,
                        })
                        .into_iter()
                        .collect()
                };
                InstrumentMapping {
                    instrument: instrument.name.clone(),
                    notes,
                    channel_map,
                }
            })
            .collect()
    }

    /// Picks a sample for `instrument` based on the normalized velocity.
    fn select_sample(&mut self, instrument_index: usize, velocity: f32) -> Option<usize> {
        let instrument = &self.kit.instruments[instrument_index];
        let velocity = velocity.clamp(0.0, 1.0);

        if instrument.is_v2() {
            self.select_v2_sample(instrument_index, velocity)
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

    /// Version 2.0 sample selection (the PowerList). Draws a Gaussian target
    /// power centered on the velocity's position in the instrument's power
    /// range, then picks the sample whose power is closest. If the draw lands
    /// on the same sample as the previous hit, it redraws up to [`MAX_RETRIES`]
    /// times (DrumGizmo's anti-repetition).
    fn select_v2_sample(&mut self, instrument_index: usize, velocity: f32) -> Option<usize> {
        let samples = &self.kit.instruments[instrument_index].samples;
        if samples.is_empty() {
            return None;
        }

        let power_span = self.power_max[instrument_index] - self.power_min[instrument_index];
        let width = samples.len().max(MIN_SAMPLE_SET_SIZE) as f32;
        let stddev = power_span / width;
        let mean = velocity * (power_span - stddev) + stddev / 2.0;
        let power_min = self.power_min[instrument_index];

        let mut retries = MAX_RETRIES;
        let chosen = loop {
            // One Box–Muller draw: two uniform values in [0,1) map to a sample
            // of the normal distribution centered on `mean`.
            let u1 = self.rng.next_f32();
            let u2 = self.rng.next_f32();
            let x = (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos();
            let lvl = mean + stddev * x + power_min;

            // The sample whose power is closest to the draw. First candidate
            // wins ties (only strictly-smaller distances replace it).
            let mut best = 0usize;
            let mut best_dist = f32::INFINITY;
            for (index, sample) in samples.iter().enumerate() {
                let dist = (sample.power - lvl).abs();
                if dist < best_dist {
                    best_dist = dist;
                    best = index;
                }
            }

            let candidate = Some(best);
            if self.last_v2_sample[instrument_index] != candidate || retries == 0 {
                break candidate;
            }
            retries -= 1;
        };

        self.last_v2_sample[instrument_index] = chosen;
        chosen
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
                    .filter(|map| map.is_main)
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
                end_attack(voice);
                voice.fade_step = voice.gain / fade_frames;
            }
        }
    }
}

/// Frames a [`CUT_FADE_MS`] fade spans at `sample_rate`.
fn cut_fade_frames(sample_rate: f32) -> f32 {
    CUT_FADE_MS / 1000.0 * sample_rate
}

/// Frames a [`ATTACK_FADE_MS`] fade-in spans at `sample_rate`.
fn attack_fade_frames(sample_rate: f32) -> f32 {
    ATTACK_FADE_MS / 1000.0 * sample_rate
}

/// Stops the attack ramp on `voice`, leaving `gain` where it is so a fade-out
/// from that point ramps to silence instead of climbing back up.
fn end_attack(voice: &mut Voice) {
    voice.attack_step = 0.0;
    voice.attack_remaining = 0.0;
}

/// Starts a click-prevention fade on `voice`, or finishes it when the fade is
/// shorter than a single frame.
fn begin_cut_fade(fade_frames: f32, voice: &mut Voice) {
    if fade_frames <= 1.0 {
        voice.finished = true;
    } else {
        end_attack(voice);
        voice.fade_step = voice.gain / fade_frames;
    }
}

/// A tiny deterministic PRNG for sample selection (v1.0 velocity groups and
/// the v2 power list). Not cryptographically strong, but cheap and good enough
/// for this.
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

/// Errors that can occur while building an [`Engine`] from a kit on disk.
#[derive(Debug, thiserror::Error)]
pub enum EngineLoadError {
    #[error("failed to load kit: {0}")]
    Kit(#[from] KitError),
    #[error("failed to load samples: {0}")]
    Samples(#[from] SampleError),
}

/// Loads a DrumGizmo kit (drumkit.xml, its instruments, midimap and samples)
/// into a new [`Engine`]. Samples are resampled to `target_sample_rate` (the
/// host rate) if it is set, so playback needs no runtime conversion. Performs
/// disk I/O and allocation, so it must not be called on the audio thread.
pub fn load_engine(
    kit_path: impl AsRef<Path>,
    target_sample_rate: Option<u32>,
) -> Result<Engine, EngineLoadError> {
    load_engine_with_progress(kit_path, target_sample_rate, &mut |_, _| {})
        .map(|(engine, _warnings)| engine)
}

/// Like [`load_engine`], but reports decoding progress via `progress(loaded, total)`
/// and returns any non-fatal loading warnings alongside the engine.
pub fn load_engine_with_progress(
    kit_path: impl AsRef<Path>,
    target_sample_rate: Option<u32>,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(Engine, Vec<String>), EngineLoadError> {
    let kit = Arc::new(DizmoKit::load(kit_path)?);
    // The default midimap (declared or convention-detected) is best-effort:
    // the convention spelling is tried first, then the same name with the
    // leading "midimap" in the other case. A missing file leaves the kit
    // unmapped instead of failing the whole load, matching DrumGizmo. Parse
    // errors are still fatal.
    let midimap = kit
        .load_midimap_candidates(&kit.default_midimap_candidates())?
        .unwrap_or_default();
    let bank = Arc::new(load_samples_with_progress(
        &kit,
        target_sample_rate,
        progress,
    )?);
    let engine = Engine::new(kit, bank, midimap);

    let mut warnings = Vec::new();
    let declared = engine.channel_names().len();
    if declared > NUM_CHANNELS {
        warnings.push(format!(
            "'{}' declares {declared} channels, but DIZMO supports {NUM_CHANNELS}. \
             Channels beyond {NUM_CHANNELS} were ignored.",
            engine.kit_name()
        ));
    }
    Ok((engine, warnings))
}
