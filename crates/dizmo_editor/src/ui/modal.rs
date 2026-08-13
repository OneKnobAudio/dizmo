//! Modal dialogs: the New Kit dialog and the Save-As overwrite confirmation.

use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Copy)]
pub enum DiscardAction {
    NewKit,
    OpenKit,
}

pub enum Modal {
    NewKit(NewKitDraft),
    ConfirmOverwrite(PathBuf),
    DiscardChanges(DiscardAction),
    Error(String),
    ResampleConfirm {
        instrument: usize,
        file: PathBuf,
        source_rate: u32,
        kit_rate: u32,
    },
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

/// Asks for confirmation before saving into a directory that already has
/// contents (files may be overwritten).
pub fn confirm_overwrite_modal<'a>(dir: &'a Path) -> Element<'a, Message> {
    let panel = container(
        column![
            text("Overwrite?").size(16).color(TEXT),
            text("The chosen directory is not empty:")
                .size(11)
                .color(TEXT_DIM),
            text(dir.display().to_string()).size(11).color(TEXT),
            text("Saving may overwrite existing files.")
                .size(11)
                .color(TEXT_DIM),
            row![
                button(text("Cancel"))
                    .on_press(Message::SaveAsCancel)
                    .style(pill(false)),
                button(text("Save anyway"))
                    .on_press(Message::SaveAsConfirmed)
                    .style(pill(true)),
            ]
            .spacing(8),
        ]
        .spacing(10)
        .width(Length::Fixed(360.0)),
    )
    .width(Length::Fixed(400.0))
    .padding(20)
    .style(panel_style);

    dialog_stack(panel, Message::SaveAsCancel)
}

/// Asks for confirmation before replacing the current (dirty) kit.
pub fn discard_changes_modal<'a>(action: DiscardAction) -> Element<'a, Message> {
    let what = match action {
        DiscardAction::NewKit => "create a new kit",
        DiscardAction::OpenKit => "open another kit",
    };
    let panel = container(
        column![
            text("Unsaved changes").size(16).color(TEXT),
            text(format!(
                "The current kit has unsaved changes. Discard them and {what}?"
            ))
            .size(11)
            .color(TEXT_DIM),
            row![
                button(text("Cancel"))
                    .on_press(Message::DiscardCancelled)
                    .style(pill(false)),
                button(text("Discard"))
                    .on_press(Message::DiscardConfirmed)
                    .style(danger_button_style),
            ]
            .spacing(8),
        ]
        .spacing(10)
        .width(Length::Fixed(340.0)),
    )
    .width(Length::Fixed(380.0))
    .padding(20)
    .style(panel_style);

    dialog_stack(panel, Message::DiscardCancelled)
}

/// Shows a load/save error with a single OK button.
pub fn error_modal<'a>(message: &'a str) -> Element<'a, Message> {
    let panel = container(
        column![
            text("Error").size(16).color(TEXT),
            text(message).size(11).color(TEXT),
            button(text("OK"))
                .on_press(Message::DismissError)
                .style(pill(true)),
        ]
        .spacing(10)
        .width(Length::Fixed(340.0)),
    )
    .width(Length::Fixed(380.0))
    .padding(20)
    .style(panel_style);

    dialog_stack(panel, Message::DismissError)
}

/// Asks whether an imported sample whose rate differs from the kit's should
/// be resampled on save.
pub fn resample_confirm_modal<'a>(
    _instrument: usize,
    file: &'a Path,
    source_rate: u32,
    kit_rate: u32,
) -> Element<'a, Message> {
    let panel = container(
        column![
            text("Sample rate mismatch").size(16).color(TEXT),
            text(
                file.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )
            .size(11)
            .color(TEXT),
            text(format!(
                "The sample is {source_rate} Hz but the kit is {kit_rate} Hz."
            ))
            .size(11)
            .color(TEXT_DIM),
            text("Resample it to match the kit when saving?")
                .size(11)
                .color(TEXT_DIM),
            row![
                button(text("Cancel"))
                    .on_press(Message::ResampleDeclined)
                    .style(pill(false)),
                button(text("Resample on save"))
                    .on_press(Message::ResampleConfirmed)
                    .style(pill(true)),
            ]
            .spacing(8),
        ]
        .spacing(10)
        .width(Length::Fixed(360.0)),
    )
    .width(Length::Fixed(400.0))
    .padding(20)
    .style(panel_style);

    dialog_stack(panel, Message::ResampleDeclined)
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
