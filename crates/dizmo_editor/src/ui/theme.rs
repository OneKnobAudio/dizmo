//! Dark palette and widget styles, mirroring the `dizmo` plugin's editor.

use iced::border::Radius;
use iced::widget::{button, container, overlay, pick_list, text_input};
use iced::{Background, Border, Color, Theme};

pub const BG: Color = Color::from_rgb8(0x14, 0x15, 0x18);
pub const CARD_BG: Color = Color::from_rgb8(0x1d, 0x1f, 0x24);
pub const CARD_BORDER: Color = Color::from_rgb8(0x2d, 0x31, 0x38);
pub const FIELD_BG: Color = Color::from_rgb8(0x23, 0x26, 0x2c);
pub const FIELD_BORDER: Color = Color::from_rgb8(0x3a, 0x3f, 0x48);
pub const FIELD_HOVER: Color = Color::from_rgb8(0x2a, 0x2e, 0x36);
pub const TEXT: Color = Color::from_rgb8(0xf2, 0xf4, 0xf7);
pub const TEXT_DIM: Color = Color::from_rgb8(0x8b, 0x90, 0x99);
pub const ACCENT: Color = Color::from_rgb8(0x55, 0x84, 0xc8);
pub const INDICATOR: Color = Color::from_rgb8(0xe8, 0xec, 0xf1);
pub const SOLO_ACTIVE: Color = Color::from_rgb8(0x9a, 0x8a, 0x3c);
pub const MUTE_ACTIVE: Color = Color::from_rgb8(0xb0, 0x4a, 0x46);

pub fn theme() -> Theme {
    Theme::custom(
        "DIZMO Editor",
        iced::theme::Palette {
            background: BG,
            text: TEXT,
            primary: ACCENT,
            success: INDICATOR,
            warning: SOLO_ACTIVE,
            danger: MUTE_ACTIVE,
        },
    )
}

/// A button style with a `selected` accent border (toolbar / tree pills).
pub fn pill_button_style(selected: bool, status: button::Status) -> button::Style {
    let (background, border) = if selected {
        (FIELD_BG, ACCENT)
    } else if status == button::Status::Hovered {
        (FIELD_HOVER, ACCENT)
    } else {
        (FIELD_BG, FIELD_BORDER)
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: TEXT,
        border: Border {
            color: border,
            width: 1.0,
            radius: Radius::from(4.0),
        },
        ..Default::default()
    }
}

pub fn pill(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| pill_button_style(selected, status)
}

pub fn danger_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border) = match status {
        button::Status::Hovered => (MUTE_ACTIVE, MUTE_ACTIVE),
        _ => (FIELD_BG, MUTE_ACTIVE),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: TEXT,
        border: Border {
            color: border,
            width: 1.0,
            radius: Radius::from(4.0),
        },
        ..Default::default()
    }
}

pub fn text_input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(FIELD_BG),
        border: Border {
            color: match status {
                text_input::Status::Focused { .. } => ACCENT,
                _ => FIELD_BORDER,
            },
            width: 1.0,
            radius: Radius::from(4.0),
        },
        icon: TEXT_DIM,
        placeholder: TEXT_DIM,
        value: TEXT,
        selection: ACCENT,
    }
}

pub fn pick_list_style(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let active = pick_list::Style {
        text_color: TEXT,
        placeholder_color: TEXT_DIM,
        handle_color: TEXT_DIM,
        background: Background::Color(FIELD_BG),
        border: Border {
            color: FIELD_BORDER,
            width: 1.0,
            radius: Radius::from(4.0),
        },
    };
    match status {
        pick_list::Status::Active => active,
        pick_list::Status::Hovered | pick_list::Status::Opened { .. } => pick_list::Style {
            border: Border {
                color: ACCENT,
                ..active.border
            },
            ..active
        },
    }
}

pub fn menu_style(_theme: &Theme) -> overlay::menu::Style {
    overlay::menu::Style {
        background: Background::Color(CARD_BG),
        border: Border {
            color: CARD_BORDER,
            width: 1.0,
            radius: Radius::from(4.0),
        },
        text_color: TEXT,
        selected_text_color: TEXT,
        selected_background: Background::Color(FIELD_BG),
        shadow: iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.4),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
    }
}

pub fn panel_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_BORDER,
            width: 1.0,
            radius: Radius::from(6.0),
        },
        ..Default::default()
    }
}

pub fn sidebar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_BORDER,
            width: 1.0,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    }
}

pub fn backdrop_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba8(0, 0, 0, 0.55))),
        ..Default::default()
    }
}
