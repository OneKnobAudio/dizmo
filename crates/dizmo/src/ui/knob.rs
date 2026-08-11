//! The pan knob, built on the iced_audio `Knob`.

use crate::ui::gesture_drag::GestureDrag;
use crate::ui::{Message, TEXT_DIM};
use iced::widget::{Column, column, container, row, text};
use iced::{Alignment, Length};
use iced_audio::{Knob, Normal, NormalParam};
use nice_plug_iced::iced;

/// Radius of the pan knob.
pub const KNOB_RADIUS: f32 = 16.0;

/// Builds the pan knob with the L/R labels and a percentage readout
/// (stereo plugin only).
pub fn show_knob<'a>(state: &'a crate::ui::DizmoGui, channel: usize) -> Column<'a, Message> {
    let s = state.scale();
    let pan = &state.editor_state.params.channels[channel].pan;
    let normal_param = NormalParam::from_nice(pan);
    let knob = Knob::new(normal_param)
        .size(Length::Fixed(KNOB_RADIUS * 2.0 * s))
        .bipolar_center(Normal::new(0.5));

    let knob = GestureDrag::new(knob, move |gesture| Message::PanGesture(channel, gesture))
        .value(normal_param.normal.as_f32())
        .default(normal_param.default.as_f32())
        // Scale the pixel->value mapping by the UI zoom so a drag spans the
        // same relative range at every zoom level.
        .drag_scalar(0.0025 / s)
        .wheel_scalar(0.01 / s);

    let readout = crate::params::v2s_f32_pan()(pan.value());

    column![
        row![
            text("L").size(9.0 * s).color(TEXT_DIM),
            container(knob)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
            text("R").size(9.0 * s).color(TEXT_DIM),
        ]
        .align_y(Alignment::Center)
        .spacing(8.0 * s)
        .width(Length::Fill),
        text(readout)
            .size(8.0 * s)
            .color(TEXT_DIM)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .spacing(2.0 * s)
    .width(Length::Fill)
}
