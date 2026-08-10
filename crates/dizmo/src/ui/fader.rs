//! The vertical volume fader, built on the iced_audio `VSlider` with a
//! realistic hardware-style track, dB tick marks along the left side, and the
//! signal LED shown as a small dot above the track.

use crate::params::fader_range;
use crate::ui::gesture_drag::GestureDrag;
use crate::ui::{ACCENT, Message};
use iced::widget::{Column, column, container, space::Space};
use iced::{Background, Border, Color, Element, Length, core::border::Radius};
use iced_audio::style::tick_marks::{Appearance as TickAppearance, Placement, Shape};
use iced_audio::style::v_slider::{
    Appearance, ClassicAppearance, ClassicHandle, ClassicRail, StyleSheet, TickMarksAppearance,
};
use iced_audio::virtual_slider::Config;
use iced_audio::{
    Normal, NormalParam, Offset,
    tick_marks::{self, Tier},
    v_slider::VSlider,
};
use nice_plug::prelude::*;
use nice_plug_iced::iced;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;

/// A peak level (linear 0..1) at or above which the signal LED lights up.
const LED_THRESHOLD: f32 = 0.0001;

/// Rail and handle colors for the realistic fader look.
const RAIL_HILIGHT: Color = Color::from_rgb8(0x55, 0x5b, 0x65);
const RAIL_SHADOW: Color = Color::from_rgb8(0x21, 0x24, 0x2a);
const HANDLE_BG: Color = Color::from_rgb8(0x2d, 0x32, 0x39);
const HANDLE_BG_HOVER: Color = Color::from_rgb8(0x36, 0x3c, 0x44);
const HANDLE_BG_DRAG: Color = Color::from_rgb8(0x40, 0x48, 0x54);
const HANDLE_BORDER: Color = Color::from_rgb8(0x4a, 0x50, 0x5a);
const HANDLE_NOTCH: Color = Color::from_rgb8(0x8b, 0x90, 0x99);
const TICK_MAJOR: Color = Color::from_rgb8(0x5a, 0x60, 0x6a);
const TICK_MINOR: Color = Color::from_rgb8(0x45, 0x4a, 0x52);

/// dB positions of the major and minor fader tick marks.
const TICKS_MAJOR_DB: [f32; 5] = [-18.0, -12.0, -6.0, 0.0, 6.0];
const TICKS_MINOR_DB: [f32; 4] = [-15.0, -9.0, -3.0, 3.0];

/// Builds the fader block: a `VSlider` styled like a hardware mixer fader
/// with a signal LED above the track.
pub fn show_fader<'a>(state: &'a crate::ui::DizmoGui, channel: usize) -> Column<'a, Message> {
    let s = state.scale();
    let fader = &state.editor_state.params.channels[channel].fader;
    let level = f32::from_bits(state.editor_state.levels[channel].load(Ordering::Relaxed));
    let config = Config {
        drag_scalar: 0.0025,
        ..Default::default()
    };
    let normal_param = NormalParam::from_nice(fader);
    let slider = VSlider::new(normal_param)
        .config(&config)
        .width(Length::Fill)
        .height(Length::Fill)
        .tick_marks(tick_group())
        .style(FaderStyle(s));

    let slider = GestureDrag::new(slider, move |gesture| {
        Message::FaderGesture(channel, gesture)
    })
    .value(normal_param.normal.as_f32())
    .default(normal_param.default.as_f32())
    .drag_scalar(config.drag_scalar);

    column![
        signal_led(level, s),
        container(slider)
            .width(Length::Fixed(24.0 * s))
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .spacing(4.0 * s)
    .width(Length::Fill)
    .height(Length::Fill)
}

/// The dB tick marks at their skewed normalized positions on the fader range.
fn tick_group() -> &'static tick_marks::Group {
    static TICKS: OnceLock<tick_marks::Group> = OnceLock::new();
    TICKS.get_or_init(|| {
        let range = fader_range();
        let at = |db: f32| Normal::new(range.normalize(util::db_to_gain(db)));
        let major = TICKS_MAJOR_DB.map(at);
        let minor = TICKS_MINOR_DB.map(at);
        let mut marks: Vec<(Normal, Tier)> = Vec::with_capacity(9);
        marks.extend(major.map(|n| (n, Tier::One)));
        marks.extend(minor.map(|n| (n, Tier::Two)));
        tick_marks::Group::from_normalized(&marks)
    })
}

/// A realistic hardware-style fader: a recessed track, a rounded cap handle,
/// and dB tick marks along the left side. Carries the UI zoom so the rail,
/// handle, and tick marks scale with the window.
struct FaderStyle(f32);

impl StyleSheet for FaderStyle {
    type Style = iced::Theme;

    fn idle(&self, _theme: &iced::Theme) -> Appearance {
        classic(self.0, HANDLE_BG, HANDLE_BORDER)
    }

    fn hovered(&self, _theme: &iced::Theme) -> Appearance {
        classic(self.0, HANDLE_BG_HOVER, HANDLE_BORDER)
    }

    fn gesturing(&self, _theme: &iced::Theme) -> Appearance {
        classic(self.0, HANDLE_BG_DRAG, ACCENT)
    }

    fn tick_marks_appearance(&self, _theme: &iced::Theme) -> Option<TickMarksAppearance> {
        let s = self.0;
        Some(TickMarksAppearance {
            style: TickAppearance {
                tier_1: Shape::Line {
                    length: 18.0 * s,
                    width: 1.0,
                    color: TICK_MAJOR,
                },
                tier_2: Shape::Line {
                    length: 11.0 * s,
                    width: 1.0,
                    color: TICK_MINOR,
                },
                tier_3: Shape::Line {
                    length: 0.0,
                    width: 0.0,
                    color: TICK_MINOR,
                },
            },
            placement: Placement::LeftOrTop {
                offset: Offset::default(),
                inside: true,
            },
        })
    }
}

fn classic(s: f32, handle_color: Color, border_color: Color) -> Appearance {
    Appearance::Classic(ClassicAppearance {
        rail: ClassicRail {
            rail_colors: (RAIL_HILIGHT, RAIL_SHADOW),
            rail_widths: (1.0, 1.0),
            rail_padding: 18.0 * s,
        },
        handle: ClassicHandle {
            color: handle_color,
            height: (22.0 * s).round() as u16,
            notch_width: 6.0 * s,
            notch_color: HANDLE_NOTCH,
            border_radius: 4.0 * s,
            border_width: 1.0,
            border_color,
        },
    })
}

/// A small round signal LED that lights green when the channel is receiving
/// audio and dims back to grey once the level decays.
fn signal_led(level: f32, s: f32) -> Element<'static, Message> {
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

    container(Space::new().width(8.0 * s).height(8.0 * s))
        .width(8.0 * s)
        .height(8.0 * s)
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: Radius::from(4.0 * s),
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
