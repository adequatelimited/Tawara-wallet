// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Paste on a phone: the Paste button beside each field that takes pasted
//! text, and the cleaning of what is pasted.
//!
//! On a phone iced's fields have no edit menu, so a paste reaches them only
//! from a hardware keyboard. On a phone, at any width, a Paste button stands
//! beside each destination field, the signed-bytes field and the
//! recovery-phrase field, and a Scan button beneath each destination field.
//! In a table row of Send, on a phone held sideways from 844 wide, Paste and
//! Scan both stand beneath the field instead. The button reads the platform
//! clipboard through iced, only on that tap, and the text read replaces the
//! field's content; an empty or refused read changes nothing. A paste is never
//! submitted automatically. The desktop keeps keyboard paste and shows no
//! button.
//!
//! iced's `text_input` drops every control character of a paste before any
//! hook sees it, so a phrase copied one word per line would arrive as one
//! long word. The recovery-phrase field alone is wrapped in `phrase_paste`,
//! which hands the field a clipboard whose reads are cleaned by [`spaced`].

use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, clipboard, overlay};
use iced::{Element, Event, Length, Rectangle, Size, Vector, mouse};

use crate::app::wipe;
use crate::theme;
use crate::ui;

/// `text` as a recovery phrase is sent and validated: every run of
/// whitespace collapsed to one space, and the ends trimmed. The words are not
/// changed. The result is built in one allocation, so no copy of the phrase
/// is left behind by a growing buffer.
#[must_use]
pub fn phrase(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// `text` as a keyboard paste puts it into the recovery-phrase field: every
/// run of whitespace collapsed to one space, as [`phrase`] does, but a run at
/// either end kept as one space rather than trimmed. A paste lands at the
/// caret, possibly between typed words, so its end spaces keep those words
/// apart; the whole phrase is trimmed when sent. Built in one allocation, as
/// [`phrase`] is.
#[must_use]
pub fn spaced(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_space = false;
    for c in text.chars() {
        if !c.is_whitespace() {
            out.push(c);
            after_space = false;
        } else if !after_space {
            out.push(' ');
            after_space = true;
        }
    }
    out
}

/// `text` as a destination is sent: trimmed of a copied line break and any
/// surrounding space. Nothing inside changes, as a destination has no
/// whitespace of its own.
#[must_use]
pub fn token(text: &str) -> String {
    text.trim().to_owned()
}

/// `text` as signed bytes are sent: every whitespace character removed, so
/// hex copied in groups or across lines reads as one run. Built in one
/// allocation, as [`phrase`] is.
#[must_use]
pub fn hex(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    out.extend(text.chars().filter(|c| !c.is_whitespace()));
    out
}

/// The Paste button beside a field on a phone: disabled when `on_press` is
/// `None`, as the field beside it is while the page waits.
pub fn button<'a, M: Clone + 'a>(on_press: Option<M>) -> Element<'a, M> {
    ui::button_with(
        "Paste",
        theme::Button::Secondary,
        ui::Size::Small,
        None,
        on_press,
    )
    .into()
}

/// The clipboard a [`PhrasePaste`]'s field is handed. Its reads come back as
/// [`spaced`] cleans them, and the text read is wiped once it is cleaned.
/// Writes pass through unchanged.
struct Normalising<'a>(&'a mut dyn Clipboard);

impl Clipboard for Normalising<'_> {
    fn read(&self, kind: clipboard::Kind) -> Option<String> {
        let mut read = self.0.read(kind)?;
        let cleaned = spaced(&read);
        wipe(&mut read);
        Some(cleaned)
    }

    fn write(&mut self, kind: clipboard::Kind, contents: String) {
        self.0.write(kind, contents);
    }
}

/// `field`, the recovery-phrase field, wrapped so that what a keyboard paste
/// brings is cleaned by [`spaced`] before iced inserts it at the caret. The
/// wrapper draws, lays out and handles events as the field does; only the
/// clipboard it hands the field differs.
pub(crate) fn phrase_paste<'a, M: 'a>(field: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(PhrasePaste {
        field: field.into(),
    })
}

/// The widget [`phrase_paste`] makes.
struct PhrasePaste<'a, M> {
    field: Element<'a, M>,
}

impl<'a, M: 'a> Widget<M, iced::Theme, iced::Renderer> for PhrasePaste<'a, M> {
    fn size(&self) -> Size<Length> {
        self.field.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.field.as_widget().size_hint()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.field)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.field));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.field
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
        self.field.as_widget().draw(
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
        self.field
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
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        self.field.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            &mut Normalising(clipboard),
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
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.field.as_widget().mouse_interaction(
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
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.field.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

/// Test support for the paste paths, shared with the tests of the
/// recovery-phrase field in `screens::first_run`.
#[cfg(test)]
pub(crate) mod support {
    use iced::Event;
    use iced::advanced::Clipboard;
    use iced::advanced::clipboard;
    use iced::keyboard::key::{NativeCode, Physical};
    use iced::keyboard::{self, Key, Location, Modifiers};

    /// A clipboard that holds `.0`, as a platform's does after a copy: its
    /// reads give that text, and its writes are dropped.
    pub(crate) struct Holding(pub(crate) &'static str);

    impl Clipboard for Holding {
        fn read(&self, _kind: clipboard::Kind) -> Option<String> {
            Some(self.0.to_owned())
        }

        fn write(&mut self, _kind: clipboard::Kind, _contents: String) {}
    }

    /// The events of a paste from the keyboard: the command key goes down,
    /// and V goes down with it held.
    pub(crate) fn paste_keys() -> [Event; 2] {
        let v = Key::Character("v".into());
        [
            Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::COMMAND)),
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: v.clone(),
                modified_key: v,
                physical_key: Physical::Unidentified(NativeCode::Unidentified),
                location: Location::Standard,
                modifiers: Modifiers::COMMAND,
                text: None,
                repeat: false,
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::support::{Holding, paste_keys};
    use super::*;
    use crate::harness::Driver;
    use iced::widget::text_input;

    #[test]
    fn a_phrase_has_its_line_breaks_tabs_and_runs_of_spaces_made_single_spaces() {
        assert_eq!(
            phrase("  abandon\nability\r\n\table  about\n\nabove \n"),
            "abandon ability able about above"
        );
        assert_eq!(
            phrase("  a\nb\r\nc\t\td  e \u{a0}f\n"),
            "a b c d e f",
            "a no-break space is a space too"
        );
        assert_eq!(phrase("abandon ability"), "abandon ability", "unchanged");
        assert_eq!(phrase(" \n\t "), "", "nothing but whitespace");
    }

    #[test]
    fn a_keyboard_paste_has_its_runs_made_single_spaces_and_keeps_its_ends() {
        assert_eq!(
            spaced("  abandon\nability\r\n\table  about\n\nabove \n"),
            " abandon ability able about above "
        );
        assert_eq!(spaced(" ability"), " ability");
        assert_eq!(spaced("abandon ability"), "abandon ability", "unchanged");
        assert_eq!(spaced(" \n\t "), " ", "nothing but whitespace");
        assert_eq!(spaced(""), "");
    }

    #[test]
    fn a_destination_is_trimmed_and_nothing_else_changes() {
        assert_eq!(token("  3vQB7B6MrGQZaxCuFg4oh\n"), "3vQB7B6MrGQZaxCuFg4oh");
        assert_eq!(token("3vQB7B6MrGQZaxCuFg4oh"), "3vQB7B6MrGQZaxCuFg4oh");
        assert_eq!(token(" \r\n "), "", "nothing but whitespace");
    }

    #[test]
    fn signed_bytes_lose_every_whitespace_character() {
        assert_eq!(hex("00ab\ncd ef\n"), "00abcdef");
        assert_eq!(hex("4f 2a\r\n\t"), "4f2a");
        assert_eq!(hex(" \n "), "");
    }

    /// What a focused, empty field publishes when `copied` is pasted into it
    /// from the keyboard, the field wrapped in [`phrase_paste`] or not.
    fn pasted(copied: &'static str, wrapped: bool) -> Vec<String> {
        pasted_after("", copied, wrapped)
    }

    /// What a field holding `typed`, focused with its caret at the end,
    /// publishes when `copied` is pasted into it from the keyboard.
    fn pasted_after(typed: &str, copied: &'static str, wrapped: bool) -> Vec<String> {
        let input = text_input("", typed).on_input(|text| text);
        let field: Element<'_, String> = if wrapped {
            phrase_paste(input)
        } else {
            input.into()
        };
        let mut driver = Driver::new(field, (300, 60));
        driver.focus_next();
        driver.send_with(&paste_keys(), &mut Holding(copied))
    }

    #[test]
    fn keyboard_paste_into_the_phrase_field_keeps_the_words_apart() {
        let copied = "one\ntwo\n\nthree";
        assert_eq!(
            pasted(copied, false),
            ["onetwothree"],
            "iced alone joins the words: the case the wrapper is for"
        );
        assert_eq!(pasted(copied, true), ["one two three"]);
    }

    #[test]
    fn keyboard_paste_after_a_typed_word_keeps_the_space_it_brings() {
        // A paste lands at the caret, so the space at its start is what keeps
        // it apart from the word before it, as iced's own paste keeps it.
        assert_eq!(
            pasted_after("abandon", " ability", false),
            ["abandon ability"]
        );
        assert_eq!(
            pasted_after("abandon", " ability", true),
            ["abandon ability"]
        );
        assert_eq!(
            pasted_after("abandon", "\nability\n", true),
            ["abandon ability "]
        );
    }

    #[test]
    fn the_field_is_handed_a_clipboard_whose_reads_are_cleaned() {
        let mut held = Holding("one\ntwo\n\nthree");
        assert_eq!(
            Normalising(&mut held)
                .read(clipboard::Kind::Standard)
                .as_deref(),
            Some("one two three")
        );
        let mut blank = Holding(" \n\t ");
        assert_eq!(
            Normalising(&mut blank)
                .read(clipboard::Kind::Standard)
                .as_deref(),
            Some(" ")
        );
    }

    #[test]
    fn the_wrapper_draws_exactly_what_the_field_draws() {
        let field = || text_input("Recovery phrase", "").width(Length::Fixed(200.0));
        let bare = Driver::<()>::new(field(), (300, 60)).draw();
        let wrapped = Driver::<()>::new(phrase_paste(field()), (300, 60)).draw();
        assert!(bare == wrapped, "the wrapper changed the drawing");
    }
}
