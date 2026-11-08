// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! A row or card that acts on release, not on press.
//!
//! iced's button captures the press, so a scrollable under the finger never
//! sees a drag that starts on it, and the button acts on any release over it
//! however far the finger travelled; on a phone a list could not be scrolled
//! from its rows. A [`Tap`] draws and lays out exactly as its content does and
//! leaves the press uncaptured: it publishes its message when the pointer or
//! finger is released within [`SLOP`] of where it went down and still over
//! the widget. A drag that starts on it scrolls the enclosing list. On the
//! desktop a click is a press and release in place, so behaviour is
//! unchanged.

use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, overlay};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse, touch};

/// How far a pointer or finger may travel between press and release and
/// still tap, in logical pixels. Travel of this much or more is a drag.
pub const SLOP: f32 = 10.0;

/// The press a [`Tap`] waits on the release of.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Pressed {
    /// The left button, where the pointer was when it went down: `None` when
    /// the tap had seen no move of the pointer yet, as when it is drawn new
    /// under a pointer that stays still. The first move then stands for where
    /// the press was, and a release with no move between is in place.
    Mouse(Option<Point>),
    /// A finger, by its id, where it went down.
    Finger(u64, Point),
}

/// The tap's state: the press it is waiting on, and the pointer's window
/// position from its moves (a mouse press carries no position, so the last
/// move supplies it). Window positions are used because a scrollable moves
/// the content, not the pointer, so the distance travelled is the pointer's
/// own. The cursor handed to a widget is not used: inside a scrollable it is
/// shifted by the scroll.
#[derive(Debug, Default)]
struct State {
    pressed: Option<Pressed>,
    pointer: Option<Point>,
}

/// A row or card that publishes its message on release (see the module
/// documentation). Build it with [`tap`]; give it a message with
/// [`Tap::on_tap`] or [`Tap::on_tap_maybe`]. Without one it is disabled: it
/// publishes nothing and shows its content's pointer.
pub struct Tap<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    on_tap: Option<Message>,
}

/// A row or card that acts on release: `content`, drawn as it is.
pub fn tap<'a, M>(content: impl Into<Element<'a, M>>) -> Tap<'a, M> {
    Tap {
        content: content.into(),
        on_tap: None,
    }
}

impl<'a, Message, Theme, Renderer> Tap<'a, Message, Theme, Renderer> {
    /// The message published when the tap is made.
    #[must_use]
    pub fn on_tap(mut self, message: Message) -> Self {
        self.on_tap = Some(message);
        self
    }

    /// The message published when the tap is made, if `Some`; a disabled tap
    /// when `None`.
    #[must_use]
    pub fn on_tap_maybe(mut self, message: Option<Message>) -> Self {
        self.on_tap = message;
        self
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Tap<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
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

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
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

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
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
        // Update the content first, as a button does: an inner button that
        // captures the event keeps its own click.
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
        let state = tree.state.downcast_mut::<State>();
        if shell.is_event_captured() {
            state.pressed = None;
            return;
        }
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                state.pointer = Some(*position);
                match state.pressed {
                    Some(Pressed::Mouse(None)) => {
                        state.pressed = Some(Pressed::Mouse(Some(*position)));
                    }
                    Some(Pressed::Mouse(Some(at))) if at.distance(*position) >= SLOP => {
                        state.pressed = None;
                    }
                    _ => {}
                }
            }
            Event::Touch(touch::Event::FingerMoved { id, position }) => {
                if let Some(Pressed::Finger(held, at)) = state.pressed
                    && held == id.0
                    && at.distance(*position) >= SLOP
                {
                    state.pressed = None;
                }
            }
            // Presses are not captured, so an enclosing scrollable can take
            // them as a drag; the release makes the tap. A new press replaces
            // the one waited on, so a lost release (the window lost the
            // pointer, or the system dropped the lift) does not block the
            // next tap, and with two fingers down only the last can tap.
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if self.on_tap.is_some() && cursor.is_over(layout.bounds()) {
                    state.pressed = Some(Pressed::Mouse(state.pointer));
                }
            }
            Event::Touch(touch::Event::FingerPressed { id, position }) => {
                if self.on_tap.is_some() && cursor.is_over(layout.bounds()) {
                    state.pressed = Some(Pressed::Finger(id.0, *position));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                // A mouse release finishes only a mouse press; a held finger
                // waits for its own lift.
                if let Some(Pressed::Mouse(at)) = state.pressed {
                    state.pressed = None;
                    // With no move since the press, the release is in place.
                    let travelled = at.zip(state.pointer).map_or(0.0, |(a, b)| a.distance(b));
                    if travelled < SLOP
                        && cursor.is_over(layout.bounds())
                        && let Some(message) = &self.on_tap
                    {
                        shell.publish(message.clone());
                        shell.capture_event();
                    }
                }
            }
            Event::Touch(touch::Event::FingerLifted { id, position }) => {
                if let Some(Pressed::Finger(held, at)) = state.pressed
                    && held == id.0
                {
                    state.pressed = None;
                    if at.distance(*position) < SLOP
                        && cursor.is_over(layout.bounds())
                        && let Some(message) = &self.on_tap
                    {
                        shell.publish(message.clone());
                        shell.capture_event();
                    }
                }
            }
            Event::Touch(touch::Event::FingerLost { id, .. }) => {
                if let Some(Pressed::Finger(held, _)) = state.pressed
                    && held == id.0
                {
                    state.pressed = None;
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
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.on_tap.is_some() && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
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

impl<'a, Message, Theme, Renderer> From<Tap<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(tap: Tap<'a, Message, Theme, Renderer>) -> Self {
        Element::new(tap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{
        Driver, finger_lift, finger_move, finger_press, mouse_move, mouse_press, mouse_release,
    };
    use iced::Length;
    use iced::event::Status;
    use iced::widget::{button, column, container, scrollable, space};

    /// The window the tests draw in: wide enough for the rows, tall enough for
    /// the list.
    const WINDOW: (u32, u32) = (300, 844);

    /// A bar of 100 by 40 at the top left, publishing `message` on a tap.
    fn bar(message: i32) -> Element<'static, i32> {
        tap(space()
            .width(Length::Fixed(100.0))
            .height(Length::Fixed(40.0)))
        .on_tap(message)
        .into()
    }

    #[test]
    fn a_mouse_press_and_release_in_place_publishes_once() {
        let mut driver = Driver::new(bar(1), WINDOW);
        let (messages, _, _) =
            driver.send(&[mouse_move(10.0, 10.0), mouse_press(), mouse_release()]);
        assert_eq!(messages, [1]);
    }

    #[test]
    fn a_finger_press_and_lift_in_place_publishes_once() {
        let mut driver = Driver::new(bar(1), WINDOW);
        let (messages, _, _) =
            driver.send(&[finger_press(1, 10.0, 10.0), finger_lift(1, 10.0, 10.0)]);
        assert_eq!(messages, [1]);
    }

    #[test]
    fn a_finger_that_moves_ten_pixels_does_not_tap_and_nine_pixels_does() {
        let mut driver = Driver::new(bar(1), WINDOW);
        let (messages, _, _) = driver.send(&[
            finger_press(1, 10.0, 10.0),
            finger_move(1, 20.0, 10.0),
            finger_lift(1, 20.0, 10.0),
        ]);
        assert!(
            messages.is_empty(),
            "a drag of {SLOP} is not a tap: {messages:?}"
        );

        let mut driver = Driver::new(bar(1), WINDOW);
        let (messages, _, _) = driver.send(&[
            finger_press(1, 10.0, 10.0),
            finger_move(1, 19.0, 10.0),
            finger_lift(1, 19.0, 10.0),
        ]);
        assert_eq!(messages, [1], "a drag of 9 still taps");
    }

    #[test]
    fn a_release_outside_the_tap_publishes_nothing() {
        // Within the slop of the press, but off the bar on the right.
        let mut driver = Driver::new(bar(1), WINDOW);
        let (messages, _, _) = driver.send(&[
            mouse_move(98.0, 10.0),
            mouse_press(),
            mouse_move(104.0, 10.0),
            mouse_release(),
        ]);
        assert!(messages.is_empty(), "{messages:?}");

        // Far below it.
        let mut driver = Driver::new(bar(1), WINDOW);
        let (messages, _, _) =
            driver.send(&[finger_press(1, 10.0, 10.0), finger_lift(1, 10.0, 200.0)]);
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn the_press_is_not_captured_so_a_scrollable_can_take_it() {
        let mut driver = Driver::new(bar(1), WINDOW);
        let (_, statuses, _) = driver.send(&[mouse_move(10.0, 10.0), mouse_press()]);
        assert!(
            statuses.iter().all(|s| *s == Status::Ignored),
            "{statuses:?}"
        );
    }

    /// Twenty rows of 40 pixels in a list 100 pixels tall, each publishing
    /// its index on a tap.
    fn list() -> Element<'static, usize> {
        let rows: Vec<Element<'static, usize>> = (0..20)
            .map(|i| {
                tap(space().width(Length::Fill).height(Length::Fixed(40.0)))
                    .on_tap(i)
                    .into()
            })
            .collect();
        scrollable(column(rows)).height(Length::Fixed(100.0)).into()
    }

    #[test]
    fn a_drag_that_starts_on_a_row_scrolls_the_list_and_taps_nothing() {
        let mut driver = Driver::new(list(), WINDOW);
        // Row 2 is under the finger at y 80; the list scrolls 60 as the finger
        // moves up to y 20.
        let (messages, _, _) = driver.send(&[
            finger_press(1, 50.0, 80.0),
            finger_move(1, 50.0, 20.0),
            finger_lift(1, 50.0, 20.0),
        ]);
        assert!(messages.is_empty(), "the drag tapped a row: {messages:?}");

        // Now the row at y 30 is row 2, not row 0: the list moved.
        let (messages, _, _) =
            driver.send(&[finger_press(1, 50.0, 30.0), finger_lift(1, 50.0, 30.0)]);
        assert_eq!(messages, [2], "the list scrolled 60 pixels");
    }

    #[test]
    fn a_disabled_tap_publishes_nothing_and_shows_no_pointer() {
        let disabled: Element<'static, i32> = tap(space()
            .width(Length::Fixed(100.0))
            .height(Length::Fixed(40.0)))
        .on_tap_maybe(None)
        .into();
        let mut driver = Driver::new(disabled, WINDOW);
        let (messages, _, state) =
            driver.send(&[mouse_move(10.0, 10.0), mouse_press(), mouse_release()]);
        assert!(messages.is_empty(), "{messages:?}");
        assert!(
            matches!(state, iced_runtime::user_interface::State::Updated { mouse_interaction, .. }
                if mouse_interaction != mouse::Interaction::Pointer),
            "a disabled tap shows no pointer"
        );

        // Enabled, the same pointer is the hand.
        let mut driver = Driver::new(bar(1), WINDOW);
        let (_, _, state) = driver.send(&[mouse_move(10.0, 10.0)]);
        assert!(
            matches!(state, iced_runtime::user_interface::State::Updated { mouse_interaction, .. }
                if mouse_interaction == mouse::Interaction::Pointer),
            "an enabled tap shows the pointer"
        );
    }

    #[test]
    fn an_inner_buttons_click_is_its_own_and_the_tap_stays_silent() {
        #[derive(Clone, Debug, PartialEq)]
        enum Msg {
            Row,
            Inner,
        }
        let inner = button(
            space()
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .on_press(Msg::Inner);
        let root: Element<'static, Msg> = tap(container(inner)
            .width(Length::Fixed(100.0))
            .height(Length::Fixed(40.0)))
        .on_tap(Msg::Row)
        .into();
        let mut driver = Driver::new(root, WINDOW);
        let (messages, _, _) =
            driver.send(&[mouse_move(10.0, 10.0), mouse_press(), mouse_release()]);
        assert_eq!(messages, [Msg::Inner], "the button's click only");

        let (messages, _, _) =
            driver.send(&[mouse_move(60.0, 10.0), mouse_press(), mouse_release()]);
        assert_eq!(messages, [Msg::Row], "outside the button, the tap");
    }

    #[test]
    fn a_second_finger_takes_the_tap_over_and_only_one_tap_is_made() {
        let mut driver = Driver::new(bar(1), WINDOW);
        // The second finger's lift is not the first finger's release.
        let (messages, _, _) =
            driver.send(&[finger_press(1, 10.0, 10.0), finger_lift(2, 20.0, 10.0)]);
        assert!(messages.is_empty(), "{messages:?}");
        // A second finger's press takes the place of the first's, so the first
        // finger's lift makes no tap, and the second's lift makes the one tap.
        let (messages, _, _) = driver.send(&[
            finger_press(2, 20.0, 10.0),
            finger_lift(1, 10.0, 10.0),
            finger_lift(2, 20.0, 10.0),
        ]);
        assert_eq!(messages, [1]);
    }

    #[test]
    fn a_press_whose_lift_was_lost_does_not_stop_the_next_tap() {
        // Finger 1's lift never comes, and it never moves again: the next
        // finger's tap on the row is made all the same.
        let mut driver = Driver::new(bar(1), WINDOW);
        let (messages, _, _) = driver.send(&[finger_press(1, 10.0, 10.0)]);
        assert!(messages.is_empty(), "{messages:?}");
        let (messages, _, _) =
            driver.send(&[finger_press(7, 60.0, 30.0), finger_lift(7, 60.0, 30.0)]);
        assert_eq!(messages, [1]);
    }

    #[test]
    fn a_click_under_a_pointer_that_has_not_moved_taps() {
        // The tap is drawn under a pointer that is already there and stays
        // still, so it sees no move before the press.
        let mut driver = Driver::new(bar(1), WINDOW);
        driver.place_pointer(10.0, 10.0);
        let (messages, _, _) = driver.send(&[mouse_press(), mouse_release()]);
        assert_eq!(messages, [1], "a press with no move before it");

        // The first move after such a press stands for where it was, so a
        // drag from there is still no tap.
        let mut driver = Driver::new(bar(1), WINDOW);
        driver.place_pointer(10.0, 10.0);
        let (messages, _, _) = driver.send(&[
            mouse_press(),
            mouse_move(12.0, 10.0),
            mouse_move(40.0, 10.0),
            mouse_release(),
        ]);
        assert!(
            messages.is_empty(),
            "a drag of 28 is not a tap: {messages:?}"
        );
    }
}
