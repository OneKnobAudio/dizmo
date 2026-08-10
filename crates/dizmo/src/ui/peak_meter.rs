//! The per-channel peak meter: a vertical LED ladder with a peak-hold cap,
//! shown beside the fader in each channel strip.

use crate::ui::{METER_GREEN, METER_OFF, METER_RED, METER_YELLOW, PEAK_HOLD_COLOR};
use iced::widget::canvas::{self, Frame, Program};
use iced::{Color, Point, Size};
use nice_plug::prelude::*;
use nice_plug_iced::iced;

/// The number of segments in the LED ladder.
const NUM_SEGMENTS: usize = 16;
/// The level (dB) mapped to the bottom of the ladder; anything below this
/// reads as off.
const FLOOR_DB: f32 = -60.0;
/// The zone boundaries: green below `YELLOW_DB`, yellow up to `RED_DB`, red
/// above (and over 0 dB the whole ladder is lit).
const YELLOW_DB: f32 = -12.0;
const RED_DB: f32 = -6.0;

/// Draws the vertical LED ladder for one channel: lit segments rising with the
/// level, colored by zone, with a bright cap that lingers at the peak.
pub struct PeakMeter {
    /// The current linear level (0..1), read from the audio thread.
    level: f32,
    /// The decaying peak-hold cap (linear 0..1), at least `level`.
    hold: f32,
    /// The uniform UI zoom, applied to the canvas padding and segment gap.
    scale: f32,
}

impl PeakMeter {
    pub fn new(level: f32, hold: f32, scale: f32) -> Self {
        Self { level, hold, scale }
    }
}

impl<Message> Program<Message> for PeakMeter {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        let pad_x = 6.0 * self.scale;
        let pad_y = 4.0 * self.scale;
        let gap = 2.0 * self.scale;
        let inner_height = bounds.height - pad_y * 2.0;
        let segment_height =
            (inner_height - gap * (NUM_SEGMENTS as f32 - 1.0)) / NUM_SEGMENTS as f32;
        let width = bounds.width - pad_x * 2.0;

        let filled = segments_for(self.level);
        let hold_at = segments_for(self.hold);

        for index in 0..NUM_SEGMENTS {
            let y = pad_y + inner_height - (index + 1) as f32 * segment_height - index as f32 * gap;
            let color = if index == hold_at {
                PEAK_HOLD_COLOR
            } else if index < filled {
                zone_color(segment_db(index))
            } else {
                METER_OFF
            };
            frame.fill_rectangle(
                Point::new(pad_x, y),
                Size::new(width, segment_height),
                color,
            );
        }

        vec![frame.into_geometry()]
    }
}

/// The number of lit segments for a linear level, scaled so 0 dB lights the
/// whole ladder and `FLOOR_DB` lights none.
fn segments_for(level: f32) -> usize {
    let db = util::gain_to_db(level);
    let fraction = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
    (fraction * NUM_SEGMENTS as f32).floor() as usize
}

/// The current post-fader peak level as a display string, e.g. `-12.3 dB`, or
/// `-inf dB` when nothing is lit on the ladder. Mirrors the fader readout's
/// formatting.
pub fn peak_readout(level: f32) -> String {
    if segments_for(level) == 0 {
        "-inf dB".to_string()
    } else {
        format!("{:+1.1} dB", util::gain_to_db(level))
    }
}

/// The dB position of segment `index` (0 = bottom of the ladder).
fn segment_db(index: usize) -> f32 {
    FLOOR_DB + -FLOOR_DB * (index + 1) as f32 / NUM_SEGMENTS as f32
}

fn zone_color(db: f32) -> Color {
    if db < YELLOW_DB {
        METER_GREEN
    } else if db < RED_DB {
        METER_YELLOW
    } else {
        METER_RED
    }
}
