//! A single channel strip: indicator label, solo/mute, pan knob, fader and
//! peak meter, matching the mockup layout.

use crate::ui::fader::{fader_readout, show_fader};
use crate::ui::knob::show_knob;
use crate::ui::peak_meter::{PeakMeter, peak_readout};
use crate::ui::{
    ACCENT, CARD_BG, CARD_BORDER, DizmoGui, FIELD_BG, FIELD_BORDER, FIELD_HOVER, LoadStatus,
    MUTE_ACTIVE, Message, SOLO_ACTIVE, TEXT,
};
use iced::core::border::Radius;
use iced::widget::{Button, Container, button, canvas, column, container, row, text};
use iced::{Background, Border, Color, Length, Padding};
use nice_plug_iced::iced;
use std::sync::atomic::Ordering;

/// Width of one channel strip card.
pub const STRIP_WIDTH: f32 = 128.0;

/// Height of one channel strip card.
pub const STRIP_HEIGHT: f32 = 460.0;

/// Builds a single channel strip matching the mockup layout.
pub fn draw_strip<'a>(state: &'a DizmoGui, channel: usize) -> Container<'a, Message> {
    let params = &state.editor_state.params;
    // The meter lights from the audio thread's post-fader block peak (the
    // channel's level after its fader, mute, and solo), with the peak-hold cap
    // decaying on the UI ticker.
    let level = f32::from_bits(state.editor_state.levels[channel].load(Ordering::Relaxed));
    let hold = state.peak_hold[channel];

    let mut strip = column![
        container(
            text(channel_indicator_label(state, channel))
                .size(12)
                .color(TEXT)
        )
        .width(Length::Fill)
        .padding(Padding::from(4))
        .align_x(iced::alignment::Horizontal::Center),
        row![
            toggle_button(
                "S",
                params.channels[channel].solo.value(),
                SOLO_ACTIVE,
                Message::ToggleSolo(channel),
            ),
            toggle_button(
                "M",
                params.channels[channel].mute.value(),
                MUTE_ACTIVE,
                Message::ToggleMute(channel),
            ),
        ]
        .spacing(6)
        .width(Length::Fill),
        iced::widget::rule::horizontal(1),
    ]
    .spacing(8);

    // Pan has no effect in the multi plugin, so its knob (and the L/R labels)
    // are hidden there; the fader fills the freed space.
    if state.editor_state.show_pan {
        strip = strip.push(show_knob(state, channel));
    }

    let fader = column![
        text(fader_readout(&params.channels[channel].fader))
            .size(8)
            .color(TEXT),
        text(format!("PK {}", peak_readout(hold)))
            .size(8)
            .color(TEXT),
        show_fader(state, channel),
    ]
    .spacing(4)
    .width(Length::Fill)
    .height(Length::Fill);

    let meter = canvas::Canvas::new(PeakMeter::new(level, hold))
        .width(Length::Fixed(26.0))
        .height(Length::Fill);

    strip = strip.push(
        row![fader, meter]
            .spacing(4)
            .width(Length::Fill)
            .height(Length::Fill),
    );

    container(strip)
        .width(STRIP_WIDTH)
        .height(STRIP_HEIGHT)
        .padding(8)
        .style(strip_style)
}

/// A small rounded toggle button for solo/mute.
fn toggle_button<'a>(
    label: &'a str,
    active: bool,
    active_color: Color,
    message: Message,
) -> Button<'a, Message> {
    let text_color = if active { Color::WHITE } else { TEXT };
    button(text(label).size(10).color(text_color))
        .on_press(message)
        .width(Length::Fill)
        .style(move |_theme, status| {
            let (background, border) = if active {
                (active_color, active_color)
            } else if status == button::Status::Hovered {
                (FIELD_HOVER, ACCENT)
            } else {
                (FIELD_BG, FIELD_BORDER)
            };
            iced::widget::button::Style {
                background: Some(Background::Color(background)),
                text_color,
                border: Border {
                    color: border,
                    width: 1.0,
                    radius: Radius::from(4.0),
                },
                ..Default::default()
            }
        })
}

/// The label shown in the channel indicator: the kit's channel name from its
/// `<channels>` section when a kit is loaded, otherwise the plain channel
/// number.
fn channel_indicator_label(state: &DizmoGui, index: usize) -> String {
    match &state.load_status {
        LoadStatus::Loaded { channels, .. } => channels
            .get(index)
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}", index + 1)),
        _ => format!("{}", index + 1),
    }
}

fn strip_style(theme: &iced::Theme) -> iced::widget::container::Style {
    let _ = theme;
    iced::widget::container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_BORDER,
            width: 1.0,
            radius: Radius::from(8.0),
        },
        ..Default::default()
    }
}
