// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The screens, each a function of the [`Model`].

mod first_run;
mod wallet;

pub(crate) use first_run::phrase_words_drawn;

use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, scroll_to};
use iced::advanced::widget::operation::{Focusable, Outcome, Scrollable};
use iced::advanced::widget::{Id, Operation};
use iced::mobile::SafeArea;
use iced::widget::operation::{RelativeOffset, snap_to};
use iced::widget::{column, container, progress_bar, scrollable};
use iced::{Element, Length, Padding, Rectangle, Task, Vector};
use tawara_wallet_core::Activity;

use crate::app::{Busy, Message, Model, Screen};
use crate::keyboard;
use crate::theme::{self, color, space};
use crate::ui::{self, t, ty};

/// The page's scrollable: on a phone, the one the screenshot example scrolls
/// to its end, and in both layouts the one [`page_top`] scrolls to its top.
pub fn page_id() -> iced::widget::Id {
    iced::widget::Id::from("page")
}

/// Scrolls the page to its top, as entering the spend review, the sent page
/// and returning to the form do. Without it the review is drawn at the form's
/// scroll: the payees and amount can be out of view, and "Sign & submit" can
/// land under the finger where "Review spend" was, so a second tap would sign
/// and broadcast. iced keeps a scrollable's offset by its place in the widget
/// tree, so a different id would not reset it.
pub fn page_top() -> Task<Message> {
    snap_to(page_id(), RelativeOffset::START)
}

/// Scrolls the page to its end, where a phone's page 2 of Send opens when a
/// signing is refused: the refusal is drawn beside "Review spend" at the foot
/// of that page.
pub fn page_end() -> Task<Message> {
    snap_to(page_id(), RelativeOffset::END)
}

/// Whether this build is the iPhone's, whose shell can report no safe area at
/// all. An expression rather than a `cfg` attribute, so that both branches
/// are compiled on every target.
const IPHONE: bool = cfg!(target_os = "ios");

/// What pads an iPhone's root while its shell reports no inset at all, 64
/// above and 48 below, as the iced fork's template does until the safe area
/// arrives.
const UNREPORTED: Padding = Padding {
    top: 64.0,
    right: 0.0,
    bottom: 48.0,
    left: 0.0,
};

/// What pads the root for `area`. On an iPhone (`iphone`), an area with no
/// inset at all is not a real one upright, where the status bar alone takes
/// 20 points, so the root is padded by [`UNREPORTED`] instead, the bottom raised
/// to the keyboard's top while it shows. An iPhone held sideways with neither
/// notch nor status bar loses that much room and nothing else.
fn root_padding(area: SafeArea, iphone: bool) -> Padding {
    if iphone && area.insets == Padding::ZERO {
        UNREPORTED.bottom(UNREPORTED.bottom.max(area.keyboard))
    } else {
        area.padding(0.0)
    }
}

/// Scrolls the page so that the focused field stands clear of the keyboard.
/// The root is padded up to the
/// keyboard's top, so the page's scrollable already ends there; this moves
/// the page, by no more than it must, until the field lies inside it with
/// [`space::S16`] of room above and below, its top kept in view when it is
/// taller than the room. Nothing moves when the field is already in view,
/// or when no field of the page has the focus. iced's own field does not
/// scroll the page it sits in.
pub fn reveal_focused() -> impl Operation + 'static {
    Reveal::default()
}

/// [`reveal_focused`] for the field `target`, focused or not: the page is
/// scrolled, by no more than [`reveal_focused`] would, so that the field lies
/// in view. "Continue", and "Review spend" when it sends the person back to
/// page 1, bring the first field refused into view this way, since the refusal
/// is drawn beneath that field and the page does not move on its own.
pub fn reveal_field(target: Id) -> impl Operation + 'static {
    Reveal {
        target: Some(target),
        ..Reveal::default()
    }
}

/// [`reveal_focused`] and [`reveal_field`]: the page's scrollable as it was laid
/// out, then the offset the field asks for, which a scroll of the page applies.
/// `target` is the field to reveal, or none for the focused one.
#[derive(Default)]
struct Reveal {
    target: Option<Id>,
    page: Option<(Rectangle, Rectangle, Vector)>,
    to: Option<f32>,
}

impl Operation for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&page_id()) {
            self.page = Some((bounds, content, translation));
        }
    }

    fn focusable(&mut self, id: Option<&Id>, field: Rectangle, state: &mut dyn Focusable) {
        let Some((bounds, content, translation)) = self.page else {
            return;
        };
        let wanted = match &self.target {
            Some(target) => id == Some(target),
            None => state.is_focused(),
        };
        if !wanted {
            return;
        }
        // The field's top and bottom within the page, and the part of the page
        // in view, from its offset.
        let top = field.y - content.y - space::S16;
        let bottom = field.y + field.height - content.y + space::S16;
        let shown = translation.y;
        let mut to = shown;
        if bottom > to + bounds.height {
            to = bottom - bounds.height;
        }
        if top < to {
            to = top;
        }
        let to = to.max(0.0);
        if (to - shown).abs() > 0.5 {
            self.to = Some(to);
        }
    }

    fn finish(&self) -> Outcome<()> {
        match self.to {
            Some(y) => Outcome::Chain(Box::new(scroll_to(
                page_id(),
                AbsoluteOffset {
                    x: None,
                    y: Some(y),
                },
            ))),
            None => Outcome::None,
        }
    }
}

/// The recovery-phrase field on the restore screen, which the screenshot
/// example focuses when it draws that screen with the keyboard up.
pub fn phrase_id() -> iced::widget::Id {
    iced::widget::Id::from("phrase")
}

/// The destination field of the spend form's row `n`, which the screenshot
/// example focuses when it draws Send with the keyboard up.
pub fn destination_id(n: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("destination-{n}"))
}

/// The memo field of the spend form's row `n`, which "Continue" brings into
/// view when it refuses the memo, and whose focus says when the person has
/// left it.
pub fn reference_id(n: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("reference-{n}"))
}

/// Which of the `rows` memo fields of the spend form shown holds the focus,
/// if one does: asked after each click, tap or Tab,
/// so that a memo the focus has left is checked.
pub fn focused_memo(rows: usize) -> impl Operation<Option<usize>> + 'static {
    FocusedMemo { rows, found: None }
}

struct FocusedMemo {
    rows: usize,
    found: Option<usize>,
}

impl Operation<Option<usize>> for FocusedMemo {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<usize>>)) {
        operate(self);
    }

    fn focusable(&mut self, id: Option<&Id>, _: Rectangle, state: &mut dyn Focusable) {
        if let Some(id) = id.filter(|_| state.is_focused()) {
            self.found = (0..self.rows).find(|n| reference_id(*n) == *id);
        }
    }

    fn finish(&self) -> Outcome<Option<usize>> {
        Outcome::Some(self.found)
    }
}

/// The height of the page `model` draws in, in logical pixels: the window's,
/// less the padding the safe area takes above and below (see [`view`]). It is
/// the height a phone's recovery phrase picks its frame by, so the
/// application reads it as the layout does.
pub(crate) fn page_height(model: &Model) -> f32 {
    let padding = root_padding(model.safe_area, model.phone() && IPHONE);
    model.height - padding.top - padding.bottom
}

/// The whole window for `model`. The safe area pads the root in every layout:
/// it is zero on the desktop, and a phone's system bars and keyboard are kept
/// clear of the content.
pub fn view(model: &Model) -> Element<'_, Message> {
    let page = if let Some(panicked) = model.stopped {
        stopped(model, panicked)
    } else {
        match &model.screen {
            Screen::Wallet(page) => wallet::view(model, page),
            _ => first_run::view(model),
        }
    };
    // On a phone the page is wrapped once, inside the safe area's padding
    // applied below. The wrapper draws nothing of its own, and it asks for the
    // keyboard again on a tap of a focused field while the keyboard is hidden.
    // The desktop has no soft keyboard,
    // and a touch screen there would have the wrapper withhold a frame's
    // request and end an input method's composition, so its page is not
    // wrapped.
    let page = if model.phone() {
        keyboard::reshow(page, model.safe_area.keyboard > 0.0)
    } else {
        page
    };
    container(page)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(root_padding(model.safe_area, model.phone() && IPHONE))
        .style(root_style(model))
        .into()
}

/// The colour of the window behind the page, which is all that shows in the
/// strips the system covers above and below it.
fn root_style(model: &Model) -> fn(&iced::Theme) -> iced::widget::container::Style {
    match (&model.stopped, &model.busy, &model.screen) {
        // A compact wallet's bars are the chrome colour, and the strips take
        // the same colour, so they read as part of the bars.
        (None, _, Screen::Wallet(_)) if model.compact() => theme::sidebar,
        // The welcome screen is the green brand field from edge to edge, so
        // the strips are green too, and the field does not stop short of the
        // status bar and the home indicator.
        (None, None, Screen::Welcome) => theme::brand_panel,
        _ => theme::page,
    }
}

/// What the worker is doing for the command the screen waits on, as a title
/// and a sentence: the opening screen, and the wallet page while it
/// refreshes.
fn activity(busy: &Busy) -> (&'static str, &'static str) {
    match busy.activity {
        None => (
            "Waiting",
            "The wallet does one thing at a time, and it is finishing what came before, usually a \
             question to the API. This starts when that is done; Cancel stops both.",
        ),
        Some(Activity::DerivingKey) => (
            "Opening the keystore",
            "Deriving the keystore's key from the password. It takes a few seconds and 64 MiB \
             of memory on purpose: that is what makes guessing a password expensive.",
        ),
        Some(Activity::AskingNode) => (
            "Reconciling",
            "Asking the API about each account and comparing the key index this keystore holds \
             with the one the chain shows.",
        ),
        Some(Activity::ReadingIndex) => (
            "Reading activity",
            "Asking the API's transaction index about each account, one request each. Nothing is \
             written.",
        ),
    }
}

/// How far the command has got, as the library counts it, once it has
/// said.
fn progress<'a>(busy: &Busy) -> Option<Element<'a, Message>> {
    let p = busy.progress?;
    let mut detail = column![t(
        format!("Account {} of {}", p.account + 1, p.accounts),
        ty::ROW_TITLE,
        color::TEXT_PRIMARY,
    )]
    .spacing(space::S8);
    if p.ceiling > 0 {
        // Positions are counted in `u32`; a bar needs only their ratio.
        #[allow(clippy::cast_precision_loss)]
        let (position, ceiling) = (p.position as f32, p.ceiling as f32);
        detail = detail
            .push(
                progress_bar(0.0..=ceiling, position)
                    .girth(4)
                    .style(theme::progress),
            )
            .push(ui::helper(format!(
                "{} of at most {} key positions searched",
                ui::group(&p.position.to_string()),
                ui::group(&p.ceiling.to_string())
            )));
    }
    Some(detail.into())
}

/// The worker has stopped: nothing more can be done in this run, apart from
/// saving the bytes of the spends signed in it.
fn stopped(model: &Model, panicked: bool) -> Element<'_, Message> {
    let body = if panicked {
        "The wallet's worker stopped because of a fault. Every secret it held was erased on \
         the way out: the keystore's key, its seed and any phrase being shown. Nothing was \
         written that had not been. Close Tawara and start it again."
    } else {
        "The wallet's worker could not start, or has stopped. Close Tawara and start it \
         again."
    };
    let mut parts = column![ui::card_for(
        model.compact(),
        column![
            ui::t(
                "THE WALLET STOPPED",
                ty::ONBOARDING_TITLE,
                color::TEXT_PRIMARY
            ),
            ui::t(body, ty::INTRO, color::TEXT_SECONDARY),
        ]
        .spacing(space::S12),
    )]
    .spacing(space::S16);
    if let Some(kept) = wallet::kept_after_stop(model) {
        parts = parts.push(kept);
    }
    if model.compact() {
        // The phone: the cards fill the width between 16-pixel margins.
        return scrollable(container(parts).padding(space::S16).width(Length::Fill))
            .id(page_id())
            .style(theme::scroll)
            .height(Length::Fill)
            .into();
    }
    container(scrollable(
        container(parts.max_width(640.0)).center_x(Length::Fill),
    ))
    .center(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// [`focused_memo`] run through the harness, which takes operations that
    /// give nothing back: what it found is kept in `found`.
    struct Probe {
        inner: FocusedMemo,
        found: Arc<Mutex<Option<usize>>>,
    }

    impl Operation for Probe {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }

        fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
            self.inner.focusable(id, bounds, state);
            *self.found.lock().unwrap() = self.inner.found;
        }
    }

    #[test]
    fn the_memo_a_click_focuses_is_the_one_found_and_none_once_it_is_left() {
        use crate::app::{DestinationRow, Page, SendPage, SendStage, SpendForm, WalletPage};
        use crate::harness::{Driver, bounds_of, mouse_move, mouse_press, mouse_release};
        let mut model = Model::new(Preferences::default(), None);
        model.width = 1440.0;
        model.wallet = Some(sample::wallet_view());
        let row = |memo: &str| DestinationRow {
            reference: memo.to_owned(),
            ..DestinationRow::default()
        };
        model.screen = Screen::Wallet(WalletPage {
            page: Page::Send(SendPage {
                from: Some(sample::wallet_view().accounts[0].id),
                form: SpendForm {
                    rows: vec![row("AB"), row("CD")],
                    ..SpendForm::default()
                },
                stage: SendStage::Compose,
            }),
            ..WalletPage::default()
        });
        let size = (1440, 1200);
        let second = bounds_of(view(&model), size, &reference_id(1)).expect("memo 2");
        let mut driver = Driver::new(view(&model), size);
        let found = |driver: &mut Driver<'_, Message>| {
            let found = Arc::new(Mutex::new(Some(99)));
            driver.operate(Box::new(Probe {
                inner: FocusedMemo {
                    rows: 2,
                    found: None,
                },
                found: found.clone(),
            }));
            *found.lock().unwrap()
        };
        let _ = driver.send(&[
            mouse_move(second.center_x(), second.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert_eq!(found(&mut driver), Some(1), "the second memo has the focus");
        // A click on nothing in particular leaves it.
        let _ = driver.send(&[mouse_move(1400.0, 1150.0), mouse_press(), mouse_release()]);
        assert_eq!(found(&mut driver), None, "no memo has the focus");
    }

    use iced::advanced::layout;
    use iced::advanced::widget::Tree;
    use iced::mobile::SafeArea;
    use iced::{Padding, Point, Rectangle, Size};
    use tawara_wallet_core::preferences::Preferences;
    use tawara_wallet_core::sample;

    use super::*;
    use crate::app::{Page, RestoreForm, UnlockForm, WalletPage};
    use crate::harness::{Driver, finger_lift, finger_press, redraw};
    use iced::time::Instant;
    use iced_runtime::user_interface::State;

    /// The phone's safe area: the status bar above, the home indicator below.
    fn phone_area(keyboard: f32) -> SafeArea {
        SafeArea::new(Padding {
            top: 47.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        })
        .with_keyboard(keyboard)
    }

    /// The sample store on the dashboard, in a phone of 390 by 844 with
    /// `keyboard` showing.
    fn phone_wallet(keyboard: f32) -> Model {
        let mut model = Model::new(Preferences::default(), None);
        model.width = 390.0;
        model.safe_area = phone_area(keyboard);
        model.wallet = Some(sample::wallet_view());
        model.screen = Screen::Wallet(WalletPage {
            page: Page::Dashboard,
            ..WalletPage::default()
        });
        model
    }

    /// The absolute bounds of every node of the window for `model` at 390 by
    /// 844, the root first.
    fn boxes(model: &Model) -> Vec<Rectangle> {
        let mut element = view(model);
        let renderer = crate::harness::renderer();
        let mut tree = Tree::new(element.as_widget());
        let limits = layout::Limits::new(Size::ZERO, Size::new(390.0, 844.0));
        let node = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let mut out = vec![node.bounds()];
        collect(&node, Point::ORIGIN, &mut out);
        out
    }

    fn collect(node: &layout::Node, origin: Point, out: &mut Vec<Rectangle>) {
        for child in node.children() {
            let bounds = child.bounds();
            let at = Point::new(origin.x + bounds.x, origin.y + bounds.y);
            out.push(Rectangle::new(at, bounds.size()));
            collect(child, at, out);
        }
    }

    #[test]
    fn the_welcome_screen_is_green_under_the_status_bar_and_the_home_indicator() {
        // The welcome screen's field reaches the window's edges: the strips
        // the safe area leaves are the brand field's green, not the page's
        // colour. Every other first-run screen keeps the page's colour there.
        let background = |screen: Screen| {
            let mut model = Model::new(Preferences::default(), None);
            model.width = 390.0;
            model.safe_area = phone_area(0.0);
            model.screen = screen;
            root_style(&model)(&iced::Theme::Dark).background
        };
        let green = Some(iced::Background::Color(color::ACCENT));
        assert_eq!(background(Screen::Welcome), green);
        assert_ne!(
            background(Screen::Unlock(UnlockForm::default())),
            green,
            "the unlock screen keeps the page's colour"
        );
        assert_ne!(
            background(Screen::Start {
                choice: crate::app::StartChoice::Create,
            }),
            green,
            "the start screen keeps the page's colour"
        );
    }

    #[test]
    fn a_phone_wallet_is_drawn_between_its_safe_area_insets() {
        let model = phone_wallet(0.0);
        let mut element = view(&model);
        let renderer = crate::harness::renderer();
        let mut tree = Tree::new(element.as_widget());
        let limits = layout::Limits::new(Size::ZERO, Size::new(390.0, 844.0));
        let root = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let page = &root.children()[0].bounds();
        assert_eq!(page.y, 47.0, "the page starts under the status bar");
        assert_eq!(
            page.y + page.height,
            810.0,
            "and ends above the home indicator"
        );
    }

    #[test]
    fn with_the_keyboard_up_nothing_is_drawn_under_it_and_the_tab_bar_goes() {
        // The page is a scrollable, so what is inside it runs past the window
        // and is clipped: what is checked is the shell, the children of the
        // root's first child, which is the frame of the wallet.
        let shell_bottom = |model: &Model| {
            let mut element = view(model);
            let renderer = crate::harness::renderer();
            let mut tree = Tree::new(element.as_widget());
            let limits = layout::Limits::new(Size::ZERO, Size::new(390.0, 844.0));
            let root = element
                .as_widget_mut()
                .layout(&mut tree, &renderer, &limits);
            let frame = root.children()[0].bounds();
            let mut lowest = frame.y + frame.height;
            for child in root.children()[0].children() {
                let bounds = child.bounds();
                lowest = lowest.max(frame.y + bounds.y + bounds.height);
            }
            lowest
        };
        let lowest = shell_bottom(&phone_wallet(336.0));
        assert!(
            lowest <= 508.0 + 0.5,
            "the frame ends at {lowest}, under the keyboard"
        );
        assert!(
            boxes(&phone_wallet(336.0)).len() < boxes(&phone_wallet(0.0)).len(),
            "the tab bar's nodes are not laid out while the keyboard shows"
        );
    }

    /// Where the page shows the focused field, in the window: its laid-out
    /// bounds less the page's scroll.
    fn focused_in_view(driver: &mut Driver<'_, Message>) -> Rectangle {
        /// The page's scroll, kept where the test reads it once the operation
        /// has run.
        struct Scroll(Arc<Mutex<Option<Vector>>>);
        impl Operation for Scroll {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&Id>,
                _: Rectangle,
                _: Rectangle,
                translation: Vector,
                _: &mut dyn Scrollable,
            ) {
                if id == Some(&page_id()) {
                    *self.0.lock().expect("not poisoned") = Some(translation);
                }
            }
        }
        let field = driver.focused_bounds().expect("a field has the focus");
        let scroll = Arc::new(Mutex::new(None));
        driver.operate(Box::new(Scroll(Arc::clone(&scroll))));
        let translation = scroll
            .lock()
            .expect("not poisoned")
            .expect("the page scrolls");
        Rectangle {
            y: field.y - translation.y,
            ..field
        }
    }

    #[test]
    fn with_the_keyboard_up_the_focused_phrase_field_is_scrolled_above_it() {
        // The restore screen on a phone of 390 by 844 with a keyboard of 336:
        // the keyboard's top is at 508, and the phrase field, the form's last,
        // lies below it until the page is scrolled.
        let mut model = Model::new(Preferences::default(), None);
        model.confine(PathBuf::from("/private"));
        model.width = 390.0;
        model.safe_area = phone_area(336.0);
        model.screen = Screen::Restore(RestoreForm::default());
        let mut driver = Driver::new(view(&model), (390, 844));
        driver.operate(Box::new(
            iced::advanced::widget::operation::focusable::focus(phrase_id()),
        ));
        let before = focused_in_view(&mut driver);
        assert!(
            before.y + before.height > 508.0,
            "the field starts under the keyboard: {before:?}"
        );
        driver.operate(Box::new(reveal_focused()));
        let after = focused_in_view(&mut driver);
        assert!(
            after.y >= 47.0 && after.y + after.height + space::S16 <= 508.0 + 0.5,
            "the field is in view above the keyboard: {after:?}"
        );
        // Once in view, a second reveal moves nothing.
        driver.operate(Box::new(reveal_focused()));
        assert_eq!(focused_in_view(&mut driver), after);
    }

    #[test]
    fn an_iphone_that_reports_no_inset_is_padded_by_the_forks_fallback() {
        let none = SafeArea::ZERO;
        assert_eq!(
            root_padding(none, true),
            UNREPORTED,
            "an iPhone that reports nothing"
        );
        assert_eq!(
            root_padding(none.with_keyboard(260.0), true),
            UNREPORTED.bottom(260.0),
            "and its keyboard still raises the bottom"
        );
        assert_eq!(
            root_padding(none, false),
            Padding::ZERO,
            "the desktop and Android: nothing"
        );
        // What an iPhone reports is used as it is, the notch held sideways
        // included.
        assert_eq!(
            root_padding(phone_area(0.0), true),
            phone_area(0.0).padding(0.0)
        );
        let sideways = SafeArea::new(Padding {
            top: 0.0,
            right: 47.0,
            bottom: 21.0,
            left: 47.0,
        });
        assert_eq!(root_padding(sideways, true), sideways.padding(0.0));
    }

    #[test]
    fn the_unlock_form_has_the_folder_field_on_the_desktop_only() {
        let form = || {
            Screen::Unlock(UnlockForm {
                dir: "/a/folder".to_owned(),
                ..UnlockForm::default()
            })
        };
        let mut desktop = Model::new(Preferences::default(), None);
        desktop.screen = form();
        let mut driver = Driver::new(view(&desktop), (1440, 900));
        assert_eq!(driver.focusable().0, 2, "the folder and the password");

        let mut phone = Model::new(Preferences::default(), None);
        phone.confine(PathBuf::from("/private"));
        phone.width = 390.0;
        phone.screen = form();
        let mut driver = Driver::new(view(&phone), (390, 844));
        assert_eq!(driver.focusable().0, 1, "the password alone");
    }

    #[test]
    fn only_a_phone_withholds_a_frames_request_after_a_tap_on_the_focused_field() {
        // The keyboard wrapper is a phone's alone: on the desktop a tap on the
        // focused field, from a touch screen,
        // leaves the field's request for the input method as it was.
        for phone in [true, false] {
            let mut model = Model::new(Preferences::default(), None);
            let size = if phone {
                model.confine(PathBuf::from("/private"));
                model.width = 390.0;
                (390, 844)
            } else {
                (1440, 900)
            };
            model.screen = Screen::Unlock(UnlockForm::default());
            let mut driver = Driver::new(view(&model), size);
            driver.focus_next();
            let at = driver.focused_bounds().expect("a field has the focus");
            let (x, y) = (at.center_x(), at.center_y());
            driver.send(&[finger_press(1, x, y), finger_lift(1, x, y)]);
            let (_, _, state) = driver.send(&[redraw(Instant::now())]);
            let State::Updated { input_method, .. } = state else {
                panic!("the interface was outdated, so it asked for nothing");
            };
            assert_eq!(
                input_method.is_enabled(),
                !phone,
                "phone {phone}: the frame after the tap"
            );
        }
    }

    #[test]
    fn tab_gives_a_phones_unlock_form_its_password_first() {
        // On a phone the password is the only field, so Tab's first focus is on
        // it.
        let mut phone = Model::new(Preferences::default(), None);
        phone.confine(PathBuf::from("/private"));
        phone.width = 390.0;
        phone.screen = Screen::Unlock(UnlockForm::default());
        let mut driver = Driver::new(view(&phone), (390, 844));
        driver.focus_next();
        assert_eq!(driver.focusable(), (1, Some(0)));
    }
}
