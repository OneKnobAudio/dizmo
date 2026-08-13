//! Sample audition widget: play/stop toggle plus playback volume.

use iced::widget::{button, row, slider, text};
use iced::{Element, Length};

use super::Message;
use super::theme::{TEXT, TEXT_DIM, pill};

/// Preview controls for the selected sample.
///
/// `playing` toggles the button between ▶ Preview and ■ Stop; `volume` is a
/// linear gain (`0.0..=1.5`) shown as a percentage (1.0 = 100%).
pub fn preview_controls<'a>(
    playing: bool,
    volume: f32,
    on_toggle: Message,
    on_volume: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    row![
        button(text(if playing { "■ Stop" } else { "▶ Preview" }))
            .on_press(on_toggle)
            .style(pill(playing)),
        text("Vol").size(11).color(TEXT_DIM),
        slider(0.0..=1.5, volume, on_volume).width(Length::Fill),
        text(format!("{:.0}%", volume * 100.0))
            .size(11)
            .color(TEXT)
            .width(Length::Fixed(40.0)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}
