//! The iced application shell: state, messages, and the overall layout.

pub mod modal;
pub mod panels;
pub mod sidebar;
pub mod theme;

use std::path::PathBuf;

use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, text};
use iced::{Element, Length, Task};
use rfd::AsyncFileDialog;

use crate::model::EditorKit;

use modal::{Modal, ModalMessage, NewKitDraft};

const SIDEBAR_WIDTH: f32 = 230.0;
const DEFAULT_SAMPLERATE: f64 = 48000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Kit,
    Instrument(usize),
    Sample(usize, usize),
    Midimap,
}

#[derive(Debug, Clone)]
pub enum Message {
    NewKitClicked,
    OpenKitClicked,
    NewKitModal(ModalMessage),
    NewKitCreate,
    NewKitCancel,
    KitOpened(Option<PathBuf>),
    Save,
    SaveAs,
    SaveAsPicked(Option<PathBuf>),
    Select(Selection),
    KitName(String),
    KitDescription(String),
    KitSamplerate(String),
    AddChannel,
    RemoveChannel(usize),
    RenameChannel(usize, String),
    AddInstrument,
    RemoveInstrument(usize),
    RenameInstrument(usize, String),
    AssignChannel(usize, Option<usize>),
    AddSample(usize),
    RemoveSample(usize, usize),
    RenameSample(usize, usize, String),
    SamplePower(usize, usize, f32),
    SampleNormalized(usize, usize, bool),
    Preview(usize, usize),
    MidimapNote(String),
    MidimapAdd,
    MidimapAssign(usize, Option<usize>),
    MidimapRemove(usize),
    DismissStatus,
}

pub struct App {
    kit: Option<EditorKit>,
    selection: Selection,
    modal: Option<Modal>,
    status: Option<String>,
    midimap_note: String,
    samplerate_text: String,
}

impl App {
    pub fn new() -> Self {
        Self {
            kit: None,
            selection: Selection::Kit,
            modal: None,
            status: None,
            midimap_note: String::new(),
            samplerate_text: DEFAULT_SAMPLERATE.to_string(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NewKitClicked => {
                self.modal = Some(Modal::NewKit(NewKitDraft::new()));
            }
            Message::OpenKitClicked => {
                return Task::perform(
                    AsyncFileDialog::new()
                        .add_filter("Kits", &["xml"])
                        .pick_file(),
                    |picked| Message::KitOpened(picked.map(|handle| handle.path().to_path_buf())),
                );
            }
            Message::NewKitModal(edit) => {
                if let Some(Modal::NewKit(draft)) = &mut self.modal {
                    match edit {
                        ModalMessage::Name(name) => draft.name = name,
                        ModalMessage::Samplerate(rate) => draft.samplerate = rate,
                        ModalMessage::RenameChannel(i, name) => {
                            if let Some(channel) = draft.channels.get_mut(i) {
                                *channel = name;
                            }
                        }
                        ModalMessage::AddChannel => {
                            let n = draft.channels.len();
                            draft.channels.push(format!("Channel {}", n + 1));
                        }
                        ModalMessage::RemoveChannel(i) => {
                            if i < draft.channels.len() {
                                draft.channels.remove(i);
                            }
                        }
                    }
                }
            }
            Message::NewKitCreate => {
                let Some(Modal::NewKit(draft)) = &self.modal else {
                    return Task::none();
                };
                if draft.name.trim().is_empty() {
                    return Task::none();
                }
                let samplerate = draft.samplerate.parse().unwrap_or(DEFAULT_SAMPLERATE);
                self.samplerate_text = draft.samplerate.clone();
                self.kit = Some(EditorKit::new_kit(&draft.name, samplerate, &draft.channels));
                self.selection = Selection::Kit;
                self.modal = None;
                self.status = None;
            }
            Message::NewKitCancel => self.modal = None,
            Message::KitOpened(Some(dir)) => {
                let name = dir
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Kit".into());
                let mut kit = EditorKit::new_kit(&name, DEFAULT_SAMPLERATE, &[]);
                kit.root_dir = Some(dir);
                self.samplerate_text = DEFAULT_SAMPLERATE.to_string();
                self.kit = Some(kit);
                self.selection = Selection::Kit;
                self.modal = None;
                self.status = Some("Opened directory — reading kit files lands in Phase 2.".into());
            }
            Message::KitOpened(None) => {}
            Message::Save => {
                if self.kit.as_ref().is_none_or(|kit| kit.root_dir.is_none()) {
                    return self.pick_save_dir();
                }
                self.status = Some("Saving kit files lands in Phase 3.".into());
            }
            Message::SaveAs => return self.pick_save_dir(),
            Message::SaveAsPicked(Some(dir)) => {
                if let Some(kit) = &mut self.kit {
                    kit.root_dir = Some(dir);
                    self.status =
                        Some("Save-as directory set — writing kit files lands in Phase 3.".into());
                }
            }
            Message::SaveAsPicked(None) => {}
            Message::Select(selection) => self.selection = selection,
            Message::KitName(name) => {
                if let Some(kit) = &mut self.kit
                    && kit.drumkit.name != name
                {
                    kit.drumkit.name = name;
                    kit.dirty = true;
                }
            }
            Message::KitDescription(description) => {
                if let Some(kit) = &mut self.kit
                    && kit.drumkit.description != description
                {
                    kit.drumkit.description = description;
                    kit.dirty = true;
                }
            }
            Message::KitSamplerate(rate) => {
                self.samplerate_text = rate;
                if let Some(kit) = &mut self.kit
                    && let Ok(value) = self.samplerate_text.parse::<f64>()
                    && kit.drumkit.samplerate != value
                {
                    kit.drumkit.samplerate = value;
                    kit.dirty = true;
                }
            }
            Message::AddChannel => {
                if let Some(kit) = &mut self.kit {
                    let n = kit.drumkit.channels.len();
                    kit.add_channel(&format!("Channel {}", n + 1));
                }
            }
            Message::RemoveChannel(i) => {
                if let Some(kit) = &mut self.kit {
                    kit.remove_channel(i);
                }
            }
            Message::RenameChannel(i, name) => {
                if let Some(kit) = &mut self.kit {
                    kit.rename_channel(i, &name);
                }
            }
            Message::AddInstrument => {
                if let Some(kit) = &mut self.kit {
                    let n = kit.instruments.len();
                    let index = kit.add_instrument(&format!("Instrument {}", n + 1));
                    self.selection = Selection::Instrument(index);
                }
            }
            Message::RemoveInstrument(i) => {
                if let Some(kit) = &mut self.kit {
                    kit.remove_instrument(i);
                }
                self.selection = Selection::Kit;
            }
            Message::RenameInstrument(i, name) => {
                if let Some(kit) = &mut self.kit {
                    kit.rename_instrument(i, &name);
                }
            }
            Message::AssignChannel(i, channel) => {
                if let Some(kit) = &mut self.kit {
                    kit.assign_channel(i, channel);
                }
            }
            Message::AddSample(i) => {
                if let Some(kit) = &mut self.kit {
                    let n = kit.instruments[i].instrument.samples.len();
                    let index =
                        kit.add_sample(i, &format!("Sample {}", n + 1), "samples/placeholder.wav");
                    self.selection = Selection::Sample(i, index);
                }
            }
            Message::RemoveSample(i, s) => {
                if let Some(kit) = &mut self.kit {
                    kit.remove_sample(i, s);
                }
                self.selection = Selection::Instrument(i);
            }
            Message::RenameSample(i, s, name) => {
                if let Some(kit) = &mut self.kit
                    && let Some(sample) = kit.instruments[i].instrument.samples.get_mut(s)
                    && sample.name != name
                {
                    sample.name = name;
                    kit.dirty = true;
                }
            }
            Message::SamplePower(i, s, value) => {
                if let Some(kit) = &mut self.kit {
                    kit.set_sample_power(i, s, value);
                }
            }
            Message::SampleNormalized(i, s, checked) => {
                if let Some(kit) = &mut self.kit {
                    kit.set_sample_normalized(i, s, checked);
                }
            }
            Message::Preview(instrument, sample) => {
                let name = self
                    .kit
                    .as_ref()
                    .and_then(|kit| kit.instruments.get(instrument))
                    .and_then(|inst| inst.instrument.samples.get(sample))
                    .map(|s| s.name.clone());
                self.status = Some(match name {
                    Some(name) => {
                        format!("Previewing {name} — sample preview lands in Phase 4 (rodio).")
                    }
                    None => "Sample preview lands in Phase 4 (rodio).".into(),
                });
            }
            Message::MidimapNote(note) => self.midimap_note = note,
            Message::MidimapAdd => {
                if let Some(kit) = &mut self.kit
                    && let Ok(note) = self.midimap_note.trim().parse::<u8>()
                    && note <= 127
                {
                    kit.add_note(note);
                    self.midimap_note.clear();
                }
            }
            Message::MidimapAssign(row, instrument) => {
                if let Some(kit) = &mut self.kit
                    && let Some(entry) = kit.midimap.entries.get_mut(row)
                    && let Some(name) = instrument
                        .and_then(|i| kit.instruments.get(i))
                        .map(|inst| inst.reference.name.clone())
                    && entry.instrument != name
                {
                    entry.instrument = name;
                    kit.dirty = true;
                }
            }
            Message::MidimapRemove(row) => {
                if let Some(kit) = &mut self.kit {
                    kit.unmap_note(row);
                }
            }
            Message::DismissStatus => self.status = None,
        }
        Task::none()
    }

    fn pick_save_dir(&self) -> Task<Message> {
        Task::perform(AsyncFileDialog::new().pick_folder(), |picked| {
            Message::SaveAsPicked(picked.map(|handle| handle.path().to_path_buf()))
        })
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.modal {
            Some(Modal::NewKit(draft)) => modal::new_kit_modal(draft),
            None => match &self.kit {
                Some(kit) => self.editor_view(kit),
                None => panels::welcome(),
            },
        }
    }

    pub fn app_theme(&self) -> iced::Theme {
        theme::theme()
    }

    fn editor_view<'a>(&'a self, kit: &'a EditorKit) -> Element<'a, Message> {
        let toolbar = container(
            row![
                toolbar_button("New", Message::NewKitClicked),
                toolbar_button("Open", Message::OpenKitClicked),
                toolbar_button("Save", Message::Save),
                toolbar_button("Save As", Message::SaveAs),
                Space::new().width(Length::Fill),
                text(&kit.drumkit.name).size(13).color(theme::TEXT),
                text(if kit.dirty { "●" } else { "•" })
                    .size(13)
                    .color(if kit.dirty {
                        theme::SOLO_ACTIVE
                    } else {
                        theme::TEXT_DIM
                    }),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding([8, 10])
        .width(Length::Fill);

        let sidebar = container(scrollable(sidebar::sidebar(kit, &self.selection)))
            .width(Length::Fixed(SIDEBAR_WIDTH))
            .height(Length::Fill);

        let context = container(scrollable(self.context_panel(kit)))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(16);

        let status: iced::widget::Container<'_, Message> = container(
            text(self.status.as_deref().unwrap_or(""))
                .size(11)
                .color(theme::TEXT_DIM),
        )
        .padding([4, 10])
        .width(Length::Fill);

        let status: Element<'_, Message> = if self.status.is_some() {
            mouse_area(status).on_press(Message::DismissStatus).into()
        } else {
            status.into()
        };

        column![
            toolbar,
            row![sidebar, context]
                .width(Length::Fill)
                .height(Length::Fill),
            status,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn context_panel<'a>(&'a self, kit: &'a EditorKit) -> Element<'a, Message> {
        match self.selection {
            Selection::Kit => panels::kit_panel(kit, &self.samplerate_text),
            Selection::Instrument(i) if i < kit.instruments.len() => {
                panels::instrument_panel(kit, i)
            }
            Selection::Sample(i, s)
                if i < kit.instruments.len() && s < kit.instruments[i].instrument.samples.len() =>
            {
                panels::sample_panel(kit, i, s)
            }
            Selection::Midimap => panels::midimap_panel(kit, &self.midimap_note),
            _ => panels::kit_panel(kit, &self.samplerate_text),
        }
    }
}

fn toolbar_button<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    button(text(label).size(12))
        .on_press(message)
        .style(theme::pill(false))
        .into()
}
