// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Showing the soft keyboard again on a tap of the focused field.
//!
//! The shell asks the platform for the keyboard only when a field requests it
//! on a frame after a frame with no request: iced's window calls
//! `set_ime_allowed(true)` on that change, and a focused text input requests
//! it on every redraw. When the keyboard is hidden (Android's Back, or a
//! return from the background), the focused field keeps requesting, so a tap
//! on it changes nothing and the keyboard stays hidden.
//!
//! `reshow` handles this without changing iced. When a finger goes down on a
//! field that is focused both before and after the page handles the touch,
//! and the safe area reports no keyboard, the field's request on the next
//! frame is withheld. The shell sees no request on that frame, and the
//! request on the following frame is a change again, so the shell asks for
//! the keyboard. The field keeps its focus, caret and text, and no message
//! is sent.
//!
//! Both update passes of one frame share the frame's instant (the shell runs
//! a second pass when the first produced a message or changed the layout),
//! and the withholding covers both. Mouse presses are ignored, since a mouse
//! has no soft keyboard. Android reports the keyboard's height up to about
//! 250 milliseconds after it appears, so a tap just after it shows may hide
//! and show it once.

use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, InputMethod, Layout, Shell, Widget, layout, overlay, renderer};
use iced::time::Instant;
use iced::{Element, Event, Length, Rectangle, Size, Vector, mouse, touch, window};

/// Whether a field of the page holds the focus.
#[derive(Default)]
struct Focused(bool);

impl Operation for Focused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        self.0 |= state.is_focused();
    }
}

/// The state of withholding a field's keyboard request.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Withholding {
    /// Nothing is withheld: every request reaches the shell.
    #[default]
    Idle,
    /// A tap left a field focused while the keyboard was hidden, so the next
    /// redraw withholds the field's request.
    Pending,
    /// The request was withheld on the redraw at this instant. Another pass of
    /// the same frame's update, at the same instant, withholds it again; a
    /// redraw at a later instant lets the field ask.
    Hidden(Instant),
}

/// The page, as a widget that draws, lays out and handles every event as the
/// page does, and withholds a field's request in the one case `reshow`
/// describes.
struct Reshow<'a, Message> {
    content: Element<'a, Message>,
    keyboard_shown: bool,
}

/// `content`, the page, with the soft keyboard requested again on a tap of
/// the focused field while `keyboard_shown` is false. The wrapper draws
/// nothing of its own.
pub(crate) fn reshow<'a, Message: 'a>(
    content: Element<'a, Message>,
    keyboard_shown: bool,
) -> Element<'a, Message> {
    Element::new(Reshow {
        content,
        keyboard_shown,
    })
}

impl<Message> Reshow<'_, Message> {
    /// Whether a field of the page holds the focus, as an operation over the
    /// page finds it.
    fn focused(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer) -> bool {
        let mut focused = Focused::default();
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut focused);
        focused.0
    }
}

impl<'a, Message: 'a> Widget<Message, iced::Theme, iced::Renderer> for Reshow<'a, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Withholding>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Withholding::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
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
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // Only a finger press over the window matters. A mouse has no soft
        // keyboard, and moves and lifts are left to the page.
        let touched = matches!(event, Event::Touch(touch::Event::FingerPressed { .. }))
            && cursor.is_over(layout.bounds());
        let focused_before = touched && self.focused(tree, layout, renderer);
        // The page takes the event first, so a request it makes on a redraw is
        // already in the shell when this wrapper decides whether to withhold it.
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
        match event {
            Event::Touch(touch::Event::FingerPressed { .. }) if touched => {
                // Focused before and after the press: it landed on a field. A
                // press outside every field removes the focus, so nothing is
                // withheld.
                if focused_before && !self.keyboard_shown && self.focused(tree, layout, renderer) {
                    *tree.state.downcast_mut::<Withholding>() = Withholding::Pending;
                    shell.request_redraw();
                }
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                let state = tree.state.downcast_mut::<Withholding>();
                // A pending withholding, or a redraw at the instant of the one
                // that withheld, withholds the request. Any other redraw ends
                // the withholding, so the field's request reaches the shell.
                if *state == Withholding::Pending || *state == Withholding::Hidden(*now) {
                    *state = Withholding::Hidden(*now);
                    *shell.input_method_mut() = InputMethod::Disabled;
                    shell.request_redraw();
                } else {
                    *state = Withholding::Idle;
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iced::widget::{column, container, text_input};
    use iced_runtime::user_interface::State;

    use super::*;
    use crate::harness::{
        Driver, finger_lift, finger_press, mouse_move, mouse_press, mouse_release, redraw,
    };

    /// The window the tests draw in: the two fields at its top.
    const WINDOW: (u32, u32) = (300, 200);

    /// Inside the first field, inside the second (it begins 40 pixels lower),
    /// and in the empty space below both.
    const FIRST: (f32, f32) = (100.0, 10.0);
    const SECOND: (f32, f32) = (100.0, 50.0);
    const EMPTY: (f32, f32) = (100.0, 150.0);

    /// Two text fields, each in a box of 200 by 40, one under the other.
    fn fields() -> Element<'static, ()> {
        let field = || {
            container(text_input("", "").on_input(|_| ()))
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(40.0))
        };
        column![field(), field()].into()
    }

    /// The fields, with the keyboard shown again on a tap while
    /// `keyboard_shown` is false.
    fn page(keyboard_shown: bool) -> Element<'static, ()> {
        reshow(fields(), keyboard_shown)
    }

    /// The redraw `frames` frames after `start`, at 60 frames a second.
    fn frame(start: Instant, frames: u64) -> Event {
        redraw(start + Duration::from_millis(16 * frames))
    }

    /// Whether the interface asked the shell for the soft keyboard on its last
    /// update.
    fn asks_for_keyboard(state: State) -> bool {
        let State::Updated { input_method, .. } = state else {
            panic!("the interface was outdated, so it asked for nothing");
        };
        input_method.is_enabled()
    }

    #[test]
    fn a_tap_on_the_focused_field_asks_for_the_keyboard_again_when_it_is_hidden() {
        let start = Instant::now();
        let mut driver = Driver::new(page(false), WINDOW);
        driver.focus_next();
        assert_eq!(driver.focusable(), (2, Some(0)));

        let (messages, _, _) = driver.send(&[
            finger_press(1, FIRST.0, FIRST.1),
            finger_lift(1, FIRST.0, FIRST.1),
        ]);
        assert!(
            messages.is_empty(),
            "the tap publishes nothing: {messages:?}"
        );

        let (_, _, state) = driver.send(&[frame(start, 1)]);
        assert!(
            !asks_for_keyboard(state),
            "the first frame after the tap withholds the field's request"
        );
        let (_, _, state) = driver.send(&[frame(start, 1)]);
        assert!(
            !asks_for_keyboard(state),
            "a second pass of the same frame withholds it again"
        );
        let (_, _, state) = driver.send(&[frame(start, 2)]);
        assert!(
            asks_for_keyboard(state),
            "the frame after lets the field ask, and the shell asks the platform"
        );
        assert_eq!(
            driver.focusable(),
            (2, Some(0)),
            "the field keeps its focus"
        );
    }

    #[test]
    fn with_the_keyboard_shown_a_tap_withholds_nothing() {
        let start = Instant::now();
        let mut driver = Driver::new(page(true), WINDOW);
        driver.focus_next();
        driver.send(&[
            finger_press(1, FIRST.0, FIRST.1),
            finger_lift(1, FIRST.0, FIRST.1),
        ]);
        let (_, _, state) = driver.send(&[frame(start, 1)]);
        assert!(
            asks_for_keyboard(state),
            "the keyboard is already shown, so the field's request reaches the shell"
        );
    }

    #[test]
    fn a_mouse_click_on_the_focused_field_withholds_nothing() {
        let start = Instant::now();
        let mut driver = Driver::new(page(false), WINDOW);
        driver.focus_next();
        driver.send(&[mouse_move(FIRST.0, FIRST.1), mouse_press(), mouse_release()]);
        let (_, _, state) = driver.send(&[frame(start, 1)]);
        assert!(
            asks_for_keyboard(state),
            "a desktop has no soft keyboard to hide"
        );
    }

    #[test]
    fn a_first_focus_by_a_tap_is_the_shells_normal_path() {
        let start = Instant::now();
        let mut driver = Driver::new(page(false), WINDOW);
        assert_eq!(driver.focusable(), (2, None));
        driver.send(&[
            finger_press(1, FIRST.0, FIRST.1),
            finger_lift(1, FIRST.0, FIRST.1),
        ]);
        assert_eq!(
            driver.focusable(),
            (2, Some(0)),
            "the tap focused the first field"
        );
        let (_, _, state) = driver.send(&[frame(start, 1)]);
        assert!(
            asks_for_keyboard(state),
            "no field had the focus before the tap, so nothing is withheld"
        );
    }

    #[test]
    fn a_tap_outside_the_focused_field_unfocuses_it_and_withholds_nothing() {
        let start = Instant::now();
        let mut driver = Driver::new(page(false), WINDOW);
        driver.focus_next();
        driver.send(&[
            finger_press(1, EMPTY.0, EMPTY.1),
            finger_lift(1, EMPTY.0, EMPTY.1),
        ]);
        assert_eq!(driver.focusable(), (2, None), "the tap took the focus away");

        let (_, _, state) = driver.send(&[frame(start, 1)]);
        assert!(
            !asks_for_keyboard(state),
            "a field without the focus asks for nothing, and nothing is withheld"
        );
        let (_, _, state) = driver.send(&[frame(start, 2)]);
        assert!(
            !asks_for_keyboard(state),
            "the frame after asks for nothing too"
        );

        // The withholding is idle again: a tap on the first field is a first
        // focus, and the frame after it asks at once.
        driver.send(&[
            finger_press(2, FIRST.0, FIRST.1),
            finger_lift(2, FIRST.0, FIRST.1),
        ]);
        let (_, _, state) = driver.send(&[frame(start, 3)]);
        assert!(
            asks_for_keyboard(state),
            "the frame after the tap asks at once"
        );
    }

    #[test]
    fn a_tap_on_a_field_straight_after_an_unfocusing_tap_asks_at_once() {
        // Without a redraw in between, a withholding left by the unfocusing tap
        // would still be pending here.
        let start = Instant::now();
        let mut driver = Driver::new(page(false), WINDOW);
        driver.focus_next();
        driver.send(&[
            finger_press(1, EMPTY.0, EMPTY.1),
            finger_lift(1, EMPTY.0, EMPTY.1),
        ]);
        driver.send(&[
            finger_press(2, FIRST.0, FIRST.1),
            finger_lift(2, FIRST.0, FIRST.1),
        ]);
        let (_, _, state) = driver.send(&[frame(start, 1)]);
        assert!(
            asks_for_keyboard(state),
            "the frame after the tap asks at once"
        );
    }

    #[test]
    fn a_tap_on_the_second_field_while_the_first_has_the_focus_is_withheld_once() {
        let start = Instant::now();
        let mut driver = Driver::new(page(false), WINDOW);
        driver.focus_next();
        driver.send(&[
            finger_press(1, SECOND.0, SECOND.1),
            finger_lift(1, SECOND.0, SECOND.1),
        ]);
        assert_eq!(
            driver.focusable(),
            (2, Some(1)),
            "the second field has the focus"
        );

        let (_, _, state) = driver.send(&[frame(start, 1)]);
        assert!(
            !asks_for_keyboard(state),
            "the tap kept a field focused, so the request is withheld"
        );
        let (_, _, state) = driver.send(&[frame(start, 2)]);
        assert!(
            asks_for_keyboard(state),
            "the frame after asks for the keyboard"
        );
    }

    #[test]
    fn the_wrapper_draws_exactly_what_the_page_draws() {
        let plain = Driver::new(fields(), WINDOW).draw();
        let wrapped = Driver::new(page(false), WINDOW).draw();
        assert!(plain == wrapped, "the wrapper changed a pixel");
    }
}
