//! The vertical volume fader, built on the iced_audio `VSlider` with the
//! signal LED shown as a small dot above the track.

use crate::ui::Message;
use iced::widget::{Column, column, container, space::Space};
use iced::{Background, Border, Color, Element, Length, core::border::Radius};
use iced_audio::{NormalParam, v_slider::VSlider};
use nice_plug::prelude::*;
use nice_plug_iced::iced;
use std::sync::atomic::Ordering;

/// A peak level (linear 0..1) at or above which the signal LED lights up.
const LED_THRESHOLD: f32 = 0.0001;

/// Builds the fader block: a plain `VSlider` with a signal LED above the track.
pub fn show_fader<'a>(state: &'a crate::ui::DizmoGui, channel: usize) -> Column<'a, Message> {
    let fader = &state.editor_state.params.channels[channel].fader;
    let level = f32::from_bits(state.editor_state.levels[channel].load(Ordering::Relaxed));

    // iced_audio 0.15.0's `VSlider` lays out its height from `self.width`
    // (`limits.resolve(self.width, self.width, ..)`), so a fixed width also
    // collapses the height. Use `Length::Fill` for both and let the container
    // clamp the width instead.
    let slider = VSlider::new(NormalParam::from_nice(fader))
        .width(Length::Fill)
        .height(Length::Fill)
        .on_gesture(move |gesture| Message::FaderGesture(channel, gesture));

    column![
        signal_led(level),
        container(slider)
            .width(Length::Fixed(24.0))
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .spacing(4)
    .width(Length::Fill)
    .height(Length::Fill)
}

/// A small round signal LED that lights green when the channel is receiving
/// audio and dims back to grey once the level decays.
fn signal_led(level: f32) -> Element<'static, Message> {
    let color = if level >= LED_THRESHOLD {
        let intensity = (level * 3.0).clamp(0.0, 1.0);
        Color::from_rgb8(
            (intensity * 255.0) as u8,
            (intensity * 230.0) as u8,
            (intensity * 120.0) as u8,
        )
    } else {
        Color::from_rgb8(40, 40, 40)
    };

    container(Space::new().width(8.0).height(8.0))
        .width(8.0)
        .height(8.0)
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: Radius::from(4.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
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
