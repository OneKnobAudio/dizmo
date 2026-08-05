use nice_plug::prelude::*;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::Engine;
use crate::params::{AUX_OUTPUT_NAMES, AUX_OUTPUT_PORTS, ChannelParams, DizmoParams, NUM_CHANNELS};

pub mod engine;
pub mod kit;
pub mod params;
mod ui;

/// A kit-load request dispatched from the editor GUI.
pub enum LoadTask {
    LoadKit { path: PathBuf },
}

/// Load status messages sent from the loader thread to the editor GUI.
pub(crate) enum KitStatus {
    /// The kit was loaded; carries its display name from drumkit.xml and, per
    /// kit channel, the instrument assigned to it and the MIDI notes from the
    /// midimap that trigger sound on it.
    Loaded {
        name: String,
        notes: Vec<Vec<u8>>,
        instruments: Vec<Option<String>>,
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
    /// Receives load status messages; moved into the editor when it is created.
    status_rx: Option<crossbeam_channel::Receiver<KitStatus>>,
    /// The sample rate the loader thread resamples to; set in `initialize`.
    host_sample_rate: Arc<AtomicU32>,
}

impl AudioCore {
    fn new() -> Self {
        Self {
            engine: None,
            scratch: Vec::new(),
            sample_rate: 44100.0,
            engine_rx: None,
            old_engine_tx: None,
            status_rx: None,
            host_sample_rate: Arc::new(AtomicU32::new(0)),
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
            let pan_pos = param.pan.value();

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
                self.core.engine = Some(engine);
                Ok(())
            }
        }

        impl Plugin for $plugin {
            const NAME: &'static str = $name;
            const VENDOR: &'static str = "DIZMO";
            const URL: &'static str = "https://dizmo.invalid";
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
                let status_rx = self.core.status_rx.take();
                Some(Box::new(ui::DizmoEditor::new(
                    self.params.clone(),
                    $show_pan,
                    load_kit,
                    status_rx,
                )))
            }

            fn task_executor(&mut self) -> TaskExecutor<Self> {
                let (engine_tx, engine_rx) = crossbeam_channel::unbounded();
                let (old_engine_tx, old_engine_rx) = crossbeam_channel::unbounded();
                let (status_tx, status_rx) = crossbeam_channel::unbounded();
                self.core.engine_rx = Some(engine_rx);
                self.core.old_engine_tx = Some(old_engine_tx);
                self.core.status_rx = Some(status_rx);

                // A dedicated thread drops retired engines so deallocation never
                // happens on the audio thread.
                std::thread::spawn(move || {
                    while let Ok(engine) = old_engine_rx.recv() {
                        drop(engine);
                    }
                });

                let host_sample_rate = self.core.host_sample_rate.clone();
                Box::new(move |task: LoadTask| {
                    let LoadTask::LoadKit { path } = task;
                    let engine_tx = engine_tx.clone();
                    let status_tx = status_tx.clone();
                    let host_sample_rate = host_sample_rate.clone();
                    std::thread::spawn(move || {
                        let rate = host_sample_rate.load(Ordering::Relaxed);
                        eprintln!("[dizmo] loading kit '{path:?}' (rate {rate})");
                        let start = std::time::Instant::now();
                        let load = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            engine::load_engine_with_progress(
                                &path,
                                (rate != 0).then_some(rate),
                                &mut |loaded, total| {
                                    let _ = status_tx.send(KitStatus::Progress { loaded, total });
                                },
                            )
                        }));
                        match load {
                            Ok(Ok(mut engine)) => {
                                engine.set_sample_rate(if rate == 0 {
                                    44100.0
                                } else {
                                    rate as f32
                                });
                                let name = engine.kit_name().to_string();
                                eprintln!("[dizmo] kit '{name}' loaded in {:?}", start.elapsed());
                                let notes = engine.notes_per_channel();
                                let instruments = engine.instruments_per_channel();
                                let _ = engine_tx.send(Ok(engine));
                                let _ = status_tx.send(KitStatus::Loaded {
                                    name,
                                    notes,
                                    instruments,
                                });
                            }
                            Ok(Err(err)) => {
                                let message = err.to_string();
                                eprintln!("[dizmo] failed to load kit: {message}");
                                let _ = engine_tx.send(Err(message.clone()));
                                let _ = status_tx.send(KitStatus::Failed(message));
                            }
                            Err(payload) => {
                                let message = panic_message(&payload);
                                eprintln!("[dizmo] kit loader thread panicked: {message}");
                                let _ = engine_tx.send(Err(message.clone()));
                                let _ = status_tx.send(KitStatus::Failed(message));
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
    "com.dizmo.dizmo",
    &[ClapFeature::Instrument, ClapFeature::Stereo],
    *b"DIZMOPluginUI_01",
    true
);

impl_dizmo_plugin!(
    DizmoMultiPlugin,
    "DIZMO Multi",
    MULTI_LAYOUT,
    "com.dizmo.dizmo-multi",
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

        let slices = buffer.as_slice();
        let (left_rest, right_rest) = slices.split_at_mut(1);
        let left = &mut *left_rest[0];
        let right = &mut *right_rest[0];
        mixdown_to_stereo(
            &self.core.scratch,
            engine.kit_channels(),
            frames,
            left,
            right,
            &self.params.channels,
        );

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
        let any_soloed = self
            .params
            .channels
            .iter()
            .take(engine.kit_channels())
            .any(|p| p.solo.value());

        for (index, output) in aux.outputs.iter_mut().enumerate() {
            if index >= engine.kit_channels() {
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

    #[test]
    fn loader_thread_loads_kit_asynchronously() {
        let dir = std::env::temp_dir().join("dizmo-async-load");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
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
    }
}
