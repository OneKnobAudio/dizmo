use nice_plug::prelude::*;
use nice_plug_iced::WindowState;
use std::num::NonZeroU32;
use std::sync::Arc;

/// The number of possible channel strips / outputs.
pub const NUM_CHANNELS: usize = 16;

/// The per-channel audio output ports for the MULTI output layout.
pub const AUX_OUTPUT_PORTS: [NonZeroU32; NUM_CHANNELS] = [new_nonzero_u32(1); NUM_CHANNELS];

/// Names for the per-channel audio output ports.
pub static AUX_OUTPUT_NAMES: [&str; NUM_CHANNELS] = [
    "Ch 1", "Ch 2", "Ch 3", "Ch 4", "Ch 5", "Ch 6", "Ch 7", "Ch 8", "Ch 9", "Ch 10", "Ch 11",
    "Ch 12", "Ch 13", "Ch 14", "Ch 15", "Ch 16",
];

/// All parameters and persistent state for the DIZMO plugin.
#[derive(Params)]
pub struct DizmoParams {
    /// The editor state, saved together with the parameter state so the window size can be
    /// restored.
    #[persist = "window-state"]
    pub editor_state: Arc<WindowState>,

    /// One set of parameters per channel strip.
    #[nested(array)]
    pub channels: [ChannelParams; NUM_CHANNELS],
}

#[derive(Params)]
pub struct ChannelParams {
    /// Volume fader: 0 dB center, -18 dB .. +6 dB.
    #[id = "fader"]
    pub fader: FloatParam,

    /// Pan position in the MAIN mix, -1 (left) .. 1 (right).
    #[id = "pan"]
    pub pan: FloatParam,

    #[id = "mute"]
    pub mute: BoolParam,

    #[id = "solo"]
    pub solo: BoolParam,
}

fn fader_param(name: &str) -> FloatParam {
    FloatParam::new(
        name,
        util::db_to_gain(0.0),
        FloatRange::SymmetricalSkewed {
            min: util::db_to_gain(-18.0),
            max: util::db_to_gain(6.0),
            factor: FloatRange::gain_skew_factor(-18.0, 6.0),
            center: util::db_to_gain(0.0),
        },
    )
    .with_smoother(SmoothingStyle::Logarithmic(50.0))
    .with_unit(" dB")
    .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
    .with_string_to_value(formatters::s2v_f32_gain_to_db())
}

fn pan_param(name: &str) -> FloatParam {
    FloatParam::new(
        name,
        0.0,
        FloatRange::Linear {
            min: -1.0,
            max: 1.0,
        },
    )
    .with_value_to_string(formatters::v2s_f32_panning())
    .with_string_to_value(formatters::s2v_f32_panning())
}

impl ChannelParams {
    fn new(channel_number: usize) -> Self {
        let number = channel_number + 1;
        Self {
            fader: fader_param(&format!("Fader {number}")),
            pan: pan_param(&format!("Pan {number}")),
            mute: BoolParam::new(format!("Mute {number}"), false),
            solo: BoolParam::new(format!("Solo {number}"), false),
        }
    }
}

impl Default for ChannelParams {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Default for DizmoParams {
    fn default() -> Self {
        Self {
            editor_state: crate::ui::default_editor_state(),
            channels: std::array::from_fn(ChannelParams::new),
        }
    }
}
