//! The iced application shell: state, messages, and the overall layout.

pub mod modal;
pub mod panels;
pub mod preview;
pub mod sidebar;
pub mod theme;

use std::path::PathBuf;

use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, text};
use iced::{Element, Length, Task};
use rfd::AsyncFileDialog;

use crate::audio::PreviewPlayer;
use crate::model::EditorKit;
use crate::model::load::load;
use crate::model::save::save;

use modal::{DiscardAction, Modal, ModalMessage, NewKitDraft};

#[derive(Debug, Clone, Copy)]
pub enum Shortcut {
    Save,
    SaveAs,
    Open,
}

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
    KitLoaded(Box<Result<(EditorKit, Option<String>), String>>),
    Save,
    SaveAs,
    SaveAsPicked(Option<PathBuf>),
    Saved(Box<Result<EditorKit, String>>),
    Select(Selection),
    KitName(String),
    KitDescription(String),
    KitSamplerate(String),
    KitMetadataVersion(String),
    KitMetadataLogo(String),
    KitMetadataLicense(String),
    KitMetadataNotes(String),
    KitMetadataAuthor(String),
    KitMetadataEmail(String),
    KitMetadataWebsite(String),
    KitMetadataImage(String),
    KitMetadataImageMap(String),
    ClickmapColour(usize, String),
    ClickmapInstrument(usize, String),
    AddClickmap,
    RemoveClickmap(usize),
    SaveAsConfirmed,
    SaveAsCancel,
    AddChannel,
    RemoveChannel(usize),
    RenameChannel(usize, String),
    AddInstrument,
    RemoveInstrument(usize),
    RenameInstrument(usize, String),
    InstrumentGroup(usize, String),
    InstrumentDescription(usize, String),
    ChokeInstrument(usize, usize, Option<usize>),
    ChokeChoketime(usize, usize, String),
    AddChoke(usize),
    RemoveChoke(usize, usize),
    ToggleChannelAssignment(usize, String),
    SetChannelMain(usize, String, bool),
    RemoveSample(usize, usize),
    RenameSample(usize, usize, String),
    SamplePower(usize, usize, f32),
    InstrumentNormalized(usize, bool),
    Preview(usize, usize),
    StopPreview,
    PreviewVolume(f32),
    MidimapNote(String),
    MidimapAdd,
    MidimapAssign(usize, Option<usize>),
    MidimapRemove(usize),
    DismissStatus,
    PreviewFinished(u64),
    Shortcut(Shortcut),
    DiscardConfirmed,
    DiscardCancelled,
    DismissError,
    ResampleConfirmed,
    ResampleDeclined,
    ImportSample(usize),
    SampleImported(usize, Option<Vec<PathBuf>>),
}

pub struct App {
    kit: Option<EditorKit>,
    selection: Selection,
    modal: Option<Modal>,
    status: Option<String>,
    midimap_note: String,
    samplerate_text: String,
    player: PreviewPlayer,
    preview_token: u64,
    previewing: Option<(usize, usize)>,
    preview_volume: f32,
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
            player: PreviewPlayer::spawn(),
            preview_token: 0,
            previewing: None,
            preview_volume: 1.0,
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        use iced::keyboard::{Event, Key};
        let keyboard = iced::keyboard::listen().filter_map(|event| match event {
            Event::KeyPressed { key, modifiers, .. } if modifiers.command() => match key {
                Key::Character(c) if c.as_ref() == "s" && modifiers.shift() => {
                    Some(Message::Shortcut(Shortcut::SaveAs))
                }
                Key::Character(c) if c.as_ref() == "s" => Some(Message::Shortcut(Shortcut::Save)),
                Key::Character(c) if c.as_ref() == "o" => Some(Message::Shortcut(Shortcut::Open)),
                _ => None,
            },
            _ => None,
        });
        // Runs once: relays "playback finished" tokens from the audio thread so
        // the Play/Stop button un-toggles when a sample ends on its own.
        let playback = iced::Subscription::run(playback_finished_stream);
        iced::Subscription::batch([keyboard, playback])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NewKitClicked => {
                if let Some(kit) = &self.kit
                    && kit.dirty
                {
                    self.modal = Some(Modal::DiscardChanges(DiscardAction::NewKit));
                } else {
                    self.modal = Some(Modal::NewKit(NewKitDraft::new()));
                }
            }
            Message::OpenKitClicked => {
                if let Some(kit) = &self.kit
                    && kit.dirty
                {
                    self.modal = Some(Modal::DiscardChanges(DiscardAction::OpenKit));
                } else {
                    return self.open_kit();
                }
            }
            Message::NewKitModal(edit) => {
                if let Some(Modal::NewKit(draft)) = &mut self.modal {
                    match edit {
                        ModalMessage::Name(name) => {
                            draft.name = crate::model::types::deny_unsafe_characters(&name)
                        }
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
                self.stop_preview();
            }
            Message::NewKitCancel => self.modal = None,
            Message::KitOpened(Some(file)) => {
                return Task::perform(async move { load(&file) }, |result| {
                    Message::KitLoaded(Box::new(match result {
                        Ok((kit, warning)) => Ok((kit, warning)),
                        Err(err) => Err(err.to_string()),
                    }))
                });
            }
            Message::KitOpened(None) => {}
            Message::KitLoaded(result) => match *result {
                Ok((kit, warning)) => {
                    self.samplerate_text = kit.drumkit.samplerate.to_string();
                    let mut status = format!("Loaded kit '{}'.", kit.drumkit.name);
                    if let Some(warning) = warning {
                        status = format!("{status} {warning}");
                    }
                    self.kit = Some(kit);
                    self.selection = Selection::Kit;
                    self.modal = None;
                    self.status = Some(status);
                    self.stop_preview();
                }
                Err(error) => {
                    self.modal = Some(Modal::Error(format!("Could not load kit: {error}")));
                }
            },
            Message::DiscardConfirmed => match self.modal.take() {
                Some(Modal::DiscardChanges(DiscardAction::NewKit)) => {
                    self.modal = Some(Modal::NewKit(NewKitDraft::new()));
                }
                Some(Modal::DiscardChanges(DiscardAction::OpenKit)) => {
                    return self.open_kit();
                }
                _ => {}
            },
            Message::DiscardCancelled => self.modal = None,
            Message::DismissError => self.modal = None,
            Message::Shortcut(shortcut) => match shortcut {
                Shortcut::Save => {
                    if let Some(kit) = &self.kit
                        && !kit.dirty
                        && kit.root_dir.is_some()
                    {
                        self.status = Some("Nothing to save — the kit is unchanged.".into());
                        return Task::none();
                    }
                    return self.start_save();
                }
                Shortcut::SaveAs => return self.pick_save_dir(),
                Shortcut::Open => {
                    if let Some(kit) = &self.kit
                        && kit.dirty
                    {
                        self.modal = Some(Modal::DiscardChanges(DiscardAction::OpenKit));
                    } else {
                        return self.open_kit();
                    }
                }
            },
            Message::Save => return self.save_or_note(),
            Message::SaveAs => return self.pick_save_dir(),
            Message::SaveAsPicked(Some(dir)) => {
                // Saving into a non-empty directory may overwrite files, so
                // ask for confirmation first.
                let non_empty = std::fs::read_dir(&dir)
                    .map(|mut entries| entries.next().is_some())
                    .unwrap_or(false);
                if non_empty {
                    self.modal = Some(Modal::ConfirmOverwrite(dir));
                } else if let Some(kit) = &mut self.kit {
                    kit.root_dir = Some(dir);
                    return self.start_save();
                }
            }
            Message::SaveAsPicked(None) => {}
            Message::SaveAsConfirmed => {
                let dir = match &self.modal {
                    Some(Modal::ConfirmOverwrite(dir)) => dir.clone(),
                    _ => return Task::none(),
                };
                self.modal = None;
                if let Some(kit) = &mut self.kit {
                    kit.root_dir = Some(dir);
                    return self.start_save();
                }
            }
            Message::SaveAsCancel => self.modal = None,
            Message::Saved(result) => match *result {
                Ok(kit) => {
                    self.samplerate_text = kit.drumkit.samplerate.to_string();
                    let name = kit.drumkit.name.clone();
                    self.kit = Some(kit);
                    self.stop_preview();
                    self.status = Some(format!("Saved kit '{}'.", name));
                }
                Err(error) => {
                    self.modal = Some(Modal::Error(format!("Could not save kit: {error}")));
                }
            },
            Message::Select(selection) => {
                if let Some((previewing_instrument, previewing_sample)) = self.previewing {
                    let staying = matches!(
                        selection,
                        Selection::Sample(i, s) if i == previewing_instrument && s == previewing_sample
                    );
                    if !staying {
                        self.stop_preview();
                    }
                }
                self.selection = selection;
            }
            Message::KitName(name) => {
                let name = crate::model::types::deny_unsafe_characters(&name);
                if let Some(kit) = &mut self.kit {
                    kit.rename_kit(&name);
                }
            }
            Message::KitDescription(description) => {
                if let Some(kit) = &mut self.kit
                    && kit.drumkit.description != description
                {
                    kit.drumkit.description = description.clone();
                    kit.drumkit.metadata.description = Some(description);
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
                self.stop_preview();
                self.selection = Selection::Kit;
            }
            Message::RenameInstrument(i, name) => {
                let name = crate::model::types::deny_unsafe_characters(&name);
                if let Some(kit) = &mut self.kit {
                    kit.rename_instrument(i, &name);
                }
            }
            Message::InstrumentGroup(i, group) => {
                if let Some(kit) = &mut self.kit {
                    kit.set_instrument_group(i, &group);
                }
            }
            Message::InstrumentDescription(i, description) => {
                if let Some(kit) = &mut self.kit {
                    kit.set_instrument_description(i, &description);
                }
            }
            Message::ChokeInstrument(instrument, choke, target) => {
                if let Some(kit) = &mut self.kit
                    && let Some(name) = target
                        .and_then(|i| kit.instruments.get(i))
                        .map(|inst| inst.reference.name.clone())
                    && let Some(inst) = kit.instruments.get(instrument)
                    && let Some(c) = inst.reference.chokes.get(choke)
                    && c.instrument != name
                {
                    let choketime_ms = inst.reference.chokes[choke].choketime_ms;
                    kit.remove_choke(instrument, choke);
                    match kit.add_choke(instrument, &name, choketime_ms) {
                        Ok(()) => {}
                        Err(error) => self.status = Some(error),
                    }
                }
            }
            Message::ChokeChoketime(instrument, choke, value) => {
                if let (Ok(ms), Some(kit)) = (value.parse::<u32>(), &mut self.kit) {
                    kit.set_choke_choketime(instrument, choke, ms);
                }
            }
            Message::AddChoke(instrument) => {
                if let Some(kit) = &mut self.kit {
                    let Some(inst) = kit.instruments.get(instrument) else {
                        return Task::none();
                    };
                    let Some(target) = kit
                        .instruments
                        .iter()
                        .find(|other| other.reference.name != inst.reference.name)
                        .map(|other| other.reference.name.clone())
                    else {
                        self.status = Some(
                            "Add another instrument first — an instrument cannot choke itself."
                                .into(),
                        );
                        return Task::none();
                    };
                    match kit.add_choke(instrument, &target, 68) {
                        Ok(()) => {}
                        Err(error) => self.status = Some(error),
                    }
                }
            }
            Message::RemoveChoke(instrument, choke) => {
                if let Some(kit) = &mut self.kit {
                    kit.remove_choke(instrument, choke);
                }
            }
            Message::KitMetadataVersion(value) => {
                self.edit_metadata_string(|m| &mut m.version, value)
            }
            Message::KitMetadataLogo(value) => self.edit_metadata_string(|m| &mut m.logo, value),
            Message::KitMetadataLicense(value) => {
                self.edit_metadata_string(|m| &mut m.license, value)
            }
            Message::KitMetadataNotes(value) => self.edit_metadata_string(|m| &mut m.notes, value),
            Message::KitMetadataAuthor(value) => {
                self.edit_metadata_string(|m| &mut m.author, value)
            }
            Message::KitMetadataEmail(value) => self.edit_metadata_string(|m| &mut m.email, value),
            Message::KitMetadataWebsite(value) => {
                self.edit_metadata_string(|m| &mut m.website, value)
            }
            Message::KitMetadataImage(value) => {
                if let Some(kit) = &mut self.kit {
                    kit.edit_metadata(|metadata| {
                        if value.trim().is_empty()
                            && metadata
                                .image
                                .as_ref()
                                .is_some_and(|image| image.clickmap.is_empty())
                        {
                            metadata.image = None;
                        } else {
                            let image = metadata.image.get_or_insert_with(|| dizmo_kit::KitImage {
                                src: String::new(),
                                map: None,
                                clickmap: Vec::new(),
                            });
                            image.src = value;
                        }
                    });
                }
            }
            Message::KitMetadataImageMap(value) => {
                if let Some(kit) = &mut self.kit {
                    kit.edit_metadata(|metadata| {
                        if let Some(image) = &mut metadata.image {
                            image.map = if value.trim().is_empty() {
                                None
                            } else {
                                Some(value)
                            };
                        }
                    });
                }
            }
            Message::ClickmapColour(row, value) => {
                if let Some(kit) = &mut self.kit {
                    kit.edit_metadata(|metadata| {
                        if let Some(image) = &mut metadata.image
                            && let Some(clickmap) = image.clickmap.get_mut(row)
                        {
                            clickmap.colour = value;
                        }
                    });
                }
            }
            Message::ClickmapInstrument(row, value) => {
                if let Some(kit) = &mut self.kit {
                    kit.edit_metadata(|metadata| {
                        if let Some(image) = &mut metadata.image
                            && let Some(clickmap) = image.clickmap.get_mut(row)
                        {
                            clickmap.instrument = value;
                        }
                    });
                }
            }
            Message::AddClickmap => {
                if let Some(kit) = &mut self.kit {
                    kit.edit_metadata(|metadata| {
                        let image = metadata.image.get_or_insert_with(|| dizmo_kit::KitImage {
                            src: String::new(),
                            map: None,
                            clickmap: Vec::new(),
                        });
                        image.clickmap.push(dizmo_kit::ClickMap {
                            colour: String::new(),
                            instrument: String::new(),
                        });
                    });
                }
            }
            Message::RemoveClickmap(row) => {
                if let Some(kit) = &mut self.kit {
                    kit.edit_metadata(|metadata| {
                        if let Some(image) = &mut metadata.image {
                            image.clickmap.remove(row);
                        }
                    });
                }
            }
            Message::ToggleChannelAssignment(i, channel) => {
                if let Some(kit) = &mut self.kit {
                    kit.toggle_channel_assignment(i, &channel);
                }
            }
            Message::SetChannelMain(i, channel, is_main) => {
                if let Some(kit) = &mut self.kit {
                    kit.set_channel_main(i, &channel, is_main);
                }
            }
            Message::RemoveSample(i, s) => {
                if let Some(kit) = &mut self.kit {
                    kit.remove_sample(i, s);
                }
                self.stop_preview();
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
            Message::InstrumentNormalized(i, checked) => {
                if let Some(kit) = &mut self.kit {
                    kit.set_instrument_normalized(i, checked);
                }
            }
            Message::Preview(instrument, sample) => {
                let Some(kit) = &self.kit else {
                    return Task::none();
                };
                let Some(inst) = kit.instruments.get(instrument) else {
                    return Task::none();
                };
                let Some(sample_ref) = inst.instrument.samples.get(sample) else {
                    return Task::none();
                };
                let Some(audio_file) = sample_ref.audio_files.first() else {
                    self.status = Some("This sample has no audio files to preview.".into());
                    return Task::none();
                };
                if !self.player.audio_available() {
                    self.status =
                        Some("No audio output device available — preview is disabled.".into());
                    return Task::none();
                }
                let path = inst.instrument.base_dir.join(&audio_file.file);
                if !path.is_file() {
                    self.status = Some(format!("Sample file not found: '{}'.", path.display()));
                    return Task::none();
                }
                if hound::WavReader::open(&path).is_err() {
                    self.status = Some(format!("'{}' is not a readable WAV file.", path.display()));
                    return Task::none();
                }
                self.preview_token += 1;
                self.player.play(
                    &path,
                    audio_file.file_channel,
                    self.preview_volume,
                    self.preview_token,
                );
                self.previewing = Some((instrument, sample));
                self.status = Some(format!("Previewing '{}'.", sample_ref.name));
            }
            Message::PreviewFinished(token) => {
                // Playback ended on its own: un-toggle the Play/Stop button,
                // unless a newer play already took over.
                if token == self.preview_token {
                    self.previewing = None;
                }
            }
            Message::StopPreview => {
                self.stop_preview();
                self.status = Some("Preview stopped.".into());
            }
            Message::PreviewVolume(volume) => {
                self.preview_volume = volume;
                self.player.set_volume(volume);
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
            Message::ImportSample(instrument) => {
                return Task::perform(
                    AsyncFileDialog::new()
                        .add_filter("Audio File", &["wav"])
                        .pick_files(),
                    move |picked| {
                        Message::SampleImported(
                            instrument,
                            picked.map(|handles| {
                                handles
                                    .into_iter()
                                    .map(|handle| handle.path().to_path_buf())
                                    .collect()
                            }),
                        )
                    },
                );
            }
            Message::SampleImported(instrument, Some(files)) => {
                let Some(kit) = &self.kit else {
                    return Task::none();
                };
                let kit_rate = kit.drumkit.samplerate.round() as u32;

                // Probe every picked file up front so an unreadable file
                // aborts the whole batch instead of importing a partial
                // selection.
                let mut samples = Vec::with_capacity(files.len());
                let mut errors = Vec::new();
                for file in files {
                    match hound::WavReader::open(&file) {
                        Ok(reader) => samples.push((file, reader.spec().sample_rate)),
                        Err(error) => {
                            errors.push(format!("'{file:?}' is not a readable WAV file: {error}"));
                        }
                    }
                }
                if !errors.is_empty() {
                    self.status = Some(errors.join("; "));
                    return Task::none();
                }

                // Warn when any sample's rate does not match the kit's; the
                // user may accept a resample on save or import as-is.
                let any_mismatch = kit_rate > 0
                    && samples
                        .iter()
                        .any(|(_, source_rate)| *source_rate != kit_rate);
                if any_mismatch {
                    self.modal = Some(Modal::ResampleConfirm {
                        instrument,
                        samples,
                        kit_rate,
                    });
                } else {
                    self.import_sample_file(instrument, &samples);
                }
            }
            Message::SampleImported(_, None) => {}
            Message::ResampleConfirmed => {
                let Some(Modal::ResampleConfirm {
                    instrument,
                    samples,
                    ..
                }) = self.modal.take()
                else {
                    return Task::none();
                };
                self.import_sample_file(instrument, &samples);
            }
            Message::ResampleDeclined => {
                let Some(Modal::ResampleConfirm {
                    instrument,
                    samples,
                    kit_rate,
                }) = self.modal.take()
                else {
                    return Task::none();
                };
                // Keep the files that already match the kit's rate and skip
                // the mismatched ones instead of aborting the whole batch.
                let total = samples.len();
                let matching: Vec<(PathBuf, u32)> = samples
                    .into_iter()
                    .filter(|(_, source_rate)| *source_rate == kit_rate)
                    .collect();
                if matching.is_empty() {
                    self.status = Some(format!(
                        "Import cancelled — none of the selected samples match the kit rate of {kit_rate} Hz."
                    ));
                    return Task::none();
                }
                self.import_sample_file(instrument, &matching);
                let skipped = total - matching.len();
                let note = if skipped == 1 {
                    "1 sample with a different rate was skipped.".to_string()
                } else {
                    format!("{skipped} samples with a different rate were skipped.")
                };
                self.status = match self.status.take() {
                    Some(mut status) => {
                        status.push(' ');
                        status.push_str(&note);
                        Some(status)
                    }
                    None => Some(note),
                };
            }
        }
        Task::none()
    }

    /// Imports the picked WAVs as new samples and selects the last one.
    /// Files whose rate differs from the kit's are flagged for resampling on
    /// save.
    fn import_sample_file(&mut self, instrument: usize, samples: &[(PathBuf, u32)]) {
        let Some(kit) = &mut self.kit else {
            return;
        };
        let kit_rate = kit.drumkit.samplerate.round() as u32;
        let mut imported = 0usize;
        let mut resampled = 0usize;
        let mut last_index = None;
        let mut last_file_name = String::new();
        let mut errors = Vec::new();

        for (file, source_rate) in samples {
            let resample = kit_rate > 0 && *source_rate != kit_rate;
            match kit.import_sample(instrument, file, resample) {
                Ok(index) => {
                    imported += 1;
                    resampled += usize::from(resample);
                    last_index = Some(index);
                    last_file_name = file
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                }
                Err(error) => {
                    if !errors.contains(&error) {
                        errors.push(error);
                    }
                }
            }
        }

        if let Some(index) = last_index {
            self.selection = Selection::Sample(instrument, index);
        }
        let name = kit
            .instruments
            .get(instrument)
            .map(|inst| inst.reference.name.clone())
            .unwrap_or_default();
        let mut status = match imported {
            0 => String::new(),
            1 if resampled == 1 => {
                format!("Imported '{last_file_name}' into '{name}' (will be resampled on save).")
            }
            1 => format!("Imported '{last_file_name}' into '{name}'."),
            _ => format!("Imported {imported} samples into '{name}'."),
        };
        if imported > 1 && resampled > 0 {
            status.push_str(&format!(" ({resampled} will be resampled on save)."));
        }
        if !errors.is_empty() {
            if status.is_empty() {
                status.push_str("Import failed: ");
            } else {
                status.push_str(" Failed: ");
            }
            status.push_str(&errors.join("; "));
        }
        if !status.is_empty() {
            self.status = Some(status);
        }
    }

    fn stop_preview(&mut self) {
        self.player.stop();
        self.previewing = None;
    }

    /// Opens the kit file picker (used by the Open button and ⌘O).
    fn open_kit(&self) -> Task<Message> {
        Task::perform(
            AsyncFileDialog::new()
                .add_filter("Kits", &["xml"])
                .pick_file(),
            |picked| Message::KitOpened(picked.map(|handle| handle.path().to_path_buf())),
        )
    }

    /// Saves, unless the kit is clean and already has a location (idempotent
    /// save: nothing to write).
    fn save_or_note(&mut self) -> Task<Message> {
        if let Some(kit) = &self.kit
            && !kit.dirty
            && kit.root_dir.is_some()
        {
            self.status = Some("Nothing to save — the kit is unchanged.".into());
            return Task::none();
        }
        self.start_save()
    }

    /// Edits one optional kit metadata string field; an empty value clears it.
    fn edit_metadata_string(
        &mut self,
        field: fn(&mut dizmo_kit::KitMetadata) -> &mut Option<String>,
        value: String,
    ) {
        if let Some(kit) = &mut self.kit {
            kit.edit_metadata(|metadata| {
                let target = field(metadata);
                *target = if value.trim().is_empty() {
                    None
                } else {
                    Some(value)
                };
            });
        }
    }

    /// Runs a save on a cloned kit off the UI thread; on success the mutated
    /// (normalized) kit replaces the live one.
    fn start_save(&self) -> Task<Message> {
        let Some(kit) = &self.kit else {
            return Task::none();
        };
        if kit.root_dir.is_none() {
            return self.pick_save_dir();
        }
        let mut kit = kit.clone();
        Task::perform(async move { save(&mut kit).map(|()| kit) }, |result| {
            Message::Saved(Box::new(result))
        })
    }

    fn pick_save_dir(&self) -> Task<Message> {
        Task::perform(AsyncFileDialog::new().pick_folder(), |picked| {
            Message::SaveAsPicked(picked.map(|handle| handle.path().to_path_buf()))
        })
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.modal {
            Some(Modal::NewKit(draft)) => modal::new_kit_modal(draft),
            Some(Modal::ConfirmOverwrite(dir)) => modal::confirm_overwrite_modal(dir),
            Some(Modal::DiscardChanges(action)) => modal::discard_changes_modal(*action),
            Some(Modal::Error(message)) => modal::error_modal(message),
            Some(Modal::ResampleConfirm {
                samples, kit_rate, ..
            }) => modal::resample_confirm_modal(samples, *kit_rate),
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
                let playing = self.previewing == Some((i, s));
                panels::sample_panel(kit, i, s, playing, self.preview_volume)
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

/// Relays "playback finished" tokens from the audio thread into iced messages.
fn playback_finished_stream() -> impl futures::Stream<Item = Message> {
    use futures::StreamExt;
    let (sender, receiver) = futures::channel::mpsc::unbounded();
    crate::audio::register_finished_listener(sender);
    futures::stream::unfold(receiver, |mut receiver| async move {
        let token = receiver.next().await?;
        Some((Message::PreviewFinished(token), receiver))
    })
}
