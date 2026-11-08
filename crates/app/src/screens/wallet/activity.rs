// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Activity: every account's transactions from the node's index, with the
//! chosen transaction's detail beside them.
//!
//! The index supplies the newest 100 rows for each account, and older ones a
//! page at a time on "Read older"; a new read starts from the newest again.
//! Spends the store has reserved and not settled come from the store's own
//! record and are listed without an amount, since the store keeps none.
//!
//! On a phone the rows are stacked in one card with no column heads, and the
//! chosen transaction's detail opens under its row inside that card.

use iced::widget::text::Wrapping;
use iced::widget::{Column, column, container, row, space, text_input};
use iced::{Alignment, Element, Length, Padding};
use tawara_wallet_core::explorer::{AccountHistory, OperationView, Party};
use tawara_wallet_core::preferences::AmountUnit;
use tawara_wallet_core::view::{AccountId, AccountState};

use super::{frame, name, pair, report, short};
use crate::app::{ActivityPage, Message, Model, ReportKey, To, WalletMsg, WalletPage};
use crate::history::{self, Filter, Kind, Row};
use crate::icon::{self, Icon};
use crate::tap::tap;
use crate::theme::{self, color, radius, space as sp};
use crate::ui::{self, Size, t, ty};

pub fn view<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    a: &'a ActivityPage,
) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let compact = model.compact();
    let search = text_input("Address, block or transaction id", &a.search)
        .font(ty::FIELD.font)
        .size(ty::FIELD.size)
        .padding(Padding::from([sp::S10, sp::S14]))
        .width(if compact {
            Length::Fill
        } else {
            Length::Fixed(300.0)
        })
        .style(theme::field(radius::R12))
        .on_input(|s| WalletMsg::Search(s).into());
    let search_row = row![
        icon::icon(Icon::Search, 18.0, 2.0, color::TEXT_MUTED),
        search
    ]
    .spacing(sp::S8)
    .align_y(Alignment::Center);
    // On a phone the filters take one line and the full-width search field
    // the line beneath.
    let controls: Element<'a, Message> = if compact {
        column![filters(a), search_row].spacing(sp::S12).into()
    } else {
        row![filters(a), space().width(Length::Fill), search_row]
            .align_y(Alignment::Center)
            .into()
    };
    let mut body: Vec<Element<'a, Message>> = vec![controls];
    // The index is read one account at a time; naming the account being
    // checked keeps rows still to come from looking missing.
    if let Some(account) = model.activity_checking {
        body.push(checking(compact, account));
    }
    let read = &model.activity;
    match &read.last {
        Some(Ok(histories)) => body.push(listing(model, a, histories)),
        other => {
            body.push(match other {
                Some(Err(refusal)) => report::show_in(
                    compact,
                    page,
                    ReportKey::Explorer,
                    report::explorer(refusal, "its transaction index"),
                ),
                _ => ui::card_for(
                    compact,
                    ui::helper(if read.reading {
                        "Reading activity…"
                    } else if model.node.url.is_none() {
                        "No API is chosen: Activity reads the transactions from the transaction \
                         index."
                    } else {
                        "Not read yet."
                    }),
                )
                .into(),
            });
            // The store's own record needs no index: its spends still
            // settling are listed whatever the index answered.
            let pending = pending_rows(model, a);
            if !pending.is_empty() {
                body.push(pending_card(model, pending));
            }
        }
    }
    frame(
        model,
        page,
        "Activity",
        "Every account, latest activity first",
        vec![
            ui::button_with(
                if read.reading {
                    "Reading…"
                } else {
                    "Read again"
                },
                theme::Button::Secondary,
                Size::Header,
                Some(Icon::Refresh),
                (idle && !read.reading).then(|| WalletMsg::ReadActivity.into()),
            )
            .into(),
        ],
        body,
    )
}

/// The line naming the account being read: "Checking account" and its name.
fn checking<'a>(compact: bool, account: AccountId) -> Element<'a, Message> {
    ui::card_for(
        compact,
        row![
            icon::icon(Icon::Refresh, 16.0, 2.0, color::ACCENT),
            t(
                format!("Checking account {}", name(account)),
                ty::BODY_SMALL,
                color::TEXT_SECONDARY,
            )
            .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(sp::S10)
        .align_y(Alignment::Center),
    )
    .into()
}

/// The four filters as segments on one line. They hug their labels and take
/// about 300 pixels, so they fit one line at a width of 360.
fn filters<'a>(a: &ActivityPage) -> iced::widget::Row<'a, Message> {
    let mut tabs = row![].spacing(sp::S8);
    for (filter, label) in [
        (Filter::All, "All"),
        (Filter::Sent, "Sent"),
        (Filter::Received, "Received"),
        (Filter::Pending, "Pending"),
    ] {
        let on_tap: Option<Message> = Some(WalletMsg::Filter(filter).into());
        tabs = tabs.push(ui::segment(
            label,
            a.filter == filter,
            Length::Shrink,
            on_tap,
        ));
    }
    tabs
}

/// The list and the chosen transaction's detail. On a phone the detail opens
/// under its row, inside the list's card.
fn listing<'a>(
    model: &'a Model,
    a: &'a ActivityPage,
    histories: &'a [AccountHistory],
) -> Element<'a, Message> {
    let compact = model.compact();
    let query = a.search.trim().to_lowercase();
    let rows: Vec<Row<'a>> = history::rows(histories)
        .into_iter()
        .filter(|r| a.filter.admits(r.kind) && r.matches(&query))
        .collect();
    let chosen = a
        .selected
        .as_ref()
        .and_then(|id| rows.iter().find(|r| r.is(id)))
        .or(rows.first());
    let mut items = pending_rows(model, a);
    let mut any = !items.is_empty();
    for r in &rows {
        any = true;
        let selected = chosen.is_some_and(|c| c.is(&r.id()));
        let index = index_row(model, r, selected);
        items.push(if compact && selected {
            column![index, detail(model, r)].into()
        } else {
            index
        });
    }
    let mut list = if compact {
        stacked(items)
    } else {
        let mut list = column![header(model), ui::divider()].spacing(sp::S4);
        for item in items {
            list = list.push(item);
        }
        list
    };
    // While accounts are still being checked, none is said to hold nothing.
    if !any && model.activity_checking.is_none() {
        list = list.push(
            container(ui::helper(if query.is_empty() && a.filter == Filter::All {
                "The transaction index holds no transaction for this keystore's accounts. An \
                 account never paid has none."
            } else {
                "No transaction matches."
            }))
            .padding(sp::S12),
        );
    }
    let unread = histories
        .iter()
        .map(AccountHistory::unread)
        .fold(0u64, u64::saturating_add);
    if unread > 0 {
        let reading = model.activity.reading;
        let on_read: Option<Message> =
            (model.busy.is_none() && !reading).then(|| WalletMsg::ReadOlderActivity.into());
        let read_older = ui::button_with(
            if reading { "Reading…" } else { "Read older" },
            theme::Button::Secondary,
            Size::Small,
            None,
            on_read,
        );
        let note = ui::helper(format!(
            "The index holds {unread} older {} for these accounts, not read yet. They \
             are read 100 for each account at a time.",
            if unread == 1 { "row" } else { "rows" }
        ));
        if compact {
            // The note sits over the button on a phone.
            list = list.push(ui::divider()).push(
                container(column![note, read_older].spacing(sp::S10))
                    .padding(sp::S12)
                    .width(Length::Fill),
            );
        } else {
            list = list.push(
                row![container(note).width(Length::Fill), read_older]
                    .spacing(sp::S12)
                    .align_y(Alignment::Center)
                    .padding(sp::S12),
            );
        }
    }
    if compact {
        return ui::card_for(true, list).into();
    }
    let chosen_detail: Element<'a, Message> = match chosen {
        Some(r) => detail(model, r),
        None => ui::card(ui::helper("Choose a transaction to see it whole.")).into(),
    };
    pair(
        model,
        ui::card(list).padding(sp::S12).into(),
        chosen_detail,
        7,
        5,
    )
}

/// A phone's list: the items stacked with a divider between each pair, and
/// no column heads.
fn stacked<'a>(items: Vec<Element<'a, Message>>) -> Column<'a, Message> {
    let mut list = column![];
    for (n, item) in items.into_iter().enumerate() {
        if n > 0 {
            list = list.push(ui::divider());
        }
        list = list.push(item);
    }
    list
}

/// The store's pending spends as a card of their own, for when no index rows
/// are listed with them (see `view`).
fn pending_card<'a>(model: &'a Model, pending: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    if model.compact() {
        return ui::card_for(true, stacked(pending)).into();
    }
    let mut list = column![header(model), ui::divider()].spacing(sp::S4);
    for row in pending {
        list = list.push(row);
    }
    ui::card(list).padding(sp::S12).into()
}

/// The list's column heads.
fn header(model: &Model) -> Element<'_, Message> {
    row![
        head("Date", 3),
        head("Description", 7),
        head("Account", 4),
        container(t(
            format!("Amount ({})", ui::unit_name(model.prefs.unit)),
            ty::TABLE_HEADER,
            color::TEXT_MUTED
        ))
        .width(Length::FillPortion(4))
        .align_x(Alignment::End),
    ]
    .spacing(sp::S12)
    .padding(Padding::from([sp::S10, sp::S12]))
    .into()
}

/// The store's reserved, unsettled spends that pass the filter and search.
/// They come from the store's own record, not the index.
fn pending_rows<'a>(model: &'a Model, a: &ActivityPage) -> Vec<Element<'a, Message>> {
    let Some(w) = &model.wallet else {
        return Vec::new();
    };
    if !a.filter.shows_pending() || !a.search.trim().is_empty() {
        return Vec::new();
    }
    history::pending(&w.accounts)
        .map(|account| pending_row(model, account))
        .collect()
}

fn head<'a>(label: &'static str, portion: u16) -> Element<'a, Message> {
    t(label, ty::TABLE_HEADER, color::TEXT_MUTED)
        .width(Length::FillPortion(portion))
        .into()
}

/// A reserved, unsettled spend, shown without an amount since the store keeps
/// none for an open reservation.
fn pending_row<'a>(
    model: &'a Model,
    account: &'a tawara_wallet_core::view::AccountRow,
) -> Element<'a, Message> {
    let (title, word) = match account.state {
        AccountState::SpendLanded { .. } => ("Spend landed: settle it", "landed"),
        _ => ("Spend settling", "settling"),
    };
    let on_tap: Option<Message> = model
        .busy
        .is_none()
        .then(|| WalletMsg::Open(To::Account(account.id)).into());
    let content: Element<'a, Message> = if model.compact() {
        column![
            row![
                container(t(title, ty::TABLE_NAME, color::TEXT_PRIMARY)).width(Length::Fill),
                t("not kept", ty::TABLE_BODY, color::TEXT_MUTED),
            ]
            .spacing(sp::S12)
            .align_y(Alignment::Center),
            row![
                t(word, ty::NOTE, color::WARNING),
                t(
                    format!("· {}", name(account.id)),
                    ty::NOTE,
                    color::TEXT_SECONDARY
                ),
            ]
            .spacing(sp::S4),
            t(
                "From the keystore's own record, not the index",
                ty::MONO_TINY,
                color::TEXT_MUTED
            )
            .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(sp::S4)
        .into()
    } else {
        row![
            t(word, ty::TABLE_BODY, color::WARNING).width(Length::FillPortion(3)),
            column![
                t(title, ty::TABLE_NAME, color::TEXT_PRIMARY),
                t(
                    "From the keystore's own record, not the index",
                    ty::MONO_TINY,
                    color::TEXT_MUTED
                ),
            ]
            .spacing(sp::S2)
            .width(Length::FillPortion(7)),
            t(name(account.id), ty::TABLE_BODY, color::TEXT_SECONDARY)
                .width(Length::FillPortion(4)),
            container(t("not kept", ty::TABLE_BODY, color::TEXT_MUTED))
                .width(Length::FillPortion(4))
                .align_x(Alignment::End),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center)
        .into()
    };
    // A row acts on release, so a drag through the list scrolls it.
    tap(container(content)
        .padding(Padding::from([sp::S12, sp::S12]))
        .width(Length::Fill))
    .on_tap_maybe(on_tap)
    .into()
}

/// A transaction's date and time on the system clock, or a dash when the
/// index gave no time.
fn when(model: &Model, r: &Row<'_>) -> String {
    r.tx.time_ms
        .map_or_else(|| "—".to_owned(), |ms| ui::date_time(ms, model.zone))
}

/// One of the index's rows. On a phone it is three lines: the title and the
/// amount, the date and the account, and the detail.
fn index_row<'a>(model: &'a Model, r: &Row<'a>, selected: bool) -> Element<'a, Message> {
    let (title, detail) = describe(r);
    let (amount, ink) = amount(r, model.prefs.unit);
    let content: Element<'a, Message> = if model.compact() {
        column![
            row![
                container(ui::arrowed(title, ty::TABLE_NAME, color::TEXT_PRIMARY))
                    .width(Length::Fill),
                t(amount, ty::TABLE_AMOUNT, ink)
                    .wrapping(Wrapping::WordOrGlyph)
                    .align_x(Alignment::End),
            ]
            .spacing(sp::S12)
            .align_y(Alignment::Center),
            t(
                format!("{} · {}", when(model, r), name(r.account)),
                ty::NOTE,
                color::TEXT_SECONDARY
            ),
            t(detail, ty::MONO_TINY, color::TEXT_MUTED).wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(sp::S4)
        .into()
    } else {
        row![
            t(when(model, r), ty::TABLE_BODY, color::TEXT_SECONDARY).width(Length::FillPortion(3)),
            column![
                ui::arrowed(title, ty::TABLE_NAME, color::TEXT_PRIMARY),
                t(detail, ty::MONO_TINY, color::TEXT_MUTED).wrapping(Wrapping::WordOrGlyph),
            ]
            .spacing(sp::S2)
            .width(Length::FillPortion(7)),
            t(name(r.account), ty::TABLE_BODY, color::TEXT_SECONDARY).width(Length::FillPortion(4)),
            container(
                t(amount, ty::TABLE_AMOUNT, ink)
                    .wrapping(Wrapping::WordOrGlyph)
                    .align_x(Alignment::End)
            )
            .width(Length::FillPortion(4))
            .align_x(Alignment::End),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center)
        .into()
    };
    let boxed = container(content)
        .padding(Padding::from([sp::S12, sp::S12]))
        .width(Length::Fill);
    // A row acts on release, so a drag through the list scrolls it.
    tap(if selected {
        boxed.style(theme::row_selected)
    } else {
        boxed
    })
    .on_tap(WalletMsg::Select(r.id()).into())
    .into()
}

/// A party for display: an account by its shortened destination, a ledger
/// address labelled as one, anything else as the node sent it.
pub(super) fn party(op: &OperationView) -> String {
    match &op.party {
        Party::Account(id) => name(*id),
        Party::Ledger(hex) => format!("ledger address {}", short(hex)),
        Party::Other(text) if text.is_empty() => "—".to_owned(),
        Party::Other(text) => text.clone(),
    }
}

/// A row's title and subtitle: the counterparty, and the references the node
/// sent, or the transaction id when it sent none.
pub(super) fn describe(r: &Row<'_>) -> (String, String) {
    let payees: Vec<&OperationView> = r.payees().collect();
    let title = match r.kind {
        Kind::Sent if payees.len() == 1 => format!("Sent to {}", party(payees[0])),
        Kind::Sent => format!("Batch send · {} destinations", payees.len()),
        Kind::Received => match r.payers().next() {
            Some(from) => format!("Received from {}", party(from)),
            None => "Received".to_owned(),
        },
        Kind::Own => match (payees.as_slice(), r.payers().next()) {
            // Seen from the account it left: to the store's other account.
            ([to], _) => format!("{} → {}", name(r.account), party(to)),
            // To several of the store's accounts at once: the amount is
            // theirs together, so no one of them is named for it.
            ([_, _, ..], _) => {
                format!("Batch between own accounts · {} destinations", payees.len())
            }
            // Only the receiving side is in the index's answer.
            ([], Some(from)) => format!("{} → {}", party(from), name(r.account)),
            ([], None) => "Between own accounts".to_owned(),
        },
        Kind::Reward => "Mining reward".to_owned(),
        Kind::Other => "Transaction".to_owned(),
    };
    let references = r.references();
    let detail = if r.kind == Kind::Own && payees.len() > 1 {
        format!(
            "From {} to {}",
            name(r.account),
            listed(payees.iter().map(|o| party(o)))
        )
    } else if r.kind == Kind::Own {
        "Between own accounts".to_owned()
    } else if references.is_empty() {
        format!("tx {}", short(&r.tx.id))
    } else {
        references.join(" · ")
    };
    (title, detail)
}

/// `names` as a person reads a list: "B and C", "B, C and D", and past
/// three, "B, C, D and 2 more".
fn listed(names: impl Iterator<Item = String>) -> String {
    const SHOWN: usize = 3;
    let names: Vec<String> = names.collect();
    let more = names.len().saturating_sub(SHOWN);
    let shown = &names[..names.len().min(SHOWN)];
    match (shown, more) {
        ([], _) => String::new(),
        ([one], 0) => one.clone(),
        (_, 0) => format!(
            "{} and {}",
            shown[..shown.len() - 1].join(", "),
            shown[shown.len() - 1]
        ),
        (_, more) => format!("{} and {more} more", shown.join(", ")),
    }
}

/// A row's amount, signed, and its colour: received in the accent, sent in
/// the primary ink, a transfer between the store's own accounts unsigned.
pub(super) fn amount(r: &Row<'_>, unit: AmountUnit) -> (String, iced::Color) {
    let figure = ui::amount(r.amount.unsigned_abs(), unit);
    match r.kind {
        Kind::Own => (figure, color::TEXT_PRIMARY),
        _ if r.amount > 0 => (format!("+{figure}"), color::ACCENT),
        _ if r.amount < 0 => (format!("−{figure}"), color::TEXT_PRIMARY),
        _ => (figure, color::TEXT_SECONDARY),
    }
}

/// The chosen transaction, whole: a card of its own beside the list, or on a
/// phone, drawn under its row inside the list (see `listing`).
fn detail<'a>(model: &'a Model, r: &Row<'a>) -> Element<'a, Message> {
    let compact = model.compact();
    let unit = model.prefs.unit;
    let (title, _) = describe(r);
    let (figure, ink) = amount(r, unit);
    let mut card = column![
        row![
            t(title, ty::CARD_TITLE, color::TEXT_PRIMARY)
                .wrapping(Wrapping::WordOrGlyph)
                .width(Length::Fill),
            ui::pill(
                r.tx.block.map_or_else(
                    || "in a block".to_owned(),
                    |b| format!("block {}", ui::group(&b.to_string()))
                ),
                ty::CHIP,
                color::ACCENT_SOFT,
                color::ACCENT,
                28.0,
            ),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center),
        t(
            r.tx.time_ms.map_or_else(
                || "time not given".to_owned(),
                |ms| ui::full_time(ms, model.zone)
            ),
            ty::NOTE,
            color::TEXT_MUTED
        ),
        row![
            t(figure, ty::STAT_VALUE, ink),
            t(ui::unit_name(unit), ty::CHIP, color::ACCENT),
        ]
        .spacing(sp::S8)
        .align_y(Alignment::End),
    ]
    .spacing(sp::S12);
    let parties: Vec<&OperationView> = match r.kind {
        Kind::Received => r.payers().collect(),
        _ => r.payees().collect(),
    };
    if !parties.is_empty() {
        let mut list = column![].spacing(sp::S10);
        for op in parties {
            // An account opens on the explorer's Account page, as a search for
            // its address would. It came from the index, so that page does not
            // offer "Send to this account".
            let who: Element<'a, Message> = match &op.party {
                Party::Account(id) => iced::widget::button(
                    t(
                        id.destination().unwrap_or_else(|| id.hex()),
                        ty::MONO_SMALL,
                        color::ACCENT,
                    )
                    .wrapping(Wrapping::WordOrGlyph),
                )
                .padding(0)
                .style(theme::button(theme::Button::Link))
                .on_press(WalletMsg::Open(To::Tag(*id)).into())
                .into(),
                _ => t(party(op), ty::MONO_SMALL, color::TEXT_PRIMARY)
                    .wrapping(Wrapping::WordOrGlyph)
                    .into(),
            };
            let mut who = column![who].spacing(sp::S2);
            if !op.memo.is_empty() {
                who = who.push(t(op.memo.clone(), ty::MONO_TINY, color::TEXT_MUTED));
            }
            // A payer's operation carries its whole balance at the time, not
            // what reached this account, so a payer is listed without an
            // amount and a payee with what it was sent.
            let mut line = row![who.width(Length::Fill)]
                .spacing(sp::S12)
                .align_y(Alignment::Center);
            if r.kind != Kind::Received {
                line = line.push(t(
                    ui::amount(op.amount.unsigned_abs(), unit),
                    ty::TABLE_AMOUNT,
                    color::TEXT_PRIMARY,
                ));
            }
            list = list.push(line);
        }
        card = card.push(
            container(list)
                .padding(sp::S14)
                .width(Length::Fill)
                .style(theme::field_box),
        );
    }
    let confirmations = match (r.tx.block, model.node.tip.map(|(tip, _)| tip)) {
        (Some(b), Some(tip)) if tip >= b => format!(
            "{} · {} confirmations",
            ui::group(&b.to_string()),
            ui::group(&(tip - b + 1).to_string())
        ),
        (Some(b), _) => ui::group(&b.to_string()),
        (None, _) => "—".to_owned(),
    };
    // A payment received carries its reference on the destination that
    // reached the account, not on the payer listed above.
    if r.kind == Kind::Received {
        let references = r.references();
        card = card.push(fact(
            "Memo",
            if references.is_empty() {
                "none".to_owned()
            } else {
                references.join(" · ")
            },
            !references.is_empty(),
            compact,
        ));
    }
    card = card
        .push(fact("Transaction id", r.tx.id.clone(), true, compact))
        .push(fact("Block", confirmations, false, compact))
        .push(fact("Account", name(r.account), false, compact));
    let fee = r.tx.fee();
    if fee > 0 {
        card = card.push(fact(
            "Fee",
            format!("{} {}", ui::amount(fee, unit), ui::unit_name(unit)),
            false,
            compact,
        ));
    }
    if compact {
        // On a phone the detail is drawn in the list's card, between the row's
        // side margins, rather than in a card of its own.
        return container(card)
            .padding(Padding {
                top: sp::S16,
                right: sp::S12,
                bottom: sp::S12,
                left: sp::S12,
            })
            .width(Length::Fill)
            .into();
    }
    ui::card(card).into()
}

/// A labelled fact of the detail: its label beside its value on the desktop,
/// and over it on a phone, where there is no room for two columns.
fn fact<'a>(label: &'static str, value: String, mono: bool, compact: bool) -> Element<'a, Message> {
    let face = if mono { ty::MONO_SMALL } else { ty::TABLE_BODY };
    if compact {
        return column![
            t(label, ty::BODY_SMALL, color::TEXT_SECONDARY),
            t(value, face, color::TEXT_PRIMARY)
                .wrapping(Wrapping::WordOrGlyph)
                .width(Length::Fill),
        ]
        .spacing(sp::S2)
        .into();
    }
    row![
        t(label, ty::BODY_SMALL, color::TEXT_SECONDARY).width(Length::FillPortion(2)),
        t(value, face, color::TEXT_PRIMARY)
            .wrapping(Wrapping::WordOrGlyph)
            .width(Length::FillPortion(5)),
    ]
    .spacing(sp::S12)
    .into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use iced::event::Status;
    use iced::{Point, Rectangle};
    use tawara_wallet_core::explorer::{IndexState, OperationKind, TransactionView};
    use tawara_wallet_core::preferences::Preferences;
    use tawara_wallet_core::sample;
    use tawara_wallet_core::view::AccountId;
    use tawara_wallet_core::{NetworkName, SyncState};

    use super::*;
    use crate::app::{Explored, Level, NARROWEST, NodeState, Page, Screen};
    use crate::harness::{
        Driver, finger_lift, finger_move, finger_press, fits, mouse_move, mouse_press,
        mouse_release, texts,
    };
    use crate::screens;
    use crate::ui::Zone;

    /// The node the sample store reads from, and the folder it is kept in, as
    /// the screenshot example names them.
    const NODE: &str = "https://node.example";
    const DIR: &str = "/home/you/.local/share/tawara/keystore";

    /// The sample store on `page`, drawn at the narrowest phone width, built as
    /// the screenshot example builds it: its node, clock and zone fixed.
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
        model.activity = Explored {
            last: Some(Ok(sample::activity())),
            ..Explored::default()
        };
        model.screen = Screen::Wallet(WalletPage {
            page,
            ..WalletPage::default()
        });
        model
    }

    /// The Activity screens the screenshot example draws, by name: the
    /// filters, a chosen payment, and the index's two refusals.
    fn variants() -> Vec<(&'static str, Model)> {
        let histories = sample::activity();
        let received = history::rows(&histories)
            .into_iter()
            .find(|r| r.tx.id == histories[0].transactions[1].id)
            .map(|r| r.id())
            .expect("the sample's payment received is a row");
        let refused = |state: IndexState| {
            let mut model = sample_model(Page::Activity(ActivityPage::default()));
            model.activity = Explored {
                last: Some(Err(sample::index_refusal(state))),
                ..Explored::default()
            };
            model
        };
        let mut index_down = refused(IndexState::Unavailable);
        if let Screen::Wallet(page) = &mut index_down.screen {
            page.open.insert(ReportKey::Explorer, Level::Summary);
        }
        vec![
            (
                "w10-activity",
                sample_model(Page::Activity(ActivityPage::default())),
            ),
            (
                "w10-activity-pending",
                sample_model(Page::Activity(ActivityPage {
                    filter: Filter::Pending,
                    ..ActivityPage::default()
                })),
            ),
            (
                "w10-activity-received",
                sample_model(Page::Activity(ActivityPage {
                    selected: Some(received),
                    ..ActivityPage::default()
                })),
            ),
            ("w10-activity-no-index", refused(IndexState::Absent)),
            ("w10-activity-index-down", index_down),
        ]
    }

    /// Where the one text that says `text` lies; a text drawn other than once
    /// fails the test, since the test could then find the wrong one.
    fn lies(texts: &[(String, Rectangle)], text: &str) -> Rectangle {
        let found: Vec<Rectangle> = texts
            .iter()
            .filter(|(drawn, _)| drawn == text)
            .map(|(_, at)| *at)
            .collect();
        assert_eq!(found.len(), 1, "`{text}` is drawn {} times", found.len());
        found[0]
    }

    /// The middle of `at`, where a finger or a pointer touches it.
    fn middle(at: Rectangle) -> Point {
        Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0)
    }

    /// The line a row's date and account are drawn as on a phone: the one
    /// text that names the row.
    fn stamp(model: &Model, r: &Row<'_>) -> String {
        format!("{} · {}", when(model, r), name(r.account))
    }

    #[test]
    fn a_payer_is_a_link_to_its_account_and_shows_no_balance() {
        // The payment received from 7Hbdz4…: the detail does not show the
        // payer's operation amount (its whole balance), and the payer's address
        // opens its Account page.
        let histories = sample::activity();
        let rows = history::rows(&histories);
        let received = rows
            .iter()
            .find(|r| r.tx.id == histories[0].transactions[1].id)
            .expect("the payment received");
        assert_eq!(received.kind, Kind::Received);
        let payer = received.payers().next().expect("its payer");
        let Party::Account(id) = payer.party else {
            panic!("the payer is an account: {payer:?}");
        };
        let mut model = sample_model(Page::Activity(ActivityPage {
            selected: Some(received.id()),
            ..ActivityPage::default()
        }));
        model.width = 1440.0;
        let drawn = texts(screens::view(&model), (1440.0, 1200.0));
        let shown = ui::amount(payer.amount.unsigned_abs(), model.prefs.unit);
        assert!(
            !drawn.iter().any(|(text, _)| *text == shown),
            "the payer's {shown} is drawn"
        );
        let address = id.destination().expect("an address");
        let at = middle(lies(&drawn, &address));
        let mut driver = Driver::new(screens::view(&model), (1440, 1200));
        let (messages, _, _) =
            driver.send(&[mouse_move(at.x, at.y), mouse_press(), mouse_release()]);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Wallet(WalletMsg::Open(To::Tag(to)))] if *to == id
            ),
            "{messages:?}"
        );
        assert!(has_subtitle(&drawn), "{drawn:?}");
    }

    /// Whether the Activity subtitle is drawn.
    fn has_subtitle(drawn: &[(String, Rectangle)]) -> bool {
        drawn
            .iter()
            .any(|(text, _)| text == "Every account, latest activity first")
    }

    #[test]
    fn index_totals_near_the_largest_count_draw_without_overflow() {
        let mut model = sample_model(Page::Activity(ActivityPage::default()));
        let mut histories = sample::activity();
        assert!(histories.len() >= 2, "the sample has one account");
        for h in &mut histories {
            h.total = u64::MAX;
        }
        model.activity = Explored {
            last: Some(Ok(histories)),
            ..Explored::default()
        };
        let drawn = texts(screens::view(&model), (360.0, 844.0));
        let max = u64::MAX.to_string();
        assert!(
            drawn.iter().any(|(text, _)| text.contains(&max)),
            "the unread total is not drawn at its ceiling"
        );
    }

    #[test]
    fn the_variants_fit_a_phone_of_the_narrowest_width() {
        for (screen, model) in variants() {
            assert_eq!(fits(screens::view(&model), NARROWEST), Ok(()), "{screen}");
        }
    }

    #[test]
    fn on_a_phone_a_tap_on_the_second_row_chooses_it() {
        let model = sample_model(Page::Activity(ActivityPage::default()));
        let histories = sample::activity();
        let rows = history::rows(&histories);
        let drawn = texts(screens::view(&model), (360.0, 844.0));
        let at = middle(lies(&drawn, &stamp(&model, &rows[1])));
        let mut driver = Driver::new(screens::view(&model), (360, 844));
        // The chosen row's detail pushes the second row below the window, so
        // the page is first scrolled up by 600 with a finger drag that starts
        // beside the detail's payees rather than on them, since each is a link.
        let (scrolled, scroll_statuses, _) = driver.send(&[
            finger_press(1, 320.0, 750.0),
            finger_move(1, 320.0, 550.0),
            finger_move(1, 320.0, 350.0),
            finger_move(1, 320.0, 150.0),
            finger_lift(1, 320.0, 150.0),
        ]);
        assert!(scrolled.is_empty(), "the scroll tapped: {scrolled:?}");
        assert_eq!(
            scroll_statuses[1],
            Status::Captured,
            "the page did not scroll"
        );
        let (messages, _, _) = driver.send(&[
            mouse_move(at.x, at.y - 600.0),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(messages.as_slice(), [Message::Wallet(WalletMsg::Select(id))] if *id == rows[1].id()),
            "{messages:?}"
        );
    }

    #[test]
    fn on_a_phone_a_drag_that_starts_on_a_row_scrolls_and_taps_nothing() {
        let model = sample_model(Page::Activity(ActivityPage::default()));
        let histories = sample::activity();
        let rows = history::rows(&histories);
        let drawn = texts(screens::view(&model), (360.0, 844.0));
        // The first row is the one drawn in the window at the top.
        let at = middle(lies(&drawn, &stamp(&model, &rows[0])));
        let mut driver = Driver::new(screens::view(&model), (360, 844));
        let (messages, statuses, _) = driver.send(&[
            finger_press(1, at.x, at.y),
            finger_move(1, at.x, at.y - 60.0),
            finger_lift(1, at.x, at.y - 60.0),
        ]);
        assert!(messages.is_empty(), "the drag tapped a row: {messages:?}");
        assert_eq!(
            statuses[1],
            Status::Captured,
            "the page did not scroll with the finger"
        );
    }

    #[test]
    fn on_the_desktop_a_click_on_a_row_chooses_it_as_before() {
        let mut model = sample_model(Page::Activity(ActivityPage::default()));
        model.width = 1440.0;
        let histories = sample::activity();
        let rows = history::rows(&histories);
        let drawn = texts(screens::view(&model), (1440.0, 1160.0));
        let at = middle(lies(&drawn, &describe(&rows[1]).0));
        let mut driver = Driver::new(screens::view(&model), (1440, 1160));
        let (messages, _, _) =
            driver.send(&[mouse_move(at.x, at.y), mouse_press(), mouse_release()]);
        assert!(
            matches!(messages.as_slice(), [Message::Wallet(WalletMsg::Select(id))] if *id == rows[1].id()),
            "{messages:?}"
        );
    }

    #[test]
    fn on_a_phone_the_chosen_detail_lies_under_its_row_and_above_the_next() {
        let histories = sample::activity();
        let rows = history::rows(&histories);
        let model = sample_model(Page::Activity(ActivityPage {
            selected: Some(rows[1].id()),
            ..ActivityPage::default()
        }));
        let drawn = texts(screens::view(&model), (360.0, 844.0));
        let row = lies(&drawn, &stamp(&model, &rows[1])).y;
        let detail = lies(&drawn, "Transaction id").y;
        let next = lies(&drawn, &stamp(&model, &rows[2])).y;
        assert!(
            row < detail && detail < next,
            "the row is at {row}, its detail at {detail} and the next row at {next}"
        );
    }

    /// The four filters as the desktop drew them before they were segments:
    /// buttons, in a row.
    fn filter_buttons<'a>(a: &ActivityPage) -> Element<'a, Message> {
        let mut tabs = row![].spacing(sp::S8);
        for (filter, label) in [
            (Filter::All, "All"),
            (Filter::Sent, "Sent"),
            (Filter::Received, "Received"),
            (Filter::Pending, "Pending"),
        ] {
            let on_press: Option<Message> = Some(WalletMsg::Filter(filter).into());
            tabs = tabs.push(ui::button_with(
                label,
                theme::Button::Segment {
                    selected: a.filter == filter,
                },
                Size::Small,
                None,
                on_press,
            ));
        }
        tabs.into()
    }

    #[test]
    fn the_filter_segments_draw_the_pixels_of_the_buttons_they_replace() {
        for filter in [Filter::All, Filter::Sent, Filter::Received, Filter::Pending] {
            let a = ActivityPage {
                filter,
                ..ActivityPage::default()
            };
            let segments: Element<'_, Message> = filters(&a).into();
            let drawn_segments = Driver::new(segments, (420, 40)).draw();
            let drawn_buttons = Driver::new(filter_buttons(&a), (420, 40)).draw();
            assert!(
                drawn_segments == drawn_buttons,
                "{filter:?}: the pixels differ"
            );
        }
    }

    fn op(kind: OperationKind, account: AccountId, amount: i128) -> OperationView {
        OperationView {
            kind,
            party: Party::Account(account),
            amount,
            memo: String::new(),
        }
    }

    fn history(account: AccountId, tx: &TransactionView) -> AccountHistory {
        AccountHistory {
            account,
            transactions: vec![tx.clone()],
            total: 1,
            next: 1,
            text: String::new(),
        }
    }

    #[test]
    fn a_transfer_to_several_own_accounts_names_none_of_them_for_the_whole() {
        use OperationKind::{Destination, Source};
        let [a, b, c] = [0xa1, 0xb2, 0xc3].map(|n| AccountId::from_tag([n; 20]));
        // A pays B 10 and C 20, with 5 back as change.
        let batch = TransactionView {
            id: "batch".to_owned(),
            block: Some(900),
            time_ms: None,
            operations: vec![
                op(Source, a, -35),
                op(Destination, b, 10),
                op(Destination, c, 20),
                op(Destination, a, 5),
            ],
        };
        let histories = [history(a, &batch), history(b, &batch), history(c, &batch)];
        let rows = history::rows(&histories);
        assert_eq!(rows.len(), 1, "listed once, from the account it left");
        assert_eq!((rows[0].kind, rows[0].amount), (Kind::Own, 30));
        let (title, detail) = describe(&rows[0]);
        assert_eq!(title, "Batch between own accounts · 2 destinations");
        assert_eq!(
            detail,
            format!("From {} to {} and {}", name(a), name(b), name(c))
        );

        // To one of them, the arrow names it.
        let single = TransactionView {
            id: "single".to_owned(),
            operations: vec![
                op(Source, a, -15),
                op(Destination, b, 10),
                op(Destination, a, 5),
            ],
            ..batch
        };
        let histories = [history(a, &single), history(b, &single)];
        let rows = history::rows(&histories);
        let (title, detail) = describe(&rows[0]);
        assert_eq!(title, format!("{} → {}", name(a), name(b)));
        assert_eq!(detail, "Between own accounts");
    }

    #[test]
    fn a_list_of_names_reads_as_a_person_writes_it() {
        let names = |n: usize| (0..n).map(|i| format!("N{i}"));
        assert_eq!(listed(names(0)), "");
        assert_eq!(listed(names(1)), "N0");
        assert_eq!(listed(names(2)), "N0 and N1");
        assert_eq!(listed(names(3)), "N0, N1 and N2");
        assert_eq!(listed(names(5)), "N0, N1, N2 and 2 more");
    }
}
