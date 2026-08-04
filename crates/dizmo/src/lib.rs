use nice_plug::prelude::*;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;

use crate::engine::Engine;
use crate::params::{AUX_OUTPUT_NAMES, AUX_OUTPUT_PORTS, DizmoParams, NUM_CHANNELS};

pub mod engine;
pub mod kit;
mod params;
mod ui;

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
}

impl AudioCore {
    fn new() -> Self {
        Self {
            engine: None,
            scratch: Vec::new(),
            sample_rate: 44100.0,
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        if let Some(engine) = self.engine.as_mut() {
            engine.set_sample_rate(sample_rate);
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
/// pair. No per-channel gain or pan is applied yet.
pub fn mixdown_to_stereo(
    scratch: &[Vec<f32>],
    channels: usize,
    frames: usize,
    left: &mut [f32],
    right: &mut [f32],
) {
    for sample in 0..frames {
        let mut sum = 0.0;
        for channel in scratch.iter().take(channels) {
            sum += channel[sample];
        }
        left[sample] = sum;
        right[sample] = sum;
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
            type BackgroundTask = ();

            fn params(&self) -> Arc<dyn Params> {
                self.params.clone()
            }

            fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
                Some(Box::new(ui::DizmoEditor::new(
                    self.params.clone(),
                    $show_pan,
                )))
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

        while let Some(event) = context.next_event() {
            if let NoteEvent::NoteOn { note, velocity, .. } = event {
                engine.note_on(note, (velocity * 127.0).round() as u8);
            }
        }

        if !ready {
            return ProcessStatus::Normal;
        }

        engine.process(frames, &mut self.core.scratch);

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

        while let Some(event) = context.next_event() {
            if let NoteEvent::NoteOn { note, velocity, .. } = event {
                engine.note_on(note, (velocity * 127.0).round() as u8);
            }
        }

        if !ready {
            return ProcessStatus::Normal;
        }

        engine.process(frames, &mut self.core.scratch);

        for (index, output) in aux.outputs.iter_mut().enumerate() {
            if index >= engine.kit_channels() {
                continue;
            }
            let source = &self.core.scratch[index][..frames];
            let destination = &mut output.as_slice()[0][..frames];
            destination.copy_from_slice(source);
        }

        ProcessStatus::Normal
    }
}

nice_export_clap!(DizmoPlugin, DizmoMultiPlugin);
nice_export_vst3!(DizmoPlugin, DizmoMultiPlugin);
