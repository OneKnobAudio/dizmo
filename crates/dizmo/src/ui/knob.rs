//! The pan knob, built on the iced_audio `Knob`.

use crate::ui::{Message, TEXT_DIM};
use iced::widget::{Row, container, row, text};
use iced::{Alignment, Length};
use iced_audio::{Knob, Normal, NormalParam};
use nice_plug_iced::iced;

/// Radius of the pan knob.
pub const KNOB_RADIUS: f32 = 16.0;

/// Builds the pan knob with the L/R labels (stereo plugin only).
pub fn show_knob<'a>(state: &'a crate::ui::MyGui, channel: usize) -> Row<'a, Message> {
    let pan = &state.editor_state.params.channels[channel].pan;
    let knob = Knob::new(NormalParam::from_nice(pan))
        .size(Length::Fixed(KNOB_RADIUS * 2.0))
        .bipolar_center(Normal::new(0.5))
        .on_gesture(move |gesture| Message::PanGesture(channel, gesture));

    row![
        text("L").size(9).color(TEXT_DIM),
        container(knob)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
        text("R").size(9).color(TEXT_DIM),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .width(Length::Fill)
}
