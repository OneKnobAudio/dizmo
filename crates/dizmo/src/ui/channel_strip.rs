//! A single channel strip: indicator label, solo/mute, pan knob, fader and
//! trigger indicator, matching the mockup layout.

use crate::ui::fader::{fader_readout, show_fader};
use crate::ui::knob::show_knob;
use crate::ui::{
    ACCENT, CARD_BG, CARD_BORDER, FIELD_BG, FIELD_BORDER, FIELD_HOVER, LoadStatus, MUTE_ACTIVE,
    Message, MyGui, SOLO_ACTIVE, TEXT, TRIGGER_ACTIVE, TRIGGER_DIM,
};
use iced::core::border::Radius;
use iced::widget::{
    Button, Container, button, canvas,
    canvas::{Frame, Path, Program, Stroke},
    column, container, row, text,
};
use iced::{Background, Border, Color, Length, Padding, Point};
use nice_plug_iced::iced;
use std::sync::atomic::Ordering;

/// Width of one channel strip card.
pub const STRIP_WIDTH: f32 = 128.0;

/// Height of one channel strip card.
pub const STRIP_HEIGHT: f32 = 460.0;

/// Builds a single channel strip matching the mockup layout.
pub fn draw_strip<'a>(state: &'a MyGui, channel: usize) -> Container<'a, Message> {
    let params = &state.editor_state.params;
    let trigger = f32::from_bits(state.editor_state.triggers[channel].load(Ordering::Relaxed));
    let lit = trigger > 0.01;
    // The trigger indicator blinks with an 80 ms on / 80 ms off phase while a
    // note has recently triggered the channel.
    let blink_on = lit && state.tick % 4 < 2;

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
        show_fader(state, channel),
    ]
    .spacing(4)
    .width(Length::Fill)
    .height(Length::Fill);

    let trigger_indicator = canvas::Canvas::new(TriggerIndicator::new(blink_on))
        .width(Length::Fixed(26.0))
        .height(Length::Fill);

    strip = strip.push(
        row![fader, trigger_indicator]
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
fn channel_indicator_label(state: &MyGui, index: usize) -> String {
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

/// Draws the trigger indicator circle: a bright amber disc that blinks while
/// lit, and a dark warm gray rest when the channel is silent.
struct TriggerIndicator {
    lit: bool,
}

impl TriggerIndicator {
    fn new(lit: bool) -> Self {
        Self { lit }
    }
}

impl<Message> Program<Message> for TriggerIndicator {
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
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        if self.lit {
            frame.fill(
                &Path::circle(center, 11.0),
                Color::from_rgba8(0xff, 0xd9, 0x6b, 0.43),
            );
            frame.fill(&Path::circle(center, 7.0), TRIGGER_ACTIVE);
        } else {
            frame.fill(&Path::circle(center, 7.0), TRIGGER_DIM);
        }
        frame.stroke(
            &Path::circle(center, 7.0),
            Stroke::default().with_color(CARD_BORDER).with_width(1.0),
        );
        vec![frame.into_geometry()]
    }
}
