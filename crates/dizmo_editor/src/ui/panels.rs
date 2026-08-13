//! Context panels: Kit, Instrument, Sample, MIDI map, and the welcome screen.

use iced::widget::{
    Space, button, checkbox, column, container, pick_list, row, scrollable, text, text_input,
};
use iced::{Element, Length};

use super::preview;
use super::theme::{
    TEXT, TEXT_DIM, danger_button_style, menu_style, pick_list_style, pill, pill_button_style,
    text_input_style,
};
use super::{Message, Selection};
use crate::model::EditorKit;

pub fn welcome() -> Element<'static, Message> {
    container(
        column![
            text("DIZMO Editor").size(36).color(TEXT),
            text("Create or edit a DrumGizmo-style kit.")
                .size(14)
                .color(TEXT_DIM),
            row![
                button(text("New Kit…"))
                    .on_press(Message::NewKitClicked)
                    .style(pill(false)),
                button(text("Open Kit…"))
                    .on_press(Message::OpenKitClicked)
                    .style(pill(false)),
            ]
            .spacing(12),
            text("Shortcuts: ⌘N new  ·  ⌘O open  ·  ⌘S save  ·  ⌘⇧S save as")
                .size(11)
                .color(TEXT_DIM),
        ]
        .spacing(18)
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

pub fn kit_panel<'a>(kit: &'a EditorKit, samplerate_text: &'a str) -> Element<'a, Message> {
    let mut content = column![
        text("Kit").size(18).color(TEXT),
        field("Title", &kit.drumkit.name, Message::KitName),
        field(
            "Description",
            &kit.drumkit.description,
            Message::KitDescription
        ),
        field("Sample rate", samplerate_text, Message::KitSamplerate),
        iced::widget::rule::horizontal(1.0),
        text("Metadata").size(12).color(TEXT),
        field(
            "Version",
            kit.drumkit.metadata.version.as_deref().unwrap_or(""),
            Message::KitMetadataVersion,
        ),
        field(
            "Logo",
            kit.drumkit.metadata.logo.as_deref().unwrap_or(""),
            Message::KitMetadataLogo,
        ),
        field(
            "License",
            kit.drumkit.metadata.license.as_deref().unwrap_or(""),
            Message::KitMetadataLicense,
        ),
        field(
            "Notes",
            kit.drumkit.metadata.notes.as_deref().unwrap_or(""),
            Message::KitMetadataNotes,
        ),
        field(
            "Author",
            kit.drumkit.metadata.author.as_deref().unwrap_or(""),
            Message::KitMetadataAuthor,
        ),
        field(
            "Email",
            kit.drumkit.metadata.email.as_deref().unwrap_or(""),
            Message::KitMetadataEmail,
        ),
        field(
            "Website",
            kit.drumkit.metadata.website.as_deref().unwrap_or(""),
            Message::KitMetadataWebsite,
        ),
        field(
            "Image",
            kit.drumkit
                .metadata
                .image
                .as_ref()
                .map(|image| image.src.as_str())
                .unwrap_or(""),
            Message::KitMetadataImage,
        ),
        field(
            "Image map",
            kit.drumkit
                .metadata
                .image
                .as_ref()
                .and_then(|image| image.map.as_deref())
                .unwrap_or(""),
            Message::KitMetadataImageMap,
        ),
        iced::widget::rule::horizontal(1.0),
        text("Output channels  ·  workflow step 1")
            .size(12)
            .color(TEXT),
    ]
    .spacing(8);

    let clickmaps = match &kit.drumkit.metadata.image {
        Some(image) if !image.clickmap.is_empty() => {
            let mut rows = column![].spacing(4);
            for (row, clickmap) in image.clickmap.iter().enumerate() {
                rows = rows.push(
                    row![
                        text_input("colour", &clickmap.colour)
                            .on_input(move |value| Message::ClickmapColour(row, value))
                            .style(text_input_style)
                            .width(Length::Fill),
                        text_input("instrument", &clickmap.instrument)
                            .on_input(move |value| Message::ClickmapInstrument(row, value))
                            .style(text_input_style)
                            .width(Length::Fill),
                        button(text("×"))
                            .on_press(Message::RemoveClickmap(row))
                            .style(danger_button_style),
                    ]
                    .spacing(6),
                );
            }
            rows
        }
        _ => column![].spacing(4),
    };
    content = content.push(
        column![
            text("Click map  ·  colour + instrument")
                .size(11)
                .color(TEXT_DIM),
            clickmaps,
            button(text("+ Add clickmap"))
                .on_press(Message::AddClickmap)
                .style(pill(false)),
        ]
        .spacing(4),
    );

    if kit.drumkit.channels.is_empty() {
        content = content.push(
            text("Define at least one channel before adding instruments.")
                .size(11)
                .color(TEXT_DIM),
        );
    } else {
        let mut channels = column![].spacing(4);
        for (i, ch) in kit.drumkit.channels.iter().enumerate() {
            channels = channels.push(
                row![
                    text_input("Channel name", &ch.name)
                        .on_input(move |name| Message::RenameChannel(i, name))
                        .style(text_input_style)
                        .width(Length::Fill),
                    button(text("×"))
                        .on_press(Message::RemoveChannel(i))
                        .style(danger_button_style),
                ]
                .spacing(6),
            );
        }
        content = content.push(channels);
    }

    content = content.push(
        button(text("+ Add channel"))
            .on_press(Message::AddChannel)
            .style(pill(false)),
    );
    content = content.push(iced::widget::rule::horizontal(1.0));
    content = content.push(
        row![
            text("Instruments").size(12).color(TEXT),
            Space::new().width(Length::Fill),
            button(text("+ Add instrument"))
                .on_press(Message::AddInstrument)
                .style(pill(false)),
        ]
        .align_y(iced::Alignment::Center),
    );

    scrollable(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub fn instrument_panel<'a>(kit: &'a EditorKit, index: usize) -> Element<'a, Message> {
    let inst = &kit.instruments[index];

    let assigned_names: Vec<&str> = inst
        .instrument
        .channels
        .iter()
        .map(|ch| ch.name.as_str())
        .collect();
    let mut channel_map = column![].spacing(4);
    if kit.drumkit.channels.is_empty() {
        channel_map = channel_map.push(
            text("No kit channels yet — define them on the Kit screen first.")
                .size(11)
                .color(TEXT_DIM),
        );
    }
    for kit_ch in &kit.drumkit.channels {
        let name = &kit_ch.name;
        let is_assigned = assigned_names.contains(&name.as_str());
        let is_main = inst
            .reference
            .channel_map
            .iter()
            .any(|m| m.in_name == *name && m.is_main);
        let assigned = checkbox(is_assigned)
            .label("assign")
            .on_toggle(move |checked| {
                if checked != is_assigned {
                    Message::ToggleChannelAssignment(index, name.clone())
                } else {
                    Message::DismissStatus
                }
            });
        let main = checkbox(is_main)
            .label("main")
            .on_toggle(move |checked| Message::SetChannelMain(index, name.clone(), checked));
        channel_map = channel_map.push(
            row![
                text(name).size(11).color(TEXT).width(Length::Fixed(140.0)),
                assigned,
                main,
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    }

    let instrument_names: Vec<String> = kit
        .instruments
        .iter()
        .map(|i| i.reference.name.clone())
        .collect();
    let mut chokes = column![].spacing(4);
    if inst.reference.chokes.is_empty() {
        chokes = chokes.push(
            text("No chokes — nothing cuts this instrument.")
                .size(11)
                .color(TEXT_DIM),
        );
    }
    for (c, choke) in inst.reference.chokes.iter().enumerate() {
        let options = instrument_names.clone();
        let selected = options
            .iter()
            .find(|name| **name == choke.instrument)
            .cloned();
        chokes = chokes.push(
            row![
                pick_list(options.clone(), selected, move |picked: String| {
                    Message::ChokeInstrument(
                        index,
                        c,
                        options.iter().position(|option| option == &picked),
                    )
                })
                .placeholder("Target")
                .style(pick_list_style)
                .menu_style(menu_style)
                .width(Length::Fill),
                text_input("ms", &choke.choketime_ms.to_string())
                    .on_input(move |value| Message::ChokeChoketime(index, c, value))
                    .style(text_input_style)
                    .width(Length::Fixed(70.0)),
                button(text("×"))
                    .on_press(Message::RemoveChoke(index, c))
                    .style(danger_button_style),
            ]
            .spacing(6),
        );
    }

    let mut samples = column![].spacing(4);
    if inst.instrument.samples.is_empty() {
        samples = samples.push(
            text("No samples yet — add some (workflow step 3).")
                .size(11)
                .color(TEXT_DIM),
        );
    }
    for (s, sample) in inst.instrument.samples.iter().enumerate() {
        samples = samples.push(
            button(text(&sample.name).size(12).color(TEXT))
                .on_press(Message::Select(Selection::Sample(index, s)))
                .style(move |_theme, status| pill_button_style(false, status))
                .width(Length::Fill),
        );
    }

    column![
        text(&inst.reference.name).size(18).color(TEXT),
        text(format!(
            "{}  ·  v{}",
            inst.file.display(),
            inst.instrument.version
        ))
        .size(11)
        .color(TEXT_DIM),
        field("Instrument name", &inst.reference.name, move |name| {
            Message::RenameInstrument(index, name)
        },),
        field(
            "Group",
            inst.reference.group.as_deref().unwrap_or(""),
            move |group| Message::InstrumentGroup(index, group),
        ),
        field(
            "Description",
            &inst.instrument.description,
            move |description| Message::InstrumentDescription(index, description),
        ),
        column![
            text("Channel map  ·  workflow step 4")
                .size(11)
                .color(TEXT_DIM),
            channel_map,
        ]
        .spacing(4),
        iced::widget::rule::horizontal(1.0),
        text("Chokes  ·  cut these instruments on trigger")
            .size(11)
            .color(TEXT_DIM),
        chokes,
        button(text("+ Add choke"))
            .on_press(Message::AddChoke(index))
            .style(pill(false)),
        iced::widget::rule::horizontal(1.0),
        text("Samples").size(12).color(TEXT),
        samples,
        button(text("+ Add sample"))
            .on_press(Message::ImportSample(index))
            .style(pill(false)),
        iced::widget::rule::horizontal(1.0),
        button(text("Remove instrument"))
            .on_press(Message::RemoveInstrument(index))
            .style(danger_button_style),
    ]
    .spacing(8)
    .into()
}

pub fn sample_panel<'a>(
    kit: &'a EditorKit,
    instrument: usize,
    sample_idx: usize,
    previewing: bool,
    preview_volume: f32,
) -> Element<'a, Message> {
    let sample = &kit.instruments[instrument].instrument.samples[sample_idx];

    let mut audio_files = column![].spacing(2);
    for af in &sample.audio_files {
        audio_files = audio_files.push(
            row![
                text(&af.channel)
                    .size(11)
                    .color(TEXT)
                    .width(Length::Fixed(120.0)),
                text(&af.file).size(11).color(TEXT_DIM),
                text(format!("file channel {}", af.file_channel + 1))
                    .size(11)
                    .color(TEXT_DIM),
            ]
            .spacing(8),
        );
    }
    if sample.audio_files.is_empty() {
        audio_files = audio_files.push(
            text(
                "No audio files yet — assign channels to this instrument first (workflow step 4).",
            )
            .size(11)
            .color(TEXT_DIM),
        );
    }
    column![
        text("Sample").size(18).color(TEXT),
        field("Name", &sample.name, move |name| Message::RenameSample(
            instrument, sample_idx, name
        ),),
        row![
            text("Power")
                .size(11)
                .color(TEXT_DIM)
                .width(Length::Fixed(60.0)),
            iced::widget::slider(0.0..=1.0, sample.power, move |value| Message::SamplePower(
                instrument, sample_idx, value
            ),)
            .width(Length::Fill),
            text(format!("{:.2}", sample.power))
                .size(11)
                .color(TEXT)
                .width(Length::Fixed(36.0)),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8),
        checkbox(sample.normalized)
            .label("Normalized")
            .on_toggle(move |checked| Message::SampleNormalized(instrument, sample_idx, checked)),
        iced::widget::rule::horizontal(1.0),
        text("Audio files").size(12).color(TEXT),
        audio_files,
        preview::preview_controls(
            previewing,
            preview_volume,
            if previewing {
                Message::StopPreview
            } else {
                Message::Preview(instrument, sample_idx)
            },
            Message::PreviewVolume,
        ),
        button(text("Remove sample"))
            .on_press(Message::RemoveSample(instrument, sample_idx))
            .style(danger_button_style),
    ]
    .spacing(8)
    .into()
}

pub fn midimap_panel<'a>(kit: &'a EditorKit, note_draft: &'a str) -> Element<'a, Message> {
    let instrument_names: Vec<String> = kit
        .instruments
        .iter()
        .map(|i| i.reference.name.clone())
        .collect();

    let mut rows = column![].spacing(4);
    if kit.midimap.entries.is_empty() {
        rows = rows.push(text("No notes mapped yet.").size(11).color(TEXT_DIM));
    }
    for (row, entry) in kit.midimap.entries.iter().enumerate() {
        let options = instrument_names.clone();
        let selected = instrument_names
            .iter()
            .find(|name| **name == entry.instrument)
            .cloned();
        rows = rows.push(
            row![
                text(format!("{}  ({})", midi_note_name(entry.note), entry.note))
                    .size(11)
                    .color(TEXT)
                    .width(Length::Fixed(110.0)),
                pick_list(options.clone(), selected, move |picked: String| {
                    Message::MidimapAssign(row, options.iter().position(|option| option == &picked))
                },)
                .placeholder("Unassigned")
                .style(pick_list_style)
                .menu_style(menu_style)
                .width(Length::Fill),
                button(text("×"))
                    .on_press(Message::MidimapRemove(row))
                    .style(danger_button_style),
            ]
            .spacing(6),
        );
    }

    column![
        text("MIDI map").size(18).color(TEXT),
        text("Map MIDI notes to instruments.")
            .size(11)
            .color(TEXT_DIM),
        iced::widget::rule::horizontal(1.0),
        rows,
        row![
            text_input("note 0–127", note_draft)
                .on_input(Message::MidimapNote)
                .on_submit(Message::MidimapAdd)
                .style(text_input_style)
                .width(Length::Fixed(120.0)),
            button(text("+ Map note"))
                .on_press(Message::MidimapAdd)
                .style(pill(false)),
        ]
        .spacing(6),
    ]
    .spacing(8)
    .into()
}

/// A labelled, styled text input.
fn field<'a>(
    label: &'a str,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        text(label).size(11).color(TEXT_DIM),
        text_input("", value)
            .on_input(on_input)
            .style(text_input_style),
    ]
    .spacing(4)
    .into()
}

pub(crate) fn midi_note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = (note / 12).saturating_sub(1);
    format!("{}{}", NAMES[(note % 12) as usize], octave)
}
