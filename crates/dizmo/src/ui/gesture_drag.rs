//! A drag controller for the iced_audio fader and knob widgets.
//!
//! iced_baseview reports `cursor.position()` (scaled by the window scale
//! factor) and `Event::CursorMoved` positions (unscaled) in different
//! coordinate spaces, and iced_audio mixes the two during a drag: the press
//! anchor comes from the cursor and each move delta from the raw event. On a
//! HiDPI display the first delta then already spans the whole widget, which
//! clamps the value to an extreme and leaves the widget stuck there.
//!
//! [`GestureDrag`] re-implements the gesture input using only
//! `cursor.position()`, so the press anchor and every move delta live in the
//! same space as the widget's layout bounds. The wrapped widget keeps its
//! drawing (and hover/gesture visuals) but its own `on_gesture` is left unset
//! so it stays silent.

use nice_plug_iced::iced::core::keyboard;
use nice_plug_iced::iced::core::layout;
use nice_plug_iced::iced::core::mouse;
use nice_plug_iced::iced::core::overlay;
use nice_plug_iced::iced::core::renderer;
use nice_plug_iced::iced::core::touch;
use nice_plug_iced::iced::core::widget::{Operation, Tree, tree};
use nice_plug_iced::iced::core::window;
use nice_plug_iced::iced::core::{
    Clipboard, Element, Event, Layout, Length, Rectangle, Shell, Size, Vector, Widget,
};

use iced_audio::Gesture;

/// Wraps a widget and emits [`Gesture`] messages driven by the mouse, using
/// only `cursor.position()` so the drag stays in a single coordinate space.
pub struct GestureDrag<'a, Message, Theme, Renderer, F>
where
    Message: Clone,
    F: FnMut(Gesture) -> Message + 'a,
{
    content: Element<'a, Message, Theme, Renderer>,
    on_gesture: F,
    /// The current normalized value, re-read from the parameter each frame.
    value: f32,
    /// The normalized default value, restored on double click.
    default: f32,
    /// How fast the value changes per logical pixel of mouse drag.
    drag_scalar: f32,
    /// How fast the value changes per scroll wheel line.
    wheel_scalar: f32,
    /// The drag speed multiplier while a fine-tune modifier is held.
    fine_tune_scalar: f32,
    /// The modifier keys that enable fine-tune dragging.
    fine_tune_modifiers: keyboard::Modifiers,
}

impl<'a, Message, Theme, Renderer, F> GestureDrag<'a, Message, Theme, Renderer, F>
where
    Message: Clone,
    F: FnMut(Gesture) -> Message + 'a,
{
    /// Creates a [`GestureDrag`] around `content`, emitting `on_gesture`.
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>, on_gesture: F) -> Self {
        Self {
            content: content.into(),
            on_gesture,
            value: 0.5,
            default: 0.5,
            drag_scalar: 0.0025,
            wheel_scalar: 0.01,
            fine_tune_scalar: 0.02,
            fine_tune_modifiers: keyboard::Modifiers::CTRL,
        }
    }

    /// Sets the current normalized value.
    #[must_use]
    pub fn value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    /// Sets the normalized default value used on double click.
    #[must_use]
    pub fn default(mut self, default: f32) -> Self {
        self.default = default;
        self
    }

    /// Sets how fast the value changes per logical pixel of mouse drag.
    #[must_use]
    pub fn drag_scalar(mut self, drag_scalar: f32) -> Self {
        self.drag_scalar = drag_scalar;
        self
    }

    /// Sets how fast the value changes per scroll wheel line.
    #[must_use]
    pub fn wheel_scalar(mut self, wheel_scalar: f32) -> Self {
        self.wheel_scalar = wheel_scalar;
        self
    }

    /// Sets the drag speed multiplier while a fine-tune modifier is held.
    #[must_use]
    pub fn fine_tune_scalar(mut self, fine_tune_scalar: f32) -> Self {
        self.fine_tune_scalar = fine_tune_scalar;
        self
    }

    /// Sets the modifier keys that enable fine-tune dragging.
    #[must_use]
    pub fn fine_tune_modifiers(mut self, fine_tune_modifiers: keyboard::Modifiers) -> Self {
        self.fine_tune_modifiers = fine_tune_modifiers;
        self
    }
}

/// The gesture state of a [`GestureDrag`].
struct DragState {
    is_dragging: bool,
    prev_drag_pos: f32,
    prev_normal: f32,
    continuous_normal: f32,
    pressed_modifiers: keyboard::Modifiers,
    last_click: Option<mouse::Click>,
    last_sent_gesture: Gesture,
}

impl DragState {
    fn new(value: f32) -> Self {
        Self {
            is_dragging: false,
            prev_drag_pos: 0.0,
            prev_normal: value,
            continuous_normal: value,
            pressed_modifiers: keyboard::Modifiers::NONE,
            last_click: None,
            last_sent_gesture: Gesture::GestureEnd,
        }
    }
}

impl<'a, Message, Theme, Renderer, F> Widget<Message, Theme, Renderer>
    for GestureDrag<'a, Message, Theme, Renderer, F>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
    F: FnMut(Gesture) -> Message + 'a,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DragState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DragState::new(self.value))
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.update_gesture(tree, event, layout, cursor, shell);

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        renderer_style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            renderer_style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer, F> From<GestureDrag<'a, Message, Theme, Renderer, F>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
    F: FnMut(Gesture) -> Message + 'a,
{
    fn from(drag: GestureDrag<'a, Message, Theme, Renderer, F>) -> Self {
        Element::new(drag)
    }
}

impl<'a, Message, Theme, Renderer, F> GestureDrag<'a, Message, Theme, Renderer, F>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
    F: FnMut(Gesture) -> Message + 'a,
{
    /// Drives the gesture state from mouse/touch events.
    ///
    /// The press anchor and every move delta come from `cursor.position()`,
    /// never from the raw event position, because iced_baseview reports those
    /// two in different coordinate spaces.
    fn update_gesture(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, Message>,
    ) {
        let state = tree.state.downcast_mut::<DragState>();

        // Re-sync with the parameter when it changed outside of a drag.
        if !state.is_dragging && state.prev_normal != self.value {
            state.prev_normal = self.value;
            state.continuous_normal = self.value;
        }

        let cursor_is_over = cursor.is_over(layout.bounds());
        let mut capture = false;

        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if state.is_dragging
                    && let Some(position) = cursor.position()
                {
                    let delta = position.y - state.prev_drag_pos;
                    state.prev_drag_pos = position.y;
                    self.move_slider(state, delta * self.drag_scalar, shell);
                    capture = true;
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor_is_over && self.wheel_scalar > 0.0 {
                    let lines = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => *y,
                        mouse::ScrollDelta::Pixels { y, .. } => {
                            if *y > 0.0 {
                                1.0
                            } else if *y < 0.0 {
                                -1.0
                            } else {
                                0.0
                            }
                        }
                    };
                    if lines != 0.0 {
                        self.move_slider(state, -lines * self.wheel_scalar, shell);
                        self.end_gesture(state, shell);
                        capture = true;
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if cursor_is_over && let Some(position) = cursor.position() {
                    let click = mouse::Click::new(position, mouse::Button::Left, state.last_click);
                    match click.kind() {
                        mouse::click::Kind::Single => {
                            state.is_dragging = true;
                            state.prev_drag_pos = position.y;
                            self.start_gesture(state, shell);
                        }
                        _ => {
                            self.set_value(self.default, state, shell);
                            self.end_gesture(state, shell);
                        }
                    }
                    state.last_click = Some(click);
                    capture = true;
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Mouse(mouse::Event::CursorLeft)
            | Event::Touch(touch::Event::FingerLifted { .. })
            | Event::Touch(touch::Event::FingerLost { .. }) => {
                self.end_gesture(state, shell);
            }
            Event::Window(window::Event::Unfocused) => {
                self.end_gesture(state, shell);
            }
            Event::Keyboard(keyboard::Event::KeyPressed { modifiers, .. })
            | Event::Keyboard(keyboard::Event::KeyReleased { modifiers, .. })
            | Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.pressed_modifiers = *modifiers;
            }
            _ => {}
        }

        if capture {
            shell.capture_event();
        }
    }

    fn start_gesture(&mut self, state: &mut DragState, shell: &mut Shell<'_, Message>) {
        if state.last_sent_gesture == Gesture::GestureEnd {
            shell.publish((self.on_gesture)(Gesture::GestureStart));
            state.last_sent_gesture = Gesture::GestureStart;
        }
    }

    fn move_slider(
        &mut self,
        state: &mut DragState,
        mut delta: f32,
        shell: &mut Shell<'_, Message>,
    ) {
        if state.pressed_modifiers.contains(self.fine_tune_modifiers) {
            delta *= self.fine_tune_scalar;
        }
        self.set_value(state.continuous_normal - delta, state, shell);
    }

    fn set_value(&mut self, value: f32, state: &mut DragState, shell: &mut Shell<'_, Message>) {
        let value = value.clamp(0.0, 1.0);
        let prev = state.continuous_normal;
        state.continuous_normal = value;
        if (value - prev).abs() <= f32::EPSILON {
            return;
        }
        self.start_gesture(state, shell);
        let gesture = Gesture::Gesturing(iced_audio::Normal::new(value));
        shell.publish((self.on_gesture)(gesture));
        state.last_sent_gesture = gesture;
    }

    fn end_gesture(&mut self, state: &mut DragState, shell: &mut Shell<'_, Message>) {
        state.is_dragging = false;
        if state.last_sent_gesture == Gesture::GestureEnd {
            return;
        }
        shell.publish((self.on_gesture)(Gesture::GestureEnd));
        state.last_sent_gesture = Gesture::GestureEnd;
    }
}
