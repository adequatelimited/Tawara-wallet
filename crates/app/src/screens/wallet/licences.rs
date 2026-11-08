// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The licences page, reached from Settings' About card: the wallet's own
//! licence (the Mochimo agreement in `LICENSE.md`) and the notices of the
//! software the application includes. A file inside a phone app cannot be
//! opened by its user, so the licence is embedded verbatim at build time. The
//! notices are the desktop's `THIRD-PARTY-NOTICES.txt`, written by
//! `ci/notices.sh`, unless the shell supplies its own (`Host::notices`): a
//! phone is built from different crates and its package carries its own file.
//!
//! The page is one scrollable column of monospace text in cards, each with a
//! copy button, so it scrolls with a finger on a phone and with the wheel on
//! the desktop. The notices are long (about 8,000 lines), so each text is cut
//! into parts of [`LINES`] lines and the page draws only the parts it shows.

use iced::widget::text::Wrapping;
use iced::widget::{column, row};
use iced::{Alignment, Element, Length};

use super::{copy, frame};
use crate::app::{Message, Model, To, WalletMsg, WalletPage};
use crate::icon::Icon;
use crate::theme::{self, color, space as sp};
use crate::ui::{self, Size, t, ty};

/// The wallet's own licence: the Mochimo Cryptocurrency Engine License
/// Agreement, version 1.0, as the repository holds it.
const LICENCE: &str = include_str!("../../../../../LICENSE.md");

/// The notices of the software the wallet includes, as `ci/notices.sh` writes
/// them, ending with the licence above. The desktop draws these; a phone draws
/// the notices its shell supplies.
const NOTICES: &str = include_str!("../../../../../THIRD-PARTY-NOTICES.txt");

/// How many lines one part of a text holds.
const LINES: usize = 40;

/// The licences page: the wallet's licence, then the third-party notices,
/// each in a card under a Back button that returns to Settings.
pub fn view<'a>(model: &'a Model, page: &'a WalletPage) -> Element<'a, Message> {
    // Like the sidebar, Back is disabled while the worker is busy.
    let to_settings: Option<Message> = model
        .busy
        .is_none()
        .then(|| WalletMsg::Open(To::Settings).into());
    let back: Element<'a, Message> = ui::button_with(
        "Back",
        theme::Button::Secondary,
        Size::Small,
        Some(Icon::ChevronLeft),
        to_settings,
    )
    .into();
    // The shell's notices on a phone, and the wallet's own file on the desktop.
    let notices = model.notices.unwrap_or(NOTICES);
    frame(
        model,
        page,
        "Licences",
        "The wallet's own licence, and the notices of the software it includes",
        vec![back],
        vec![
            text_card(model, page, "Tawara's licence", "Copy the licence", LICENCE),
            text_card(
                model,
                page,
                "Third-party notices",
                "Copy the notices",
                notices,
            ),
        ],
    )
}

/// `whole`, under `title` and beside its copy button, in a card. Its parts
/// are drawn one after another in monospace, wrapped at words and, where a
/// word is wider than the line, at glyphs, so that nothing passes the edge of
/// a phone.
fn text_card<'a>(
    model: &Model,
    page: &'a WalletPage,
    title: &'a str,
    label: &'a str,
    whole: &'static str,
) -> Element<'a, Message> {
    let compact = model.compact();
    let title_text = t(title, ty::CARD_TITLE, color::TEXT_PRIMARY);
    let copy_button = copy(page, whole, label);
    let head: Element<'a, Message> = if compact {
        column![title_text, copy_button].spacing(sp::S12).into()
    } else {
        row![title_text.width(Length::Fill), copy_button]
            .spacing(sp::S16)
            .align_y(Alignment::Center)
            .into()
    };
    let mut lines: iced::widget::Column<'a, Message> = column![].width(Length::Fill);
    for part in split(whole) {
        lines = lines.push(
            t(part, ty::PAGE, color::TEXT_SECONDARY)
                .wrapping(Wrapping::WordOrGlyph)
                .width(Length::Fill),
        );
    }
    ui::card_for(compact, column![head, lines].spacing(sp::S16)).into()
}

/// `whole` cut into parts of [`LINES`] lines, each without its last line
/// break, which the page's own layout puts back: joined with line breaks the
/// parts give `whole` again exactly.
fn split(whole: &'static str) -> Vec<&'static str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut lines = 0;
    for (at, byte) in whole.bytes().enumerate() {
        if byte == b'\n' {
            lines += 1;
            if lines == LINES {
                parts.push(&whole[start..at]);
                start = at + 1;
                lines = 0;
            }
        }
    }
    parts.push(&whole[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tawara_wallet_core::preferences::Preferences;
    use tawara_wallet_core::sample;

    use iced::advanced::widget::operation::scrollable::{RelativeOffset, snap_to};
    use iced::{Event, mouse};

    use super::*;
    use crate::app::{Page, Screen};
    use crate::harness::{
        Driver, finger_lift, finger_move, finger_press, fits_at, mouse_move, mouse_press,
        mouse_release, texts,
    };
    use crate::screens;

    /// A window of the desktop, as the wallet's first window is drawn.
    const DESKTOP: (f32, f32) = (1024.0, 700.0);

    /// A phone of the narrowest width and shortest height the screens support.
    const PHONE: (f32, f32) = (360.0, 640.0);

    /// The sample store with its licences page shown, in a window `width`
    /// wide.
    fn licences_at(width: f32) -> Model {
        let mut model = Model::new(Preferences::default(), Some(PathBuf::from("/home/you")));
        model.wallet = Some(sample::wallet_view());
        model.width = width;
        model.screen = Screen::Wallet(WalletPage {
            page: Page::Licences,
            ..WalletPage::default()
        });
        model
    }

    /// The messages a press on the drawn text `label` produces in a window of
    /// `size`, as a person presses it.
    fn pressed(model: &Model, label: &str, size: (f32, f32)) -> Vec<Message> {
        let mut driver = Driver::new(screens::view(model), (size.0 as u32, size.1 as u32));
        let at = driver
            .text_bounds(label)
            .unwrap_or_else(|| panic!("`{label}` is not drawn"));
        let (messages, _, _) = driver.send(&[
            mouse_move(at.center_x(), at.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        messages
    }

    #[test]
    fn the_embedded_texts_are_the_files_and_the_notices_begin_as_the_generator_writes_them() {
        assert!(
            LICENCE.starts_with("# MOCHIMO CRYPTOCURRENCY ENGINE LICENSE AGREEMENT VERSION 1.0")
        );
        // The generator's header, and its end: it appends LICENSE.md, so the
        // notices end with the wallet's licence.
        assert!(NOTICES.starts_with("THIRD-PARTY NOTICES\n===================\n"));
        assert!(
            NOTICES.ends_with(LICENCE),
            "the notices end with LICENSE.md"
        );
    }

    #[test]
    fn the_parts_give_each_text_back_whole() {
        for whole in [LICENCE, NOTICES] {
            let parts = split(whole);
            assert_eq!(parts.join("\n"), whole);
            assert!(parts.iter().all(|part| part.matches('\n').count() < LINES));
        }
        // Splitting in the middle and at the very end keeps every break.
        assert_eq!(split("a\nb\nc").join("\n"), "a\nb\nc");
        assert_eq!(split("a\nb\n").join("\n"), "a\nb\n");
        assert_eq!(split("").join("\n"), "");
    }

    #[test]
    fn the_page_draws_every_part_of_both_texts_and_its_back_link() {
        let drawn: Vec<String> = texts(screens::view(&licences_at(DESKTOP.0)), DESKTOP)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        for whole in [LICENCE, NOTICES] {
            for part in split(whole) {
                assert!(
                    drawn.iter().any(|text| text == part),
                    "a part of the text is not drawn: {part:.60?}"
                );
            }
        }
        for line in [
            "Tawara includes the third-party software listed in this file.",
            "MOCHIMO CRYPTOCURRENCY ENGINE LICENSE AGREEMENT VERSION 1.0",
            "Back",
            "Licences",
        ] {
            assert!(
                drawn.iter().any(|text| text.contains(line)),
                "`{line}` is not drawn"
            );
        }
    }

    #[test]
    fn the_page_fits_a_phone_of_360_by_640_and_the_desktop_of_1024_by_700() {
        assert_eq!(
            fits_at(screens::view(&licences_at(PHONE.0)), PHONE.0, PHONE.1),
            Ok(())
        );
        assert_eq!(
            fits_at(screens::view(&licences_at(DESKTOP.0)), DESKTOP.0, DESKTOP.1),
            Ok(())
        );
    }

    #[test]
    fn back_leads_to_settings_on_the_desktop_and_on_a_phone() {
        for size in [DESKTOP, (360.0, 4000.0)] {
            let width = size.0;
            let messages = pressed(&licences_at(width), "Back", size);
            assert!(
                matches!(
                    messages.as_slice(),
                    [Message::Wallet(WalletMsg::Open(To::Settings))]
                ),
                "{messages:?}"
            );
        }
    }

    #[test]
    fn a_finger_drags_the_licences_on_a_phone() {
        // The text is one scrollable column with no touch of its own, so a finger
        // on it drags the page, as a phone's page is dragged: 200 pixels up scrolls
        // the page 200 down.
        let model = licences_at(PHONE.0);
        let mut driver = Driver::new(screens::view(&model), (PHONE.0 as u32, PHONE.1 as u32));
        let _ = driver.send(&[
            finger_press(1, 180.0, 500.0),
            finger_move(1, 180.0, 300.0),
            finger_lift(1, 180.0, 300.0),
        ]);
        let scrolled = driver
            .scroll_of(&screens::page_id())
            .map_or(0.0, |(_, offset)| offset.y);
        assert!(
            (scrolled - 200.0).abs() < 1.0,
            "a finger dragged 200 pixels, and the page scrolled {scrolled}"
        );
    }

    #[test]
    fn the_licences_are_scrolled_to_their_top_when_they_open() {
        // A page keeps the scroll of the one it replaces, since iced keeps a
        // scrollable's state with its place in the tree; the wallet's Open scrolls
        // the page to its top for the licences, and this is that operation.
        let model = licences_at(PHONE.0);
        let mut driver = Driver::new(screens::view(&model), (PHONE.0 as u32, PHONE.1 as u32));
        let _ = driver.send(&[
            mouse_move(180.0, 300.0),
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -30.0 },
            }),
        ]);
        let scrolled = driver
            .scroll_of(&screens::page_id())
            .map_or(0.0, |(_, offset)| offset.y);
        assert!(scrolled > 0.0, "the page did not scroll: {scrolled}");
        driver.operate(Box::new(snap_to(
            screens::page_id(),
            RelativeOffset::START.into(),
        )));
        let top = driver
            .scroll_of(&screens::page_id())
            .map_or(0.0, |(_, offset)| offset.y);
        assert_eq!(top, 0.0, "the page is at its top");
    }

    #[test]
    fn the_page_draws_the_notices_the_shell_gives_and_copies_them() {
        // A phone's shell gives the notices its package carries (`Host::notices`),
        // and the page draws those, whole, in place of the wallet's own file.
        let body: String = (0..LINES * 2 + 5)
            .map(|line| format!("a notice of the shell's own package, line {line}\n"))
            .collect();
        let shell: &'static str = format!("THIRD-PARTY NOTICES\n{body}").leak();
        let mut model = licences_at(DESKTOP.0);
        model.notices = Some(shell);
        let drawn: Vec<String> = texts(screens::view(&model), DESKTOP)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        for part in split(shell) {
            assert!(
                drawn.iter().any(|text| text == part),
                "a part of the shell's notices is not drawn: {part:.60?}"
            );
        }
        assert!(
            !drawn
                .iter()
                .any(|text| text.contains("Tawara includes the third-party software")),
            "the wallet's own notices are drawn in place of the shell's"
        );
        // The button copies what the shell gave, and it is drawn below the licence.
        let tall = (DESKTOP.0, 20_000.0);
        let messages = pressed(&model, "Copy the notices", tall);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Wallet(WalletMsg::Copy(text))] if text == shell
            ),
            "{messages:?}"
        );
    }

    #[test]
    fn each_copy_button_copies_its_whole_text() {
        let messages = pressed(&licences_at(DESKTOP.0), "Copy the licence", DESKTOP);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Wallet(WalletMsg::Copy(text))] if text == LICENCE
            ),
            "{messages:?}"
        );
        // The notices' button is below the licence, so the page is drawn tall
        // enough for it to be pressed.
        let tall = (DESKTOP.0, 20_000.0);
        let messages = pressed(&licences_at(DESKTOP.0), "Copy the notices", tall);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Wallet(WalletMsg::Copy(text))] if text == NOTICES
            ),
            "{messages:?}"
        );
    }
}
