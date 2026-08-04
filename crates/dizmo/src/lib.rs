use nice_plug::prelude::*;
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::params::{AUX_OUTPUT_NAMES, AUX_OUTPUT_PORTS, DizmoParams};

pub mod engine;
pub mod kit;
mod params;
mod ui;

/// The drum engine is not implemented yet, so output silence on all ports.
fn process_dizmo(buffer: &mut Buffer, aux: &mut AuxiliaryBuffers) -> ProcessStatus {
    for channel_samples in buffer.iter_samples() {
        for sample in channel_samples {
            *sample = 0.0;
        }
    }
    for output in aux.outputs.iter_mut() {
        for channel_samples in output.iter_samples() {
            for sample in channel_samples {
                *sample = 0.0;
            }
        }
    }

    ProcessStatus::Normal
}

/// The stereo plugin: all channels are mixed down to the MAIN bus.
pub struct DizmoPlugin {
    params: Arc<DizmoParams>,
}

/// The multi-output plugin: each of the 16 channels gets its own output bus.
pub struct DizmoMultiPlugin {
    params: Arc<DizmoParams>,
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
                }
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

            fn process(
                &mut self,
                buffer: &mut Buffer,
                aux: &mut AuxiliaryBuffers,
                _context: &mut impl ProcessContext<Self>,
            ) -> ProcessStatus {
                process_dizmo(buffer, aux)
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

nice_export_clap!(DizmoPlugin, DizmoMultiPlugin);
nice_export_vst3!(DizmoPlugin, DizmoMultiPlugin);
