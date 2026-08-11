use nice_plug::prelude::*;
use nice_plug_iced::iced::PollSubNotifier;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::engine::{Engine, InstrumentMapping};
use crate::params::{AUX_OUTPUT_NAMES, AUX_OUTPUT_PORTS, ChannelParams, DizmoParams, NUM_CHANNELS};

pub mod engine;
pub mod kit;
pub mod params;
mod ui;

/// A kit-load request dispatched from the editor GUI.
pub enum LoadTask {
    LoadKit { path: PathBuf },
}

/// The currently loaded kit, shared with the editor so a reopened window can
/// show the kit that is already loaded instead of relying on a one-shot
/// channel message that the previous window already consumed.
#[derive(Clone, Default)]
pub(crate) struct KitInfo {
    pub name: String,
    pub channels: Vec<String>,
    pub mappings: Vec<InstrumentMapping>,
    /// Non-fatal loading problems (e.g. more channels than outputs).
    pub warnings: Vec<String>,
}

impl KitInfo {
    fn to_status(&self) -> KitStatus {
        KitStatus::Loaded {
            name: self.name.clone(),
            channels: self.channels.clone(),
            mappings: self.mappings.clone(),
            warnings: self.warnings.clone(),
        }
    }
}

/// Load status messages sent from the loader thread to the editor GUI.
#[derive(Debug)]
pub(crate) enum KitStatus {
    /// The kit was loaded; carries its display name from drumkit.xml and the
    /// kit's channel names.
    Loaded {
        name: String,
        /// The kit's channel names from its `<channels>` section.
        channels: Vec<String>,
        /// The per-instrument MIDI note and channel mappings for the dialog.
        mappings: Vec<InstrumentMapping>,
        /// Non-fatal loading problems (e.g. more channels than outputs).
        warnings: Vec<String>,
    },
    /// The kit failed to load; carries the error message.
    Failed(String),
    /// Decoding progress, as `(files_decoded, total_files)`.
    Progress { loaded: usize, total: usize },
}

/// Extracts the panic message from a caught panic payload, if it has one.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// The audio-thread state shared by both plugin variants: the loaded engine and
/// the reusable per-kit-channel scratch buffers. Everything here is mutated off
/// the audio thread (initialize, kit loads) except `render`, which runs inside
/// `process`.
struct AudioCore {
    engine: Option<Engine>,
    /// One mono buffer per possible kit channel, sized to the maximum host
    /// block size in `initialize` so process never allocates.
    scratch: Vec<Vec<f32>>,
    sample_rate: f32,
    /// Receives fully-loaded engines from the loader thread.
    engine_rx: Option<crossbeam_channel::Receiver<Result<Engine, String>>>,
    /// Retired engines are sent here so they get dropped off the audio thread.
    old_engine_tx: Option<crossbeam_channel::Sender<Engine>>,
    /// The kit-status sender, re-pointed to a fresh channel every time the
    /// editor opens. Load progress and results therefore reach whichever editor
    /// is currently visible, and a reopened window is seeded with the current
    /// kit; sends are dropped harmlessly while the editor is closed. The loader
    /// thread sends through the mutex so it always targets the live editor.
    status_tx: Arc<Mutex<Option<crossbeam_channel::Sender<KitStatus>>>>,
    /// The most recently loaded kit. The editor seeds its header from here at
    /// every boot, so a reopened window shows the kit that is already loaded;
    /// the loader thread and the synchronous `load_kit` path keep it current.
    kit_info: Arc<Mutex<Option<KitInfo>>>,
    /// The sample rate the loader thread resamples to; set in `initialize`.
    host_sample_rate: Arc<AtomicU32>,
    /// Per-channel linear peak levels (as `f32` bits), written here each block
    /// on the audio thread and shared with the editor for its peak meters.
    levels: Arc<[AtomicU32; NUM_CHANNELS]>,
    /// The held raw per-channel peaks (as `f32` bits): the loudest sample seen
    /// recently, decaying on a slow release. The fader gain is applied on top
    /// each block, so the published level is post-fader while the underlying
    /// peak still follows the sound without flickering.
    held_peaks: [AtomicU32; NUM_CHANNELS],
    /// An atomic flag the audio thread sets when a note lights an indicator;
    /// the iced editor polls it before every draw and redraws when set.
    notifier: PollSubNotifier,
}

impl AudioCore {
    fn new() -> Self {
        Self {
            engine: None,
            scratch: Vec::new(),
            sample_rate: 44100.0,
            engine_rx: None,
            old_engine_tx: None,
            status_tx: Arc::new(Mutex::new(None)),
            kit_info: Arc::new(Mutex::new(None)),
            host_sample_rate: Arc::new(AtomicU32::new(0)),
            levels: Arc::new(std::array::from_fn(|_| AtomicU32::new(0))),
            held_peaks: std::array::from_fn(|_| AtomicU32::new(0)),
            notifier: PollSubNotifier::new(),
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.host_sample_rate
            .store(sample_rate as u32, Ordering::Relaxed);
        if let Some(engine) = self.engine.as_mut() {
            engine.set_sample_rate(sample_rate);
        }
    }

    /// Installs engines finished by the loader thread, retiring the previous
    /// engine off the audio thread. Called at the top of every process block;
    /// never blocks or allocates.
    fn check_engine_updates(&mut self) {
        let Some(engine_rx) = &self.engine_rx else {
            return;
        };
        while let Ok(loaded) = engine_rx.try_recv() {
            if let Ok(engine) = loaded
                && let Some(old) = self.engine.replace(engine)
            {
                // The loader thread always sets `old_engine_tx` alongside
                // `engine_rx`, so this is only `None` when no load can happen.
                if let Some(tx) = &self.old_engine_tx {
                    let _ = tx.send(old);
                }
            }
        }
    }

    fn set_block_size(&mut self, block_size: usize) {
        self.scratch = vec![vec![0.0; block_size]; NUM_CHANNELS];
    }

    fn scratch_ready(&self, frames: usize) -> bool {
        self.scratch.len() == NUM_CHANNELS
            && self
                .scratch
                .first()
                .is_some_and(|buffer| buffer.len() >= frames)
    }

    /// Updates the shared per-channel peak levels from the scratch buffers.
    /// Each channel's raw peak is held on a slow release (so the meter follows
    /// the sound without flickering), then scaled by the channel's post-fader
    /// gain every block, so the published level is what the channel actually
    /// contributes to the mix and fader, mute, and solo changes show up
    /// immediately. Realtime-safe: no allocation, no locks.
    fn update_levels(
        &self,
        frames: usize,
        active_channels: usize,
        channel_params: &[ChannelParams; NUM_CHANNELS],
    ) {
        let dt = frames as f32 / self.sample_rate;
        let decay = (-dt / 0.25).exp();
        let any_soloed = channel_params
            .iter()
            .take(active_channels)
            .any(|param| param.solo.value());
        for (index, buffer) in self.scratch.iter().enumerate() {
            let peak = buffer[..frames]
                .iter()
                .fold(0.0f32, |max, sample| max.max(sample.abs()));
            let held = &self.held_peaks[index];
            let prev = f32::from_bits(held.load(Ordering::Relaxed));
            let held_peak = peak.max(prev * decay);
            held.store(held_peak.to_bits(), Ordering::Relaxed);

            let gain = if index < active_channels {
                post_fader_gain(&channel_params[index], any_soloed)
            } else {
                0.0
            };
            self.levels[index].store((held_peak * gain).to_bits(), Ordering::Relaxed);
        }
    }

    /// Tells the editor that `engine` is the current kit, so the header and
    /// strips show its name and channel names even when it was loaded outside
    /// the LOAD KIT button flow. The kit is remembered in shared state for any
    /// editor that boots later; the live window also gets a status message.
    fn notify_loaded(&self, engine: &Engine) {
        let info = KitInfo {
            name: engine.kit_name().to_string(),
            channels: engine.channel_names(),
            mappings: engine.mappings(),
            warnings: Vec::new(),
        };
        *self.kit_info.lock().expect("kit info mutex poisoned") = Some(info.clone());
        self.send_status(info.to_status());
    }

    /// Sends a kit-status message to the currently open editor, if any. The
    /// loader thread and the synchronous `load_kit` path both go through here
    /// so they always reach the live editor; messages are dropped while the
    /// editor is closed.
    fn send_status(&self, status: KitStatus) {
        let slot = self.status_tx.lock().expect("status tx mutex poisoned");
        if let Some(tx) = slot.as_ref() {
            let _ = tx.send(status);
        }
    }
}

/// Sums the first `channels` kit-channel scratch buffers into the MAIN stereo
/// pair, applying per-channel gain, pan, mute, and solo.
pub fn mixdown_to_stereo(
    scratch: &[Vec<f32>],
    channels: usize,
    frames: usize,
    left: &mut [f32],
    right: &mut [f32],
    channel_params: &[ChannelParams; NUM_CHANNELS],
) {
    // Determine if any channel is soloed
    let any_soloed = channel_params.iter().take(channels).any(|p| p.solo.value());

    for sample in 0..frames {
        let mut left_sum = 0.0;
        let mut right_sum = 0.0;
        for (channel_idx, channel_data) in scratch.iter().enumerate().take(channels) {
            let param = &channel_params[channel_idx];

            // Solo logic: if any channel is soloed, only soloed channels play
            if any_soloed && !param.solo.value() {
                continue;
            }

            // Mute logic: skip if muted (solo overrides mute)
            if param.mute.value() && !param.solo.value() {
                continue;
            }

            // Fader is stored as linear gain (not dB); step the smoother once
            // per sample so automation ramps instead of zipper-stepping.
            let gain_linear = if param.fader.smoothed.is_smoothing() {
                param.fader.smoothed.next()
            } else {
                param.fader.value()
            };
            // Pan is stored as a percentage (-100 .. 100); step the smoother
            // once per sample so automation ramps instead of zipper-stepping,
            // then scale to -1 .. 1.
            let pan_pos = if param.pan.smoothed.is_smoothing() {
                param.pan.smoothed.next()
            } else {
                param.pan.value()
            } / 100.0;

            // Pan law: Constant power -3dB law
            // pan_pos: -1 (full left) to +1 (full right)
            // At center (0): both channels get 0.707 (-3dB)
            // At extremes: panned channel gets 1.0, opposite gets 0.0
            let pan_angle = (pan_pos + 1.0) * std::f32::consts::FRAC_PI_4;
            let pan_left = pan_angle.cos();
            let pan_right = pan_angle.sin();

            left_sum += channel_data[sample] * gain_linear * pan_left;
            right_sum += channel_data[sample] * gain_linear * pan_right;
        }

        left[sample] = left_sum;
        right[sample] = right_sum;
    }
}

/// The gain a channel's raw output passes through before it reaches the mix:
/// the fader's linear gain, or 0 when the channel is muted or soloed out.
/// Matches the audible-state logic in [`mixdown_to_stereo`] and the multi
/// plugin's output pass, so the meters read the level the channel actually
/// contributes.
fn post_fader_gain(param: &ChannelParams, any_soloed: bool) -> f32 {
    // Solo overrides mute; otherwise a muted channel, or any non-soloed
    // channel while something else is soloed, is inaudible.
    let audible = param.solo.value() || (!any_soloed && !param.mute.value());
    if audible { param.fader.value() } else { 0.0 }
}

/// The stereo plugin: all channels are mixed down to the MAIN bus.
pub struct DizmoPlugin {
    params: Arc<DizmoParams>,
    core: AudioCore,
}

/// The multi-output plugin: each of the 16 channels gets its own output bus.
pub struct DizmoMultiPlugin {
    params: Arc<DizmoParams>,
    core: AudioCore,
}

const STEREO_LAYOUT: AudioIOLayout = AudioIOLayout {
    main_input_channels: None,
    main_output_channels: NonZeroU32::new(2),
    aux_input_ports: &[],
    aux_output_ports: &[],
    names: PortNames {
        layout: Some("Stereo"),
        main_output: Some("MAIN"),
        ..PortNames::const_default()
    },
};

const MULTI_LAYOUT: AudioIOLayout = AudioIOLayout {
    main_input_channels: None,
    main_output_channels: None,
    aux_input_ports: &[],
    aux_output_ports: &AUX_OUTPUT_PORTS,
    names: PortNames {
        layout: Some("Multi"),
        aux_outputs: &AUX_OUTPUT_NAMES,
        ..PortNames::const_default()
    },
};

/// Implements the shared plugin plumbing for a DIZMO variant, given the bits that differ:
/// its display name, audio IO layout, format-specific IDs/features, and whether the editor
/// shows the pan knob (pan has no effect in the multi plugin).
macro_rules! impl_dizmo_plugin {
    (
        $plugin:ty,
        $name:literal,
        $layout:ident,
        $clap_id:literal,
        $clap_features:expr,
        $vst3_id:expr,
        $show_pan:expr
    ) => {
        impl Default for $plugin {
            fn default() -> Self {
                Self {
                    params: Arc::new(DizmoParams::default()),
                    core: AudioCore::new(),
                }
            }
        }

        impl $plugin {
            /// Loads a DrumGizmo kit (drumkit.xml, its instruments, midimap and
            /// samples) into the engine. Performs disk I/O and allocation, so it
            /// must not be called on the audio thread.
            pub fn load_kit(
                &mut self,
                kit_path: impl AsRef<Path>,
            ) -> Result<(), engine::EngineLoadError> {
                let mut engine = engine::load_engine(kit_path, Some(self.core.sample_rate as u32))?;
                engine.set_sample_rate(self.core.sample_rate);
                self.core.notify_loaded(&engine);
                self.core.engine = Some(engine);
                Ok(())
            }
        }

        impl Plugin for $plugin {
            const NAME: &'static str = $name;
            const VENDOR: &'static str = "OneKnobAudio";
            const URL: &'static str = "https://github.com/yorodm/dizmo";
            const EMAIL: &'static str = "info@dizmo.invalid";
            const VERSION: &'static str = env!("CARGO_PKG_VERSION");

            const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[$layout];

            const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
            const SAMPLE_ACCURATE_AUTOMATION: bool = true;

            type SysExMessage = ();
            type BackgroundTask = LoadTask;

            fn params(&self) -> Arc<dyn Params> {
                self.params.clone()
            }

            fn editor(&mut self, async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
                let load_kit: Arc<dyn Fn(PathBuf) + Send + Sync> = Arc::new(move |path| {
                    async_executor.execute_background(LoadTask::LoadKit { path });
                });
                // Point the kit-status channel at this fresh editor window, so
                // live load progress and results reach it. The current kit is
                // seeded at boot from the shared `kit_info` instead, which is
                // why a reopened window still shows what is loaded.
                let (status_tx, status_rx) = crossbeam_channel::unbounded();
                *self
                    .core
                    .status_tx
                    .lock()
                    .expect("status tx mutex poisoned") = Some(status_tx);
                let levels = self.core.levels.clone();
                let kit_info = self.core.kit_info.clone();
                let notifier = self.core.notifier.clone();
                Some(Box::new(ui::DizmoEditor::new(
                    self.params.clone(),
                    $show_pan,
                    load_kit,
                    Some(status_rx),
                    levels,
                    kit_info,
                    notifier,
                )))
            }

            fn task_executor(&mut self) -> TaskExecutor<Self> {
                let (engine_tx, engine_rx) = crossbeam_channel::unbounded();
                let (old_engine_tx, old_engine_rx) = crossbeam_channel::unbounded();
                self.core.engine_rx = Some(engine_rx);
                self.core.old_engine_tx = Some(old_engine_tx);

                // A dedicated thread drops retired engines so deallocation never
                // happens on the audio thread.
                std::thread::spawn(move || {
                    while let Ok(engine) = old_engine_rx.recv() {
                        drop(engine);
                    }
                });

                let host_sample_rate = self.core.host_sample_rate.clone();
                let status_tx = self.core.status_tx.clone();
                let kit_info = self.core.kit_info.clone();
                Box::new(move |task: LoadTask| {
                    let LoadTask::LoadKit { path } = task;
                    let engine_tx = engine_tx.clone();
                    let status_tx = status_tx.clone();
                    let kit_info = kit_info.clone();
                    let host_sample_rate = host_sample_rate.clone();
                    std::thread::spawn(move || {
                        let rate = host_sample_rate.load(Ordering::Relaxed);
                        let start = std::time::Instant::now();
                        let load = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            engine::load_engine_with_progress(
                                &path,
                                (rate != 0).then_some(rate),
                                &mut |loaded, total| {
                                    let slot = status_tx.lock().expect("status tx mutex poisoned");
                                    if let Some(tx) = slot.as_ref() {
                                        let _ = tx.send(KitStatus::Progress { loaded, total });
                                    }
                                },
                            )
                        }));
                        match load {
                            Ok(Ok((mut engine, warnings))) => {
                                engine.set_sample_rate(if rate == 0 {
                                    44100.0
                                } else {
                                    rate as f32
                                });
                                let info = KitInfo {
                                    name: engine.kit_name().to_string(),
                                    channels: engine.channel_names(),
                                    mappings: engine.mappings(),
                                    warnings,
                                };
                                let _ = engine_tx.send(Ok(engine));
                                *kit_info.lock().expect("kit info mutex poisoned") =
                                    Some(info.clone());
                                let slot = status_tx.lock().expect("status tx mutex poisoned");
                                if let Some(tx) = slot.as_ref() {
                                    let _ = tx.send(info.to_status());
                                }
                            }
                            Ok(Err(err)) => {
                                let message = err.to_string();
                                eprintln!("[dizmo] failed to load kit: {message}");
                                let _ = engine_tx.send(Err(message.clone()));
                                let slot = status_tx.lock().expect("status tx mutex poisoned");
                                if let Some(tx) = slot.as_ref() {
                                    let _ = tx.send(KitStatus::Failed(message));
                                }
                            }
                            Err(payload) => {
                                let message = panic_message(&payload);
                                eprintln!("[dizmo] kit loader thread panicked: {message}");
                                let _ = engine_tx.send(Err(message.clone()));
                                let slot = status_tx.lock().expect("status tx mutex poisoned");
                                if let Some(tx) = slot.as_ref() {
                                    let _ = tx.send(KitStatus::Failed(message));
                                }
                            }
                        }
                    });
                })
            }

            fn initialize(
                &mut self,
                _audio_io_layout: &AudioIOLayout,
                buffer_config: &BufferConfig,
                _context: &mut impl InitContext<Self>,
            ) -> bool {
                self.core.set_sample_rate(buffer_config.sample_rate);
                self.core
                    .set_block_size(buffer_config.max_buffer_size as usize);
                if let Ok(path) = std::env::var("DIZMO_KIT") {
                    if let Err(err) = self.load_kit(path) {
                        eprintln!("DIZMO: failed to load kit: {err}");
                    }
                }
                true
            }

            fn reset(&mut self) {
                if let Some(engine) = self.core.engine.as_mut() {
                    engine.set_sample_rate(self.core.sample_rate);
                    engine.all_notes_off();
                }
            }

            fn process(
                &mut self,
                buffer: &mut Buffer,
                aux: &mut AuxiliaryBuffers,
                context: &mut impl ProcessContext<Self>,
            ) -> ProcessStatus {
                self.process_block(buffer, aux, context)
            }

            fn deactivate(&mut self) {}
        }

        impl ClapPlugin for $plugin {
            const CLAP_ID: &'static str = $clap_id;
            const CLAP_DESCRIPTION: Option<&'static str> =
                Some("A drum sampler that plays DrumGizmo kits");
            const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
            const CLAP_SUPPORT_URL: Option<&'static str> = None;
            const CLAP_FEATURES: &'static [ClapFeature] = $clap_features;
        }

        impl Vst3Plugin for $plugin {
            const VST3_CLASS_ID: [u8; 16] = $vst3_id;
            const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument];
        }
    };
}

impl_dizmo_plugin!(
    DizmoPlugin,
    "DIZMO",
    STEREO_LAYOUT,
    "com.githum.yorodm.dizmo",
    &[ClapFeature::Instrument, ClapFeature::Stereo],
    *b"DIZMOPluginUI_01",
    true
);

impl_dizmo_plugin!(
    DizmoMultiPlugin,
    "DIZMO Multi",
    MULTI_LAYOUT,
    "com.github.yorodm.dizmo-multi",
    &[ClapFeature::Instrument],
    *b"DIZMOPluginUI_02",
    false
);

impl DizmoPlugin {
    fn process_block(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let frames = buffer.samples();

        self.core.check_engine_updates();

        for channel_samples in buffer.iter_samples() {
            for sample in channel_samples {
                *sample = 0.0;
            }
        }

        let ready = self.core.scratch_ready(frames);
        let Some(engine) = self.core.engine.as_mut() else {
            while context.next_event().is_some() {}
            return ProcessStatus::Normal;
        };

        if !ready {
            while context.next_event().is_some() {}
            return ProcessStatus::Normal;
        }

        for channel in &mut self.core.scratch {
            channel[..frames].fill(0.0);
        }

        let mut current_frame = 0;
        while let Some(event) = context.next_event() {
            let timing = event.timing() as usize;
            if timing > current_frame {
                let chunk = (timing - current_frame).min(frames - current_frame);
                engine.process(current_frame, chunk, &mut self.core.scratch);
                current_frame += chunk;
            }

            match event {
                NoteEvent::NoteOn { note, velocity, .. } => {
                    engine.note_on(note, (velocity * 127.0).round() as u8);
                }
                NoteEvent::NoteOff { note, velocity, .. } => {
                    engine.note_off(note, (velocity * 127.0).round() as u8);
                }
                NoteEvent::MidiCC { cc: 123, .. } => {
                    engine.all_notes_off();
                }
                _ => {}
            }
        }

        if current_frame < frames {
            engine.process(
                current_frame,
                frames - current_frame,
                &mut self.core.scratch,
            );
        }

        let kit_channels = engine.kit_channels();
        let slices = buffer.as_slice();
        let (left_rest, right_rest) = slices.split_at_mut(1);
        let left = &mut *left_rest[0];
        let right = &mut *right_rest[0];
        mixdown_to_stereo(
            &self.core.scratch,
            kit_channels,
            frames,
            left,
            right,
            &self.params.channels,
        );
        self.core
            .update_levels(frames, kit_channels, &self.params.channels);

        ProcessStatus::Normal
    }
}

impl DizmoMultiPlugin {
    fn process_block(
        &mut self,
        _buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let frames = aux.outputs.first().map_or(0, |output| output.samples());

        self.core.check_engine_updates();

        for output in aux.outputs.iter_mut() {
            for channel_samples in output.iter_samples() {
                for sample in channel_samples {
                    *sample = 0.0;
                }
            }
        }

        let ready = self.core.scratch_ready(frames);
        let Some(engine) = self.core.engine.as_mut() else {
            while context.next_event().is_some() {}
            return ProcessStatus::Normal;
        };

        if !ready {
            while context.next_event().is_some() {}
            return ProcessStatus::Normal;
        }

        for channel in &mut self.core.scratch {
            channel[..frames].fill(0.0);
        }

        let mut current_frame = 0;
        while let Some(event) = context.next_event() {
            let timing = event.timing() as usize;
            if timing > current_frame {
                let chunk = (timing - current_frame).min(frames - current_frame);
                engine.process(current_frame, chunk, &mut self.core.scratch);
                current_frame += chunk;
            }

            match event {
                NoteEvent::NoteOn { note, velocity, .. } => {
                    engine.note_on(note, (velocity * 127.0).round() as u8);
                }
                NoteEvent::NoteOff { note, velocity, .. } => {
                    engine.note_off(note, (velocity * 127.0).round() as u8);
                }
                NoteEvent::MidiCC { cc: 123, .. } => {
                    engine.all_notes_off();
                }
                _ => {}
            }
        }

        if current_frame < frames {
            engine.process(
                current_frame,
                frames - current_frame,
                &mut self.core.scratch,
            );
        }

        // Determine if any channel is soloed
        let kit_channels = engine.kit_channels();
        let any_soloed = self
            .params
            .channels
            .iter()
            .take(kit_channels)
            .any(|p| p.solo.value());

        for (index, output) in aux.outputs.iter_mut().enumerate() {
            if index >= kit_channels {
                continue;
            }
            let source = &self.core.scratch[index][..frames];
            let destination = &mut output.as_slice()[0][..frames];
            let param = &self.params.channels[index];

            // Solo logic: if any channel is soloed, only soloed channels play
            if any_soloed && !param.solo.value() {
                destination.fill(0.0);
                continue;
            }

            // Mute logic: skip if muted (solo overrides mute)
            if param.mute.value() && !param.solo.value() {
                destination.fill(0.0);
                continue;
            }

            // Fader is stored as linear gain (not dB), no pan in multi mode;
            // step the smoother once per sample so automation ramps instead of
            // zipper-stepping.
            for (sample_idx, sample) in source.iter().enumerate() {
                let gain = if param.fader.smoothed.is_smoothing() {
                    param.fader.smoothed.next()
                } else {
                    param.fader.value()
                };
                destination[sample_idx] = sample * gain;
            }
        }

        self.core
            .update_levels(frames, kit_channels, &self.params.channels);

        ProcessStatus::Normal
    }
}

nice_export_clap!(DizmoPlugin, DizmoMultiPlugin);
nice_export_vst3!(DizmoPlugin, DizmoMultiPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const DRUMKIT: &str = r#"<drumkit version="2.0">
  <metadata>
    <title>Async Load Kit</title>
    <defaultmidimap src="midimap.xml"/>
  </metadata>
  <channels>
    <channel name="Kick"/>
  </channels>
  <instruments>
    <instrument name="Kick" file="inst_kick.xml">
      <channelmap in="Kick" out="Kick" main="true"/>
    </instrument>
  </instruments>
</drumkit>
"#;

    const INST: &str = r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="Kick" file="kick.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;

    const MIDIMAP: &str = r#"<midimap>
  <map note="36" instr="Kick"/>
</midimap>
"#;

    /// Writes a minimal loadable kit into `dir` and returns the directory.
    fn write_test_kit(dir: &std::path::Path) -> &std::path::Path {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("drumkit.xml"), DRUMKIT).unwrap();
        std::fs::write(dir.join("inst_kick.xml"), INST).unwrap();
        std::fs::write(dir.join("midimap.xml"), MIDIMAP).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(dir.join("kick.wav"), spec).unwrap();
        writer.write_sample(0i16).unwrap();
        writer.finalize().unwrap();
        dir
    }

    #[test]
    fn loader_thread_loads_kit_asynchronously() {
        let dir_path = std::env::temp_dir().join("dizmo-async-load");
        let dir = write_test_kit(&dir_path);

        let mut plugin = DizmoPlugin::default();
        plugin.core.host_sample_rate.store(48000, Ordering::Relaxed);
        let executor = plugin.task_executor();
        executor(LoadTask::LoadKit {
            path: dir.join("drumkit.xml"),
        });

        let engine = plugin
            .core
            .engine_rx
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_secs(30))
            .expect("loader thread should finish")
            .expect("kit should load");
        assert_eq!(engine.kit_name(), "Async Load Kit");
        assert_eq!(engine.kit_channels(), 1);

        // The loader thread also records the kit for future editor boots.
        let guard = plugin.core.kit_info.lock().unwrap();
        let info = guard.as_ref().expect("kit recorded by loader thread");
        assert_eq!(info.name, "Async Load Kit");
    }

    /// A reopened editor must be seeded with the current kit. The window is
    /// booted fresh every time it opens, so the kit lives in shared
    /// `kit_info` (read at boot) rather than a one-shot channel message, which
    /// the previous window would already have consumed.
    #[test]
    fn reopened_editor_receives_current_kit_state() {
        let dir_path = std::env::temp_dir().join("dizmo-editor-reopen");
        let dir = write_test_kit(&dir_path);
        let engine = engine::load_engine(dir.join("drumkit.xml"), None).expect("kit loads");

        let core = AudioCore::new();

        // Loaded before any editor window exists: the kit is remembered in
        // shared state for the next window to read at boot.
        core.notify_loaded(&engine);
        let guard = core.kit_info.lock().unwrap();
        let seeded = guard.as_ref().expect("kit remembered");
        assert_eq!(seeded.name, "Async Load Kit");
        assert_eq!(seeded.channels, vec!["Kick".to_string()]);
        drop(guard);

        // A live window also receives the status message on its channel.
        let (tx1, rx1) = crossbeam_channel::unbounded();
        *core.status_tx.lock().unwrap() = Some(tx1);
        core.notify_loaded(&engine);
        match rx1.try_recv() {
            Ok(KitStatus::Loaded { name, channels, .. }) => {
                assert_eq!(name, "Async Load Kit");
                assert_eq!(channels, vec!["Kick".to_string()]);
            }
            other => panic!("expected a Loaded status, got {other:?}"),
        }

        // The window closes and reopens: boot reads `kit_info` again and still
        // sees the kit, and the stale window's channel receives nothing more.
        let guard = core.kit_info.lock().unwrap();
        let seed_again = guard.as_ref().expect("kit remembered");
        assert_eq!(seed_again.name, "Async Load Kit");
        drop(guard);
        assert!(rx1.try_recv().is_err());
    }

    /// Sets a channel param's plain value through the same host path the
    /// wrapper uses (`set_parameter_normalized` on the param pointer).
    fn set_plain(params: &DizmoParams, id: &str, plain: f32) {
        use nice_plug::params::Params;
        let (_, ptr, _) = params
            .param_map()
            .into_iter()
            .find(|(pid, _, _)| pid == id)
            .expect("param should exist");
        unsafe {
            ptr._internal_set_normalized_value(ptr.preview_normalized(plain));
        }
    }

    #[test]
    fn meter_peaks_are_post_fader() {
        let params = DizmoParams::default();
        let channels = &params.channels;

        // A muted channel contributes nothing post-fader.
        set_plain(&params, "mute_1", 1.0);
        assert_eq!(post_fader_gain(&channels[0], false), 0.0);

        // Solo-ing another channel silences the rest, mute or not.
        set_plain(&params, "solo_2", 1.0);
        assert_eq!(post_fader_gain(&channels[0], true), 0.0);
        assert_eq!(post_fader_gain(&channels[1], true), 1.0);

        // update_levels reports the fader-scaled level: with the fader at
        // -6 dB, a 0.5 raw peak reads ~0.25. A 1-second block forces the
        // 250 ms release to decay fully so the new level is reached.
        let params = DizmoParams::default();
        set_plain(&params, "fader_1", util::db_to_gain(-6.0));
        let mut core = AudioCore::new();
        core.set_block_size(44100);
        core.sample_rate = 44100.0;
        for buffer in &mut core.scratch {
            buffer.fill(0.5);
        }
        core.update_levels(44100, 2, &params.channels);
        let level = f32::from_bits(core.levels[0].load(Ordering::Relaxed));
        assert!(
            (level - 0.25).abs() < 1e-3,
            "meter should follow the fader: {level}"
        );
    }

    /// Lowering the fader (or muting) must drop the meter on that same block,
    /// not over the 250 ms raw-peak release, while an unchanged gain keeps the
    /// smooth non-flickering peak. Regression for the meter masking the fader.
    #[test]
    fn meter_falls_immediately_when_fader_lowered() {
        let params = DizmoParams::default();
        let min_gain = util::db_to_gain(-18.0);

        let mut core = AudioCore::new();
        // Short (10 ms) blocks so the old code's slow release was still
        // holding the previous level instead of showing the new one.
        core.set_block_size(441);
        core.sample_rate = 44100.0;
        for buffer in &mut core.scratch {
            buffer.fill(0.5);
        }

        // Unity fader: a 0.5 peak reads 0.5.
        set_plain(&params, "fader_1", 1.0);
        core.update_levels(441, 2, &params.channels);
        let level = f32::from_bits(core.levels[0].load(Ordering::Relaxed));
        assert!(
            (level - 0.5).abs() < 1e-3,
            "unity fader should read 0.5: {level}"
        );

        // Dragging the fader to its -18 dB floor reads that level immediately;
        // before, the meter kept showing the old 0.5 decaying over 250 ms.
        set_plain(&params, "fader_1", min_gain);
        core.update_levels(441, 2, &params.channels);
        let level = f32::from_bits(core.levels[0].load(Ordering::Relaxed));
        let expected = 0.5 * min_gain;
        assert!(
            (level - expected).abs() < 1e-3,
            "fader at -18 dB should read {expected}, got {level}"
        );

        // Muting reads exactly zero at once, even though the input is loud.
        set_plain(&params, "mute_1", 1.0);
        core.update_levels(441, 2, &params.channels);
        let level = f32::from_bits(core.levels[0].load(Ordering::Relaxed));
        assert!(
            level < 1e-5,
            "muted channel should read 0 at once, got {level}"
        );

        // Unmuting with a live peak snaps the meter back up right away.
        set_plain(&params, "mute_1", 0.0);
        core.update_levels(441, 2, &params.channels);
        let level = f32::from_bits(core.levels[0].load(Ordering::Relaxed));
        assert!(
            (level - expected).abs() < 1e-3,
            "unmuted meter should snap back to {expected}: {level}"
        );
    }
}
