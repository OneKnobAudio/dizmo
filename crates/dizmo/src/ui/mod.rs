//! The egui-based editor for DIZMO, matching `assets/MOCKUP.svg`.

use crate::KitStatus;
use crate::params::{DizmoParams, NUM_CHANNELS};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use egui_file_dialog::FileDialog;
use nice_plug::editor::dpi::{LogicalSize, PhysicalSize, Size};
use nice_plug::prelude::*;
use nice_plug_egui::{EguiNiceSettings, EguiState, create_egui_editor};
use std::any::Any;
use std::path::PathBuf;
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

/// GUI-only state that is not persisted (buffers for the editable text fields).
pub struct EditorState {
    pub params: Arc<DizmoParams>,
    /// Whether the pan knob is shown: the multi plugin routes each channel to its own output
    /// where pan has no effect, so it is hidden there.
    pub show_pan: bool,
    /// Dispatches a kit load to the loader thread and returns immediately.
    pub load_kit: Arc<dyn Fn(PathBuf) + Send + Sync>,
    /// Receives load results from the loader thread, polled each frame.
    pub status_rx: Option<crossbeam_channel::Receiver<KitStatus>>,
    /// What the header reports about the current kit.
    pub load_status: LoadStatus,
    /// The in-window kit picker.
    pub file_dialog: FileDialog,
}

/// A `FileDialog` configured for picking a DrumGizmo `drumkit.xml`.
fn kit_file_dialog() -> FileDialog {
    FileDialog::new()
        .id(egui::Id::new("dizmo-kit-picker"))
        .title("Load DrumGizmo kit")
        .add_file_filter_extensions("DrumGizmo kit", vec!["xml"])
        .default_file_filter("DrumGizmo kit")
        .as_modal(true)
        .default_size(vec2(720.0, 480.0))
        .min_size(vec2(480.0, 320.0))
        .resizable(true)
}

impl EditorState {
    pub fn new(
        params: Arc<DizmoParams>,
        show_pan: bool,
        load_kit: Arc<dyn Fn(PathBuf) + Send + Sync>,
        status_rx: Option<crossbeam_channel::Receiver<KitStatus>>,
    ) -> Self {
        Self {
            params,
            show_pan,
            load_kit,
            status_rx,
            load_status: LoadStatus::Idle,
            file_dialog: kit_file_dialog(),
        }
    }
}

/// The kit-load state shown in the editor header.
#[derive(Default)]
pub enum LoadStatus {
    #[default]
    Idle,
    /// Decoding in progress, as `(files_decoded, total_files)`.
    Loading {
        loaded: usize,
        total: usize,
    },
    Loaded {
        name: String,
        notes: Vec<Vec<u8>>,
        instruments: Vec<Option<String>>,
    },
    Failed(String),
}

/// The plugin editor: wraps the `nice-plug-egui` editor and hands it the parameter set.
pub struct DizmoEditor {
    inner: Box<dyn Editor>,
}

impl DizmoEditor {
    pub fn new(
        params: Arc<DizmoParams>,
        show_pan: bool,
        load_kit: Arc<dyn Fn(PathBuf) + Send + Sync>,
        status_rx: Option<crossbeam_channel::Receiver<KitStatus>>,
    ) -> Self {
        let egui_state = params.editor_state.clone();
        let editor_state = EditorState::new(params, show_pan, load_kit, status_rx);

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
        let (_tx, rx) = crossbeam_channel::unbounded();
        Self::new(
            Arc::new(DizmoParams::default()),
            true,
            Arc::new(|_| {}),
            Some(rx),
        )
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
    ui.set_min_width(420.0);
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

    // Separator
    ui.painter().rect_filled(
        Rect::from_min_size(
            pos2(rect.left() + 196.0, header.top() + 7.0),
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
        pos2(rect.left() + 210.0, header.top() + 10.0),
    );

    draw_load_kit(ui, state, rect);
}

/// A compact drag value for the number of visible channel strips.
fn draw_strips_count(ui: &mut Ui, setter: &ParamSetter, state: &mut EditorState, origin: Pos2) {
    ui.painter().text(
        origin,
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

/// The kit load button and current kit status at the right end of the header.
fn draw_load_kit(ui: &mut Ui, state: &mut EditorState, rect: Rect) {
    // Poll the loader for finished loads.
    if let Some(status_rx) = &state.status_rx {
        while let Ok(status) = status_rx.try_recv() {
            state.load_status = match status {
                KitStatus::Loaded {
                    name,
                    notes,
                    instruments,
                } => LoadStatus::Loaded {
                    name,
                    notes,
                    instruments,
                },
                KitStatus::Failed(message) => LoadStatus::Failed(message),
                KitStatus::Progress { loaded, total } => LoadStatus::Loading { loaded, total },
            };
        }
    }

    let center_y = rect.top() + HEADER_HEIGHT / 2.0;
    let button = Rect::from_min_size(
        pos2(rect.right() - 12.0 - 92.0, center_y - 11.0),
        vec2(92.0, 22.0),
    );

    let (label, tooltip) = match &state.load_status {
        LoadStatus::Idle => ("NO KIT LOADED".to_string(), None),
        LoadStatus::Loading { loaded, total } => (
            if *total > 0 {
                format!("LOADING… ({loaded}/{total})")
            } else {
                "LOADING…".to_string()
            },
            None,
        ),
        LoadStatus::Loaded { name, .. } => (name.clone(), Some(name.clone())),
        LoadStatus::Failed(message) => (message.clone(), Some(message.clone())),
    };
    let label_color = match &state.load_status {
        LoadStatus::Idle | LoadStatus::Loading { .. } => TEXT_DIM,
        LoadStatus::Loaded { .. } => TEXT,
        LoadStatus::Failed(_) => MUTE_ACTIVE,
    };
    draw_status_label(
        ui,
        label,
        FontId::proportional(9.0),
        label_color,
        button.left() - 10.0,
        center_y,
        tooltip,
    );

    let response = ui.interact(button, ui.id().with("dizmo-load-kit"), Sense::click());
    let clicked = response.clicked();
    let hovered = response.hovered();
    ui.painter()
        .rect_filled(button, 4.0, if hovered { FIELD_BG } else { TRACK_BG });
    ui.painter().rect_stroke(
        button,
        4.0,
        Stroke::new(1.0, if hovered { ACCENT } else { FIELD_BORDER }),
        StrokeKind::Inside,
    );
    ui.painter().text(
        button.center(),
        Align2::CENTER_CENTER,
        "LOAD KIT",
        FontId::proportional(10.0),
        TEXT,
    );
    let _ = response.on_hover_text("Load a DrumGizmo kit (drumkit.xml)");

    if clicked {
        state.file_dialog.pick_file();
    }

    state.file_dialog.update(ui.ctx());
    if let Some(path) = state.file_dialog.take_picked() {
        state.load_status = LoadStatus::Loading {
            loaded: 0,
            total: 0,
        };
        (state.load_kit)(path);
    }
}

/// Draws the kit status label, right-aligned ending at `right`, truncating it
/// with an ellipsis if it would run into the DIZMO logo. When `tooltip` is
/// set (loaded kit name or a load error) the full text is shown on hover.
fn draw_status_label(
    ui: &mut Ui,
    text: String,
    font_id: FontId,
    color: Color32,
    right: f32,
    center_y: f32,
    tooltip: Option<String>,
) {
    let max_width = (right - 170.0).max(250.0);
    let text = truncate_to_width(ui, text, &font_id, max_width);
    let galley = ui
        .ctx()
        .fonts_mut(|fonts| fonts.layout_no_wrap(text, font_id.clone(), color));
    let (width, height) = (galley.size().x, galley.size().y);
    ui.painter()
        .galley(pos2(right - width, center_y - height / 2.0), galley, color);

    if let Some(tooltip) = tooltip {
        let label_rect = Rect::from_min_max(
            pos2((right - width).max(0.0), center_y - 11.0),
            pos2(right, center_y + 11.0),
        );
        let response = ui.interact(
            label_rect,
            ui.id().with("dizmo-load-status"),
            Sense::hover(),
        );
        let _ = response.on_hover_text(tooltip);
    }
}

/// Truncates `text` to `max_width` pixels, appending an ellipsis.
fn truncate_to_width(ui: &Ui, text: String, font_id: &FontId, max_width: f32) -> String {
    let fits = |candidate: &str| {
        ui.ctx()
            .fonts_mut(|fonts| {
                fonts.layout_no_wrap(candidate.to_string(), font_id.clone(), Color32::WHITE)
            })
            .size()
            .x
            <= max_width
    };
    if fits(&text) {
        return text;
    }
    let mut chars: Vec<char> = text.chars().collect();
    while chars.len() > 1 {
        chars.pop();
        let candidate: String = chars.iter().chain(std::iter::once(&'…')).collect();
        if fits(&candidate) {
            return candidate;
        }
    }
    "…".to_string()
}
