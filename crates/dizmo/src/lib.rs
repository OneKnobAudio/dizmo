use nice_plug::prelude::*;
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::params::{AUX_OUTPUT_NAMES, AUX_OUTPUT_PORTS, DizmoParams};

mod params;
mod state;
mod ui;

pub struct DizmoPlugin {
    params: Arc<DizmoParams>,
}

impl Default for DizmoPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(DizmoParams::default()),
        }
    }
}

impl Plugin for DizmoPlugin {
    const NAME: &'static str = "DIZMO";
    const VENDOR: &'static str = "DIZMO";
    const URL: &'static str = "https://dizmo.invalid";
    const EMAIL: &'static str = "info@dizmo.invalid";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        // STEREO: everything is mixed down to the MAIN bus.
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[],
            aux_output_ports: &[],
            names: PortNames {
                layout: Some("Stereo"),
                main_output: Some("MAIN"),
                ..PortNames::const_default()
            },
        },
        // MULTI: MAIN is disabled and each channel gets its own output bus.
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[],
            aux_output_ports: &AUX_OUTPUT_PORTS,
            names: PortNames {
                layout: Some("Multi"),
                main_output: Some("MAIN"),
                aux_outputs: &AUX_OUTPUT_NAMES,
                ..PortNames::const_default()
            },
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(ui::DizmoEditor::new(self.params.clone())))
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // The drum engine is not implemented yet, so output silence on all ports.
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

    fn deactivate(&mut self) {}
}

impl ClapPlugin for DizmoPlugin {
    const CLAP_ID: &'static str = "com.dizmo.dizmo";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A drum sampler that plays DrumGizmo kits");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Stereo];
}

impl Vst3Plugin for DizmoPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"DIZMOPluginUI_01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument];
}

nice_export_clap!(DizmoPlugin);
nice_export_vst3!(DizmoPlugin);
