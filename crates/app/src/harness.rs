// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The test harness: an interface built from an element at a size and driven
//! by hand with events, operations and drawing, as the runtime drives one.
//! The widget and screen tests use it, as does the phone layout check: every
//! node of a screen at 360 logical pixels, and the words of every text, must
//! lie within the width. A screen that must show all its words at once is
//! also held to the area between the system insets, with no scrolling page
//! and no word wider than its box (`fits_the_viewport`).
//!
//! Pointer and finger positions are window coordinates, as the shell gives
//! them. The cursor the interface sees is each event's position, which is
//! what a scrollable reads a finger's drag from.

use std::borrow::Cow;
use std::path::PathBuf;

use iced::advanced::clipboard;
use iced::advanced::graphics;
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::operation::{Outcome, focusable};
use iced::advanced::widget::{Id, Operation};
use iced::event::Status;
use iced::theme::Base;
use iced::{Element, Event, Padding, Point, Rectangle, Size, Vector, mouse, touch, window};
use iced_runtime::user_interface::{Cache, State, UserInterface};

use crate::host::{Device, DeviceCheck, Host, Released, ScanDone, ScanPreview, ScanStart};
use crate::scan::Scanner;
use crate::{fonts, overflow, theme};

/// The renderer the tests draw with: the software renderer, with the bundled
/// faces loaded as the screenshot example loads them.
pub(crate) fn renderer() -> iced::Renderer {
    static FONTS: std::sync::Once = std::sync::Once::new();
    FONTS.call_once(|| {
        if let Ok(mut font_system) = graphics::text::font_system().write() {
            for bytes in fonts::FILES {
                font_system.load_font(Cow::Borrowed(bytes));
            }
        }
    });
    iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
        fonts::BODY,
        iced::Pixels(16.0),
        Some("tiny-skia"),
    ))
    .expect("the software renderer starts")
}

/// An interface built from `root` at `size`, and the messages it produces
/// when it is driven.
pub(crate) struct Driver<'a, M> {
    ui: UserInterface<'a, M, iced::Theme, iced::Renderer>,
    renderer: iced::Renderer,
    size: (u32, u32),
    cursor: Option<Point>,
}

impl<'a, M: 'a> Driver<'a, M> {
    /// The interface for `root` in a window of `size`, with nothing under
    /// the pointer yet.
    pub(crate) fn new(root: impl Into<Element<'a, M>>, size: (u32, u32)) -> Self {
        Self::with_cache(root, size, Cache::default())
    }

    /// The interface for `root` in a window of `size`, holding the state
    /// `cache` kept from an earlier one, scrolls included, as the runtime keeps
    /// it between two drawings of a view.
    pub(crate) fn with_cache(
        root: impl Into<Element<'a, M>>,
        size: (u32, u32),
        cache: Cache,
    ) -> Self {
        let mut renderer = renderer();
        let bounds = Size::new(size.0 as f32, size.1 as f32);
        let ui = UserInterface::build(root, bounds, cache, &mut renderer);
        Driver {
            ui,
            renderer,
            size,
            cursor: None,
        }
    }

    /// The state the interface holds, for [`Driver::with_cache`].
    pub(crate) fn into_cache(self) -> Cache {
        self.ui.into_cache()
    }

    /// Hands `events` to the interface one at a time, each with the cursor
    /// where the events put it. Returns the messages they produced, the
    /// status each event got, and the interface's state after the last.
    pub(crate) fn send(&mut self, events: &[Event]) -> (Vec<M>, Vec<Status>, State) {
        let mut messages = Vec::new();
        let mut statuses = Vec::new();
        let mut state = State::Outdated;
        for event in events {
            if let Some(at) = position_of(event) {
                self.cursor = Some(at);
            }
            let cursor = self
                .cursor
                .map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
            let (next, status) = self.ui.update(
                std::slice::from_ref(event),
                cursor,
                &mut self.renderer,
                &mut clipboard::Null,
                &mut messages,
            );
            statuses.extend(status);
            state = next;
        }
        (messages, statuses, state)
    }

    /// Hands `events` to the interface one at a time with `clipboard` as the
    /// platform's, and returns the messages they produced.
    pub(crate) fn send_with(
        &mut self,
        events: &[Event],
        clipboard: &mut dyn iced::advanced::Clipboard,
    ) -> Vec<M> {
        let mut messages = Vec::new();
        for event in events {
            let cursor = self
                .cursor
                .map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
            let _ = self.ui.update(
                std::slice::from_ref(event),
                cursor,
                &mut self.renderer,
                clipboard,
                &mut messages,
            );
        }
        messages
    }

    /// Puts the pointer at `(x, y)` without an event, as when an interface is
    /// drawn new under a pointer that does not move: the widgets see the
    /// cursor there, and no move of it.
    pub(crate) fn place_pointer(&mut self, x: f32, y: f32) {
        self.cursor = Some(Point::new(x, y));
    }

    /// Runs `operation` over the interface to its end, as the runtime does.
    pub(crate) fn operate(&mut self, operation: Box<dyn Operation>) {
        let mut next = Some(operation);
        while let Some(mut operation) = next.take() {
            self.ui.operate(&self.renderer, operation.as_mut());
            if let Outcome::Chain(chained) = operation.finish() {
                next = Some(chained);
            }
        }
    }

    /// Gives Tab's first focus: the first focusable widget the keys reach.
    pub(crate) fn focus_next(&mut self) {
        self.operate(Box::new(focusable::focus_next()));
    }

    /// How many focusable widgets the interface holds, and which of them is
    /// focused by its index in Tab's order.
    pub(crate) fn focusable(&mut self) -> (usize, Option<usize>) {
        let mut counted = Focusables::default();
        self.ui.operate(&self.renderer, &mut counted);
        (counted.total, counted.focused)
    }

    /// Where the focused widget is laid out, in the window as it would be
    /// with nothing scrolled, if one is focused.
    pub(crate) fn focused_bounds(&mut self) -> Option<Rectangle> {
        let mut counted = Focusables::default();
        self.ui.operate(&self.renderer, &mut counted);
        counted.bounds
    }

    /// Where the scrollable `id` is in the window, and how far it is scrolled,
    /// if it is drawn.
    pub(crate) fn scroll_of(&mut self, id: &Id) -> Option<(Rectangle, Vector)> {
        let mut scroll = Scroll {
            id: id.clone(),
            found: None,
        };
        self.ui.operate(&self.renderer, &mut scroll);
        scroll.found
    }

    /// Where `text` is laid out, if it is drawn. Inside a scrolled page it is
    /// drawn higher by the page's scroll, which [`Driver::scroll_of`] gives.
    pub(crate) fn text_bounds(&mut self, text: &str) -> Option<Rectangle> {
        let mut texts = Texts::default();
        self.ui.operate(&self.renderer, &mut texts);
        texts
            .0
            .into_iter()
            .find(|(drawn, _)| drawn == text)
            .map(|(_, bounds)| bounds)
    }

    /// The window as RGBA pixels, drawn the way the screenshot example draws
    /// a screen.
    pub(crate) fn draw(&mut self) -> Vec<u8> {
        let theme = theme::theme();
        let style = theme.base();
        let _ = self.ui.update(
            &[redraw(iced::time::Instant::now())],
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut Vec::new(),
        );
        self.ui.draw(
            &mut self.renderer,
            &theme,
            &renderer::Style {
                text_color: style.text_color,
            },
            mouse::Cursor::Unavailable,
        );
        self.renderer.screenshot(
            Size::new(self.size.0, self.size.1),
            1.0,
            style.background_color,
        )
    }
}

/// The focusable widgets an interface holds, counted in the order Tab goes
/// through them, and the index and the bounds of the focused one.
#[derive(Default)]
struct Focusables {
    total: usize,
    focused: Option<usize>,
    bounds: Option<Rectangle>,
}

impl iced::advanced::widget::Operation for Focusables {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        _: Option<&iced::advanced::widget::Id>,
        bounds: Rectangle,
        state: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        if state.is_focused() {
            self.focused = Some(self.total);
            self.bounds = Some(bounds);
        }
        self.total += 1;
    }
}

/// The scroll of the scrollable with `id`: its bounds in the window, and how
/// far it is translated.
struct Scroll {
    id: Id,
    found: Option<(Rectangle, Vector)>,
}

impl Operation for Scroll {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _content: Rectangle,
        translation: Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&self.id) {
            self.found = Some((bounds, translation));
        }
    }
}

/// What a window draws as text, and where each text lies: the layout's own
/// answer, so that a test finds a row or a button by what it says.
#[derive(Default)]
struct Texts(Vec<(String, Rectangle)>);

impl Operation for Texts {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text(&mut self, _: Option<&iced::advanced::widget::Id>, bounds: Rectangle, text: &str) {
        self.0.push((text.to_owned(), bounds));
    }
}

/// Where the container or field with `id` is drawn when `root` is laid out
/// at `size`, if it is.
pub(crate) fn bounds_of<'a, M: 'a>(
    root: impl Into<Element<'a, M>>,
    size: (u32, u32),
    id: &Id,
) -> Option<Rectangle> {
    struct Find<'b> {
        id: &'b Id,
        found: Option<Rectangle>,
    }
    impl Operation for Find<'_> {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }

        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            if id == Some(self.id) {
                self.found = Some(bounds);
            }
        }

        fn focusable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            _: &mut dyn iced::advanced::widget::operation::Focusable,
        ) {
            if id == Some(self.id) {
                self.found = Some(bounds);
            }
        }
    }
    let mut renderer = renderer();
    let bounds = Size::new(size.0 as f32, size.1 as f32);
    let mut ui = UserInterface::build(root, bounds, Cache::default(), &mut renderer);
    let mut find = Find { id, found: None };
    ui.operate(&renderer, &mut find);
    find.found
}

/// Every text `root` draws in a window of `size`, unscrolled.
pub(crate) fn texts<'a, M: 'a>(
    root: impl Into<Element<'a, M>>,
    size: (f32, f32),
) -> Vec<(String, Rectangle)> {
    let mut renderer = renderer();
    let bounds = Size::new(size.0, size.1);
    let mut ui = UserInterface::build(root, bounds, Cache::default(), &mut renderer);
    let mut found = Texts::default();
    ui.operate(&renderer, &mut found);
    found.0
}

/// The window position an event carries, if it carries one.
fn position_of(event: &Event) -> Option<Point> {
    match event {
        Event::Mouse(mouse::Event::CursorMoved { position })
        | Event::Touch(
            touch::Event::FingerPressed { position, .. }
            | touch::Event::FingerMoved { position, .. }
            | touch::Event::FingerLifted { position, .. }
            | touch::Event::FingerLost { position, .. },
        ) => Some(*position),
        _ => None,
    }
}

/// The pointer moves to `(x, y)`.
pub(crate) fn mouse_move(x: f32, y: f32) -> Event {
    Event::Mouse(mouse::Event::CursorMoved {
        position: Point::new(x, y),
    })
}

/// The left button goes down where the pointer is.
pub(crate) fn mouse_press() -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
}

/// The left button comes up where the pointer is.
pub(crate) fn mouse_release() -> Event {
    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
}

/// Finger `id` goes down at `(x, y)`.
pub(crate) fn finger_press(id: u64, x: f32, y: f32) -> Event {
    Event::Touch(touch::Event::FingerPressed {
        id: touch::Finger(id),
        position: Point::new(x, y),
    })
}

/// Finger `id` moves to `(x, y)`.
pub(crate) fn finger_move(id: u64, x: f32, y: f32) -> Event {
    Event::Touch(touch::Event::FingerMoved {
        id: touch::Finger(id),
        position: Point::new(x, y),
    })
}

/// Finger `id` comes up at `(x, y)`.
pub(crate) fn finger_lift(id: u64, x: f32, y: f32) -> Event {
    Event::Touch(touch::Event::FingerLifted {
        id: touch::Finger(id),
        position: Point::new(x, y),
    })
}

/// The window is drawn at `at`, as the shell asks it each frame.
pub(crate) fn redraw(at: iced::time::Instant) -> Event {
    Event::Window(window::Event::RedrawRequested(at))
}

fn no_copy(_: &str) {}

fn no_share(_: &str, _: &str) -> Result<(), String> {
    Ok(())
}

fn no_back() {}

fn face_id() -> Option<DeviceCheck> {
    Some(DeviceCheck::FaceId)
}

fn no_keep(_: &[u8; 32], _: &str) -> Result<(), String> {
    Ok(())
}

fn no_release(_: &[u8; 32], _: &str, answer: Box<dyn FnOnce(Released) + Send>) {
    answer(Released::Declined);
}

fn no_forget() -> Result<(), String> {
    Ok(())
}

/// The device unlock of a phone whose shell offers Face ID, for the screens'
/// tests. Its answers go nowhere.
pub(crate) fn face_id_device() -> Device {
    let host = Host {
        private_dir: PathBuf::new(),
        copy: no_copy,
        share: no_share,
        back: no_back,
        notices: include_str!("../../../THIRD-PARTY-NOTICES.txt"),
        captured: None,
        device_check: Some(face_id),
        keep_password: Some(no_keep),
        release_password: Some(no_release),
        forget_password: Some(no_forget),
        scan: None,
        cancel_scan: None,
        scan_preview: None,
    };
    let (answers, _received) = iced::futures::channel::mpsc::unbounded();
    Device::of(&host, answers).expect("a shell that gives every device field offers the unlock")
}

/// The QR scanner of a phone whose shell supports all three scan fields, with
/// its camera preview drawn by `preview`, for the screens' tests. Its replies
/// go nowhere, and a scan started with it finds no camera.
pub(crate) fn scanner(preview: ScanPreview) -> Scanner {
    fn start(_: ScanDone) -> Result<(), String> {
        Ok(())
    }
    fn cancel() {}
    let host = Host {
        private_dir: PathBuf::new(),
        copy: no_copy,
        share: no_share,
        back: no_back,
        notices: include_str!("../../../THIRD-PARTY-NOTICES.txt"),
        captured: None,
        device_check: None,
        keep_password: None,
        release_password: None,
        forget_password: None,
        scan: Some(start as ScanStart),
        cancel_scan: Some(cancel as fn()),
        scan_preview: Some(preview),
    };
    let (replies, _received) = iced::futures::channel::mpsc::unbounded();
    Scanner::of(&host, replies).expect("a shell that gives every scan field offers a scanner")
}

/// Every node of `root` whose right edge passes `width` plus half a pixel
/// when it is laid out at `width` by 844, and every text whose words do, as a
/// window of that width would draw them, with `Ok` when there is none (see
/// `crate::overflow`).
pub(crate) fn fits<'a, M: 'a>(
    root: impl Into<Element<'a, M>>,
    width: f32,
) -> Result<(), Vec<Rectangle>> {
    fits_at(root, width, 844.0)
}

/// [`fits`], laid out at `width` by `height`. A page that scrolls is also laid
/// out in a shorter window, as a shorter phone draws it, and held to the
/// width there too.
pub(crate) fn fits_at<'a, M: 'a>(
    root: impl Into<Element<'a, M>>,
    width: f32,
    height: f32,
) -> Result<(), Vec<Rectangle>> {
    let past = overflow::past_the_edge(root.into(), &renderer(), width, height);
    if past.is_empty() {
        Ok(())
    } else {
        Err(past.into_iter().map(|past| past.bounds).collect())
    }
}

/// What `root` draws outside the visible part of a window of `size`, with the
/// system's `insets` covered at the top and bottom: each problem
/// [`overflow::viewport_problems`] finds, one per line, and none when the
/// screen fits. Used for screens that must show all their words at once.
pub(crate) fn fits_the_viewport<'a, M: 'a>(
    root: impl Into<Element<'a, M>>,
    size: (f32, f32),
    insets: Padding,
) -> Vec<String> {
    overflow::viewport_problems(root.into(), &renderer(), Size::new(size.0, size.1), insets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Length;
    use iced::widget::{scrollable, space};

    #[test]
    fn fits_reports_a_node_laid_out_past_the_width_and_passes_one_within_it() {
        // iced clamps a node to the room its parent gives it, so a node past the
        // edge needs a layout that does not clamp: the content of a horizontal
        // scrollable is laid out at its full width.
        let wide: Element<'_, ()> = scrollable(
            space()
                .width(Length::Fixed(500.0))
                .height(Length::Fixed(10.0)),
        )
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default(),
        ))
        .into();
        let past = fits(wide, 360.0).expect_err("a node past the width");
        assert!(past.iter().any(|r| r.x + r.width > 360.5), "{past:?}");

        let within: Element<'_, ()> = space()
            .width(Length::Fixed(300.0))
            .height(Length::Fixed(10.0))
            .into();
        assert_eq!(fits(within, 360.0), Ok(()));
    }
}
