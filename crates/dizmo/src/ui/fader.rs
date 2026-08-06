use crate::ui::{ACCENT, CARD_BG, INDICATOR, KNOB_BORDER, TEXT, TEXT_DIM, TRACK_BG};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use nice_plug::prelude::*;

/// The fader range in dB: minimum (fully down) and maximum (fully up).
pub const FADER_MIN_DB: f32 = -18.0;
pub const FADER_MAX_DB: f32 = 6.0;

/// A peak level (linear 0..1) at or above which the signal LED lights up.
const LED_THRESHOLD: f32 = 0.0001;

/// Draws a vertical volume fader for one channel.
///
/// * 0 dB sits at the vertical center of the track.
/// * Dragging vertically sets the fader value; double clicking resets it to 0 dB.
/// * The filled portion runs from the bottom of the track up to the fader position.
/// * `level` (linear peak 0..1) drives a small signal LED at the top of the track.
pub fn show_fader(
    ui: &mut Ui,
    setter: &ParamSetter,
    fader: &FloatParam,
    channel: usize,
    rect: Rect,
    level: f32,
) {
    let response = ui.interact(
        rect,
        ui.id().with(("dizmo-fader", channel)),
        Sense::click_and_drag(),
    );

    let track_width = 10.0;
    let track = Rect::from_min_max(
        pos2(rect.center().x - track_width / 2.0, rect.top() + 24.0),
        pos2(rect.center().x + track_width / 2.0, rect.bottom() - 16.0),
    );

    if response.drag_started() {
        setter.begin_set_parameter(fader);
    }
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let span = FADER_MAX_DB - FADER_MIN_DB;
        let db = FADER_MAX_DB - (pos.y - track.top()) / track.height() * span;
        setter.set_parameter(
            fader,
            util::db_to_gain(db.clamp(FADER_MIN_DB, FADER_MAX_DB)),
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
    let span = FADER_MAX_DB - FADER_MIN_DB;
    let normalized = ((db - FADER_MIN_DB) / span).clamp(0.0, 1.0);
    let handle_y = track.bottom() - normalized * track.height();
    let zero_y = track.bottom() - (0.0 - FADER_MIN_DB) / span * track.height();

    let painter = ui.painter();
    let label_x = track.left() - 6.0;

    // Scale labels: +6 / 0 / -18
    for (fraction, label) in [
        (1.0, "+6"),
        ((0.0 - FADER_MIN_DB) / span, "0"),
        (0.0, "-18"),
    ] {
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

    // Signal LED: lights up when the channel is receiving audio, brighter with level
    let led_center = pos2(rect.center().x, track.top() - 12.0);
    let lit = level >= LED_THRESHOLD;
    let led_color = if lit {
        let intensity = (level * 3.0).clamp(0.0, 1.0);
        Color32::from_rgb(
            (intensity * 255.0) as u8,
            (intensity * 230.0) as u8,
            (intensity * 120.0) as u8,
        )
    } else {
        Color32::from_rgb(40, 40, 40)
    };
    painter.circle_filled(led_center, 3.5, led_color);
    painter.circle_stroke(led_center, 3.5, Stroke::new(1.0, KNOB_BORDER));

    // Current value readout below the LED
    let value_text = if db.abs() < 0.05 {
        "0.0 dB".to_string()
    } else {
        format!("{db:+.1} dB")
    };
    painter.text(
        pos2(rect.center().x, track.top() - 20.0),
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
