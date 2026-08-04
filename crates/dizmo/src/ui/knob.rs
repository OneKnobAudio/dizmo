use crate::ui::{ACCENT, CARD_BG, FIELD_BG, INDICATOR, KNOB_BORDER};
use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use nice_plug::prelude::*;

/// Radius of the pan knob.
pub const KNOB_RADIUS: f32 = 16.0;

/// Draws a small rotary-style pan knob.
///
/// The indicator points up at center, to the right for right pan and to the left for left pan.
/// Dragging horizontally sets the pan value; double clicking resets it to center.
pub fn show_knob(ui: &mut Ui, setter: &ParamSetter, pan: &FloatParam, channel: usize, rect: Rect) {
    let center = rect.center();
    let response = ui.interact(
        rect,
        ui.id().with(("dizmo-knob", channel)),
        Sense::click_and_drag(),
    );

    if response.drag_started() {
        setter.begin_set_parameter(pan);
    }
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let value = ((pos.x - center.x) / KNOB_RADIUS).clamp(-1.0, 1.0);
        setter.set_parameter(pan, value);
    }
    if response.drag_stopped() {
        setter.end_set_parameter(pan);
    }
    if response.double_clicked() {
        setter.begin_set_parameter(pan);
        setter.set_parameter(pan, 0.0);
        setter.end_set_parameter(pan);
    }

    let pan_value = pan.value();

    let painter = ui.painter();
    painter.circle_stroke(center, KNOB_RADIUS, Stroke::new(1.0, KNOB_BORDER));
    painter.circle_filled(center, KNOB_RADIUS, FIELD_BG);
    painter.circle_filled(center, KNOB_RADIUS - 4.0, CARD_BG);

    let angle = pan_value * std::f32::consts::FRAC_PI_2;
    let dir = vec2(angle.sin(), -angle.cos());
    painter.line_segment(
        [center, center + dir * (KNOB_RADIUS - 5.0)],
        Stroke::new(2.5, INDICATOR),
    );

    if response.hovered() {
        painter.text(
            pos2(center.x, rect.bottom() + 8.0),
            Align2::CENTER_CENTER,
            format!("{:.2}", pan_value),
            egui::FontId::proportional(8.0),
            ACCENT,
        );
    }
}
