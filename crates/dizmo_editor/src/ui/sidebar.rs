//! Sidebar tree: the kit, its instruments and samples, and the MIDI map.

use iced::widget::{button, column, container, scrollable, text};
use iced::{Element, Length};

use super::theme::{TEXT, pill_button_style, sidebar_style};
use super::{Message, Selection};
use crate::model::EditorKit;

pub fn sidebar<'a>(kit: &'a EditorKit, selection: &Selection) -> Element<'a, Message> {
    let mut list = column![].spacing(2);

    list = list.push(tree_row(
        kit.drumkit.name.clone(),
        0,
        *selection == Selection::Kit,
        Message::Select(Selection::Kit),
    ));

    for (i, inst) in kit.instruments.iter().enumerate() {
        let assigned = inst.reference.channel_map.iter().any(|m| m.is_main);
        let label = if assigned {
            inst.reference.name.clone()
        } else {
            format!("{}  (unassigned)", inst.reference.name)
        };
        list = list.push(tree_row(
            label,
            0,
            *selection == Selection::Instrument(i),
            Message::Select(Selection::Instrument(i)),
        ));
        for (s, sample) in inst.instrument.samples.iter().enumerate() {
            list = list.push(tree_row(
                sample.name.clone(),
                1,
                *selection == Selection::Sample(i, s),
                Message::Select(Selection::Sample(i, s)),
            ));
        }
    }

    list = list.push(tree_row(
        "MIDI map".to_string(),
        0,
        *selection == Selection::Midimap,
        Message::Select(Selection::Midimap),
    ));

    container(scrollable(container(list).width(Length::Fill).padding(8)))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(sidebar_style)
        .into()
}

fn tree_row<'a>(
    label: String,
    indent: u16,
    selected: bool,
    message: Message,
) -> Element<'a, Message> {
    button(
        container(text(label).size(12).color(TEXT))
            .width(Length::Fill)
            .padding(iced::Padding {
                top: 6.0,
                right: 8.0,
                bottom: 6.0,
                left: 8.0 + f32::from(indent) * 14.0,
            }),
    )
    .on_press(message)
    .style(move |_theme, status| pill_button_style(selected, status))
    .width(Length::Fill)
    .into()
}
