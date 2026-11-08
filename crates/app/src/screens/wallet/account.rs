// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! An account: its figures, and the panel for its state with the actions
//! that state allows. Nothing on this page moves a key index: a diverged
//! account's remedy is the account-recovery screen, reached deliberately,
//! or, for a settle a reorganisation reverted, resending its saved copy from
//! the Submit page.

use iced::widget::{column, container, row};
use iced::{Alignment, Element, Length};
use tawara_wallet_core::view::{AccountRow, AccountState, DivergenceKind, ReservationState};

use super::{
    NEW_ACCOUNT, action, balance_text, destination, destination_with_copy, figure, frame, kind,
    name, report, row_of, status,
};
use crate::app::{AccountPage, Message, Model, ReportKey, To, WalletMsg, WalletPage};
use crate::icon::Icon;
use crate::theme::{self, color, space as sp};
use crate::ui::{self, Size, t, ty};

pub fn view<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    a: &'a AccountPage,
) -> Element<'a, Message> {
    let Some(account) = row_of(model, a.account) else {
        return frame(
            model,
            page,
            "Account",
            "This keystore holds no such account.",
            Vec::new(),
            Vec::new(),
        );
    };
    let shown = destination(account.id);
    let (word, ink) = status(account);
    let figures = ui::card_for(
        model.compact(),
        column![
            ui::section_label("Destination"),
            destination_with_copy(
                page,
                shown,
                ty::MONO_WORD,
                color::TEXT_PRIMARY,
                iced::widget::Id::new("account-copy"),
            ),
            ui::divider(),
            figure("Balance", balance_text(model, account, "not known", true)),
            figure("Next key", format!("#{}", account.index)),
            figure("Origin", kind(account)),
            row![
                t("State", ty::BODY_SMALL, color::TEXT_SECONDARY).width(Length::Fill),
                ui::dot(ink, 6.0),
                t(word, ty::TABLE_BODY, ink),
            ]
            .spacing(sp::S6)
            .align_y(Alignment::Center),
        ]
        .spacing(sp::S12),
    );
    let mut body = vec![figures.into(), panel(model, page, account)];
    if let Some((done, text)) = &a.report {
        body.push(report::show_in(
            model.compact(),
            page,
            ReportKey::Done,
            report::done(*done, account, text),
        ));
    }
    // The name is part of a Base58 destination, so it is not set in the
    // title's capitals.
    frame(
        model,
        page,
        "Account",
        format!("{} · {}", name(account.id), kind(account)),
        vec![action(
            model,
            "Check now",
            theme::Button::Secondary,
            Size::Header,
            Some(Icon::Refresh),
            WalletMsg::Check(account.id),
        )],
        body,
    )
}

/// The panel for the account's state: what it means, and the actions it
/// allows.
fn panel<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    account: &'a AccountRow,
) -> Element<'a, Message> {
    let id = account.id;
    let button = |label: &'a str, variant, msg: WalletMsg| {
        action(model, label, variant, Size::Medium, None, msg)
    };
    let (callout, buttons): (Element<'a, Message>, Vec<Element<'a, Message>>) = match &account.state
    {
        // A new account: nothing to review, and Receive to be paid.
        AccountState::Diverged { .. } if account.is_new() => (
            ui::accent_callout(Icon::ShieldCheck, "New Account", NEW_ACCOUNT),
            vec![button(
                "Receive",
                theme::Button::Secondary,
                WalletMsg::Open(To::Receive(Some(id))),
            )],
        ),
        AccountState::InSync { .. } if account.spendable => (
            ui::accent_callout(
                Icon::ShieldCheck,
                "Address confirmed",
                "The chain holds this account at the key this keystore expects.",
            ),
            vec![
                button(
                    "Send from it",
                    theme::Button::Primary,
                    WalletMsg::Open(To::Send(Some(id))),
                ),
                button(
                    "Receive",
                    theme::Button::Secondary,
                    WalletMsg::Open(To::Receive(Some(id))),
                ),
            ],
        ),
        // In sync now, and set aside when the wallet opened: it spends only
        // once a refresh opens the wallet with it.
        AccountState::InSync { .. } => (
            ui::accent_callout(
                Icon::ShieldCheck,
                "Address confirmed, not yet spendable",
                "The chain holds this account at the key this keystore expects. It was set aside \
                 when the wallet opened, so it spends again once Refresh on the wallet's page \
                 opens the wallet with it.",
            ),
            vec![button(
                "Receive",
                theme::Button::Secondary,
                WalletMsg::Open(To::Receive(Some(id))),
            )],
        ),
        AccountState::SpendOutstanding { reservation, .. } => {
            let why = match reservation {
                ReservationState::Live => {
                    "A spend from this account is signed and waiting to land. Settle once the \
                     chain shows it landed; until then the account does not spend again. If \
                     the signed bytes were lost, re-sign the same spend, or submit the bytes \
                     saved from the sent page."
                }
                // A dead reservation never settles, so its text promises no
                // settle. Its last sentence is the library's wording for a
                // dead reservation.
                ReservationState::Dead {
                    balance_moved: true,
                    ..
                } => {
                    "This reservation can no longer be accepted: the balance it was built on \
                     has moved. Re-signing would reproduce bytes the ledger refuses. A dead \
                     reservation has no route out."
                }
                ReservationState::Dead { .. } => {
                    "This reservation can no longer be accepted: its block-to-live has passed. \
                     Re-signing would reproduce bytes the ledger refuses. A dead reservation \
                     has no route out."
                }
                ReservationState::Unrecorded => {
                    "A spend from this account is reserved, and the keystore records no figures \
                     for it (a keystore from before they were recorded). Check now for the wallet \
                     library's report; settle once the chain shows it landed."
                }
                ReservationState::Unclassified => {
                    "A spend from this account is reserved, and its expiry could not be read. \
                     Check now for the wallet library's report on it; settle once the chain \
                     shows it landed."
                }
            };
            let mut buttons = Vec::new();
            if settle_offered(&account.state) {
                buttons.push(button(
                    "Settle",
                    theme::Button::WarningPrimary,
                    WalletMsg::Settle(id),
                ));
            }
            buttons.push(button(
                "Re-sign",
                theme::Button::Secondary,
                WalletMsg::Open(To::Resign(id)),
            ));
            buttons.push(button(
                "Submit saved artifact",
                theme::Button::Secondary,
                WalletMsg::Open(To::Submit),
            ));
            (
                ui::warning_callout("Reserved and not settled", why),
                buttons,
            )
        }
        AccountState::SpendLanded { .. } => (
            ui::warning_callout(
                "The spend landed",
                "The chain shows the change key: the spend from this account landed. Settle \
                 records it and frees the account to spend again.",
            ),
            // The fallback for a settle the automatic step did not make.
            vec![button(
                "Settle",
                theme::Button::WarningPrimary,
                WalletMsg::Settle(id),
            )],
        ),
        AccountState::Diverged {
            kind,
            report: text,
            advance_to,
            rollback_to: _,
            reverted_settle,
        } => {
            // A store that did not open refuses the saved copy's command, so the
            // button needs an open wallet.
            let opened = model.wallet.as_ref().is_some_and(|w| w.opened);
            let parts = column![report::show_in(
                model.compact(),
                page,
                ReportKey::Diverged,
                report::diverged(*kind, text)
            )]
            .spacing(sp::S12);
            // Account recovery, from here or from Settings.
            let recover = || {
                button(
                    "Account recovery",
                    theme::Button::WarningTonal,
                    WalletMsg::Open(To::Recovery),
                )
            };
            (
                parts.into(),
                match kind {
                    // A settle the chain reverted leaves the account one key
                    // behind with its settle retained: the saved copy may be
                    // resent from the Submit page in an open wallet, and
                    // nothing else here moves the account.
                    DivergenceKind::Behind { gap: 1 } if *reverted_settle && opened => {
                        vec![button(
                            "Submit saved artifact",
                            theme::Button::Secondary,
                            WalletMsg::Open(To::Resubmit(id)),
                        )]
                    }
                    DivergenceKind::NotFound => vec![button(
                        "Receive to it again",
                        theme::Button::Secondary,
                        WalletMsg::Open(To::Receive(Some(id))),
                    )],
                    _ if advance_to.is_some() => vec![recover()],
                    DivergenceKind::Unlocated => vec![recover()],
                    _ => Vec::new(),
                },
            )
        }
        AccountState::NotReconciled => (
            ui::warning_callout(
                "Not yet confirmed",
                "No API has been asked about this account, so its balance and state are not known \
                 and it cannot spend. Choose an API, or Refresh once it answers.",
            ),
            Vec::new(),
        ),
    };
    let mut panel = column![callout].spacing(sp::S16);
    if !buttons.is_empty() {
        if model.compact() {
            // On a phone the buttons wrap onto the next line when the row is
            // too narrow. Each sits in a container with an id so the tests can
            // find where it is drawn.
            let buttons = buttons
                .into_iter()
                .enumerate()
                .map(|(n, b)| Element::from(container(b).id(button_id(n))));
            panel = panel.push(
                iced::widget::Row::with_children(buttons)
                    .spacing(sp::S10)
                    .wrap()
                    .vertical_spacing(sp::S10),
            );
        } else {
            let mut line = row![].spacing(sp::S10);
            for b in buttons {
                line = line.push(b);
            }
            panel = panel.push(line);
        }
    }
    ui::card_for(model.compact(), panel).into()
}

/// Whether the account's panel offers Settle: on a landed spend, as the
/// fallback for a settle the automatic step did not make, and on an
/// outstanding spend whose reservation can still land. A dead reservation
/// never settles, so it is not offered there.
fn settle_offered(state: &AccountState) -> bool {
    match state {
        AccountState::SpendLanded { .. } => true,
        AccountState::SpendOutstanding { reservation, .. } => {
            !matches!(reservation, ReservationState::Dead { .. })
        }
        _ => false,
    }
}

/// The id of the `n`-th button of a state panel on a phone. It draws nothing:
/// the tests find where the button is drawn by it.
fn button_id(n: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("account-panel-button-{n}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AccountPage, Level, NARROWEST, Page, Screen};
    use crate::harness::{Driver, fits, mouse_move, mouse_press, mouse_release, texts};
    use iced::advanced::widget::Operation;
    use iced_runtime::user_interface::{Cache, UserInterface};
    use tawara_wallet_core::preferences::Preferences;
    use tawara_wallet_core::sample;
    use tawara_wallet_core::view::{AccountId, WalletView};

    /// A phone's window, tall enough that the whole panel is drawn.
    const TALL: (u32, u32) = (360, 1200);

    /// The bounds in the window of the container that carries `id`, as `root`
    /// draws it in a window of `size`.
    fn bounds_of(
        root: Element<'_, Message>,
        size: (u32, u32),
        id: &iced::widget::Id,
    ) -> iced::Rectangle {
        find_bounds(root, size, id).expect("the container is drawn")
    }

    /// The bounds of the container that carries `id`, or `None` when `root`
    /// draws none.
    fn find_bounds(
        root: Element<'_, Message>,
        size: (u32, u32),
        id: &iced::widget::Id,
    ) -> Option<iced::Rectangle> {
        struct Find<'a> {
            id: &'a iced::widget::Id,
            found: Option<iced::Rectangle>,
        }
        impl Operation for Find<'_> {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }

            fn container(&mut self, id: Option<&iced::widget::Id>, bounds: iced::Rectangle) {
                if id == Some(self.id) {
                    self.found = Some(bounds);
                }
            }
        }
        let mut renderer = crate::harness::renderer();
        let bounds = iced::Size::new(size.0 as f32, size.1 as f32);
        let mut ui = UserInterface::build(root, bounds, Cache::default(), &mut renderer);
        let mut find = Find { id, found: None };
        ui.operate(&renderer, &mut find);
        find.found
    }

    /// The store `wallet` on the account page for `account`, on a phone 360
    /// logical pixels wide, with one report opened when `open` is given.
    fn phone(wallet: WalletView, account: AccountId, open: Option<(ReportKey, Level)>) -> Model {
        let mut model = Model::new(Preferences::default(), None);
        model.width = NARROWEST;
        model.wallet = Some(wallet);
        let mut page = WalletPage {
            page: Page::Account(AccountPage {
                account,
                report: None,
            }),
            ..WalletPage::default()
        };
        if let Some((key, level)) = open {
            page.open.insert(key, level);
        }
        model.screen = Screen::Wallet(page);
        model
    }

    #[test]
    fn the_phone_account_pages_fit_at_360_in_each_state_the_example_draws() {
        let wallet = sample::wallet_view();
        let accounts = wallet.accounts.clone();
        // The second account in sync now, and set aside when the wallet opened,
        // as the example draws it.
        let mut aside = sample::wallet_view();
        aside.accounts[1].state = AccountState::InSync {
            balance: 3_180_000_000_000,
        };
        aside.accounts[1].spendable = false;
        // The third account's settle reverted, with its saved copy to send
        // again: the panel draws one button more.
        let mut reverted = sample::wallet_view();
        reverted.accounts[2].state = reverted_settle();
        let cases = [
            ("reconciled", phone(wallet.clone(), accounts[0].id, None)),
            ("outstanding", phone(wallet.clone(), accounts[1].id, None)),
            ("set aside", phone(aside, accounts[1].id, None)),
            ("diverged", phone(wallet.clone(), accounts[2].id, None)),
            (
                "diverged, summarized",
                phone(
                    wallet.clone(),
                    accounts[2].id,
                    Some((ReportKey::Diverged, Level::Summary)),
                ),
            ),
            (
                "diverged, whole",
                phone(
                    wallet.clone(),
                    accounts[2].id,
                    Some((ReportKey::Diverged, Level::Full)),
                ),
            ),
            (
                "diverged, reverted settle",
                phone(reverted, accounts[2].id, None),
            ),
        ];
        for (label, model) in cases {
            assert_eq!(
                fits(crate::screens::view(&model), NARROWEST),
                Ok(()),
                "{label}"
            );
        }
    }

    #[test]
    fn the_landed_and_dead_panels_fit_at_360() {
        let accounts = sample::wallet_view().accounts;
        let (landed, dead) = landed_and_dead();
        for (label, wallet) in [("landed", landed), ("dead", dead)] {
            let model = phone(wallet, accounts[1].id, None);
            assert_eq!(
                fits(crate::screens::view(&model), NARROWEST),
                Ok(()),
                "{label}"
            );
        }
    }

    #[test]
    fn a_landed_spend_offers_settle_and_a_dead_reservation_does_not() {
        // On a phone each button of the panel is drawn with its id: a landed
        // spend has Settle alone; a dead reservation has Re-sign and Submit
        // saved artifact, and no third button.
        let accounts = sample::wallet_view().accounts;
        let (landed, dead) = landed_and_dead();
        let drawn = |wallet: WalletView| {
            let model = phone(wallet, accounts[1].id, None);
            (0..3)
                .filter(|&n| {
                    find_bounds(crate::screens::view(&model), TALL, &button_id(n)).is_some()
                })
                .count()
        };
        assert_eq!(drawn(landed), 1, "Settle, the fallback");
        assert_eq!(drawn(dead), 2, "no Settle");
    }

    #[test]
    fn settle_is_offered_for_a_landed_spend_and_a_live_one_never_for_a_dead_one() {
        let landed = AccountState::SpendLanded {
            balance: 1,
            spent_index: 1,
            settled_index: 2,
        };
        let outstanding = |reservation| AccountState::SpendOutstanding {
            balance: 1,
            spent_index: 1,
            reservation,
        };
        let dead = |balance_moved, expired| {
            outstanding(ReservationState::Dead {
                balance_moved,
                expired,
            })
        };
        assert!(settle_offered(&landed));
        assert!(settle_offered(&outstanding(ReservationState::Live)));
        assert!(settle_offered(&outstanding(ReservationState::Unrecorded)));
        assert!(!settle_offered(&dead(true, false)));
        assert!(!settle_offered(&dead(false, true)));
        assert!(!settle_offered(&AccountState::InSync { balance: 1 }));
    }

    /// The sample store with the second account's spend landed, and the same
    /// store with that account's reservation dead, the balance it was built
    /// on having moved.
    fn landed_and_dead() -> (WalletView, WalletView) {
        let mut dead = sample::wallet_view();
        dead.accounts[1].state = AccountState::SpendOutstanding {
            balance: 3_180_000_000_000,
            spent_index: 6,
            reservation: ReservationState::Dead {
                balance_moved: true,
                expired: false,
            },
        };
        (sample::landed_view(), dead)
    }

    #[test]
    fn on_a_phone_the_state_buttons_wrap_onto_a_second_line() {
        // Outstanding: Settle, Re-sign and Submit saved artifact. The first two
        // share a line, and the third is too wide for that line at 360, so it
        // wraps. `fits` cannot see this: a row squeezes a child rather than
        // pushing it past the edge.
        let wallet = sample::wallet_view();
        let model = phone(wallet.clone(), wallet.accounts[1].id, None);
        let at = |n: usize| bounds_of(crate::screens::view(&model), TALL, &button_id(n));
        let (first, second, third) = (at(0), at(1), at(2));
        assert_eq!(second.y, first.y, "Re-sign sits beside Settle");
        assert!(
            third.y > first.y,
            "Submit saved artifact wraps onto a line of its own"
        );
    }

    /// The state of an account whose settle a reorganisation reverted: one key
    /// behind the store, with its settle retained and no reservation.
    fn reverted_settle() -> AccountState {
        AccountState::Diverged {
            kind: DivergenceKind::Behind { gap: 1 },
            report: "This store recorded a SETTLE at index 4".to_owned(),
            advance_to: None,
            rollback_to: None,
            reverted_settle: true,
        }
    }

    /// The text `model` draws, the buttons' labels among it.
    fn drawn(model: &Model) -> Vec<String> {
        texts(crate::screens::view(model), (NARROWEST, 1200.0))
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    #[test]
    fn a_reverted_settle_offers_submit_saved_artifact_and_no_remedy() {
        // The saved copy of a settle that a reorganisation reverted may be
        // resent with Submit saved artifact, which opens the Submit page in
        // resubmit mode for this account. Nothing here moves a key index, and
        // the recovery page offers no remedy.
        let mut wallet = sample::wallet_view();
        let id = wallet.accounts[2].id;
        wallet.accounts[2].state = reverted_settle();
        assert_eq!(crate::app::remedy(&wallet.accounts[2].state), None);
        let model = phone(wallet, id, None);
        assert!(
            drawn(&model).contains(&"Submit saved artifact".to_owned()),
            "{:?}",
            drawn(&model)
        );
        assert_eq!(
            find_bounds(crate::screens::view(&model), TALL, &button_id(1)),
            None,
            "it is the only button"
        );
        // Tapping it opens the Submit page in resubmit mode for this account.
        let at = bounds_of(crate::screens::view(&model), TALL, &button_id(0));
        let mut driver = Driver::new(crate::screens::view(&model), TALL);
        let (messages, _, _) = driver.send(&[
            mouse_move(at.center_x(), at.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Wallet(WalletMsg::Open(To::Resubmit(account)))] if *account == id
            ),
            "{messages:?}"
        );
    }

    #[test]
    fn the_resubmit_button_is_drawn_only_for_a_settle_the_chain_reverted() {
        // Only one case offers it: one key behind, with the settle retained.
        // Every other diverged state draws no such button.
        let cases = [
            (DivergenceKind::Behind { gap: 1 }, false),
            (DivergenceKind::Behind { gap: 2 }, true),
            (DivergenceKind::Ahead { gap: 1 }, true),
            (DivergenceKind::Unlocated, false),
        ];
        for (kind, reverted_settle) in cases {
            let mut wallet = sample::wallet_view();
            let id = wallet.accounts[2].id;
            wallet.accounts[2].state = AccountState::Diverged {
                kind,
                report: String::new(),
                advance_to: None,
                rollback_to: None,
                reverted_settle,
            };
            let model = phone(wallet, id, None);
            assert!(
                !drawn(&model).contains(&"Submit saved artifact".to_owned()),
                "{kind:?}, with its settle retained: {reverted_settle}"
            );
        }
    }

    #[test]
    fn a_store_that_did_not_open_offers_no_saved_copy() {
        // A store whose only account diverged this way does not open, and the
        // command is refused in that session, so no button is drawn there. The
        // same account draws it once the wallet is open.
        let mut wallet = sample::wallet_view();
        let id = wallet.accounts[2].id;
        wallet.accounts[2].state = reverted_settle();
        assert!(
            drawn(&phone(wallet.clone(), id, None)).contains(&"Submit saved artifact".to_owned()),
            "an open wallet offers it"
        );
        wallet.opened = false;
        let model = phone(wallet, id, None);
        assert!(
            !drawn(&model).contains(&"Submit saved artifact".to_owned()),
            "{:?}",
            drawn(&model)
        );
    }
}
