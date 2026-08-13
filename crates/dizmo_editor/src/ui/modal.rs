//! Modal dialogs. Only the New Kit dialog exists in the skeleton.

use iced::widget::{
    Space, button, column, container, mouse_area, row, scrollable, text, text_input,
};
use iced::{Element, Length};

use super::Message;
use super::theme::{
    TEXT, TEXT_DIM, backdrop_style, danger_button_style, panel_style, pill, text_input_style,
};

#[derive(Debug, Clone)]
pub enum ModalMessage {
    Name(String),
    Samplerate(String),
    RenameChannel(usize, String),
    AddChannel,
    RemoveChannel(usize),
}

pub enum Modal {
    NewKit(NewKitDraft),
}

#[derive(Debug, Clone)]
pub struct NewKitDraft {
    pub name: String,
    pub samplerate: String,
    pub channels: Vec<String>,
}

impl Default for NewKitDraft {
    fn default() -> Self {
        Self::new()
    }
}

impl NewKitDraft {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            samplerate: "48000".into(),
            channels: Vec::new(),
        }
    }
}

pub fn new_kit_modal<'a>(draft: &'a NewKitDraft) -> Element<'a, Message> {
    let mut channels = column![].spacing(4);
    for (i, name) in draft.channels.iter().enumerate() {
        channels = channels.push(
            row![
                text_input("Channel name", name)
                    .on_input(
                        move |value| Message::NewKitModal(ModalMessage::RenameChannel(i, value))
                    )
                    .style(text_input_style)
                    .width(Length::Fill),
                button(text("×"))
                    .on_press(Message::NewKitModal(ModalMessage::RemoveChannel(i)))
                    .style(danger_button_style),
            ]
            .spacing(6),
        );
    }

    let create = if draft.name.trim().is_empty() {
        button(text("Create kit")).style(pill(false))
    } else {
        button(text("Create kit"))
            .on_press(Message::NewKitCreate)
            .style(pill(true))
    };

    let panel = container(
        scrollable(
            column![
                text("New Kit").size(16).color(TEXT),
                text("Workflow: channels first, instruments after.")
                    .size(11)
                    .color(TEXT_DIM),
                text_input("Kit name", &draft.name)
                    .on_input(|value| Message::NewKitModal(ModalMessage::Name(value)))
                    .style(text_input_style),
                text_input("Sample rate (Hz)", &draft.samplerate)
                    .on_input(|value| Message::NewKitModal(ModalMessage::Samplerate(value)))
                    .style(text_input_style),
                text("Output channels  ·  workflow step 1")
                    .size(11)
                    .color(TEXT_DIM),
                channels,
                button(text("+ Add channel"))
                    .on_press(Message::NewKitModal(ModalMessage::AddChannel))
                    .style(pill(false)),
                row![
                    button(text("Cancel"))
                        .on_press(Message::NewKitCancel)
                        .style(pill(false)),
                    create,
                ]
                .spacing(8),
            ]
            .spacing(10),
        )
        .width(Length::Fixed(420.0))
        .height(Length::Fixed(320.0)),
    )
    .width(Length::Fixed(460.0))
    .padding(20)
    .style(panel_style);

    dialog_stack(panel, Message::NewKitCancel)
}

fn dialog_stack<'a>(
    panel: iced::widget::Container<'a, Message>,
    dismiss: Message,
) -> Element<'a, Message> {
    let backdrop = mouse_area(container(
        Space::new().width(Length::Fill).height(Length::Fill),
    ))
    .on_press(dismiss);
    iced::widget::Stack::with_children([
        container(backdrop)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(backdrop_style)
            .into(),
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into(),
    ])
    .into()
}
