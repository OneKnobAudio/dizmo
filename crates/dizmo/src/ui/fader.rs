use crate::ui::{ACCENT, CARD_BG, INDICATOR, KNOB_BORDER, TEXT, TEXT_DIM, TRACK_BG};
use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use nice_plug::prelude::*;

/// The fader range in dB above and below unity.
pub const FADER_RANGE_DB: f32 = 12.0;

/// Draws a vertical volume fader for one channel.
///
/// * 0 dB sits at the vertical center of the track.
/// * Dragging vertically sets the fader value; double clicking resets it to 0 dB.
/// * The filled portion runs from the bottom of the track up to the fader position.
pub fn show_fader(
    ui: &mut Ui,
    setter: &ParamSetter,
    fader: &FloatParam,
    channel: usize,
    rect: Rect,
) {
    let response = ui.interact(
        rect,
        ui.id().with(("dizmo-fader", channel)),
        Sense::click_and_drag(),
    );

    let track_width = 10.0;
    let track = Rect::from_min_max(
        pos2(rect.center().x - track_width / 2.0, rect.top() + 16.0),
        pos2(rect.center().x + track_width / 2.0, rect.bottom() - 16.0),
    );

    if response.drag_started() {
        setter.begin_set_parameter(fader);
    }
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let db = FADER_RANGE_DB - (pos.y - track.top()) / track.height() * 2.0 * FADER_RANGE_DB;
        setter.set_parameter(
            fader,
            util::db_to_gain(db.clamp(-FADER_RANGE_DB, FADER_RANGE_DB)),
        );
    }
    if response.drag_stopped() {
        setter.end_set_parameter(fader);
    }
    if response.double_clicked() {
        setter.begin_set_parameter(fader);
        setter.set_parameter(fader, util::db_to_gain(0.0));
        setter.end_set_parameter(fader);
    }

    let db = util::gain_to_db(fader.value());
    let normalized = ((db + FADER_RANGE_DB) / (2.0 * FADER_RANGE_DB)).clamp(0.0, 1.0);
    let handle_y = track.bottom() - normalized * track.height();
    let zero_y = track.bottom() - 0.5 * track.height();

    let painter = ui.painter();
    let label_x = track.left() - 6.0;

    // Scale labels: +12 / 0 / -12
    for (fraction, label) in [(1.0, "+12"), (0.5, "0"), (0.0, "-12")] {
        let y = track.bottom() - fraction * track.height();
        painter.text(
            pos2(label_x, y),
            Align2::RIGHT_CENTER,
            label,
            FontId::proportional(7.0),
            TEXT_DIM,
        );
    }

    // Track and zero tick
    painter.rect_filled(track, 5.0, TRACK_BG);
    painter.rect_filled(
        Rect::from_min_size(pos2(track.left() - 5.0, zero_y - 0.5), vec2(3.0, 1.0)),
        0.0,
        KNOB_BORDER,
    );

    // Fill from the bottom of the track up to the fader position
    let fill = Rect::from_min_max(pos2(track.left(), handle_y), track.right_bottom());
    if fill.height() > 0.0 {
        painter.rect_filled(fill, 5.0, ACCENT);
    }

    // Handle cap with a small tick
    let cap = Rect::from_center_size(pos2(rect.center().x, handle_y), vec2(32.0, 12.0));
    painter.rect_filled(cap, 3.0, INDICATOR);
    painter.line_segment(
        [
            pos2(cap.center().x, cap.top()),
            pos2(cap.center().x, cap.bottom()),
        ],
        Stroke::new(1.5, CARD_BG),
    );

    // Current value readout above the track
    let value_text = if db.abs() < 0.05 {
        "0.0 dB".to_string()
    } else {
        format!("{db:+.1} dB")
    };
    painter.text(
        pos2(rect.center().x, track.top() - 8.0),
        Align2::CENTER_CENTER,
        value_text,
        FontId::proportional(8.0),
        TEXT,
    );

    // Zero line
    painter.line_segment(
        [
            pos2(track.left() - 2.0, zero_y),
            pos2(track.right() + 2.0, zero_y),
        ],
        Stroke::new(1.0, ACCENT),
    );
}
