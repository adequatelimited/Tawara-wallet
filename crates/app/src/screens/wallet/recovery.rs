// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Account recovery: what the chain shows for every account, and the one
//! action that applies to each account the library can act on.
//!
//! An account the chain shows ahead of this keystore, because another install
//! of the same recovery phrase spent from it, is resynced on its own (see
//! `App::resync`); Resync here does the same at once. An account the chain
//! shows behind this keystore can be rolled back to the index the chain shows,
//! once the person confirms it. An account whose chain position was not among
//! the keys searched can be searched further with Find live index. The library
//! moves a key index only to the index its live report names.
//!
//! On a phone each account's head is a column, with its action beneath.

use iced::widget::{column, container, row};
use iced::{Alignment, Element, Length};
use tawara_wallet_core::AccountReport;

use super::{action, frame, kind, name, report, row_of, state_status, status};
use crate::app::{
    Message, Model, RecoveryPage, Remedy, ReportKey, SEARCH_FURTHER_TO, WalletMsg, WalletPage,
    remedy,
};
use crate::theme::{self, color, space as sp};
use crate::ui::{self, Size, t, ty};

pub fn view<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    r: &'a RecoveryPage,
) -> Element<'a, Message> {
    let compact = model.compact();
    let mut body: Vec<Element<'a, Message>> = Vec::new();
    match &r.reports {
        None => body.push(
            ui::card_for(
                compact,
                ui::helper(if model.busy.is_some() {
                    "Reading every account from the chain…"
                } else {
                    "Not read yet: Read again asks the chain about every account."
                }),
            )
            .into(),
        ),
        Some(reports) => {
            let mut list = column![ui::section_label("What the chain shows for every account")]
                .spacing(sp::S12);
            for (n, a) in reports.iter().enumerate() {
                list = list.push(account(model, page, r, n, a));
            }
            body.push(list.into());
            if !reports.iter().any(|a| remedy(&a.state).is_some()) {
                body.push(
                    ui::card_for(
                        compact,
                        ui::helper(
                            "Every account is where the chain shows it, or has no action that \
                             applies: there is nothing to recover here.",
                        ),
                    )
                    .into(),
                );
            }
        }
    }
    if let Some((ok, text)) = &r.result {
        body.push(report::show_in(
            compact,
            page,
            ReportKey::Advanced,
            report::advanced(*ok, text),
        ));
    }
    frame(
        model,
        page,
        "Account recovery",
        "What the chain shows for every account, and what can be done for each",
        vec![action(
            model,
            "Read again",
            theme::Button::Secondary,
            Size::Header,
            Some(crate::icon::Icon::Refresh),
            WalletMsg::ReviewAll,
        )],
        body,
    )
}

/// One account: its name and state, its report whole, and its action when
/// one applies.
fn account<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    r: &'a RecoveryPage,
    n: usize,
    a: &'a AccountReport,
) -> Element<'a, Message> {
    let compact = model.compact();
    let (word, ink) = match row_of(model, a.account) {
        Some(row) if row.is_new() => status(row),
        _ => state_status(&a.state),
    };
    let kind_word = row_of(model, a.account).map_or_else(String::new, kind);
    let name_and_kind = column![
        t(name(a.account), ty::ROW_TITLE, color::TEXT_PRIMARY),
        t(kind_word, ty::TINY, color::TEXT_MUTED),
    ]
    .spacing(sp::S2)
    .width(Length::Fill);
    let state = row![ui::dot(ink, 6.0), t(word, ty::TABLE_BODY, ink)]
        .spacing(sp::S6)
        .align_y(Alignment::Center);
    let remedy_action: Option<Element<'a, Message>> = remedy(&a.state).map(|m| match m {
        Remedy::Advance(to) => ui::button_with(
            format!("Resync to key {}", ui::group(&to.to_string())),
            theme::Button::Primary,
            Size::Small,
            Some(crate::icon::Icon::Refresh),
            model
                .busy
                .is_none()
                .then(|| WalletMsg::Resync(a.account).into()),
        )
        .into(),
        Remedy::Rollback(to) => ui::button_with(
            format!("Roll back to key {}", ui::group(&to.to_string())),
            theme::Button::WarningOutline,
            Size::Small,
            None,
            (model.busy.is_none() && r.rollback.is_none())
                .then(|| WalletMsg::Rollback(a.account).into()),
        )
        .into(),
        Remedy::SearchFurther => action(
            model,
            "Find live index",
            theme::Button::Secondary,
            Size::Small,
            Some(crate::icon::Icon::Search),
            WalletMsg::SearchFurther(a.account),
        ),
    });
    let note: Option<Element<'a, Message>> = match (remedy(&a.state), r.rollback) {
        (_, Some((asked, to))) if asked == a.account => Some(confirm_rollback(model, to)),
        (Some(Remedy::SearchFurther), _) => Some(
            ui::helper(format!(
                "Searches keys 0 to {}, which can take a few minutes.",
                ui::group(&SEARCH_FURTHER_TO.to_string())
            ))
            .into(),
        ),
        _ => None,
    };
    // On a phone the head is a column: the name, the state beneath it, then
    // the action.
    let head: Element<'a, Message> = if compact {
        let mut head = column![name_and_kind, state].spacing(sp::S10);
        if let Some(remedy_action) = remedy_action {
            head = head.push(remedy_action);
        }
        head.into()
    } else {
        let mut head = row![name_and_kind, state]
            .spacing(sp::S12)
            .align_y(Alignment::Center);
        if let Some(remedy_action) = remedy_action {
            head = head.push(remedy_action);
        }
        head.into()
    };
    let key = ReportKey::Review(u16::try_from(n).unwrap_or(u16::MAX));
    let mut body = column![head].spacing(sp::S12);
    if let Some(note) = note {
        body = body.push(note);
    }
    let body = body.push(report::show_in(compact, page, key, report::review(a)));
    if compact {
        ui::card_for(true, body).into()
    } else {
        ui::card(body).padding(sp::S18).into()
    }
}

/// The rollback asked for, to confirm: the index the chain shows, what
/// rolling back risks, and the two choices.
fn confirm_rollback<'a>(model: &'a Model, to: u32) -> Element<'a, Message> {
    let to = ui::group(&to.to_string());
    let idle = model.busy.is_none();
    let buttons = row![
        ui::button_with(
            format!("Roll back to key {to}"),
            theme::Button::WarningPrimary,
            Size::Medium,
            None,
            idle.then(|| WalletMsg::ConfirmRollback.into()),
        ),
        ui::button_with(
            "Cancel",
            theme::Button::Secondary,
            Size::Medium,
            None,
            idle.then(|| WalletMsg::CancelRollback.into()),
        ),
    ]
    .spacing(sp::S12)
    .wrap()
    .vertical_spacing(sp::S8);
    container(
        column![
            t(
                format!(
                    "Live index on the network is {to}, and the local index is ahead of it. Are \
                     you sure you want to rollback your index to {to}? If the network is \
                     unstable it is usually safer to wait a couple hours, as rolling the Index \
                     backward may allow a double-spend of a one-time use key. A third-spend \
                     completely erodes account security. So proceed cautiously."
                ),
                ty::BODY_SMALL,
                color::TEXT_ON_WARNING_SOFT,
            )
            .width(Length::Fill),
            buttons,
        ]
        .spacing(sp::S14),
    )
    .padding(if model.compact() { sp::S16 } else { sp::S20 })
    .width(Length::Fill)
    .style(theme::callout_warning)
    .into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use tawara_wallet_core::preferences::Preferences;
    use tawara_wallet_core::sample;
    use tawara_wallet_core::{NetworkName, SyncState};

    use super::*;
    use crate::app::{Level, NARROWEST, NodeState, Page, Screen, WalletPage};
    use crate::harness::fits;
    use crate::screens;
    use crate::ui::Zone;

    /// The node and the folder the sample store is kept in, as the screenshot
    /// example names them.
    const NODE: &str = "https://node.example";
    const DIR: &str = "/home/you/.local/share/tawara/keystore";

    /// The sample store on `page`, drawn at the narrowest phone width, built as
    /// the screenshot example builds it.
    fn sample_model(page: Page) -> Model {
        let mut model = Model::new(Preferences::default(), Some(PathBuf::from(DIR)));
        model.node = NodeState {
            url: Some(NODE.to_owned()),
            tip: Some((871_173, Duration::from_millis(42))),
            solved_ms: Some(sample::TIP_MS),
            sync: Some(SyncState {
                stage: "synchronized".to_owned(),
                synced: true,
            }),
            network: Some(NetworkName {
                blockchain: "mochimo".to_owned(),
                network: "mainnet".to_owned(),
            }),
            error: None,
        };
        model.clock_ms = sample::TIP_MS + 48_000;
        model.zone = Zone::Fixed(0);
        model.width = NARROWEST;
        model.wallet = Some(sample::wallet_view());
        model.screen = Screen::Wallet(WalletPage {
            page,
            ..WalletPage::default()
        });
        model
    }

    /// The recovery screen with every account's report, as the screenshot
    /// example builds it: the other two reports at `others` and the paused
    /// one at `paused`.
    fn recovery(others: Level, paused: Level) -> Model {
        let reports = sample::review();
        let mut model = sample_model(Page::Recovery(RecoveryPage {
            reports: Some(reports),
            rollback: None,
            result: None,
        }));
        if let Screen::Wallet(page) = &mut model.screen {
            for n in 0..2u16 {
                page.open.insert(ReportKey::Review(n), others);
            }
            page.open.insert(ReportKey::Review(2), paused);
        }
        model
    }

    #[test]
    fn the_recovery_screens_fit_a_phone_of_the_narrowest_width() {
        let screens_to_draw = [
            ("w12-recovery", recovery(Level::Summary, Level::Summary)),
            ("w12-recovery-full", recovery(Level::Full, Level::Full)),
            (
                "w12-recovery-closed",
                recovery(Level::Closed, Level::Summary),
            ),
        ];
        for (screen, model) in screens_to_draw {
            assert_eq!(fits(screens::view(&model), NARROWEST), Ok(()), "{screen}");
        }
    }

    #[test]
    fn the_rollback_confirmation_fits_a_phone_and_says_the_index() {
        let mut reports = sample::review();
        let behind = reports[2].account;
        reports[2].state = tawara_wallet_core::view::AccountState::Diverged {
            kind: tawara_wallet_core::view::DivergenceKind::Behind { gap: 2 },
            report: "behind".to_owned(),
            advance_to: None,
            rollback_to: Some(31),
            reverted_settle: false,
        };
        let model = sample_model(Page::Recovery(RecoveryPage {
            reports: Some(reports),
            rollback: Some((behind, 31)),
            result: None,
        }));
        assert_eq!(fits(screens::view(&model), NARROWEST), Ok(()));
        let warning = "Live index on the network is 31, and the local index is ahead of it. Are \
                       you sure you want to rollback your index to 31? If the network is \
                       unstable it is usually safer to wait a couple hours, as rolling the Index \
                       backward may allow a double-spend of a one-time use key. A third-spend \
                       completely erodes account security. So proceed cautiously.";
        let _ = lies_on_a_phone(&model, warning);
    }

    /// What a window draws as text, and where each text lies.
    #[derive(Default)]
    struct Texts(Vec<(String, iced::Rectangle)>);

    impl iced::advanced::widget::Operation for Texts {
        fn traverse(
            &mut self,
            operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation),
        ) {
            operate(self);
        }

        fn text(
            &mut self,
            _: Option<&iced::advanced::widget::Id>,
            bounds: iced::Rectangle,
            text: &str,
        ) {
            self.0.push((text.to_owned(), bounds));
        }
    }

    /// Where the one text that says `text` lies in a phone's window.
    fn lies_on_a_phone(model: &Model, text: &str) -> iced::Rectangle {
        let mut renderer = crate::harness::renderer();
        let mut ui = iced_runtime::user_interface::UserInterface::build(
            screens::view(model),
            iced::Size::new(NARROWEST, 844.0),
            iced_runtime::user_interface::Cache::default(),
            &mut renderer,
        );
        let mut texts = Texts::default();
        ui.operate(&renderer, &mut texts);
        let found: Vec<iced::Rectangle> = texts
            .0
            .iter()
            .filter(|(drawn, _)| drawn == text)
            .map(|(_, at)| *at)
            .collect();
        assert_eq!(found.len(), 1, "`{text}` is drawn {} times", found.len());
        found[0]
    }

    #[test]
    fn on_a_phone_each_head_puts_its_state_beneath_its_name() {
        // On the desktop the state sits beside the name; on a phone it starts
        // at the name's left edge, below the name's text. Its word follows a
        // dot of 6 pixels and a gap of 6, so it starts within 16 pixels of it.
        let model = recovery(Level::Summary, Level::Summary);
        let reports = sample::review();
        let name_at = lies_on_a_phone(&model, &name(reports[0].account));
        let state_at = lies_on_a_phone(&model, "Address confirmed");
        assert!(
            state_at.x < name_at.x + 16.0 && state_at.y > name_at.y,
            "the state is not beneath the name: {state_at:?} and {name_at:?}"
        );
    }
}
