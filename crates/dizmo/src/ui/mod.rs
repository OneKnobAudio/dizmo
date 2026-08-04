//! The egui-based editor for DIZMO, matching `assets/MOCKUP.svg`.

use crate::params::{DizmoParams, NUM_CHANNELS};
use crate::state::OutputMode;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use nice_plug::editor::dpi::{LogicalSize, PhysicalSize, Size};
use nice_plug::formatters;
use nice_plug::prelude::*;
use nice_plug_egui::{EguiNiceSettings, EguiState, create_egui_editor};
use std::any::Any;
use std::sync::Arc;

pub mod channel_strip;
pub mod fader;
pub mod knob;

// --- Mockup palette ---------------------------------------------------------

pub(crate) const BG: Color32 = Color32::from_rgb(0x14, 0x15, 0x18);
pub(crate) const HEADER_BG: Color32 = Color32::from_rgb(0x1c, 0x1e, 0x23);
pub(crate) const HEADER_BORDER: Color32 = Color32::from_rgb(0x26, 0x28, 0x2d);
pub(crate) const CARD_BG: Color32 = Color32::from_rgb(0x1d, 0x1f, 0x24);
pub(crate) const CARD_BORDER: Color32 = Color32::from_rgb(0x2d, 0x31, 0x38);
pub(crate) const FIELD_BG: Color32 = Color32::from_rgb(0x23, 0x26, 0x2c);
pub(crate) const FIELD_BORDER: Color32 = Color32::from_rgb(0x3a, 0x3f, 0x48);
pub(crate) const TRACK_BG: Color32 = Color32::from_rgb(0x23, 0x26, 0x2c);
pub(crate) const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf4, 0xf7);
pub(crate) const TEXT_DIM: Color32 = Color32::from_rgb(0x8b, 0x90, 0x99);
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x55, 0x84, 0xc8);
pub(crate) const ACCENT_BORDER: Color32 = Color32::from_rgb(0x3f, 0x6f, 0xb5);
pub(crate) const NOTE_BG: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x23);
pub(crate) const NOTE_TEXT: Color32 = Color32::from_rgb(0xe6, 0xed, 0xf7);
pub(crate) const KNOB_BORDER: Color32 = Color32::from_rgb(0x3a, 0x3f, 0x48);
pub(crate) const INDICATOR: Color32 = Color32::from_rgb(0xe8, 0xec, 0xf1);
pub(crate) const SOLO_ACTIVE: Color32 = Color32::from_rgb(0x9a, 0x8a, 0x3c);
pub(crate) const MUTE_ACTIVE: Color32 = Color32::from_rgb(0xb0, 0x4a, 0x46);

/// The header bar height in the mockup.
const HEADER_HEIGHT: f32 = 42.0;

/// The default editor window size in logical pixels.
const WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(1240.0, 560.0);

/// Persistent editor state used to restore the window size.
pub fn default_editor_state() -> Arc<EguiState> {
    EguiState::from_size(WINDOW_SIZE)
}

/// GUI-only state that is not persisted (buffers for the editable text fields and the active
/// choke-assign target).
pub struct EditorState {
    pub params: Arc<DizmoParams>,
    pub name_buffers: Vec<String>,
    pub note_buffers: Vec<String>,
    pub choke_assign: Option<usize>,
}

impl EditorState {
    pub fn new(params: Arc<DizmoParams>) -> Self {
        let name_buffers = {
            let names = params.channel_names.lock().unwrap();
            names.to_vec()
        };
        let note_buffers = params
            .channels
            .iter()
            .map(|channel| (formatters::v2s_i32_note_formatter())(channel.note.value()))
            .collect();
        Self {
            params,
            name_buffers,
            note_buffers,
            choke_assign: None,
        }
    }
}

/// The plugin editor: wraps the `nice-plug-egui` editor and hands it the parameter set.
pub struct DizmoEditor {
    inner: Box<dyn Editor>,
}

impl DizmoEditor {
    pub fn new(params: Arc<DizmoParams>) -> Self {
        let egui_state = params.editor_state.clone();
        let editor_state = EditorState::new(params);

        let inner = create_egui_editor(
            egui_state,
            editor_state,
            EguiNiceSettings::new().with_tile("DIZMO"),
            |ctx, _commands, _state| {
                ctx.set_visuals(egui::Visuals::dark());
            },
            |ui, setter, _commands, state| {
                draw_ui(ui, setter, state);
            },
        )
        .expect("Failed to create the DIZMO editor");

        Self { inner }
    }
}

impl Default for DizmoEditor {
    fn default() -> Self {
        Self::new(Arc::new(DizmoParams::default()))
    }
}

impl Editor for DizmoEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn Any> {
        self.inner.spawn(parent, context)
    }

    fn size(&self) -> Size {
        self.inner.size()
    }

    fn param_value_changed(&self, id: &str, normalized_value: f32) {
        self.inner.param_value_changed(id, normalized_value);
    }

    fn param_modulation_changed(&self, id: &str, modulation_offset: f32) {
        self.inner.param_modulation_changed(id, modulation_offset);
    }

    fn param_values_changed(&self) {
        self.inner.param_values_changed();
    }

    fn on_virtual_key_from_host(
        &self,
        key_code: VirtualKeyCode,
        is_down: bool,
        modifiers: Modifiers,
    ) -> bool {
        self.inner
            .on_virtual_key_from_host(key_code, is_down, modifiers)
    }

    fn set_size(&self, physical_size: PhysicalSize<u32>) -> bool {
        self.inner.set_size(physical_size)
    }

    fn set_scale_factor(&self, factor: f64) -> bool {
        self.inner.set_scale_factor(factor)
    }

    fn resize_hint(&self) -> ResizeHint {
        self.inner.resize_hint()
    }
}

/// Draws the complete editor: header bar and the scrollable channel strip area.
fn draw_ui(ui: &mut Ui, setter: &ParamSetter, state: &mut EditorState) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, BG);

    draw_header(ui, setter, state, rect);

    // Advance the cursor below the header so the scroll area covers the rest.
    ui.allocate_exact_size(vec2(rect.width(), HEADER_HEIGHT + 6.0), Sense::hover());

    let num_strips = state
        .params
        .num_strips
        .value()
        .clamp(1, NUM_CHANNELS as i32) as usize;

    let content_rect = Rect::from_min_max(
        pos2(rect.left(), rect.top() + HEADER_HEIGHT + 6.0),
        pos2(rect.right(), rect.bottom()),
    );

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(content_rect.width() - 12.0);
            ui.set_min_height(content_rect.height() - 12.0);
            ui.horizontal_top(|ui| {
                ui.add_space(6.0);
                for index in 0..num_strips {
                    channel_strip::draw_strip(ui, setter, state, index);
                    ui.add_space(8.0);
                }
            });
        });
}

fn draw_header(ui: &mut Ui, setter: &ParamSetter, state: &mut EditorState, rect: Rect) {
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_HEIGHT));
    let center_y = header.center().y;

    ui.painter().rect_filled(header, 0.0, HEADER_BG);
    ui.painter().rect_filled(
        Rect::from_min_max(
            pos2(rect.left(), header.bottom()),
            pos2(rect.right(), header.bottom() + 1.0),
        ),
        0.0,
        HEADER_BORDER,
    );

    // DIZMO logo
    ui.painter().text(
        pos2(rect.left() + 20.0, center_y),
        Align2::LEFT_CENTER,
        "DIZMO",
        FontId::proportional(16.0),
        TEXT,
    );

    // CHANNELS section label
    ui.painter().text(
        pos2(rect.left() + 112.0, center_y + 1.0),
        Align2::LEFT_CENTER,
        "CHANNELS",
        FontId::proportional(11.0),
        TEXT_DIM,
    );

    // STEREO / MULTI output mode selector
    let mode_rect = Rect::from_min_size(
        pos2(rect.left() + 196.0, header.top() + 10.0),
        vec2(150.0, 22.0),
    );
    draw_mode_selector(ui, setter, state, mode_rect);

    // Separator
    ui.painter().rect_filled(
        Rect::from_min_size(
            pos2(rect.left() + 362.0, header.top() + 7.0),
            vec2(6.0, 28.0),
        ),
        3.0,
        INDICATOR,
    );

    // Number of visible channel strips
    draw_strips_count(
        ui,
        setter,
        state,
        pos2(rect.left() + 376.0, header.top() + 10.0),
    );

    // Choke-assign mode hint
    if let Some(victim) = state.choke_assign {
        ui.painter().text(
            pos2(rect.left() + 560.0, center_y),
            Align2::LEFT_CENTER,
            format!("CHOKE ASSIGN: channel {} · click strips to toggle chokers · double-click CHOKE to exit", victim + 1),
            FontId::proportional(10.0),
            ACCENT,
        );
    }
}

/// The STEREO / MULTI output mode selector.
fn draw_mode_selector(ui: &mut Ui, setter: &ParamSetter, state: &mut EditorState, rect: Rect) {
    let options = OutputMode::variants();
    let active = state.params.output_mode.value();
    let active_index = OutputMode::to_index(active);

    let segment_width = rect.width() / options.len() as f32;
    for (index, label) in options.iter().enumerate() {
        let segment = Rect::from_min_size(
            pos2(rect.left() + index as f32 * segment_width, rect.top()),
            vec2(segment_width, rect.height()),
        );
        let is_active = index == active_index;
        let response = ui.interact(segment, ui.id().with(("dizmo-mode", index)), Sense::click());

        if is_active {
            ui.painter().rect_filled(segment, 11.0, ACCENT);
        }
        ui.painter().text(
            segment.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(9.0),
            if is_active { Color32::WHITE } else { TEXT_DIM },
        );

        if index + 1 < options.len() {
            ui.painter().line_segment(
                [
                    pos2(segment.right(), segment.top() + 4.0),
                    pos2(segment.right(), segment.bottom() - 4.0),
                ],
                Stroke::new(1.0, HEADER_BORDER),
            );
        }

        if response.clicked() && !is_active {
            setter.begin_set_parameter(&state.params.output_mode);
            setter.set_parameter(&state.params.output_mode, OutputMode::from_index(index));
            setter.end_set_parameter(&state.params.output_mode);
        }
    }

    ui.painter().rect_stroke(
        rect,
        11.0,
        Stroke::new(1.0, KNOB_BORDER),
        StrokeKind::Inside,
    );
}

/// A compact drag value for the number of visible channel strips.
fn draw_strips_count(ui: &mut Ui, setter: &ParamSetter, state: &mut EditorState, origin: Pos2) {
    let label_pos = origin;
    ui.painter().text(
        label_pos,
        Align2::LEFT_CENTER,
        "STRIPS",
        FontId::proportional(9.0),
        TEXT_DIM,
    );

    let pill = Rect::from_min_size(pos2(origin.x + 44.0, origin.y), vec2(40.0, 22.0));
    let response = ui.interact(
        pill,
        ui.id().with("dizmo-num-strips"),
        Sense::click_and_drag(),
    );

    if response.drag_started() {
        setter.begin_set_parameter(&state.params.num_strips);
    }
    if response.dragged() {
        let delta = response.drag_delta().x;
        let current = state.params.num_strips.value();
        let value = (current as f32 + delta * 0.2)
            .round()
            .clamp(1.0, NUM_CHANNELS as f32) as i32;
        if value != current {
            setter.set_parameter(&state.params.num_strips, value);
        }
    }
    if response.drag_stopped() {
        setter.end_set_parameter(&state.params.num_strips);
    }
    if response.double_clicked() {
        setter.begin_set_parameter(&state.params.num_strips);
        setter.set_parameter(&state.params.num_strips, NUM_CHANNELS as i32);
        setter.end_set_parameter(&state.params.num_strips);
    }

    ui.painter().rect_filled(pill, 4.0, FIELD_BG);
    ui.painter().rect_stroke(
        pill,
        4.0,
        Stroke::new(1.0, FIELD_BORDER),
        StrokeKind::Inside,
    );
    ui.painter().text(
        pill.center(),
        Align2::CENTER_CENTER,
        format!("{}", state.params.num_strips.value()),
        FontId::proportional(10.0),
        TEXT,
    );
    let _ = response
        .on_hover_text("Number of visible channel strips · drag to change, double-click to reset");
}
