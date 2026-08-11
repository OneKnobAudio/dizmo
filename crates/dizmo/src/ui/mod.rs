//! The iced-based editor for DIZMO, matching `assets/MOCKUP.svg`.

use crate::params::{DizmoParams, NUM_CHANNELS};
use crate::{KitInfo, KitStatus};
use iced_audio::Gesture;
use nice_plug::editor::dpi::{LogicalSize, PhysicalSize, Size};
use nice_plug::prelude::*;
use nice_plug_iced::iced::core::border::Radius;
use nice_plug_iced::iced::widget::scrollable::{Direction, Scrollbar};
use nice_plug_iced::iced::widget::{
    Button, Space, button, column, container, mouse_area, row, scrollable, text,
};
use nice_plug_iced::iced::{
    self, Alignment, Background, Border, Color, Element, Length, PollSubNotifier,
};
use nice_plug_iced::{
    EditorSettings, EditorState as IcedEditorState, NiceGuiContext, WindowState, create_iced_editor,
};
use std::any::Any;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced_futures::backend::default::time::every;

pub mod browser;
pub mod channel_strip;
pub mod fader;
pub mod gesture_drag;
pub mod knob;
pub mod peak_meter;

// --- Mockup palette ---------------------------------------------------------

pub(crate) const BG: Color = Color::from_rgb8(0x14, 0x15, 0x18);
pub(crate) const HEADER_BG: Color = Color::from_rgb8(0x1c, 0x1e, 0x23);
pub(crate) const HEADER_BORDER: Color = Color::from_rgb8(0x26, 0x28, 0x2d);
pub(crate) const CARD_BG: Color = Color::from_rgb8(0x1d, 0x1f, 0x24);
pub(crate) const CARD_BORDER: Color = Color::from_rgb8(0x2d, 0x31, 0x38);
pub(crate) const FIELD_BG: Color = Color::from_rgb8(0x23, 0x26, 0x2c);
pub(crate) const FIELD_BORDER: Color = Color::from_rgb8(0x3a, 0x3f, 0x48);
pub(crate) const FIELD_HOVER: Color = Color::from_rgb8(0x2a, 0x2e, 0x36);
pub(crate) const TRACK_BG: Color = Color::from_rgb8(0x23, 0x26, 0x2c);
pub(crate) const TEXT: Color = Color::from_rgb8(0xf2, 0xf4, 0xf7);
pub(crate) const TEXT_DIM: Color = Color::from_rgb8(0x8b, 0x90, 0x99);
pub(crate) const ACCENT: Color = Color::from_rgb8(0x55, 0x84, 0xc8);
pub(crate) const INDICATOR: Color = Color::from_rgb8(0xe8, 0xec, 0xf1);
pub(crate) const SOLO_ACTIVE: Color = Color::from_rgb8(0x9a, 0x8a, 0x3c);
pub(crate) const MUTE_ACTIVE: Color = Color::from_rgb8(0xb0, 0x4a, 0x46);
/// The peak meter ladder: green below -12 dB, yellow up to -6 dB, red above.
pub(crate) const METER_GREEN: Color = Color::from_rgb8(0x3f, 0xc2, 0x63);
pub(crate) const METER_YELLOW: Color = Color::from_rgb8(0xea, 0xc0, 0x3c);
pub(crate) const METER_RED: Color = Color::from_rgb8(0xd9, 0x54, 0x4f);
/// The resting fill of an unlit meter segment, visible against the card.
pub(crate) const METER_OFF: Color = Color::from_rgb8(0x2a, 0x2d, 0x33);
/// The peak-hold cap drawn on top of the lit meter segments.
pub(crate) const PEAK_HOLD_COLOR: Color = Color::from_rgb8(0xf2, 0xf4, 0xf7);

/// The header bar height in the mockup.
const HEADER_HEIGHT: f32 = 42.0;

/// The default editor window size in logical pixels.
const WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(1240.0, 560.0);

/// How often the ticker fires, driving the meter and LED animations.
const TICK_INTERVAL: Duration = Duration::from_millis(40);

/// Counts editor spawns so the debug prints can pair each open with its drop.
static GUI_INSTANCES: AtomicUsize = AtomicUsize::new(0);

/// How long the per-channel peak-hold cap takes to fall after the signal
/// drops, so the meter's cap lingers briefly on each hit. Kept short so a
/// fader / mute / solo change is visibly reflected in the peak almost at once,
/// rather than the cap sitting on the old post-fader level for a full second.
const PEAK_HOLD_RELEASE: Duration = Duration::from_millis(250);

/// Persistent editor state used to restore the window size.
pub fn default_editor_state() -> Arc<WindowState> {
    WindowState::from_size(WINDOW_SIZE)
}

/// GUI-only state that persists between editor opens: the things the editor
/// needs to talk to the plugin. All GUI-only state lives in [`MyGui`].
pub struct EditorState {
    pub params: Arc<DizmoParams>,
    /// Whether the pan knob is shown: the multi plugin routes each channel to
    /// its own output where pan has no effect, so it is hidden there.
    pub show_pan: bool,
    /// Dispatches a kit load to the loader thread and returns immediately.
    pub load_kit: Arc<dyn Fn(PathBuf) + Send + Sync>,
    /// Receives load results from the loader thread, polled each frame.
    pub status_rx: Option<crossbeam_channel::Receiver<KitStatus>>,
    /// The most recently loaded kit, shared with the plugin. The editor seeds
    /// its header from here at boot, so a reopened window shows the kit that
    /// is already loaded.
    pub kit_info: Arc<Mutex<Option<KitInfo>>>,
    /// Per-channel linear peak levels (as `f32` bits), written by the audio
    /// thread each block and read by the strips to light their peak meters.
    pub levels: Arc<[AtomicU32; NUM_CHANNELS]>,
}

impl EditorState {
    pub fn new(
        params: Arc<DizmoParams>,
        show_pan: bool,
        load_kit: Arc<dyn Fn(PathBuf) + Send + Sync>,
        status_rx: Option<crossbeam_channel::Receiver<KitStatus>>,
        kit_info: Arc<Mutex<Option<KitInfo>>>,
        levels: Arc<[AtomicU32; NUM_CHANNELS]>,
    ) -> Self {
        Self {
            params,
            show_pan,
            load_kit,
            status_rx,
            kit_info,
            levels,
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
        /// The kit's channel names from its `<channels>` section.
        channels: Vec<String>,
        /// The per-instrument MIDI note and channel mappings for the dialog.
        mappings: Vec<crate::engine::InstrumentMapping>,
    },
    Failed(String),
}

/// The messages handled by the editor program.
#[derive(Debug, Clone, Copy)]
pub enum Message {
    /// A tick from the 40 ms timer, driving the meter and LED animations.
    Tick,
    /// The host resized the editor view; carries the new logical size.
    Resized(iced::Size),
    /// The audio thread set a param or lit an indicator; poll for new state.
    Poll,
    FaderGesture(usize, Gesture),
    PanGesture(usize, Gesture),
    ToggleSolo(usize),
    ToggleMute(usize),
    OpenBrowser,
    BrowserUp,
    BrowserHome,
    BrowserClose,
    BrowserEntry(usize),
    ToggleMappings,
    DismissError,
    DismissWarning,
}

/// The program state for the iced editor.
pub struct DizmoGui {
    /// The persistent editor state.
    pub editor_state: IcedEditorState<EditorState>,
    /// For sending parameter changes to the host.
    pub nice_ctx: NiceGuiContext,
    /// What the header reports about the current kit.
    pub load_status: LoadStatus,
    /// Whether the Mappings dialog is currently open.
    pub show_mappings: bool,
    /// A load error to display in a modal dialog, if any.
    pub show_error: Option<String>,
    /// A non-fatal load warning to display in a modal dialog, if any.
    pub show_warning: Option<String>,
    /// The in-window kit picker.
    pub browser: browser::Browser,
    /// The peak-hold cap per channel (linear 0..1): the highest level seen
    /// recently, decaying toward the live level each tick.
    pub peak_hold: [f32; NUM_CHANNELS],
    /// The current logical window size, driven by host resize events. Used to
    /// compute the uniform UI zoom; see [`DizmoGui::scale`].
    pub ui_size: iced::Size,
    /// Sequence number of this editor instance, for pairing open/drop debug prints.
    pub instance: usize,
}

impl Drop for DizmoGui {
    fn drop(&mut self) {
        eprintln!("[dizmo] editor DROPPED #{}", self.instance);
    }
}

impl DizmoGui {
    fn new(editor_state: IcedEditorState<EditorState>, nice_ctx: NiceGuiContext) -> Self {
        let instance = GUI_INSTANCES.fetch_add(1, Ordering::SeqCst) + 1;
        eprintln!("[dizmo] editor OPEN #{instance}");
        // Seed the header with whatever kit is already loaded, so reopening the
        // window does not lose the current kit.
        let load_status = match &*editor_state
            .kit_info
            .lock()
            .expect("kit info mutex poisoned")
        {
            Some(info) => LoadStatus::Loaded {
                name: info.name.clone(),
                channels: info.channels.clone(),
                mappings: info.mappings.clone(),
            },
            None => LoadStatus::Idle,
        };
        Self {
            editor_state,
            nice_ctx,
            load_status,
            show_mappings: false,
            show_error: None,
            show_warning: None,
            browser: browser::Browser::new(),
            peak_hold: [0.0; NUM_CHANNELS],
            ui_size: iced::Size::new(WINDOW_SIZE.width, WINDOW_SIZE.height),
            instance,
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Tick => {
                self.decay_peak_hold();
                self.poll_status();
            }
            Message::Resized(size) => {
                self.ui_size = size;

                eprintln!(
                    "RESIZED: {} x {}, scale = {}",
                    size.width,
                    size.height,
                    self.scale()
                );
            }
            Message::Poll => self.poll_status(),
            Message::FaderGesture(channel, gesture) => {
                let param = &self.editor_state.params.channels[channel].fader;
                set_nice_param(param, gesture, &self.nice_ctx.param_setter());
            }
            Message::PanGesture(channel, gesture) => {
                let param = &self.editor_state.params.channels[channel].pan;
                set_nice_param(param, gesture, &self.nice_ctx.param_setter());
            }
            Message::ToggleSolo(channel) => {
                let setter = self.nice_ctx.param_setter();
                let solo = &self.editor_state.params.channels[channel].solo;
                setter.set_parameter(solo, !solo.value());
            }
            Message::ToggleMute(channel) => {
                let setter = self.nice_ctx.param_setter();
                let mute = &self.editor_state.params.channels[channel].mute;
                setter.set_parameter(mute, !mute.value());
            }
            Message::OpenBrowser => self.browser.open(),
            Message::BrowserUp => self.browser.up(),
            Message::BrowserHome => self.browser.home(),
            Message::BrowserClose => self.browser.close(),
            Message::BrowserEntry(index) => {
                if let Some(path) = self.browser.open_entry(index) {
                    self.browser.close();
                    self.load_status = LoadStatus::Loading {
                        loaded: 0,
                        total: 0,
                    };
                    (self.editor_state.load_kit)(path);
                }
            }
            Message::ToggleMappings => self.show_mappings = !self.show_mappings,
            Message::DismissError => self.show_error = None,
            Message::DismissWarning => self.show_warning = None,
        }
    }

    /// Captures the current level per channel and decays the peak-hold cap
    /// toward it, so the meter's cap lingers on a hit and then falls slowly.
    fn decay_peak_hold(&mut self) {
        let decay = (-TICK_INTERVAL.as_secs_f32() / PEAK_HOLD_RELEASE.as_secs_f32()).exp();
        for (index, hold) in self.peak_hold.iter_mut().enumerate() {
            let level = f32::from_bits(self.editor_state.levels[index].load(Ordering::Relaxed));
            *hold = level.max(*hold * decay);
        }
    }

    /// Drains the kit-load status channel, updating the header and opening any
    /// error / warning dialogs.
    fn poll_status(&mut self) {
        let rx = match &self.editor_state.status_rx {
            Some(rx) => rx.clone(),
            None => return,
        };
        while let Ok(status) = rx.try_recv() {
            self.load_status = match status {
                KitStatus::Loaded {
                    name,
                    channels,
                    mappings,
                    warnings,
                } => {
                    if !warnings.is_empty() {
                        self.show_warning = Some(warnings.join("\n"));
                    }
                    LoadStatus::Loaded {
                        name,
                        channels,
                        mappings,
                    }
                }
                KitStatus::Failed(message) => {
                    self.show_error = Some(message.clone());
                    LoadStatus::Failed(message)
                }
                KitStatus::Progress { loaded, total } => LoadStatus::Loading { loaded, total },
            };
        }
    }

    /// The uniform zoom applied to the whole UI, anchored so the mockup is
    /// pixel-identical at the default window size. A larger window zooms the
    /// UI in; a wider window reveals more strips in the scrollable.
    fn scale(&self) -> f32 {
        (self.ui_size.width / WINDOW_SIZE.width)
            .min(self.ui_size.height / WINDOW_SIZE.height)
            .clamp(0.5, 2.0)
    }

    fn view(&self) -> Element<'_, Message> {
        let scale = self.scale();
        let mut strips = iced::widget::row![].spacing(8.0 * scale);
        for index in 0..NUM_CHANNELS {
            strips = strips.push(channel_strip::draw_strip(self, index));
        }

        let scroll = scrollable(container(strips).padding(6.0 * scale))
            .direction(Direction::Both {
                vertical: Scrollbar::new(),
                horizontal: Scrollbar::new(),
            })
            .width(Length::Fill)
            .height(Length::Fill);

        let base = container(column![self.header(), scroll].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(background_style);

        let base: Element<'_, Message> = base.into();

        if self.browser.is_open() {
            return iced::widget::Stack::with_children([
                base,
                browser::view(&self.browser, scale).into(),
            ])
            .into();
        }
        if self.show_mappings {
            return iced::widget::Stack::with_children([base, mappings_dialog(self).into()]).into();
        }
        if let Some(message) = &self.show_error {
            return iced::widget::Stack::with_children([
                base,
                modal_dialog("Error loading kit", message, Message::DismissError, scale).into(),
            ])
            .into();
        }
        if let Some(message) = &self.show_warning {
            return iced::widget::Stack::with_children([
                base,
                modal_dialog(
                    "Kit loaded with warnings",
                    message,
                    Message::DismissWarning,
                    scale,
                )
                .into(),
            ])
            .into();
        }
        base
    }

    /// The header bar: logo, section label, separator, Mappings button and the
    /// kit status / LOAD KIT button at the right end.
    fn header(&self) -> iced::widget::Container<'_, Message> {
        let s = self.scale();
        let mut header_row = row![
            text("DIZMO").size(16.0 * s).color(TEXT),
            text("CHANNELS").size(11.0 * s).color(TEXT_DIM),
            container(Space::new().width(6.0 * s).height(28.0 * s)).style(separator_style),
        ]
        .align_y(Alignment::Center)
        .spacing(16.0 * s)
        .padding([0.0, 20.0 * s]);

        header_row = header_row.push(self.mappings_button());
        header_row = header_row.push(Space::new().width(Length::Fill));

        let (label, color) = match &self.load_status {
            LoadStatus::Idle => ("NO KIT LOADED".to_string(), TEXT_DIM),
            LoadStatus::Loading { loaded, total } => (
                if *total > 0 {
                    format!("LOADING… ({loaded}/{total})")
                } else {
                    "LOADING…".to_string()
                },
                TEXT_DIM,
            ),
            LoadStatus::Loaded { name, .. } => (name.clone(), TEXT),
            LoadStatus::Failed(message) => (message.clone(), MUTE_ACTIVE),
        };
        header_row = header_row.push(text(label).size(9.0 * s).color(color));
        header_row = header_row.push(self.load_kit_button());

        container(header_row)
            .width(Length::Fill)
            .height(HEADER_HEIGHT * s)
            .style(header_style)
    }

    /// The Mappings button that opens the MIDI map / channel assignment dialog.
    fn mappings_button(&self) -> Button<'_, Message> {
        let s = self.scale();
        let active = self.show_mappings;
        button(text("MAPPINGS").size(9.0 * s).color(TEXT))
            .on_press(Message::ToggleMappings)
            .width(Length::Fixed(76.0 * s))
            .style(move |_theme, status| pill_button_style(active, status))
    }

    /// The LOAD KIT button at the right end of the header.
    fn load_kit_button(&self) -> Button<'_, Message> {
        let s = self.scale();
        button(text("LOAD KIT").size(10.0 * s).color(TEXT))
            .on_press(Message::OpenBrowser)
            .width(Length::Fixed(92.0 * s))
            .style(load_kit_button_style)
    }
}

/// Free function view so `nice_plug_iced::application` gets a higher-ranked
/// (for-any-lifetime) `ViewFn` rather than a closure with a fixed lifetime.
fn view(state: &DizmoGui) -> Element<'_, Message> {
    state.view()
}

/// Free function theme, for the same higher-ranked lifetime reason.
fn theme(_state: &DizmoGui) -> iced::Theme {
    iced::Theme::custom(
        "DIZMO",
        iced::theme::Palette {
            background: BG,
            text: TEXT,
            primary: ACCENT,
            success: INDICATOR,
            warning: SOLO_ACTIVE,
            danger: MUTE_ACTIVE,
        },
    )
}

/// The plugin editor: wraps the `nice-plug-iced` editor and hands it the
/// parameter set.
pub struct DizmoEditor {
    window_state: Arc<WindowState>,
    params: Arc<DizmoParams>,
    show_pan: bool,
    load_kit: Arc<dyn Fn(PathBuf) + Send + Sync>,
    status_rx: Option<crossbeam_channel::Receiver<KitStatus>>,
    kit_info: Arc<Mutex<Option<KitInfo>>>,
    levels: Arc<[AtomicU32; NUM_CHANNELS]>,
    notifier: PollSubNotifier,
    /// The currently active editor. Replaced with a fresh editor on every
    /// `spawn` (see [`DizmoEditor::spawn`]) so that each open starts from a
    /// fresh `Some` editor state, regardless of how quickly the host tears
    /// down the previous editor instance.
    inner: Mutex<Box<dyn Editor>>,
}

impl DizmoEditor {
    pub fn new(
        params: Arc<DizmoParams>,
        show_pan: bool,
        load_kit: Arc<dyn Fn(PathBuf) + Send + Sync>,
        status_rx: Option<crossbeam_channel::Receiver<KitStatus>>,
        levels: Arc<[AtomicU32; NUM_CHANNELS]>,
        kit_info: Arc<Mutex<Option<KitInfo>>>,
        notifier: PollSubNotifier,
    ) -> Self {
        let window_state = params.editor_state.clone();
        let inner = Self::build_editor(
            window_state.clone(),
            &params,
            show_pan,
            &load_kit,
            &status_rx,
            &kit_info,
            &levels,
            &notifier,
        );

        Self {
            window_state,
            params,
            show_pan,
            load_kit,
            status_rx,
            kit_info,
            levels,
            notifier,
            inner: Mutex::new(inner),
        }
    }

    /// Builds a brand-new editor with its own fresh `Some` editor state. Each
    /// `spawn` gets one of these, so `from_shared` can never find `None`.
    fn build_editor(
        window_state: Arc<WindowState>,
        params: &Arc<DizmoParams>,
        show_pan: bool,
        load_kit: &Arc<dyn Fn(PathBuf) + Send + Sync>,
        status_rx: &Option<crossbeam_channel::Receiver<KitStatus>>,
        kit_info: &Arc<Mutex<Option<KitInfo>>>,
        levels: &Arc<[AtomicU32; NUM_CHANNELS]>,
        notifier: &PollSubNotifier,
    ) -> Box<dyn Editor> {
        let editor_state = EditorState::new(
            params.clone(),
            show_pan,
            load_kit.clone(),
            status_rx.clone(),
            kit_info.clone(),
            levels.clone(),
        );

        create_iced_editor(
            window_state,
            editor_state,
            notifier.clone(),
            EditorSettings {
                window_title: "DIZMO".to_string(),
                ignore_non_modifier_keys: true,
                always_redraw: false,
            },
            move |editor_state, nice_ctx| {
                nice_plug_iced::application(
                    editor_state,
                    nice_ctx,
                    DizmoGui::new,
                    |state: &mut DizmoGui, message: Message| state.update(message),
                    view,
                )
                .theme(theme)
                .subscription(|_| {
                    iced::Subscription::batch([
                        every(TICK_INTERVAL).map(|_| Message::Tick),
                        iced::poll_events().map(|()| Message::Poll),
                        iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
                    ])
                })
                .run()
            },
        )
        .expect("Failed to create the DIZMO editor")
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
            Arc::new(std::array::from_fn(|_| AtomicU32::new(0))),
            Arc::new(Mutex::new(None)),
            PollSubNotifier::new(),
        )
    }
}

impl Editor for DizmoEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn Any> {
        eprintln!("[dizmo] Editor::spawn called");
        let editor = Self::build_editor(
            self.window_state.clone(),
            &self.params,
            self.show_pan,
            &self.load_kit,
            &self.status_rx,
            &self.kit_info,
            &self.levels,
            &self.notifier,
        );
        let handle = editor.spawn(parent, context);
        *self.inner.lock().expect("editor mutex poisoned") = editor;
        handle
    }

    fn size(&self) -> Size {
        self.inner.lock().expect("editor mutex poisoned").size()
    }

    fn param_value_changed(&self, id: &str, normalized_value: f32) {
        self.inner.lock().expect("editor mutex poisoned").param_value_changed(id, normalized_value);
    }

    fn param_modulation_changed(&self, id: &str, modulation_offset: f32) {
        self.inner
            .lock()
            .expect("editor mutex poisoned")
            .param_modulation_changed(id, modulation_offset);
    }

    fn param_values_changed(&self) {
        self.inner
            .lock()
            .expect("editor mutex poisoned")
            .param_values_changed();
    }

    fn on_virtual_key_from_host(
        &self,
        key_code: VirtualKeyCode,
        is_down: bool,
        modifiers: Modifiers,
    ) -> bool {
        self.inner
            .lock()
            .expect("editor mutex poisoned")
            .on_virtual_key_from_host(key_code, is_down, modifiers)
    }

    fn set_size(&self, physical_size: PhysicalSize<u32>) -> bool {
        // The host has resized the plugin view (or accepted an earlier resize
        // request). The baseview view follows the host's frame automatically
        // and the UI re-layouts via the `Resized` event, so we only need to
        // accept the size rather than reject it (which would make the host
        // revert the resize).
        let _ = physical_size;
        true
    }

    fn set_scale_factor(&self, factor: f64) -> bool {
        self.inner.lock().expect("editor mutex poisoned").set_scale_factor(factor)
    }

    fn resize_hint(&self) -> ResizeHint {
        ResizeHint {
            can_resize: true,
            can_resize_horizontally: true,
            can_resize_vertically: true,
            preserve_aspect_ratio: true,
            aspect_ratio_width: 3,
            aspect_ratio_height: 2,
        }
    }
}

// --- Shared widget styles ---------------------------------------------------

/// Applies an iced_audio [`Gesture`] to a nice-plug parameter through `setter`.
fn set_nice_param<P: Param>(param: &P, gesture: Gesture, setter: &ParamSetter) {
    match gesture {
        Gesture::GestureStart => setter.begin_set_parameter(param),
        Gesture::Gesturing(new_normal) => {
            setter.set_parameter_normalized(param, new_normal.as_f32());
        }
        Gesture::GestureEnd => setter.end_set_parameter(param),
    }
}

fn background_style(theme: &iced::Theme) -> iced::widget::container::Style {
    let _ = theme;
    iced::widget::container::Style {
        background: Some(Background::Color(BG)),
        ..Default::default()
    }
}

fn header_style(theme: &iced::Theme) -> iced::widget::container::Style {
    let _ = theme;
    iced::widget::container::Style {
        background: Some(Background::Color(HEADER_BG)),
        border: Border {
            color: HEADER_BORDER,
            width: 1.0,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    }
}

fn separator_style(theme: &iced::Theme) -> iced::widget::container::Style {
    let _ = theme;
    iced::widget::container::Style {
        background: Some(Background::Color(INDICATOR)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::from(3.0),
        },
        ..Default::default()
    }
}

/// The pill style shared by the header buttons: flat field fill, with the
/// accent border while pressed-open or hovered.
fn pill_button_style(
    active: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let (background, border) = if active {
        (FIELD_BG, ACCENT)
    } else if status == iced::widget::button::Status::Hovered {
        (FIELD_HOVER, ACCENT)
    } else {
        (FIELD_BG, FIELD_BORDER)
    };
    iced::widget::button::Style {
        background: Some(Background::Color(background)),
        text_color: TEXT,
        border: Border {
            color: border,
            width: 1.0,
            radius: Radius::from(4.0),
        },
        ..Default::default()
    }
}

fn load_kit_button_style(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let _ = theme;
    let (background, border) = match status {
        iced::widget::button::Status::Hovered => (FIELD_BG, ACCENT),
        _ => (TRACK_BG, FIELD_BORDER),
    };
    iced::widget::button::Style {
        background: Some(Background::Color(background)),
        text_color: TEXT,
        border: Border {
            color: border,
            width: 1.0,
            radius: Radius::from(4.0),
        },
        ..Default::default()
    }
}

fn panel_style(theme: &iced::Theme) -> iced::widget::container::Style {
    let _ = theme;
    iced::widget::container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_BORDER,
            width: 1.0,
            radius: Radius::from(6.0),
        },
        ..Default::default()
    }
}

fn backdrop_style(theme: &iced::Theme) -> iced::widget::container::Style {
    let _ = theme;
    iced::widget::container::Style {
        background: Some(Background::Color(Color::from_rgba8(0, 0, 0, 0.55))),
        ..Default::default()
    }
}

// --- Modal dialogs ----------------------------------------------------------

/// A centered, modal dialog with `title` and `message`, dismissed by the OK
/// button or by clicking the backdrop.
fn modal_dialog<'a>(
    title: &'a str,
    message: &'a str,
    dismiss: Message,
    s: f32,
) -> iced::widget::Stack<'a, Message> {
    let panel = container(
        column![
            text(title).size(14.0 * s).color(TEXT),
            iced::widget::rule::horizontal(1.0 * s),
            text(message)
                .size(11.0 * s)
                .color(TEXT)
                .width(Length::Fixed(360.0 * s))
                .wrapping(iced::core::text::Wrapping::Word),
            button(text("OK").size(11.0 * s).color(Color::WHITE))
                .on_press(dismiss)
                .style(ok_button_style),
        ]
        .spacing(10.0 * s),
    )
    .width(Length::Fixed(380.0 * s))
    .padding(16.0 * s)
    .style(panel_style);

    dialog_stack(panel, dismiss)
}

/// The Mappings dialog: MIDI note map and channel assignment per instrument.
fn mappings_dialog<'a>(state: &'a DizmoGui) -> iced::widget::Stack<'a, Message> {
    let (title, body) = match &state.load_status {
        LoadStatus::Loaded { name, mappings, .. } => (name.as_str(), Some(mappings.as_slice())),
        _ => ("Mappings", None),
    };

    let s = state.scale();
    let mut content = column![
        text(title).size(14.0 * s).color(TEXT),
        iced::widget::rule::horizontal(1.0 * s),
    ]
    .spacing(10.0 * s);

    match body {
        Some(mappings) if !mappings.is_empty() => {
            content = content.push(
                row![
                    text("Instrument")
                        .size(10.0 * s)
                        .color(TEXT_DIM)
                        .width(Length::Fixed(200.0 * s)),
                    text("MIDI notes")
                        .size(10.0 * s)
                        .color(TEXT_DIM)
                        .width(Length::Fixed(160.0 * s)),
                    text("Channel map")
                        .size(10.0 * s)
                        .color(TEXT_DIM)
                        .width(Length::Fill),
                ]
                .spacing(8.0 * s),
            );
            let mut list = column![].spacing(4.0 * s);
            for mapping in mappings {
                list = list.push(
                    row![
                        text(&mapping.instrument)
                            .size(11.0 * s)
                            .color(TEXT)
                            .width(Length::Fixed(200.0 * s)),
                        text(note_text(&mapping.notes))
                            .size(11.0 * s)
                            .color(TEXT)
                            .width(Length::Fixed(160.0 * s)),
                        text(channel_text(&mapping.channel_map))
                            .size(11.0 * s)
                            .color(TEXT_DIM)
                            .width(Length::Fill),
                    ]
                    .spacing(8.0 * s),
                );
            }
            content = content.push(
                scrollable(container(list).width(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        }
        _ => {
            content = content.push(
                text("Load a kit to see its MIDI and channel mappings")
                    .size(11.0 * s)
                    .color(TEXT_DIM),
            );
        }
    }

    let panel = container(content)
        .width(Length::Fixed(560.0 * s))
        .height(Length::Fixed(360.0 * s))
        .padding(16.0 * s)
        .style(panel_style);

    dialog_stack(panel, Message::ToggleMappings)
}

/// Stacks a modal `panel` over a dim backdrop that dismisses on click.
fn dialog_stack<'a>(
    panel: iced::widget::Container<'a, Message>,
    dismiss: Message,
) -> iced::widget::Stack<'a, Message> {
    let backdrop = mouse_area(container(
        Space::new().width(Length::Fill).height(Length::Fill),
    ))
    .on_press(dismiss);
    iced::widget::Stack::with_children([
        container(backdrop)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(backdrop_style)
            .into(),
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into(),
    ])
}

fn ok_button_style(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let _ = theme;
    let (background, border) = match status {
        iced::widget::button::Status::Hovered => (FIELD_HOVER, ACCENT),
        _ => (FIELD_BG, FIELD_BORDER),
    };
    iced::widget::button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::WHITE,
        border: Border {
            color: border,
            width: 1.0,
            radius: Radius::from(4.0),
        },
        ..Default::default()
    }
}

/// "C3 · D#3 …" for a set of MIDI notes, or "—" when unmapped.
fn note_text(notes: &[u8]) -> String {
    if notes.is_empty() {
        return "—".to_string();
    }
    notes
        .iter()
        .map(|&note| midi_note_name(note))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// "kick-L → kick · kick-R → kick" for the main channel map entries only.
fn channel_text(assignments: &[crate::engine::ChannelAssignment]) -> String {
    let main: Vec<_> = assignments
        .iter()
        .filter(|map| map.is_main)
        .map(|map| format!("{} → {}", map.in_name, map.out_name))
        .collect();
    if main.is_empty() {
        return "—".to_string();
    }
    main.join(" · ")
}

/// Converts a MIDI note number to a note name like "C3" or "A#2".
fn midi_note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = (note / 12).saturating_sub(1);
    format!("{}{}", NAMES[(note % 12) as usize], octave)
}
