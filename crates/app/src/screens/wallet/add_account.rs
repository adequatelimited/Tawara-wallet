// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Add an account: a sweep of the derived accounts this store's seed makes,
//! what the chain holds for each, and adding the ones it holds that the store
//! does not. The library adds a derived account only where the chain already
//! shows it, so a new one is paid first, at the destination the sweep shows
//! for it, and added after.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text_input};
use iced::{Alignment, Element, Length, Padding};
use tawara_wallet_core::Discovered;

use super::{action, copy, frame, report, short};
use crate::app::{AddAccountPage, AutoAdd, Message, Model, ReportKey, WalletMsg, WalletPage};
use crate::icon::Icon;
use crate::theme::{self, color, radius, space as sp};
use crate::ui::{self, Size, t, ty};

pub fn view<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    a: &'a AddAccountPage,
) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let compact = model.compact();
    let mut to = text_input("", &a.to)
        .font(ty::MONO.font)
        .size(ty::MONO.size)
        .padding(Padding::from([sp::S12, sp::S14]))
        // On a phone the index field takes the card's width.
        .width(if compact {
            Length::Fill
        } else {
            Length::Fixed(120.0)
        })
        .style(theme::field(radius::R12));
    if idle {
        to = to
            .on_input(|s| WalletMsg::DiscoverTo(s).into())
            .on_submit(WalletMsg::Discover.into());
    }
    // The button `action` draws, which takes the card's width on a phone.
    let discover = ui::button_with(
        "Discover accounts",
        theme::Button::Primary,
        Size::Medium,
        Some(Icon::Search),
        idle.then(|| Message::from(WalletMsg::Discover)),
    );
    let search: Element<'a, Message> = if compact {
        column![
            t(
                "Derived accounts 0 to",
                ty::FORM_LABEL,
                color::TEXT_SECONDARY
            ),
            to,
            discover.width(Length::Fill),
        ]
        .spacing(sp::S8)
        .width(Length::Fill)
        .into()
    } else {
        row![
            t(
                "Derived accounts 0 to",
                ty::FORM_LABEL,
                color::TEXT_SECONDARY
            ),
            to,
            discover,
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center)
        .into()
    };
    let mut body = vec![
        ui::card_for(
            compact,
            column![
                t(
                    "Every account this keystore derives from its seed has a place on the chain \
                     once it is paid. Discover asks the API about the first ones, one request \
                     each, and writes nothing; an account the chain holds can then be added. To \
                     start a new one, pay the next account the sweep shows, then discover again \
                     and add it.",
                    ty::BODY_SMALL,
                    color::TEXT_SECONDARY,
                ),
                search,
                ui::helper(format!(
                    "Up to {}: a request to the API each.",
                    tawara_wallet_core::DISCOVER_MAX_TO
                )),
            ]
            .spacing(sp::S16),
        )
        .into(),
    ];
    if let Some(summary) = a.auto.as_ref().and_then(auto_summary) {
        body.insert(0, summary);
    }
    if let Some((ok, text)) = &a.added {
        body.push(report::show_in(
            compact,
            page,
            ReportKey::Added,
            report::added(*ok, text),
        ));
    }
    if let Some((text, rows)) = &a.found {
        // The first derived account nothing holds yet: where a new one
        // receives.
        let next = rows
            .iter()
            .find(|r| !r.held && r.ledger_balance.is_none())
            .map(|r| r.account_index);
        let list = if compact {
            found_stacked(model, page, rows, next)
        } else {
            found_table(model, page, rows, next)
        };
        body.push(
            ui::card_for(
                compact,
                column![ui::section_label("What the chain holds"), list].spacing(sp::S8),
            )
            .into(),
        );
        body.push(report::show_in(
            compact,
            page,
            ReportKey::Found,
            report::found(text, rows),
        ));
    }
    frame(
        model,
        page,
        "Add an account",
        "Find the accounts this keystore's seed derives, and add the ones the chain holds.",
        Vec::new(),
        body,
    )
}

/// "account 1", "accounts 1 and 3", "accounts 1, 3 and 4".
fn accounts(indices: &[u32]) -> String {
    let named: Vec<String> = indices.iter().map(u32::to_string).collect();
    match named.as_slice() {
        [] => String::new(),
        [one] => format!("account {one}"),
        [rest @ .., last] => format!("accounts {} and {last}", rest.join(", ")),
    }
}

/// What the additions after a restore did, once they have ended.
fn auto_summary<'a>(auto: &AutoAdd) -> Option<Element<'a, Message>> {
    if auto.running {
        return None;
    }
    let to = tawara_wallet_core::DISCOVER_DEFAULT_TO;
    let summary = if !auto.not_added.is_empty() {
        ui::warning_callout(
            "Not every account was added",
            format!(
                "Tawara did not add {}. The reports below say why; use Add account to try \
                 again.",
                accounts(&auto.not_added)
            ),
        )
    } else if auto.added.is_empty() {
        ui::accent_callout(
            Icon::Search,
            "No other accounts found",
            format!(
                "Accounts 0 to {to} of this recovery phrase hold no other balance. An account \
                 with no balance can be added here once it has been paid."
            ),
        )
    } else {
        ui::accent_callout(
            Icon::Check,
            "Accounts added",
            format!(
                "Tawara looked through accounts 0 to {to} of this recovery phrase and added {}, \
                 which hold a balance.",
                accounts(&auto.added)
            ),
        )
    };
    Some(summary)
}

/// The found accounts as the desktop's table: a header row, then one row each
/// with its index, its destination, what the chain holds and what can be done.
fn found_table<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    rows: &'a [Discovered],
    next: Option<u32>,
) -> Element<'a, Message> {
    let mut table = column![
        row![
            head("#", 1),
            head("Destination", 7),
            head("On the chain", 4),
            head("", 4),
        ]
        .spacing(sp::S12)
        .padding(Padding::from([sp::S10, 0.0])),
        ui::divider(),
    ];
    for (n, found) in rows.iter().enumerate() {
        let shown = found.id.destination().unwrap_or_else(|| found.id.hex());
        table = table.push(
            row![
                t(
                    found.account_index.to_string(),
                    ty::MONO,
                    color::TEXT_PRIMARY
                )
                .width(Length::FillPortion(1)),
                column![
                    t(short(&shown), ty::TABLE_NAME, color::TEXT_PRIMARY),
                    t(shown.clone(), ty::MONO_TINY, color::TEXT_MUTED)
                        .wrapping(Wrapping::WordOrGlyph),
                ]
                .spacing(sp::S2)
                .width(Length::FillPortion(7)),
                t(ledger(model, found), ty::TABLE_AMOUNT, color::TEXT_PRIMARY)
                    .wrapping(Wrapping::WordOrGlyph)
                    .width(Length::FillPortion(4)),
                container(found_action(model, page, found, &shown, next))
                    .width(Length::FillPortion(4)),
            ]
            .spacing(sp::S12)
            .align_y(Alignment::Center)
            .padding(Padding::from([sp::S12, 0.0])),
        );
        if n + 1 < rows.len() {
            table = table.push(ui::divider());
        }
    }
    table.into()
}

/// The found accounts on a phone: each is
/// a stacked item, its index, short name and balance on one line, its full
/// destination under them, and its action at the end.
fn found_stacked<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    rows: &'a [Discovered],
    next: Option<u32>,
) -> Element<'a, Message> {
    let mut list = column![].width(Length::Fill);
    for (n, found) in rows.iter().enumerate() {
        let shown = found.id.destination().unwrap_or_else(|| found.id.hex());
        list = list.push(
            column![
                row![
                    t(
                        format!("#{}", found.account_index),
                        ty::MONO,
                        color::TEXT_PRIMARY
                    ),
                    t(short(&shown), ty::TABLE_NAME, color::TEXT_PRIMARY).width(Length::Fill),
                    t(ledger(model, found), ty::TABLE_AMOUNT, color::TEXT_PRIMARY)
                        .wrapping(Wrapping::WordOrGlyph),
                ]
                .spacing(sp::S12)
                .align_y(Alignment::Center)
                .width(Length::Fill),
                t(shown.clone(), ty::MONO_TINY, color::TEXT_MUTED)
                    .wrapping(Wrapping::WordOrGlyph)
                    .width(Length::Fill),
                container(found_action(model, page, found, &shown, next))
                    .width(Length::Fill)
                    .align_x(Alignment::End),
            ]
            .spacing(sp::S8)
            .padding(Padding::from([sp::S12, 0.0]))
            .width(Length::Fill),
        );
        if n + 1 < rows.len() {
            list = list.push(ui::divider());
        }
    }
    list.into()
}

/// What a found account offers: that it is in this store, that it can be
/// added, that it is the next one to pay here (with its destination to copy),
/// or nothing.
fn found_action<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    found: &Discovered,
    shown: &str,
    next: Option<u32>,
) -> Element<'a, Message> {
    if found.held {
        t("In this keystore", ty::TABLE_BODY, color::ACCENT).into()
    } else if found.ledger_balance.is_some() {
        action(
            model,
            "Add account",
            theme::Button::Primary,
            Size::Small,
            Some(Icon::Plus),
            WalletMsg::Add(found.account_index),
        )
    } else if next == Some(found.account_index) {
        column![
            t(
                "Next account: pay it here",
                ty::TABLE_BODY,
                color::TEXT_PRIMARY
            ),
            copy(page, shown, "Copy"),
        ]
        .spacing(sp::S6)
        .into()
    } else {
        t("—", ty::TABLE_BODY, color::TEXT_MUTED).into()
    }
}

/// What the chain holds at a found account's destination, in the store's unit,
/// or that it is not found.
fn ledger(model: &Model, found: &Discovered) -> String {
    found.ledger_balance.map_or_else(
        || "not found".to_owned(),
        |b| {
            format!(
                "{} {}",
                ui::amount(b, model.prefs.unit),
                ui::unit_name(model.prefs.unit)
            )
        },
    )
}

fn head<'a>(label: &'static str, portion: u16) -> Element<'a, Message> {
    t(label, ty::TABLE_HEADER, color::TEXT_MUTED)
        .width(Length::FillPortion(portion))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{NARROWEST, Page, Screen};
    use crate::harness::fits;
    use tawara_wallet_core::preferences::Preferences;
    use tawara_wallet_core::sample;

    /// The sample store on Add an account, in a phone 360 logical pixels
    /// wide, with `a` as the page.
    fn phone(a: AddAccountPage) -> Model {
        let mut model = Model::new(Preferences::default(), None);
        model.width = NARROWEST;
        model.screen = Screen::Wallet(WalletPage {
            page: Page::AddAccount(a),
            ..WalletPage::default()
        });
        model.wallet = Some(sample::wallet_view());
        model
    }

    #[test]
    fn the_phone_discovery_fits_at_360() {
        let model = phone(AddAccountPage::default());
        assert_eq!(fits(crate::screens::view(&model), NARROWEST), Ok(()));
    }

    #[test]
    fn the_phone_found_accounts_fit_at_360() {
        let model = phone(AddAccountPage {
            found: Some(sample::discovered()),
            added: Some((true, "The account was added.".to_owned())),
            ..AddAccountPage::default()
        });
        assert_eq!(fits(crate::screens::view(&model), NARROWEST), Ok(()));
    }

    #[test]
    fn the_accounts_a_restore_added_are_named_in_a_list() {
        assert_eq!(accounts(&[]), "");
        assert_eq!(accounts(&[1]), "account 1");
        assert_eq!(accounts(&[1, 3]), "accounts 1 and 3");
        assert_eq!(accounts(&[1, 3, 4]), "accounts 1, 3 and 4");
    }

    #[test]
    fn each_summary_of_a_restores_additions_fits_at_360() {
        let ended = |added: Vec<u32>, not_added: Vec<u32>| AutoAdd {
            swept: true,
            added,
            not_added,
            ..AutoAdd::default()
        };
        for auto in [
            ended(vec![2], vec![]),
            ended(vec![], vec![]),
            ended(vec![1], vec![2, 3]),
        ] {
            assert!(auto_summary(&auto).is_some());
            let model = phone(AddAccountPage {
                found: Some(sample::discovered()),
                auto: Some(auto),
                ..AddAccountPage::default()
            });
            assert_eq!(fits(crate::screens::view(&model), NARROWEST), Ok(()));
        }
        let running = AutoAdd {
            running: true,
            ..AutoAdd::default()
        };
        assert!(
            auto_summary(&running).is_none(),
            "nothing is said while it runs"
        );
    }
}
