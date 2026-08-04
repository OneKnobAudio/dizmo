use crate::ui::fader::show_fader;
use crate::ui::knob::{KNOB_RADIUS, show_knob};
use crate::ui::{
    CARD_BG, CARD_BORDER, EditorState, FIELD_BG, FIELD_BORDER, MUTE_ACTIVE, SOLO_ACTIVE, TEXT,
    TEXT_DIM,
};
use egui::{
    Align2, Color32, FontId, Margin, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2,
};
use nice_plug::prelude::*;

/// Width of one channel strip card.
pub const STRIP_WIDTH: f32 = 128.0;

/// Height of one channel strip card.
pub const STRIP_HEIGHT: f32 = 460.0;

/// Draws a single channel strip matching the mockup layout:
/// number badge, editable name, solo/mute, choke assign, pan knob and vertical fader.
pub fn draw_strip(ui: &mut Ui, setter: &ParamSetter, state: &mut EditorState, index: usize) {
    let params = state.params.clone();
    let (card_rect, _) = ui.allocate_exact_size(vec2(STRIP_WIDTH, STRIP_HEIGHT), Sense::hover());

    ui.painter().rect_filled(card_rect, 8.0, CARD_BG);
    ui.painter().rect_stroke(
        card_rect,
        8.0,
        Stroke::new(1.0, CARD_BORDER),
        StrokeKind::Inside,
    );

    let inner = Rect::from_min_max(
        pos2(card_rect.left() + 8.0, card_rect.top() + 8.0),
        pos2(card_rect.right() - 8.0, card_rect.bottom() - 8.0),
    );
    let center_x = card_rect.center().x;

    // --- Channel number indicator ---
    ui.painter().text(
        pos2(center_x, inner.top() + 8.0),
        Align2::CENTER_CENTER,
        format!("{}", index + 1),
        FontId::proportional(13.0),
        TEXT,
    );

    // --- Channel name (editable) ---
    let name_rect = Rect::from_min_size(
        pos2(inner.left(), inner.top() + 16.0),
        vec2(inner.width(), 22.0),
    );
    let name_response = text_field(
        ui,
        name_rect,
        &mut state.name_buffers[index],
        FIELD_BG,
        TEXT,
        FIELD_BORDER,
    );
    if name_response.changed()
        && let Ok(mut names) = params.channel_names.lock()
    {
        names[index] = state.name_buffers[index].clone();
    }

    // --- Solo / Mute ---
    let button_y = name_rect.bottom() + 6.0;
    let button_gap = 6.0;
    let button_width = (inner.width() - button_gap) / 2.0;
    let solo_rect = Rect::from_min_size(pos2(inner.left(), button_y), vec2(button_width, 24.0));
    let mute_rect = Rect::from_min_size(
        pos2(inner.left() + button_width + button_gap, button_y),
        vec2(button_width, 24.0),
    );

    let solo_response = rounded_toggle(
        ui,
        solo_rect,
        index,
        "solo",
        params.channels[index].solo.value(),
        "S",
        SOLO_ACTIVE,
    );
    if solo_response.clicked() {
        toggle_bool(setter, &params.channels[index].solo);
    }

    let mute_response = rounded_toggle(
        ui,
        mute_rect,
        index,
        "mute",
        params.channels[index].mute.value(),
        "M",
        MUTE_ACTIVE,
    );
    if mute_response.clicked() {
        toggle_bool(setter, &params.channels[index].mute);
    }

    // Choke is defined by the kit, no UI for configuration

    // --- Separator ---
    let separator_y = mute_rect.bottom() + 6.0;
    ui.painter().line_segment(
        [
            pos2(inner.left(), separator_y),
            pos2(inner.right(), separator_y),
        ],
        Stroke::new(1.0, CARD_BORDER),
    );

    // --- Pan knob with L/R labels (stereo plugin only) ---
    let fader_top = if state.show_pan {
        let knob_center = pos2(center_x, separator_y + 26.0);
        let knob_rect =
            Rect::from_center_size(knob_center, vec2(KNOB_RADIUS * 3.0, KNOB_RADIUS * 3.0));
        show_knob(ui, setter, &params.channels[index].pan, index, knob_rect);
        ui.painter().text(
            pos2(inner.left() + 4.0, knob_center.y),
            Align2::LEFT_CENTER,
            "L",
            FontId::proportional(9.0),
            TEXT_DIM,
        );
        ui.painter().text(
            pos2(inner.right() - 4.0, knob_center.y),
            Align2::RIGHT_CENTER,
            "R",
            FontId::proportional(9.0),
            TEXT_DIM,
        );
        knob_rect.bottom() + 8.0
    } else {
        separator_y + 8.0
    };

    // --- Vertical fader ---
    let fader_rect = Rect::from_min_max(
        pos2(inner.left(), fader_top),
        pos2(inner.right(), inner.bottom()),
    );
    show_fader(ui, setter, &params.channels[index].fader, index, fader_rect);
}

/// A styled single-line text field with a custom background and border.
fn text_field(
    ui: &mut Ui,
    rect: Rect,
    buffer: &mut String,
    bg: Color32,
    text_color: Color32,
    border: Color32,
) -> egui::Response {
    let text_edit = TextEdit::singleline(buffer)
        .frame(egui::Frame::NONE)
        .background_color(bg)
        .text_color(text_color)
        .font(FontId::proportional(10.0))
        .margin(Margin::symmetric(6, 4));
    let response = ui.put(rect, text_edit);
    ui.painter()
        .rect_stroke(rect, 4.0, Stroke::new(1.0, border), StrokeKind::Inside);
    response
}

/// A small rounded toggle button drawn manually.
#[allow(clippy::too_many_arguments)]
fn rounded_toggle(
    ui: &mut Ui,
    rect: Rect,
    channel: usize,
    kind: &str,
    active: bool,
    label: &str,
    active_color: Color32,
) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.id().with(("dizmo-toggle", channel, kind)),
        Sense::click(),
    );
    let bg = if active { active_color } else { FIELD_BG };
    let border = if active { active_color } else { FIELD_BORDER };
    ui.painter().rect_filled(rect, 4.0, bg);
    ui.painter()
        .rect_stroke(rect, 4.0, Stroke::new(1.0, border), StrokeKind::Inside);
    let color = if active { Color32::WHITE } else { TEXT };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.0),
        color,
    );
    response
}

fn toggle_bool(setter: &ParamSetter, param: &BoolParam) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, !param.value());
    setter.end_set_parameter(param);
}
