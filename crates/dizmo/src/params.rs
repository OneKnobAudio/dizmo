use nice_plug::prelude::*;
use std::sync::{Arc, Mutex};
use vizia_plug::ViziaState;

/// The number of possible channel strips / outputs.
pub const NUM_CHANNELS: usize = 16;

/// All parameters and persistent state for the DIZMO plugin.
#[derive(Params)]
pub struct DizmoParams {
    /// The editor state, saved together with the parameter state so the window size can be
    /// restored.
    #[persist = "editor-state"]
    pub editor_state: Arc<ViziaState>,

    /// Number of visible channel strips.
    #[id = "num-strips"]
    pub num_strips: IntParam,

    /// One set of parameters per channel strip.
    #[nested(array)]
    pub channels: [ChannelParams; NUM_CHANNELS],

    /// User editable channel names. Persistent, not automatable.
    #[persist = "channel-names"]
    pub channel_names: Arc<Mutex<[String; NUM_CHANNELS]>>,

    /// Choke assignments. `chokers[victim][choker]` is `true` when `choker` cuts `victim`'s
    /// voices on trigger. Persistent, not automatable.
    #[persist = "chokers"]
    pub chokers: Arc<Mutex<ChokeMatrix>>,
}

/// A `true` value at `[victim][choker]` means that `choker` chokes `victim`.
pub type ChokeMatrix = [[bool; NUM_CHANNELS]; NUM_CHANNELS];

#[derive(Params)]
pub struct ChannelParams {
    /// Volume fader: 0 dB center, -12 dB .. +12 dB.
    #[id = "fader"]
    pub fader: FloatParam,

    /// Pan position in the MAIN mix, -1 (left) .. 1 (right).
    #[id = "pan"]
    pub pan: FloatParam,

    #[id = "mute"]
    pub mute: BoolParam,

    #[id = "solo"]
    pub solo: BoolParam,

    /// The MIDI note assigned to this channel.
    #[id = "note"]
    pub note: IntParam,
}

fn default_channel_names() -> [String; NUM_CHANNELS] {
    std::array::from_fn(|idx| format!("Channel {}", idx + 1))
}

fn default_chokers() -> ChokeMatrix {
    // Self-choke is enabled by default: a retrigger cuts the previous voice.
    let mut chokers = [[false; NUM_CHANNELS]; NUM_CHANNELS];
    for (victim, row) in chokers.iter_mut().enumerate() {
        row[victim] = true;
    }
    chokers
}

fn fader_param(name: &str) -> FloatParam {
    FloatParam::new(
        name,
        util::db_to_gain(0.0),
        FloatRange::SymmetricalSkewed {
            min: util::db_to_gain(-12.0),
            max: util::db_to_gain(12.0),
            factor: FloatRange::gain_skew_factor(-12.0, 12.0),
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

fn note_param(name: &str) -> IntParam {
    IntParam::new(name, 35, IntRange::Linear { min: 0, max: 127 })
        .with_value_to_string(formatters::v2s_i32_note_formatter())
        .with_string_to_value(formatters::s2v_i32_note_formatter())
}

impl ChannelParams {
    fn new(channel_number: usize) -> Self {
        let number = channel_number + 1;
        Self {
            fader: fader_param(&format!("Fader {number}")),
            pan: pan_param(&format!("Pan {number}")),
            mute: BoolParam::new(format!("Mute {number}"), false),
            solo: BoolParam::new(format!("Solo {number}"), false),
            note: note_param(&format!("Note {number}")),
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
            num_strips: IntParam::new(
                "Channels",
                NUM_CHANNELS as i32,
                IntRange::Linear {
                    min: 1,
                    max: NUM_CHANNELS as i32,
                },
            )
            .with_value_to_string(Arc::new(|value| format!("{value}"))),
            channels: std::array::from_fn(ChannelParams::new),
            channel_names: Arc::new(Mutex::new(default_channel_names())),
            chokers: Arc::new(Mutex::new(default_chokers())),
        }
    }
}
