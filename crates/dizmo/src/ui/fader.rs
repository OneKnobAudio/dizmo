//! The vertical volume fader, built on the iced_audio `VSlider`.

use crate::ui::{ACCENT, KNOB_BORDER, Message, TEXT_DIM};
use iced::widget::{
    canvas,
    canvas::{Frame, Path, Program, Stroke, Text as CanvasText},
    container,
};
use iced::{Color, Length, Point, alignment, widget::Stack};
use iced_audio::{NormalParam, v_slider::VSlider};
use nice_plug::prelude::*;
use nice_plug_iced::iced;
use std::sync::atomic::Ordering;

/// The fader range in dB: minimum (fully down) and maximum (fully up).
pub const FADER_MIN_DB: f32 = -18.0;
pub const FADER_MAX_DB: f32 = 6.0;

/// A peak level (linear 0..1) at or above which the signal LED lights up.
const LED_THRESHOLD: f32 = 0.0001;

/// The top of the track inside the decoration canvas, leaving room for the
/// signal LED.
const TRACK_TOP: f32 = 22.0;

/// Builds the fader block: an iced_audio `VSlider` overlaid on a canvas that
/// draws the dB scale labels, the 0 dB line and the signal LED.
pub fn show_fader<'a>(state: &'a crate::ui::MyGui, channel: usize) -> Stack<'a, Message> {
    let fader = &state.editor_state.params.channels[channel].fader;
    let db = util::gain_to_db(fader.value());
    let level = f32::from_bits(state.editor_state.levels[channel].load(Ordering::Relaxed));

    let slider = VSlider::new(NormalParam::from_nice(fader))
        .width(Length::Fixed(24.0))
        .height(Length::Fill)
        .on_gesture(move |gesture| Message::FaderGesture(channel, gesture));

    let decorations = canvas::Canvas::new(FaderDecoration::new(db, level))
        .width(Length::Fill)
        .height(Length::Fill);

    Stack::with_children([
        decorations.into(),
        container(slider)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .into(),
    ])
    .width(Length::Fill)
    .height(Length::Fill)
}

/// The current fader value as a display string, e.g. `0.0 dB` or `-5.3 dB`.
pub fn fader_readout(fader: &FloatParam) -> String {
    let db = util::gain_to_db(fader.value());
    if db.abs() < 0.05 {
        "0.0 dB".to_string()
    } else {
        format!("{db:+.1} dB")
    }
}

/// Draws the scale labels, the 0 dB line and the signal LED behind the slider.
struct FaderDecoration {
    db: f32,
    level: f32,
}

impl FaderDecoration {
    fn new(db: f32, level: f32) -> Self {
        Self { db, level }
    }

    /// The y position (inside the canvas) for a given dB value.
    fn y_for_db(&self, db: f32, track_bottom: f32) -> f32 {
        let span = FADER_MAX_DB - FADER_MIN_DB;
        let normalized = ((db - FADER_MIN_DB) / span).clamp(0.0, 1.0);
        track_bottom - normalized * (track_bottom - TRACK_TOP)
    }
}

impl<Message> Program<Message> for FaderDecoration {
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
        let center_x = bounds.width / 2.0;
        let track_bottom = bounds.height - 6.0;

        // dB scale labels: +6 at the top, 0 at the center, -18 at the bottom.
        for (db, label) in [(FADER_MAX_DB, "+6"), (0.0, "0"), (FADER_MIN_DB, "-18")] {
            let y = self.y_for_db(db, track_bottom);
            frame.fill_text(CanvasText {
                content: label.to_string(),
                position: Point::new(6.0, y),
                color: TEXT_DIM,
                size: iced::Pixels::from(7.0),
                align_x: iced::core::text::Alignment::Left,
                align_y: alignment::Vertical::Center,
                ..Default::default()
            });
        }

        // The 0 dB line, from the labels over to the slider rail.
        let zero_y = self.y_for_db(0.0, track_bottom);
        frame.stroke(
            &Path::line(Point::new(16.0, zero_y), Point::new(center_x, zero_y)),
            Stroke::default().with_color(ACCENT).with_width(1.0),
        );

        // A short marker on the rail at the current fader position.
        let current_y = self.y_for_db(self.db, track_bottom);
        frame.stroke(
            &Path::line(
                Point::new(center_x - 8.0, current_y),
                Point::new(center_x, current_y),
            ),
            Stroke::default().with_color(ACCENT).with_width(2.0),
        );

        // Signal LED at the top of the track.
        let lit = self.level >= LED_THRESHOLD;
        let led_color = if lit {
            let intensity = (self.level * 3.0).clamp(0.0, 1.0);
            Color::from_rgb8(
                (intensity * 255.0) as u8,
                (intensity * 230.0) as u8,
                (intensity * 120.0) as u8,
            )
        } else {
            Color::from_rgb8(40, 40, 40)
        };
        let led_center = Point::new(center_x, 10.0);
        frame.fill(&Path::circle(led_center, 3.5), led_color);
        frame.stroke(
            &Path::circle(led_center, 3.5),
            Stroke::default().with_color(KNOB_BORDER).with_width(1.0),
        );

        vec![frame.into_geometry()]
    }
}
