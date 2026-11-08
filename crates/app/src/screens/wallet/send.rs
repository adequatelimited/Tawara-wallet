// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The send flow: composing a spend, reviewing it and the sent page; re-signing
//! the reserved spend; and submitting a saved artifact.
//!
//! The person composes, then reads the library's "NOT SIGNED" page, then
//! signs on a step of its own: there is no password field, because the
//! store is open and the library's page is the confirmation. After signing,
//! the library's page with its three facts is shown whole, and the signed
//! bytes, which the store does not keep, can be saved or copied.
//!
//! On a phone the composing and the signing are four pages before the sent
//! page: the destinations, the amounts, the review, and the signing. The
//! desktop keeps the form and the review as one page each.

use std::collections::BTreeMap;

use iced::widget::text::Wrapping;
use iced::widget::{Row, column, container, row, space, text_input};
use iced::{Alignment, ContentFit, Element, Length, Padding};
use tawara_wallet_core::preferences::AmountUnit;
use tawara_wallet_core::view::AccountId;
use tawara_wallet_core::{FEE_PER_DESTINATION, MAX_DESTINATIONS, PlanView, PlannedDestination};

use super::{
    account_choice, action, beside, copy, destination, figure, frame, frame_beside, name, pair,
    report, row_of, short,
};
use crate::app::{
    DestinationRow, FormOf, Message, Model, PHONE_TABLE, PasteInto, RefusedField, ReportKey,
    ResignPage, SendPage, SendStage, SentPage, SpendForm, SubmitMode, SubmitPage, To, WalletMsg,
    WalletPage, memo_ok, memos_ok,
};
use crate::icon::Icon;
use crate::paste;
use crate::theme::{self, color, radius, space as sp};
use crate::ui::{self, Size, t, ty};

/// Whether Return in the saved-artifact field broadcasts the signed bytes. On
/// the desktop it does, since Return is how a keyboard submits there. On a
/// phone the soft keyboard's Return is easy to press straight after a paste,
/// and a broadcast cannot be undone, so Return does nothing there and the
/// Submit button is the only way to send. The button sends the same message
/// and is disabled while busy, as the field is.
///
/// An expression rather than a `cfg` attribute, so that both branches are
/// compiled and type-checked on every target. The tests run on the desktop,
/// so they give the phones' value to [`submit_form`] directly; that this
/// constant is false on Android and iOS is checked by reading it and by the
/// phone builds, not by a test.
const RETURN_BROADCASTS: bool = !cfg!(any(target_os = "android", target_os = "ios"));

/// The page each stage of the send flow is drawn as. On a phone the flow is
/// four pages, and its review is page 3; on the desktop it is the form, the
/// review and the sent page, since a desktop does not reach the phone's pages.
pub fn view<'a>(model: &'a Model, page: &'a WalletPage, s: &'a SendPage) -> Element<'a, Message> {
    match &s.stage {
        // A phone is never on the desktop's one-page form: its first page is
        // drawn in its place.
        SendStage::Compose if model.phone() => to_page(model, page, s, &NO_REFUSALS),
        SendStage::Compose => compose(model, page, s),
        SendStage::To(refused) => to_page(model, page, s, refused),
        SendStage::Amount => amount_page(model, page, s),
        SendStage::Review(plan) if model.phone() => review_page(model, page, plan),
        SendStage::Review(plan) => review(model, page, plan),
        SendStage::Sign(_) => sign_page(model, page),
        SendStage::Sent(sent) => sent_page(model, page, sent),
    }
}

/// The scanner's page on a phone whose shell draws the camera's preview in the
/// application: the camera's latest frame, a line on what to point at, and
/// Cancel. It is drawn in place of the page the scan began on, which it
/// returns to when the scan ends. The text a code holds is not drawn here,
/// since it is untrusted until it is checked.
pub fn scanner<'a>(model: &'a Model, page: &'a WalletPage) -> Element<'a, Message> {
    // The frame is drawn whole, letterboxed in the box, so that the code can be
    // seen in it; until the first frame comes, the box is empty.
    let shown: Element<'a, Message> = match &model.scan_preview {
        Some(frame) => iced::widget::image(frame.clone())
            .content_fit(ContentFit::Contain)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        None => space().width(Length::Fill).height(Length::Fill).into(),
    };
    let viewfinder = container(shown)
        .width(Length::Fill)
        .height(Length::Fixed(VIEWFINDER))
        .clip(true)
        .style(theme::field_box);
    let cancel = ui::button_with(
        "Cancel",
        theme::Button::Secondary,
        Size::Medium,
        None,
        Some(Message::CancelScan),
    )
    .width(Length::Fill);
    frame(
        model,
        page,
        "Scan a destination",
        "Point the camera at a QR code with a destination in it.",
        Vec::new(),
        vec![
            ui::card_for(
                model.compact(),
                column![viewfinder, cancel].spacing(sp::S16),
            )
            .into(),
        ],
    )
}

/// The height of the scanner's viewfinder, in logical pixels: the most the
/// page holds at 360 by 640 without scrolling, with its title, its line and
/// Cancel.
const VIEWFINDER: f32 = 200.0;

/// The Scan button of destination row `row`, which stands beneath the row's
/// field on a phone whose shell gives a scanner: drawn with the Scan glyph,
/// and enabled while nothing is being done for the page and no scan is under
/// way. `None` where no scanner is offered, which is every desktop.
fn scan_button<'a>(model: &Model, row: usize) -> Option<Element<'a, Message>> {
    model.scan_offered().then(|| {
        let idle = model.busy.is_none() && model.scanning.is_none();
        ui::button_with(
            "Scan",
            theme::Button::Secondary,
            Size::Small,
            Some(Icon::Scan),
            idle.then_some(Message::Scan(row)),
        )
        .into()
    })
}

/// The address field of destination row `n`, with the buttons a phone has, in
/// a table row's cell or in a block. On a phone, in a block, the Paste button
/// stands beside the field, and the Scan button, where a shell gives a
/// scanner, on a line of its own beneath it. In a table row the field takes
/// the cell's full width, and the Paste and the Scan both stand beneath it, on
/// one line. On the desktop the field stands alone. `shown` is the form the
/// Paste button records, as [`destinations`] records it, and `table` says
/// whether the field is a table row's.
fn address_field<'a>(
    model: &Model,
    n: usize,
    to: iced::widget::TextInput<'a, Message>,
    shown: Option<(FormOf, usize)>,
    table: bool,
) -> Element<'a, Message> {
    if !model.phone() {
        return to.width(Length::Fill).into();
    }
    let paste = paste::button(
        shown.map(|(form, rows)| Message::Paste(PasteInto::Destination { form, row: n, rows })),
    );
    if table {
        let mut buttons = row![paste].spacing(sp::S8).align_y(Alignment::Center);
        if let Some(scan) = scan_button(model, n) {
            buttons = buttons.push(scan);
        }
        return column![to.width(Length::Fill), buttons]
            .spacing(sp::S8)
            .width(Length::Fill)
            .into();
    }
    let beside = row![to.width(Length::Fill), paste]
        .spacing(sp::S8)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    let mut cell = column![beside].spacing(sp::S8).width(Length::Fill);
    if let Some(scan) = scan_button(model, n) {
        cell = cell.push(scan);
    }
    cell.into()
}

/// The Remove button of a destination's table row: an icon, as the desktop's
/// table has it, pressed as `press` says, and `None` while it is not offered.
fn table_remove<'a>(press: Option<Message>) -> Element<'a, Message> {
    iced::widget::button(crate::icon::icon(Icon::Trash, 16.0, 2.0, color::TEXT_MUTED))
        .padding(sp::S10)
        .style(theme::button(theme::Button::Ghost))
        .on_press_maybe(press)
        .into()
}

/// How a table row's cells stand beside each other. On a phone a destination's
/// cell is taller than its neighbours, with its buttons beneath its field, so
/// the cells stand at the top and their fields line up with the destination's;
/// on the desktop they are centred.
fn table_alignment(model: &Model) -> Alignment {
    if model.phone() {
        Alignment::Start
    } else {
        Alignment::Center
    }
}

/// Whether the Send destinations and the fees are drawn as table rows, on the
/// form's pages 1 and 2 and on the re-sign page. On the desktop, from
/// [`crate::app::COMPACT`] wide. On a phone, only from [`PHONE_TABLE`] wide: a
/// phone held sideways narrower than that draws its blocks, as it does
/// upright, so that a destination's field keeps its width.
fn table_of(model: &Model) -> bool {
    if model.phone() {
        model.width >= PHONE_TABLE
    } else {
        !model.compact()
    }
}

/// No refusals, for a page drawn without any. A static, since the page
/// borrows what it shows for its whole life.
static NO_REFUSALS: BTreeMap<(usize, RefusedField), String> = BTreeMap::new();

/// The accounts a spend may be laid out from now, in the wallet's order.
fn spendable_of(model: &Model) -> Vec<AccountId> {
    model
        .wallet
        .iter()
        .flat_map(|w| w.accounts.iter())
        .filter(|a| a.spendable)
        .map(|a| a.id)
        .collect()
}

/// Why no spend can be laid out from the account chosen: it is `blocked`, the
/// account asked for, which cannot spend now, or none can (`none_can`). `None`
/// when a spend can be laid out.
fn cannot_spend<'a>(blocked: Option<AccountId>, none_can: bool) -> Option<Element<'a, Message>> {
    if let Some(id) = blocked {
        Some(ui::warning_callout(
            "The account chosen cannot spend now",
            format!(
                "{} spends once the wallet has reconciled it on opening and it is in sync; an \
                 account set aside when the wallet opened spends again after Refresh. Choose \
                 another account, or Refresh the wallet first.",
                name(id)
            ),
        ))
    } else if none_can {
        Some(ui::warning_callout(
            "No account can spend now",
            "A spend needs the wallet open against the API, from an account that reconciled and \
             is in sync. Refresh, or open an account's page from the wallet to see why it cannot.",
        ))
    } else {
        None
    }
}

/// The send form: from which account, to whom, how much, with which references.
fn compose<'a>(model: &'a Model, page: &'a WalletPage, s: &'a SendPage) -> Element<'a, Message> {
    let spendable = spendable_of(model);
    // The account asked for, when it cannot spend now: it stays the one
    // chosen, and the page says why, rather than spending from another.
    let blocked = s.from.filter(|f| !spendable.contains(f));
    let mut form = column![ui::section_label("From account")].spacing(sp::S12);
    if let Some(callout) = cannot_spend(blocked, spendable.is_empty()) {
        form = form.push(callout);
    }
    if !spendable.is_empty() {
        form = form.push(account_choice(model, spendable.into_iter(), s.from, |id| {
            WalletMsg::From(id).into()
        }));
    }
    form = form.push(destinations(model, &s.form, false));
    let summary = column![
        // The change is said only of an account that can sign.
        summary(model, s, s.from.filter(|_| blocked.is_none())),
        ui::button_with(
            "Review spend",
            theme::Button::Primary,
            Size::Xl,
            None,
            (model.busy.is_none() && s.from.is_some() && blocked.is_none() && memos_ok(&s.form))
                .then_some(WalletMsg::Review.into()),
        ),
    ]
    .spacing(sp::S16);
    frame(
        model,
        page,
        "Send",
        format!("One transaction, up to {MAX_DESTINATIONS} destinations, each with its own memo."),
        Vec::new(),
        vec![pair(
            model,
            ui::card_for(model.compact(), form.spacing(sp::S20)).into(),
            summary.into(),
            7,
            4,
        )],
    )
}

/// What the Send page says a memo may be, within the wallet library's limits
/// (`REFERENCE_RULE`): up to sixteen characters, capitals and digits,
/// neighbouring groups of different kinds. Every example here is one the
/// library accepts.
pub(crate) const MEMO_RULE: &str = "A valid memo is up to 16 capital letters and digits: a \
                                    group of letters or of digits, or groups separated by \
                                    hyphens that alternate between letters and digits. Valid \
                                    examples: 'THISISAMEMO', '1234567890', 'MEMO-1234-MEMO', \
                                    '123-LETTERS-456'.";

/// The examples [`MEMO_RULE`] gives, for the test that holds them to the
/// library's rule.
#[cfg(test)]
const MEMO_EXAMPLES: [&str; 4] = [
    "THISISAMEMO",
    "1234567890",
    "MEMO-1234-MEMO",
    "123-LETTERS-456",
];

/// What the Send page says of the fee, which it never asks for: ".0000005
/// MCM", from the fee per destination.
fn fee_line() -> String {
    let fee = tawara_wallet_core::amount::format_mcm_short(FEE_PER_DESTINATION);
    format!(
        "Network fee is {} MCM per destination.",
        fee.strip_prefix('0').unwrap_or(&fee)
    )
}

/// Destination `i`'s amount field with Max on its left. Max fills the field
/// with the most the destination can send
/// ([`Model::max_of`]), and is offered while nothing is being done and the
/// account's balance covers the rest.
fn amount_with_max<'a>(
    model: &Model,
    i: usize,
    field: iced::widget::TextInput<'a, Message>,
) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let field = if idle {
        field.on_input(move |v| WalletMsg::Amount(i, v).into())
    } else {
        field
    };
    let max = ui::button_with(
        "Max",
        theme::Button::Secondary,
        Size::Small,
        None,
        (idle && model.max_of(i).is_some()).then(|| Message::Wallet(WalletMsg::Max(i))),
    );
    container(
        row![max, field.align_x(Alignment::End).width(Length::Fill)]
            .spacing(sp::S8)
            .align_y(Alignment::Center),
    )
    .id(max_id(i))
    .width(Length::Fill)
    .into()
}

/// What is said of a memo once the person has left its field: "Good Format"
/// in green, or "Bad Format", which
/// holds "Review spend" back until the memo is fixed. Nothing for a row whose
/// memo has not been left, or is empty, since a spend may carry none.
pub(crate) const GOOD_FORMAT: &str = "Good Format";
pub(crate) const BAD_FORMAT: &str = "Bad Format";

fn memo_label<'a>(r: &DestinationRow) -> Option<Element<'a, Message>> {
    (r.memo_checked && !r.reference.trim().is_empty()).then(|| {
        if memo_ok(&r.reference) {
            t(GOOD_FORMAT, ty::NOTE, color::ACCENT).into()
        } else {
            t(BAD_FORMAT, ty::NOTE, color::WARNING).into()
        }
    })
}

/// The memo's field under its "Memo" line, which carries the format's label
/// on its right once the field has been left.
fn memo_block<'a>(
    r: &DestinationRow,
    field: iced::widget::TextInput<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    let mut line = row![t("Memo", ty::FORM_LABEL, color::TEXT_SECONDARY)]
        .spacing(sp::S8)
        .align_y(Alignment::Center);
    if let Some(label) = memo_label(r) {
        line = line.push(space().width(Length::Fill)).push(label);
    }
    column![line, field].spacing(sp::S6)
}

/// A table row's memo label, on a line of its own above the row, in the memo's
/// column: the cells before it given as their widths, so that the label stands
/// over the field and the row's fields stay in line.
fn memo_over<'a>(r: &DestinationRow, before: &[Length]) -> Option<Element<'a, Message>> {
    let label = memo_label(r)?;
    let mut line = Row::new().spacing(sp::S12);
    for width in before {
        line = line.push(space().width(*width));
    }
    Some(
        line.push(container(label).width(Length::FillPortion(3)))
            .push(space().width(Length::Fixed(36.0)))
            .into(),
    )
}

/// The id of destination `i`'s amount with its Max, for the tests.
pub(crate) fn max_id(i: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("send-max-{i}"))
}

/// A text field of a phone's page: the field of [`destinations`], in the
/// monospace face for an address or a memo, and in the body's for an
/// amount.
fn entry<'a>(
    value: &'a str,
    placeholder: &'a str,
    mono: bool,
) -> iced::widget::TextInput<'a, Message> {
    let face = if mono { ty::MONO } else { ty::FIELD };
    text_input(placeholder, value)
        .font(face.font)
        .size(face.size)
        .padding(Padding::from([sp::S12, sp::S14]))
        .style(theme::field(radius::R12))
}

/// The place a phone's page holds in the flow, "n of 4".
fn place<'a>(n: u8) -> Element<'a, Message> {
    t(format!("{n} of 4"), ty::NOTE, color::TEXT_SECONDARY).into()
}

/// The Back at the top of a phone's page after the first: back one page,
/// keeping what was typed.
fn back<'a>(model: &Model) -> Element<'a, Message> {
    action(
        model,
        "Back",
        theme::Button::Secondary,
        Size::Medium,
        Some(Icon::ChevronLeft),
        WalletMsg::Back,
    )
}

/// Page 1 of 4 on a phone: the account the spend comes from, with "Change"
/// beside it when another account can spend too, and the destinations, each
/// with its Paste and its Scan, its reference and its Remove; then "Add
/// another destination" and "Continue". What "Continue" found wrong with an
/// address or a reference is shown under that field, and what it refused
/// beside it.
fn to_page<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    s: &'a SendPage,
    refused: &'a BTreeMap<(usize, RefusedField), String>,
) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let spendable = spendable_of(model);
    let blocked = s.from.filter(|f| !spendable.contains(f));
    let mut from = column![ui::section_label("From account")].spacing(sp::S12);
    if let Some(id) = s.from {
        let mut line = row![
            column![
                t(name(id), ty::RADIO_TITLE, color::TEXT_PRIMARY),
                t(destination(id), ty::MONO_TINY, color::TEXT_MUTED)
                    .wrapping(Wrapping::WordOrGlyph),
            ]
            .spacing(sp::S2)
            .width(Length::Fill),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center);
        // Another account that can spend is the choice the callout below asks
        // for, when the one chosen cannot.
        if spendable.iter().any(|other| *other != id) {
            line = line.push(ui::button_with(
                "Change",
                theme::Button::Secondary,
                Size::Small,
                None,
                idle.then(|| Message::Wallet(WalletMsg::ChooseAccount(!page.choosing))),
            ));
        }
        from = from.push(line);
    }
    if let Some(callout) = cannot_spend(blocked, spendable.is_empty()) {
        from = from.push(callout);
    }
    if page.choosing {
        from = from.push(account_choice(
            model,
            spendable.iter().copied(),
            s.from,
            |id| WalletMsg::From(id).into(),
        ));
    }
    let shown = model.form_of().filter(|_| idle);
    let several = s.form.rows.len() > 1;
    // The destinations are rows of a table on the desktop from 600 wide, and on
    // a phone held sideways from 844 wide; below that a phone draws a block of
    // its own for each destination, as it does upright.
    let table = table_of(model);
    let mut list = if table {
        column![
            row![
                t("#", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::Fixed(20.0)),
                t("Destination", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::FillPortion(6)),
                t("Memo", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::FillPortion(3)),
                space().width(Length::Fixed(36.0)),
            ]
            .spacing(sp::S12),
        ]
        .spacing(sp::S10)
    } else {
        column![].spacing(sp::S16)
    };
    for (i, r) in s.form.rows.iter().enumerate() {
        let mut to = entry(&r.to, "Base58 destination", true).id(crate::screens::destination_id(i));
        let mut reference = entry(&r.reference, "none", true).id(crate::screens::reference_id(i));
        if idle {
            to = to.on_input(move |v| WalletMsg::DestinationTo(i, v).into());
            reference = reference.on_input(move |v| WalletMsg::Reference(i, v).into());
        }
        if table {
            // A row, with what Continue refused under it: a block holds each
            // refusal under its own field instead.
            let mut line = column![].spacing(sp::S8);
            if let Some(over) = memo_over(r, &[Length::Fixed(20.0), Length::FillPortion(6)]) {
                line = line.push(over);
            }
            line = line.push(
                row![
                    t((i + 1).to_string(), ty::MONO, color::TEXT_MUTED).width(Length::Fixed(20.0)),
                    container(address_field(model, i, to, shown, true))
                        .width(Length::FillPortion(6)),
                    container(reference).width(Length::FillPortion(3)),
                    table_remove(
                        (idle && several).then(|| Message::Wallet(WalletMsg::RemoveDestination(i)))
                    ),
                ]
                .spacing(sp::S12)
                .align_y(table_alignment(model)),
            );
            if let Some(why) = refused.get(&(i, RefusedField::Address)) {
                line = line.push(ui::refusal(why.as_str()));
            }
            if let Some(why) = refused.get(&(i, RefusedField::Reference)) {
                line = line.push(ui::refusal(why.as_str()));
            }
            list = list.push(line);
        } else {
            if i > 0 {
                list = list.push(ui::divider());
            }
            let remove = ui::button_with(
                "Remove",
                theme::Button::Ghost,
                Size::Small,
                Some(Icon::Trash),
                (idle && several).then(|| Message::Wallet(WalletMsg::RemoveDestination(i))),
            );
            // In a block the Paste button stands beside the field, on a phone at
            // any width, and the Scan button, where a shell gives a scanner, on a
            // line of its own beneath it, as [`destinations`] has them.
            let address = address_field(model, i, to, shown, false);
            let mut block = column![
                row![
                    t(
                        format!("Destination {}", i + 1),
                        ty::FORM_LABEL,
                        color::TEXT_SECONDARY
                    )
                    .width(Length::Fill),
                    remove,
                ]
                .align_y(Alignment::Center),
                address,
            ]
            .spacing(sp::S12);
            if let Some(why) = refused.get(&(i, RefusedField::Address)) {
                block = block.push(ui::refusal(why.as_str()));
            }
            block = block.push(memo_block(r, reference));
            if let Some(why) = refused.get(&(i, RefusedField::Reference)) {
                block = block.push(ui::refusal(why.as_str()));
            }
            list = list.push(block);
        }
    }
    let section = column![
        row![
            ui::section_label("Destinations"),
            space().width(Length::Fill),
            t(
                format!("{} of {MAX_DESTINATIONS}", s.form.rows.len()),
                ty::NOTE,
                color::TEXT_SECONDARY
            ),
        ],
        list,
        ui::button_with(
            "Add another destination",
            theme::Button::Dashed,
            Size::Medium,
            Some(Icon::Plus),
            (idle && s.form.rows.len() < usize::from(MAX_DESTINATIONS))
                .then(|| Message::Wallet(WalletMsg::AddDestination)),
        )
        .width(Length::Fill),
    ]
    .spacing(sp::S12);
    let form = column![from, section, ui::helper(MEMO_RULE),].spacing(sp::S20);
    let mut next = column![ui::button_with(
        "Continue",
        theme::Button::Primary,
        Size::Xl,
        None,
        (idle && memos_ok(&s.form)).then(|| Message::Wallet(WalletMsg::Continue)),
    )]
    .spacing(sp::S16);
    for part in beside(model, page) {
        next = next.push(part);
    }
    frame_beside(
        model,
        page,
        "Send",
        format!("One transaction, up to {MAX_DESTINATIONS} destinations, each with its own memo."),
        Vec::new(),
        vec![
            place(1),
            ui::card_for(model.compact(), form).into(),
            next.into(),
        ],
    )
}

/// Page 2 of 4 on a phone: the amount to each destination, each with its Max,
/// the figures as they stand, and "Review spend", with what it refused beside
/// it. The fee is the fee per destination and there is no block-to-live.
fn amount_page<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    s: &'a SendPage,
) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let spendable = spendable_of(model);
    let blocked = s.from.filter(|f| !spendable.contains(f));
    // The amounts are rows of a table on the desktop from 600 wide, and on a
    // phone held sideways from 844 wide; below that, on a phone, a block of
    // their own for each destination, as upright.
    let table = table_of(model);
    let mut amounts = column![ui::section_label("Amounts")].spacing(sp::S12);
    if table {
        amounts = amounts.push(
            row![
                t("#", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::Fixed(20.0)),
                t("Destination", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::FillPortion(5)),
                t("Amount (MCM)", ty::TABLE_HEADER, color::TEXT_MUTED)
                    .width(Length::FillPortion(4)),
            ]
            .spacing(sp::S12),
        );
    }
    for (i, r) in s.form.rows.iter().enumerate() {
        let amount = amount_with_max(model, i, entry(&r.amount, "0.000000000", false));
        if table {
            amounts = amounts.push(
                row![
                    t((i + 1).to_string(), ty::MONO, color::TEXT_MUTED).width(Length::Fixed(20.0)),
                    container(t(short(&r.to), ty::MONO_SMALL, color::TEXT_MUTED))
                        .width(Length::FillPortion(5)),
                    container(amount).width(Length::FillPortion(4)),
                ]
                .spacing(sp::S12)
                .align_y(Alignment::Center),
            );
        } else {
            if i > 0 {
                amounts = amounts.push(ui::divider());
            }
            amounts = amounts.push(
                column![
                    row![
                        t(
                            format!("Destination {}", i + 1),
                            ty::FORM_LABEL,
                            color::TEXT_SECONDARY
                        ),
                        space().width(Length::Fill),
                        t(short(&r.to), ty::MONO_SMALL, color::TEXT_MUTED),
                    ]
                    .align_y(Alignment::Center),
                    column![
                        t("Amount (MCM)", ty::FORM_LABEL, color::TEXT_SECONDARY),
                        amount
                    ]
                    .spacing(sp::S6),
                ]
                .spacing(sp::S12),
            );
        }
    }
    let mut next = column![ui::button_with(
        "Review spend",
        theme::Button::Primary,
        Size::Xl,
        None,
        (idle && s.from.is_some() && blocked.is_none() && memos_ok(&s.form))
            .then(|| Message::Wallet(WalletMsg::Review)),
    )]
    .spacing(sp::S16);
    if let Some(callout) = cannot_spend(blocked, spendable.is_empty()) {
        next = next.push(callout);
    }
    for part in beside(model, page) {
        next = next.push(part);
    }
    frame_beside(
        model,
        page,
        "Send",
        "The amount to each destination.",
        vec![back(model)],
        vec![
            place(2),
            ui::card_for(model.compact(), amounts).into(),
            summary(model, s, s.from.filter(|_| blocked.is_none())),
            next.into(),
        ],
    )
}

/// The destinations as typed, each amount with its Max: the send form, and the
/// re-sign form, which alone (`exact`) has the fee and the block-to-live, since
/// it gives a reserved spend's figures again exactly; the Send page pays the
/// fee per destination and has no block-to-live. The layout follows the width:
/// on a phone each destination is a block of its own, with no table header,
/// unless the phone is held sideways from 844 wide, where they are rows of a
/// table; the desktop has the table from 600 wide up. The Paste and Scan
/// buttons follow `phone()`: in a block a phone's Paste stands beside each
/// field and its Scan beneath it, upright and sideways alike; in a table row
/// both stand beneath the field.
fn destinations<'a>(model: &'a Model, f: &'a SpendForm, exact: bool) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let table = table_of(model);
    let field = |value: &'a str, placeholder: &'a str, mono: bool| {
        let face = if mono { ty::MONO } else { ty::FIELD };
        text_input(placeholder, value)
            .font(face.font)
            .size(face.size)
            .padding(Padding::from([sp::S12, sp::S14]))
            .style(theme::field(radius::R12))
    };
    let mut list = if table {
        column![
            row![
                t("#", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::Fixed(20.0)),
                t("Destination", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::FillPortion(5)),
                // Typed in MCM whatever unit the wallet shows, exactly to nine
                // places, so a figure is never read in the wrong unit.
                t("Amount (MCM)", ty::TABLE_HEADER, color::TEXT_MUTED)
                    .width(Length::FillPortion(4)),
                t("Memo", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::FillPortion(3)),
                space().width(Length::Fixed(36.0)),
            ]
            .spacing(sp::S12),
        ]
        .spacing(sp::S10)
    } else {
        column![].spacing(sp::S16)
    };
    let several = f.rows.len() > 1;
    // Each Paste button records the form it was drawn on and its rows, so
    // that a read which comes back late goes nowhere else.
    let shown = model.form_of().filter(|_| idle);
    for (i, r) in f.rows.iter().enumerate() {
        let mut to = field(&r.to, "Base58 destination", true).id(crate::screens::destination_id(i));
        let mut reference = field(&r.reference, "none", true).id(crate::screens::reference_id(i));
        let amount = amount_with_max(model, i, field(&r.amount, "0.000000000", false));
        if idle {
            to = to.on_input(move |v| WalletMsg::DestinationTo(i, v).into());
            reference = reference.on_input(move |v| WalletMsg::Reference(i, v).into());
        }
        let remove =
            table_remove((idle && several).then_some(WalletMsg::RemoveDestination(i).into()));
        if table {
            // A table row, as the desktop has it. On a phone the destination's
            // cell holds its Paste and its Scan button beneath the field (see
            // `address_field`). A memo's label stands on a line of its own above
            // the row.
            if let Some(over) = memo_over(
                r,
                &[
                    Length::Fixed(20.0),
                    Length::FillPortion(5),
                    Length::FillPortion(4),
                ],
            ) {
                list = list.push(over);
            }
            list = list.push(
                row![
                    t((i + 1).to_string(), ty::MONO, color::TEXT_MUTED).width(Length::Fixed(20.0)),
                    container(address_field(model, i, to, shown, true))
                        .width(Length::FillPortion(5)),
                    container(amount).width(Length::FillPortion(4)),
                    container(reference).width(Length::FillPortion(3)),
                    remove,
                ]
                .spacing(sp::S12)
                .align_y(table_alignment(model)),
            );
        } else {
            // The blocks are divided, with none above the first.
            if i > 0 {
                list = list.push(ui::divider());
            }
            // The destination takes the width, then the amount with its Max,
            // then the memo, so that neither is cut short on a phone. Beside the
            // destination stands its Paste button, and beneath it, where a shell
            // gives a scanner, its Scan button.
            list = list.push(
                column![
                    row![
                        t(
                            format!("Destination {}", i + 1),
                            ty::FORM_LABEL,
                            color::TEXT_SECONDARY
                        )
                        .width(Length::Fill),
                        remove,
                    ]
                    .width(Length::Fill)
                    .align_y(Alignment::Center),
                    address_field(model, i, to, shown, false),
                    column![
                        t("Amount (MCM)", ty::FORM_LABEL, color::TEXT_SECONDARY),
                        amount
                    ]
                    .spacing(sp::S6)
                    .width(Length::Fill),
                    memo_block(r, reference).width(Length::Fill),
                ]
                .spacing(sp::S12),
            );
        }
    }
    let mut part = column![
        row![
            ui::section_label("Destinations"),
            space().width(Length::Fill),
            t(
                format!("{} of {MAX_DESTINATIONS}", f.rows.len()),
                ty::NOTE,
                color::TEXT_SECONDARY
            ),
        ],
        list,
        ui::button_with(
            "Add destination",
            theme::Button::Dashed,
            Size::Medium,
            Some(Icon::Plus),
            (idle && f.rows.len() < usize::from(MAX_DESTINATIONS))
                .then_some(WalletMsg::AddDestination.into()),
        )
        .width(Length::Fill),
    ]
    .spacing(sp::S12);
    if exact {
        let mut fee = field(&f.fee, "the fee per destination", true);
        let mut btl = field(&f.blk_to_live, "none", true);
        if idle {
            fee = fee.on_input(|v| WalletMsg::Fee(v).into());
            btl = btl.on_input(|v| WalletMsg::BlockToLive(v).into());
        }
        let fee = column![
            t(
                "Fee, nanoMCM in total",
                ty::FORM_LABEL,
                color::TEXT_SECONDARY
            ),
            fee
        ]
        .spacing(sp::S6)
        .width(Length::Fill);
        let btl = column![
            t("Block-to-live", ty::FORM_LABEL, color::TEXT_SECONDARY),
            btl
        ]
        .spacing(sp::S6)
        .width(Length::Fill);
        // On a phone held upright the two fields stand one over the other; in
        // the table's width, side by side.
        let fees: Element<'a, Message> = if table {
            row![fee, btl].spacing(sp::S12).into()
        } else {
            column![fee, btl].spacing(sp::S12).into()
        };
        part = part.push(fees);
    }
    part.push(ui::helper(MEMO_RULE)).into()
}

/// The send form's summary: what the typed figures come to, as far as they
/// can be read, the fee per destination, and the change left on `from`. The
/// figures that are signed are the library's, on the next step.
fn summary<'a>(
    model: &'a Model,
    s: &'a SendPage,
    from: Option<tawara_wallet_core::view::AccountId>,
) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let rows: Vec<_> = s
        .form
        .rows
        .iter()
        .filter(|r| !r.to.trim().is_empty() || !r.amount.trim().is_empty())
        .collect();
    let count = rows.len().max(1);
    let sent: Option<u64> = rows
        .iter()
        .map(|r| tawara_wallet_core::amount::parse_mcm(&r.amount).ok())
        .try_fold(0u64, |sum, a| a.and_then(|a| sum.checked_add(a)));
    let fee = FEE_PER_DESTINATION.checked_mul(u64::try_from(count).unwrap_or(u64::MAX));
    let shown = |v: Option<u64>| v.map_or_else(|| "—".to_owned(), |v| ui::amount(v, unit));
    let total = sent.zip(fee).and_then(|(a, b)| a.checked_add(b));
    let from = from.and_then(|id| row_of(model, id));
    let change = from
        .and_then(|a| a.state.balance())
        .zip(total)
        .and_then(|(balance, total)| balance.checked_sub(total));
    let mut card = column![
        ui::section_label("Summary"),
        figure(
            if count == 1 {
                "1 destination".to_owned()
            } else {
                format!("{count} destinations")
            },
            shown(sent)
        ),
        figure("Network fee", shown(fee)),
        ui::helper(fee_line()),
        ui::divider(),
        row![
            t("Total", ty::BODY, color::TEXT_SECONDARY).width(Length::Fill),
            t(shown(total), ty::STAT_VALUE, color::TEXT_PRIMARY),
            t(ui::unit_name(unit), ty::CHIP, color::ACCENT),
        ]
        .spacing(sp::S8)
        .align_y(Alignment::Center),
    ]
    .spacing(sp::S12);
    if from.is_some() {
        card = card.push(figure("Change", shown(change)));
    }
    ui::card_for(model.compact(), card).into()
}

/// A row of buttons. On a phone it wraps onto the next line when the buttons
/// do not fit; on the desktop it is one row.
fn button_row<'a>(compact: bool, buttons: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let buttons = Row::with_children(buttons).spacing(sp::S10);
    if compact {
        buttons.wrap().vertical_spacing(sp::S10).into()
    } else {
        buttons.into()
    }
}

/// The review: the library's page for the spend, not signed, whole; then signing,
/// on its own step.
fn review<'a>(model: &'a Model, page: &'a WalletPage, plan: &'a PlanView) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let compact = model.compact();
    let index = row_of(model, plan.from).map(|a| a.index);
    // What to check, one destination each, as they go on the wire. The
    // desktop's review draws them as a table with its header. A phone never
    // reaches this function: its page 3 is `review_page`, which draws each
    // destination as a block at every width.
    let mut list = column![ui::section_label(
        "Check each destination against its payee"
    )]
    .spacing(sp::S12);
    if !compact {
        list = list.push(
            row![
                t("#", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::Fixed(20.0)),
                t("Destination", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::FillPortion(6)),
                t("Memo", ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::FillPortion(3)),
                container(t(
                    format!("Amount ({})", ui::unit_name(unit)),
                    ty::TABLE_HEADER,
                    color::TEXT_MUTED
                ))
                .width(Length::FillPortion(3))
                .align_x(Alignment::End),
            ]
            .spacing(sp::S12),
        );
    }
    for (i, d) in plan.destinations.iter().enumerate() {
        let entry: Element<'a, Message> = if compact {
            planned_block(unit, i, d)
        } else {
            row![
                t((i + 1).to_string(), ty::MONO, color::TEXT_MUTED).width(Length::Fixed(20.0)),
                t(d.destination.clone(), ty::MONO, color::TEXT_PRIMARY)
                    .wrapping(Wrapping::WordOrGlyph)
                    .width(Length::FillPortion(6)),
                t(
                    if d.reference.is_empty() {
                        "none".to_owned()
                    } else {
                        d.reference.clone()
                    },
                    ty::MONO,
                    color::TEXT_SECONDARY,
                )
                .width(Length::FillPortion(3)),
                container(
                    t(
                        ui::amount(d.amount, unit),
                        ty::TABLE_AMOUNT,
                        color::TEXT_PRIMARY
                    )
                    .wrapping(Wrapping::WordOrGlyph),
                )
                .width(Length::FillPortion(3))
                .align_x(Alignment::End),
            ]
            .spacing(sp::S12)
            .align_y(Alignment::Center)
            .into()
        };
        list = list.push(ui::divider()).push(entry);
    }
    let main = column![
        report::show_in(compact, page, ReportKey::Plan, report::plan(plan, index)),
        ui::card_for(compact, list),
    ]
    .spacing(sp::S16);
    let sign = action(
        model,
        "Sign & submit",
        theme::Button::Primary,
        Size::Xl,
        None,
        WalletMsg::Sign,
    );
    let edit = action(
        model,
        "Edit the spend",
        theme::Button::Secondary,
        Size::Medium,
        None,
        WalletMsg::Edit,
    );
    // Stacked, "Sign & submit" would stand where "Review spend" was pressed on
    // the form: a second tap would sign. So Edit stands first there, and the
    // side column keeps its order.
    let buttons: [Element<'a, Message>; 2] = if compact { [edit, sign] } else { [sign, edit] };
    let side = column![ui::card_for(compact, plan_totals(unit, plan))]
        .extend(buttons)
        .spacing(sp::S16);
    frame(
        model,
        page,
        "Send",
        "Check every destination against its payee. Nothing has been signed yet.",
        Vec::new(),
        vec![pair(model, main.into(), side.into(), 7, 4)],
    )
}

/// One destination of a plan as a phone's review draws it: the number and the
/// amount on one line, then the destination and the reference, each wrapping
/// at its glyphs.
fn planned_block<'a>(
    unit: AmountUnit,
    i: usize,
    d: &'a PlannedDestination,
) -> Element<'a, Message> {
    let reference = if d.reference.is_empty() {
        "none"
    } else {
        d.reference.as_str()
    };
    column![
        row![
            t(format!("#{}", i + 1), ty::MONO, color::TEXT_MUTED),
            space().width(Length::Fill),
            t(
                ui::amount(d.amount, unit),
                ty::TABLE_AMOUNT,
                color::TEXT_PRIMARY
            )
            .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center),
        t(d.destination.clone(), ty::MONO_SMALL, color::TEXT_PRIMARY)
            .wrapping(Wrapping::WordOrGlyph),
        t(
            format!("Memo: {reference}"),
            ty::MONO_SMALL,
            color::TEXT_SECONDARY
        )
        .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(sp::S6)
    .into()
}

/// The library's figures for a plan: how many destinations, what they send,
/// the fee and the total, the change, and the balance the plan was laid out
/// on.
fn plan_totals<'a>(unit: AmountUnit, plan: &'a PlanView) -> Element<'a, Message> {
    let count = plan.destinations.len();
    column![
        ui::section_label("Summary"),
        figure(
            if count == 1 {
                "1 destination".to_owned()
            } else {
                format!("{count} destinations")
            },
            ui::amount(plan.send_total, unit)
        ),
        figure("Network fee", ui::amount(plan.fee_total, unit)),
        ui::divider(),
        row![
            t("Total", ty::BODY, color::TEXT_SECONDARY).width(Length::Fill),
            t(
                ui::amount(
                    u128::from(plan.send_total) + u128::from(plan.fee_total),
                    unit
                ),
                ty::STAT_VALUE,
                color::TEXT_PRIMARY
            ),
            t(ui::unit_name(unit), ty::CHIP, color::ACCENT),
        ]
        .spacing(sp::S8)
        .align_y(Alignment::Center),
        figure(
            "Change",
            if plan.empties_account {
                "none: this empties the account".to_owned()
            } else {
                ui::amount(plan.change_total, unit)
            }
        ),
        figure("Balance it was laid out on", ui::amount(plan.balance, unit)),
    ]
    .spacing(sp::S12)
    .into()
}

/// Page 3 of 4 on a phone: the library's page for the spend, not signed, as
/// its banner; the destinations in full; the library's figures; and "Continue
/// to signing", with no Sign button.
fn review_page<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    plan: &'a PlanView,
) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let compact = model.compact();
    let index = row_of(model, plan.from).map(|a| a.index);
    let mut list = column![ui::section_label(
        "Check each destination against its payee"
    )]
    .spacing(sp::S12);
    for (i, d) in plan.destinations.iter().enumerate() {
        list = list.push(ui::divider()).push(planned_block(unit, i, d));
    }
    let mut next = column![action(
        model,
        "Continue to signing",
        theme::Button::Primary,
        Size::Xl,
        None,
        WalletMsg::ToSigning,
    )]
    .spacing(sp::S16);
    for part in beside(model, page) {
        next = next.push(part);
    }
    frame_beside(
        model,
        page,
        "Send",
        "Check every destination against its payee. Nothing has been signed yet.",
        vec![back(model)],
        vec![
            place(3),
            report::show_in(compact, page, ReportKey::Plan, report::plan(plan, index)),
            ui::card_for(compact, list).into(),
            ui::card_for(compact, plan_totals(unit, plan)).into(),
            next.into(),
        ],
    )
}

/// Page 4 of 4 on a phone: "Sign & submit" alone, with a line in the sent
/// page's words that sending is not yet the network's verdict.
fn sign_page<'a>(model: &'a Model, page: &'a WalletPage) -> Element<'a, Message> {
    let mut next = column![
        action(
            model,
            "Sign & submit",
            theme::Button::Primary,
            Size::Xl,
            None,
            WalletMsg::Sign,
        ),
        ui::helper(SUBMIT_NOTE),
    ]
    .spacing(sp::S16);
    for part in beside(model, page) {
        next = next.push(part);
    }
    frame_beside(
        model,
        page,
        "Send",
        "The key is reserved and the spend signed when the button below is pressed.",
        vec![back(model)],
        vec![place(4), next.into()],
    )
}

/// What page 4 says under "Sign & submit": that sending is not yet the
/// network's verdict.
const SUBMIT_NOTE: &str = "Submitting sends the signed spend to the API. That is not yet the \
                           network's verdict: the spend settles once a block includes it.";

/// The sent page's Retry artifact card, for bytes not saved: Tawara keeps its
/// own copy, and sends it again after a reorganisation, so a copy of the
/// person's own is optional.
const UNSAVED_ARTIFACT: &str = "The signed bytes. Tawara keeps a copy beside the keystore until \
                                the account spends again and more than 256 blocks have passed, \
                                and if a reorganisation of the chain reverts the spend, sends \
                                that copy again by itself. Save or copy them here for a copy of \
                                your own; this page's is gone once you leave it.";

/// The sent page's Retry artifact card, for bytes saved to a file.
const SAVED_ARTIFACT: &str = "The signed bytes, saved to the file named below. Tawara also keeps a \
                              copy beside the keystore until the account spends again and more \
                              than 256 blocks have passed, and sends it again by itself if a \
                              reorganisation of the chain reverts the spend.";

/// What the sent page says once Dismiss is pressed, before it is confirmed.
pub(crate) const DISMISS_WARNING: &str = "Dismissing wipes this page's copy of the bytes. \
                                          Tawara's own copy beside the keystore stays until the \
                                          account spends again and more than 256 blocks have \
                                          passed, so a spend a reorganisation reverts is still \
                                          sent again.";

/// The sent page's Dismiss for bytes not saved, on its own row below the
/// card's other controls. Pressed once, it asks to be confirmed and says what
/// is lost.
fn dismiss<'a>(model: &Model, compact: bool, s: &'a SentPage) -> Element<'a, Message> {
    if !s.dismissing {
        return button_row(
            compact,
            vec![action(
                model,
                "Dismiss",
                theme::Button::Secondary,
                Size::Medium,
                None,
                WalletMsg::AskDismiss,
            )],
        );
    }
    column![
        ui::warning_callout("Dismiss the signed bytes?", DISMISS_WARNING),
        button_row(
            compact,
            vec![
                action(
                    model,
                    "Dismiss the bytes",
                    theme::Button::WarningPrimary,
                    Size::Medium,
                    None,
                    WalletMsg::DismissSigned(s.sent.artifact_hex.clone()),
                ),
                action(
                    model,
                    "Keep them",
                    theme::Button::Secondary,
                    Size::Medium,
                    None,
                    WalletMsg::CancelDismiss,
                ),
            ],
        ),
    ]
    .spacing(sp::S12)
    .into()
}

/// The sent page: what happened, the bytes the store does not keep, and the
/// library's page with its three facts, whole.
fn sent_page<'a>(model: &'a Model, page: &'a WalletPage, s: &'a SentPage) -> Element<'a, Message> {
    let sent = &s.sent;
    let compact = model.compact();
    let buttons = vec![
        action(
            model,
            "Save artifact",
            theme::Button::Primary,
            Size::Medium,
            Some(Icon::Download),
            WalletMsg::SaveArtifact,
        ),
        copy(page, &sent.artifact_hex, "Copy hex"),
    ];
    // What the card says follows whether the bytes are saved.
    let about = if s.unsaved() {
        UNSAVED_ARTIFACT
    } else {
        SAVED_ARTIFACT
    };
    let mut artifact = column![
        ui::section_label("Retry artifact"),
        t(about, ty::BODY_SMALL, color::TEXT_SECONDARY),
        button_row(compact, buttons),
    ]
    .spacing(sp::S12);
    if let Some(note) = super::saved_note(s) {
        artifact = artifact.push(note);
    }
    if let Some(id) = &sent.tx_id {
        artifact = artifact.push(
            t(
                format!("Transaction id {id}"),
                ty::MONO_SMALL,
                color::TEXT_MUTED,
            )
            .wrapping(Wrapping::WordOrGlyph),
        );
    }
    // Dismiss is offered only for bytes that are not saved, apart from Save
    // and only once confirmed, since it wipes the only copy Tawara holds.
    if s.unsaved() {
        artifact = artifact.push(dismiss(model, compact, s));
    }
    let body = vec![
        report::show_in(compact, page, ReportKey::Sent, report::sent(sent)),
        ui::card_for(compact, artifact).into(),
        button_row(
            compact,
            vec![
                action(
                    model,
                    "Back to the wallet",
                    theme::Button::Secondary,
                    Size::Medium,
                    None,
                    WalletMsg::Open(To::Dashboard),
                ),
                action(
                    model,
                    "Open the account",
                    theme::Button::Secondary,
                    Size::Medium,
                    None,
                    WalletMsg::Open(To::Account(sent.from)),
                ),
            ],
        ),
    ];
    frame(
        model,
        page,
        if s.resigned { "Re-signed" } else { "Sent" },
        if sent.submitted {
            "Signed and sent to the API. That is not yet the network's verdict: the spend settles \
             once a block includes it."
        } else {
            "Signed, and not sent to the API: the bytes below are the spend."
        },
        Vec::new(),
        body,
    )
}

/// The re-sign page: the reserved spend from one account, typed again
/// exactly, signed again and submitted.
pub fn resign<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    r: &'a ResignPage,
) -> Element<'a, Message> {
    let form = column![
        ui::warning_callout(
            "The same spend, exactly",
            "Give the destinations, amounts, memos, fee and block-to-live the reserved spend \
             was made with. Anything else is refused as a different spend, and nothing \
             is signed.",
        ),
        destinations(model, &r.form, true),
    ]
    .spacing(sp::S20);
    let side = column![
        ui::button_with(
            "Re-sign & submit",
            theme::Button::Primary,
            Size::Xl,
            None,
            (model.busy.is_none() && memos_ok(&r.form)).then(|| Message::Wallet(WalletMsg::Resign)),
        ),
        ui::helper(
            "Re-signing reproduces the reserved spend's bytes with its key, which is already \
             reserved: no new key is used.",
        ),
    ]
    .spacing(sp::S16);
    frame(
        model,
        page,
        "Re-sign",
        format!(
            "The spend reserved from {}, signed again and submitted.",
            name(r.account)
        ),
        Vec::new(),
        vec![pair(
            model,
            ui::card_for(model.compact(), form).into(),
            side.into(),
            7,
            4,
        )],
    )
}

/// The saved-artifact page: signed bytes saved earlier, written to the
/// node's socket again. No store is used.
pub fn submit<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    s: &'a SubmitPage,
) -> Element<'a, Message> {
    submit_form(model, page, s, RETURN_BROADCASTS)
}

/// The saved-artifact page with Return's binding given, so that the tests,
/// which run on the desktop, can draw the page as a phone has it. Its Submit
/// stands beside its helper from [`crate::app::COMPACT`] wide, a phone held
/// sideways included, and above it narrower.
fn submit_form<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    s: &'a SubmitPage,
    return_broadcasts: bool,
) -> Element<'a, Message> {
    let mut hex = text_input("the hex the sent page saved or copied", &s.hex)
        .font(ty::MONO_SMALL.font)
        .size(ty::MONO_SMALL.size)
        .padding(Padding::from([sp::S12, sp::S14]))
        .style(theme::field(radius::R12));
    if model.busy.is_none() {
        hex = hex
            .on_input(|v| WalletMsg::ArtifactHex(v).into())
            .on_submit_maybe(return_broadcasts.then_some(WalletMsg::Submit.into()));
    }
    let compact = model.compact();
    let label = t(
        "Signed bytes, as hex",
        ty::FORM_LABEL,
        color::TEXT_SECONDARY,
    );
    let submit = action(
        model,
        "Submit",
        theme::Button::Primary,
        Size::Medium,
        None,
        WalletMsg::Submit,
    );
    // What the page says of the bytes depends on what they are sent as.
    let (helper, subtitle) = match s.mode {
        SubmitMode::Artifact => (
            ui::helper("Only the bytes' layout is checked here; the network validates the rest."),
            "A signed spend saved earlier, sent to the API again. Nothing is signed.",
        ),
        SubmitMode::Resubmit(_) => (
            ui::helper(
                "Only the identical saved copy is accepted, and only while the balance at its \
                 source has not moved and its block-to-live has not passed. The library checks \
                 the copy before anything is written.",
            ),
            "A saved copy of a settle the chain reverted, sent to the API again. Nothing is \
             signed, and no key index moves.",
        ),
    };
    // On a phone, at any width, the field has the width of its row, less the
    // Paste button beside it; the desktop has no Paste. The layout follows the
    // window's width from 600 wide, as the desktop's does, and not the table's
    // 844 (`COMPACT`, not `PHONE_TABLE`): Submit stands above its helper on a
    // phone held upright, and beside it in a wider window, a phone held
    // sideways included.
    let paste = paste::button(
        model
            .busy
            .is_none()
            .then_some(Message::Paste(PasteInto::SignedBytes)),
    );
    let field: Element<'a, Message> = if model.phone() {
        row![hex, paste]
            .spacing(sp::S8)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
    } else {
        hex.into()
    };
    let actions: Element<'a, Message> = if compact {
        column![submit, helper].spacing(sp::S12).into()
    } else {
        row![submit, helper]
            .spacing(sp::S12)
            .align_y(Alignment::Center)
            .into()
    };
    let entry: Element<'a, Message> = column![label, field, actions].spacing(sp::S12).into();
    let mut body = vec![ui::card_for(compact, entry).into()];
    if let Some((accepted, text)) = &s.result {
        // A saved copy's answer is a write: a refusal is the page's error.
        let answer = match s.mode {
            SubmitMode::Artifact => report::submitted(*accepted, text),
            SubmitMode::Resubmit(_) => report::resubmitted(text),
        };
        body.push(report::show_in(compact, page, ReportKey::Submitted, answer));
    }
    frame(
        model,
        page,
        "Submit saved artifact",
        subtitle,
        Vec::new(),
        body,
    )
}

#[cfg(test)]
mod tests {
    use iced::advanced::clipboard::Null;
    use iced::advanced::layout;
    use iced::advanced::renderer::Headless;
    use iced::advanced::widget::operation::{Focusable, Outcome, focusable};
    use iced::advanced::widget::{Operation, Tree};
    use iced::keyboard::key::{Named, NativeCode, Physical};
    use iced::keyboard::{Key, Location, Modifiers};
    use iced::mobile::SafeArea;
    use iced::{Event, Padding, Rectangle, keyboard, mouse};
    use iced_runtime::user_interface::{Cache, UserInterface};
    use tawara_wallet_core::preferences::Preferences;
    use tawara_wallet_core::sample;
    use tawara_wallet_core::view::AccountState;

    use super::*;
    use crate::app::{Busy, DestinationRow, FormOf, Level, NARROWEST, Page, Scanning, Screen};
    use crate::harness::{Driver, fits, fits_at, mouse_move, mouse_press, mouse_release, texts};
    use crate::host::ScanPreview;
    use crate::screens;

    /// A key press as the keyboard sends it, with the text it types, if any.
    fn press(key: Key, text: Option<&str>) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: text.map(Into::into),
            repeat: false,
        })
    }

    /// Draws the saved-artifact page with `view`, gives its field Tab's first
    /// focus, then sends `events` one at a time and returns the messages each
    /// produced.
    fn drive(
        view: impl for<'a> Fn(&'a Model, &'a WalletPage, &'a SubmitPage) -> Element<'a, Message>,
        events: &[Event],
    ) -> Vec<Vec<Message>> {
        let model = Model::new(Preferences::default(), None);
        let page = WalletPage {
            page: Page::Submit(SubmitPage::default()),
            error: None,
            copied: None,
            open: Default::default(),
            expanded: None,
            choosing: false,
        };
        let s = SubmitPage::default();
        let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            iced::Font::DEFAULT,
            iced::Pixels(16.0),
            Some("tiny-skia"),
        ))
        .expect("the software renderer starts");
        let mut ui = UserInterface::build(
            view(&model, &page, &s),
            iced::Size::new(1440.0, 900.0),
            Cache::default(),
            &mut renderer,
        );
        // Tab's first focus: the field is the page's first focusable widget,
        // so the keys below reach it.
        let mut next: Option<Box<dyn Operation>> = Some(Box::new(focusable::focus_next()));
        while let Some(mut operation) = next.take() {
            ui.operate(&renderer, operation.as_mut());
            if let Outcome::Chain(chained) = operation.finish() {
                next = Some(chained);
            }
        }
        events
            .iter()
            .map(|event| {
                let mut messages = Vec::new();
                ui.update(
                    std::slice::from_ref(event),
                    mouse::Cursor::Unavailable,
                    &mut renderer,
                    &mut Null,
                    &mut messages,
                );
                messages
            })
            .collect()
    }

    #[test]
    fn return_in_the_signed_bytes_field_submits_on_the_host() {
        // The page as the application draws it, so this pins the desktop's
        // value of RETURN_BROADCASTS.
        let sent = drive(submit, &[press(Key::Named(Named::Enter), None)]);
        assert!(
            matches!(sent.as_slice(), [m] if matches!(m.as_slice(), [Message::Wallet(WalletMsg::Submit)])),
            "{sent:?}"
        );
    }

    #[test]
    fn return_in_the_signed_bytes_field_does_nothing_on_a_phone() {
        // The page as a phone draws it. The
        // letter after Return shows that the field had the focus and still
        // took input, so the silence on Return is the binding's and not a
        // missed focus.
        let sent = drive(
            |model, page, s| submit_form(model, page, s, false),
            &[
                press(Key::Named(Named::Enter), None),
                press(Key::Character("a".into()), Some("a")),
            ],
        );
        assert!(
            matches!(
                sent.as_slice(),
                [on_return, on_letter]
                    if on_return.is_empty()
                        && matches!(on_letter.as_slice(), [Message::Wallet(WalletMsg::ArtifactHex(v))] if v == "a")
            ),
            "{sent:?}"
        );
    }

    /// The sample wallet open on `page` at the phone's narrowest width, with
    /// the report `open` (if any) opened to its summary.
    fn phone(page: Page, open: Option<ReportKey>) -> Model {
        let mut model = Model::new(Preferences::default(), None);
        model.width = NARROWEST;
        model.wallet = Some(sample::wallet_view());
        let mut wallet = WalletPage {
            page,
            ..WalletPage::default()
        };
        if let Some(key) = open {
            wallet.open.insert(key, Level::Summary);
        }
        model.screen = Screen::Wallet(wallet);
        model
    }

    /// The sample's spend as a person types it: its two payees, with the
    /// figures the sample lays out (`sample::plan_view`).
    fn typed() -> SpendForm {
        SpendForm {
            rows: sample::plan_view()
                .destinations
                .iter()
                .map(|d| DestinationRow {
                    to: d.destination.clone(),
                    amount: tawara_wallet_core::amount::format_mcm(d.amount)
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_owned(),
                    reference: d.reference.clone(),
                    memo_checked: false,
                })
                .collect(),
            ..SpendForm::default()
        }
    }

    /// The sample wallet's accounts, the first of which the sample spend comes
    /// from.
    fn accounts() -> Vec<tawara_wallet_core::view::AccountId> {
        sample::wallet_view()
            .accounts
            .iter()
            .map(|a| a.id)
            .collect()
    }

    /// The send form from the sample's first account, with `form`.
    fn compose(form: SpendForm) -> Page {
        Page::Send(SendPage {
            from: Some(accounts()[0]),
            form,
            stage: SendStage::Compose,
        })
    }

    /// The send flow in `stage` from the sample's first account, with `form`.
    fn stage_page(stage: SendStage, form: SpendForm) -> Page {
        Page::Send(SendPage {
            from: Some(accounts()[0]),
            form,
            stage,
        })
    }

    /// The sample's first account's plan, as the worker holds it on page 3 and
    /// page 4.
    fn plan() -> Box<PlanView> {
        Box::new(sample::plan_view())
    }

    /// `page` in the sample wallet as a phone holds it: its folder is the
    /// phone's, so the page is a phone's, and its window is `width` wide.
    fn phone_at(page: Page, width: f32) -> Model {
        let mut model = phone(page, None);
        model.width = width;
        model.confine(std::env::temp_dir().join("tawara-phone-folder"));
        model
    }

    /// `page` in the sample wallet as the desktop holds it, `width` wide.
    fn desktop_at(page: Page, width: f32) -> Model {
        let mut model = phone(page, None);
        model.width = width;
        model
    }

    /// The sample wallet with its second account able to spend as well.
    fn two_can_spend() -> tawara_wallet_core::view::WalletView {
        let mut wallet = sample::wallet_view();
        wallet.accounts[1].state = AccountState::InSync {
            balance: 3_180_000_000_000,
        };
        wallet.accounts[1].spendable = true;
        wallet
    }

    /// `model` with its page's account list open, or shut.
    fn choosing(mut model: Model, open: bool) -> Model {
        if let Screen::Wallet(page) = &mut model.screen {
            page.choosing = open;
        }
        model
    }

    /// Every node and word of `root` that passes the edge of a window `width`
    /// by `height`, as [`fits`] finds them at 844: the phone's two sizes are
    /// held here, 360 by 640 and 390 by 844.
    fn past_the_edge(root: Element<'_, Message>, width: f32, height: f32) -> Vec<Rectangle> {
        crate::overflow::past_the_edge(root, &crate::harness::renderer(), width, height)
            .into_iter()
            .map(|past| past.bounds)
            .collect()
    }

    /// The phone's two sizes, the narrowest and the common one.
    const PHONES: [(f32, f32); 2] = [(360.0, 640.0), (390.0, 844.0)];

    /// Every text `model` draws in a window 2400 pixels tall, at its width.
    fn drawn_on(model: &Model) -> Vec<(String, Rectangle)> {
        texts(screens::view(model), (model.width, 2400.0))
    }

    /// Where the first text `drawn` holds that is `what` lies, or a panic.
    fn at(drawn: &[(String, Rectangle)], what: &str) -> Rectangle {
        drawn
            .iter()
            .find(|(text, _)| text == what)
            .map(|(_, at)| *at)
            .unwrap_or_else(|| panic!("{what:?} is not drawn: {drawn:?}"))
    }

    /// Whether `drawn` holds the text `what`.
    fn has(drawn: &[(String, Rectangle)], what: &str) -> bool {
        drawn.iter().any(|(text, _)| text == what)
    }

    /// The bounds of the focusable widgets of `root` in a phone's window, in
    /// Tab's order: a text field's bounds are its drawn box.
    fn focusable_bounds(root: Element<'_, Message>) -> Vec<Rectangle> {
        focusable_bounds_at(root, NARROWEST, 844.0)
    }

    /// [`focusable_bounds`] in a window `width` by `height`.
    fn focusable_bounds_at(root: Element<'_, Message>, width: f32, height: f32) -> Vec<Rectangle> {
        #[derive(Default)]
        struct Bounds(Vec<Rectangle>);

        impl Operation for Bounds {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }

            fn focusable(
                &mut self,
                _: Option<&iced::advanced::widget::Id>,
                bounds: Rectangle,
                _: &mut dyn Focusable,
            ) {
                self.0.push(bounds);
            }
        }

        let mut renderer = crate::harness::renderer();
        let mut ui = UserInterface::build(
            root,
            iced::Size::new(width, height),
            Cache::default(),
            &mut renderer,
        );
        let mut bounds = Bounds::default();
        ui.operate(&renderer, &mut bounds);
        bounds.0
    }

    /// The sample wallet with its second account set aside when the wallet
    /// opened, as the sample draws it (`w4-send-cannot-spend`): in sync, and
    /// not spendable until Refresh.
    fn set_aside() -> tawara_wallet_core::view::WalletView {
        let mut wallet = sample::wallet_view();
        wallet.accounts[1].state = AccountState::InSync {
            balance: 3_180_000_000_000,
        };
        wallet.accounts[1].spendable = false;
        wallet
    }

    /// `model` with a refusal `why` shown on its page, as a command gives it.
    fn with_error(mut model: Model, why: &str) -> Model {
        if let Screen::Wallet(page) = &mut model.screen {
            page.error = Some(why.to_owned());
        }
        model
    }

    /// `model` with its page's report `key` open to its summary.
    fn opened(mut model: Model, key: ReportKey) -> Model {
        if let Screen::Wallet(page) = &mut model.screen {
            page.open.insert(key, Level::Summary);
        }
        model
    }

    /// `model` with the worker answering a command of the page: busy, not yet
    /// started on.
    fn busy(mut model: Model) -> Model {
        model.busy = Some(Busy {
            id: tawara_wallet_core::RequestId::example(),
            activity: None,
            progress: None,
        });
        model
    }

    /// The phone's pages, each in the state that makes it longest, at `width`:
    /// the most destinations, the account set aside, the list of accounts
    /// open, refusals under the fields and beside the buttons, the busy card,
    /// and the review with its report open and shut.
    fn phone_pages(width: f32) -> Vec<(&'static str, Model)> {
        let mut three = typed();
        three.rows.push(DestinationRow {
            to: three.rows[0].to.clone(),
            amount: "1".to_owned(),
            reference: "PAYROLL-13".to_owned(),
            memo_checked: false,
        });
        let one = SpendForm {
            rows: typed().rows[..1].to_vec(),
            ..SpendForm::default()
        };
        let mut under_fields = BTreeMap::new();
        under_fields.insert(
            (0, RefusedField::Address),
            "Give this destination's address.".to_owned(),
        );
        under_fields.insert(
            (1, RefusedField::Reference),
            tawara_wallet_core::REFERENCE_RULE.to_owned(),
        );
        let to = || SendStage::To(BTreeMap::new());
        let mut set_aside_model = phone_at(stage_page(to(), typed()), width);
        set_aside_model.wallet = Some(set_aside());
        let mut set_aside_amounts = phone_at(
            Page::Send(SendPage {
                from: Some(accounts()[1]),
                form: typed(),
                stage: SendStage::Amount,
            }),
            width,
        );
        set_aside_amounts.wallet = Some(set_aside());
        let mut list = choosing(phone_at(stage_page(to(), typed()), width), true);
        list.wallet = Some(two_can_spend());
        let busy_planning = busy(phone_at(stage_page(SendStage::Amount, typed()), width));
        vec![
            ("page 1", phone_at(stage_page(to(), typed()), width)),
            (
                "page 1 with three destinations",
                phone_at(stage_page(to(), three.clone()), width),
            ),
            ("page 1 with the account set aside", set_aside_model),
            ("page 1 with the account list open", list),
            (
                "page 1 with a scanner, so Scan stands beneath each destination's field",
                with_scanner(
                    phone_at(stage_page(to(), typed()), width),
                    ScanPreview::Shell,
                ),
            ),
            (
                "the scanner's page, drawn by the application",
                scanning(
                    with_scanner(
                        phone_at(stage_page(to(), typed()), width),
                        ScanPreview::Application,
                    ),
                    0,
                ),
            ),
            (
                "page 1 with refusals under its fields",
                phone_at(
                    stage_page(SendStage::To(under_fields), three.clone()),
                    width,
                ),
            ),
            (
                "page 1 with a refusal beside Continue",
                with_error(
                    phone_at(stage_page(to(), typed()), width),
                    "Give at least one destination.",
                ),
            ),
            (
                "page 2",
                phone_at(stage_page(SendStage::Amount, typed()), width),
            ),
            (
                "page 2 with one destination",
                phone_at(stage_page(SendStage::Amount, one), width),
            ),
            ("page 2 with the account set aside", set_aside_amounts),
            (
                "page 2 with a refusal beside Review spend",
                with_error(
                    phone_at(stage_page(SendStage::Amount, typed()), width),
                    "The amount to destination 1 is not a number.",
                ),
            ),
            ("page 2 while the worker plans", busy_planning),
            (
                "page 3 with its report open",
                opened(
                    phone_at(stage_page(SendStage::Review(plan()), typed()), width),
                    ReportKey::Plan,
                ),
            ),
            (
                "page 3 with its report shut",
                phone_at(stage_page(SendStage::Review(plan()), typed()), width),
            ),
            (
                "page 4",
                phone_at(stage_page(SendStage::Sign(plan()), typed()), width),
            ),
            (
                "page 4 while the worker signs",
                busy(phone_at(
                    stage_page(SendStage::Sign(plan()), typed()),
                    width,
                )),
            ),
        ]
    }

    #[test]
    fn each_phone_page_fits_at_360_by_640_and_390_by_844() {
        for (width, height) in PHONES {
            for (name, model) in phone_pages(width) {
                let found = past_the_edge(screens::view(&model), width, height);
                assert!(found.is_empty(), "{name} at {width} by {height}: {found:?}");
            }
        }
    }

    #[test]
    fn each_phone_page_shows_its_place_and_its_buttons_and_only_page_four_signs() {
        // What each page draws, and what it must not: "Sign & submit" is on
        // page 4 alone, and "Continue to signing" on page 3.
        let pages: [(&str, Model, &[&str], &[&str]); 4] = [
            (
                "page 1",
                phone_at(
                    stage_page(SendStage::To(BTreeMap::new()), typed()),
                    NARROWEST,
                ),
                &[
                    "1 of 4",
                    "FROM ACCOUNT",
                    "Destination 1",
                    "Memo",
                    "Remove",
                    "Add another destination",
                    "Continue",
                    "Paste",
                ],
                &[
                    "Back",
                    "Review spend",
                    "Continue to signing",
                    "Sign & submit",
                    "Edit the spend",
                ],
            ),
            (
                "page 2",
                phone_at(stage_page(SendStage::Amount, typed()), NARROWEST),
                &[
                    "2 of 4",
                    "AMOUNTS",
                    "Amount (MCM)",
                    "Max",
                    "Review spend",
                    "Back",
                ],
                &[
                    "Fee, nanoMCM in total",
                    "Block-to-live",
                    "Continue",
                    "Continue to signing",
                    "Sign & submit",
                    "Remove",
                    "Paste",
                    "Edit the spend",
                ],
            ),
            (
                "page 3",
                phone_at(stage_page(SendStage::Review(plan()), typed()), NARROWEST),
                &[
                    "3 of 4",
                    "Not signed yet: check every destination against its payee",
                    "SUMMARY",
                    "Continue to signing",
                    "Back",
                ],
                &[
                    "Continue",
                    "Review spend",
                    "Sign & submit",
                    "Edit the spend",
                    "Paste",
                ],
            ),
            (
                "page 4",
                phone_at(stage_page(SendStage::Sign(plan()), typed()), NARROWEST),
                &["4 of 4", "Sign & submit", "Back", SUBMIT_NOTE],
                &[
                    "Reserve",
                    "Settle",
                    "Continue",
                    "Continue to signing",
                    "Review spend",
                    "Edit the spend",
                    "Paste",
                    "Remove",
                ],
            ),
        ];
        for (name, model, must, must_not) in &pages {
            let drawn = drawn_on(model);
            for what in *must {
                assert!(has(&drawn, what), "{name} draws {what:?}");
            }
            for what in *must_not {
                assert!(!has(&drawn, what), "{name} does not draw {what:?}");
            }
        }
        // Page 2 has a Max for a single destination, and no "Send everything".
        let one = SpendForm {
            rows: typed().rows[..1].to_vec(),
            ..SpendForm::default()
        };
        let drawn = drawn_on(&phone_at(stage_page(SendStage::Amount, one), NARROWEST));
        assert!(has(&drawn, "Max"));
        assert!(
            !drawn
                .iter()
                .any(|(text, _)| text.starts_with("Send everything"))
        );
    }

    #[test]
    fn change_is_drawn_only_when_another_account_can_spend_and_opens_the_list() {
        let one = phone_at(
            stage_page(SendStage::To(BTreeMap::new()), typed()),
            NARROWEST,
        );
        assert!(
            !has(&drawn_on(&one), "Change"),
            "no other account can spend"
        );

        let mut two = phone_at(
            stage_page(SendStage::To(BTreeMap::new()), typed()),
            NARROWEST,
        );
        two.wallet = Some(two_can_spend());
        let shut = drawn_on(&two);
        assert!(has(&shut, "Change"), "{shut:?}");
        assert!(
            !has(&shut, &name(accounts()[1])),
            "the list is shut: {shut:?}"
        );

        // Tapping Change asks to open the list; with the list open, Change
        // asks to shut it.
        let press = |model: &Model, spot: Rectangle| {
            let mut driver = Driver::new(screens::view(model), (360, 844));
            let (messages, _, _) = driver.send(&[
                mouse_move(spot.center_x(), spot.center_y()),
                mouse_press(),
                mouse_release(),
            ]);
            messages
        };
        let messages = press(&two, at(&shut, "Change"));
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Wallet(WalletMsg::ChooseAccount(true))]
            ),
            "{messages:?}"
        );

        let open = choosing(two, true);
        let drawn = drawn_on(&open);
        assert!(
            has(&drawn, &name(accounts()[1])),
            "the list is open: {drawn:?}"
        );
        let messages = press(&open, at(&drawn, "Change"));
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Wallet(WalletMsg::ChooseAccount(false))]
            ),
            "{messages:?}"
        );

        // Picking the second account chooses it.
        let messages = press(&open, at(&drawn, &name(accounts()[1])));
        assert!(
            matches!(messages.as_slice(), [Message::Wallet(WalletMsg::From(id))] if *id == accounts()[1]),
            "{messages:?}"
        );
    }

    #[test]
    fn change_is_drawn_when_the_account_chosen_cannot_spend_and_another_can() {
        // The account asked for cannot spend now, and the sample's first can.
        // The callout says to choose another account, so "Change" is drawn to
        // choose it, though it is the only other account that can spend.
        let model = phone_at(
            Page::Send(SendPage {
                from: Some(accounts()[1]),
                form: typed(),
                stage: SendStage::To(BTreeMap::new()),
            }),
            NARROWEST,
        );
        let shut = drawn_on(&model);
        assert!(has(&shut, "Change"), "{shut:?}");
        assert!(
            !has(&shut, &name(accounts()[0])),
            "the list is shut: {shut:?}"
        );

        let spot = at(&shut, "Change");
        let mut driver = Driver::new(screens::view(&model), (360, 844));
        let (messages, _, _) = driver.send(&[
            mouse_move(spot.center_x(), spot.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Wallet(WalletMsg::ChooseAccount(true))]
            ),
            "{messages:?}"
        );
    }

    #[test]
    fn a_phone_refusal_stands_beneath_the_button_that_caused_it() {
        // Review spend's refusal is drawn under that button, and Continue's
        // under Continue. The desktop draws its refusal at the top of the page.
        let why = "The amount to destination 1 is not a number.";
        let model = with_error(
            phone_at(stage_page(SendStage::Amount, typed()), NARROWEST),
            why,
        );
        let drawn = drawn_on(&model);
        assert!(
            at(&drawn, why).y > at(&drawn, "Review spend").y,
            "{drawn:?}"
        );

        let model = with_error(
            phone_at(
                stage_page(SendStage::To(BTreeMap::new()), typed()),
                NARROWEST,
            ),
            "Give at least one destination.",
        );
        let drawn = drawn_on(&model);
        assert!(
            at(&drawn, "Give at least one destination.").y > at(&drawn, "Continue").y,
            "{drawn:?}"
        );

        let model = with_error(desktop_at(compose(typed()), 1440.0), why);
        let drawn = drawn_on(&model);
        assert!(
            at(&drawn, why).y < at(&drawn, "Review spend").y,
            "{drawn:?}"
        );
    }

    #[test]
    fn the_busy_card_stands_beneath_the_button_its_command_came_from() {
        // While the worker signs, the card is under Sign & submit, which takes
        // no press.
        let model = busy(phone_at(
            stage_page(SendStage::Sign(plan()), typed()),
            NARROWEST,
        ));
        let drawn = drawn_on(&model);
        assert!(
            at(&drawn, "Waiting").y > at(&drawn, "Sign & submit").y,
            "{drawn:?}"
        );
        let sign = at(&drawn, "Sign & submit");
        let mut driver = Driver::new(screens::view(&model), (360, 844));
        let (messages, _, _) = driver.send(&[
            mouse_move(sign.center_x(), sign.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn the_desktop_keeps_w4_and_w5_as_one_page_each() {
        // The desktop's send form has its fields and "Review spend" on one page,
        // with no place in a flow; its review has "Sign & submit" and "Edit the
        // spend". A desktop is never on the phone's pages.
        let w4 = drawn_on(&desktop_at(compose(typed()), 1440.0));
        assert!(
            has(&w4, "Review spend") && has(&w4, "Add destination") && has(&w4, "FROM ACCOUNT"),
            "{w4:?}"
        );
        assert!(
            !has(&w4, "1 of 4") && !has(&w4, "Continue") && !has(&w4, "Remove"),
            "{w4:?}"
        );
        let w5 = drawn_on(&desktop_at(
            stage_page(SendStage::Review(plan()), typed()),
            1440.0,
        ));
        assert!(
            has(&w5, "Sign & submit") && has(&w5, "Edit the spend"),
            "{w5:?}"
        );
        assert!(
            !has(&w5, "3 of 4") && !has(&w5, "Continue to signing"),
            "{w5:?}"
        );
    }

    #[test]
    fn w6_fits_a_phone_screen_with_its_report_open() {
        // On a phone Save artifact asks the system for the share sheet, and the
        // page says so.
        let sent = SentPage {
            sent: sample::sent_view(),
            order: 0,
            resigned: false,
            saved: Some(Err(crate::app::SHARED.to_owned())),
            dismissing: false,
        };
        let model = phone(
            Page::Send(SendPage {
                from: Some(accounts()[0]),
                form: typed(),
                stage: SendStage::Sent(Box::new(sent)),
            }),
            Some(ReportKey::Sent),
        );
        assert_eq!(fits(screens::view(&model), NARROWEST), Ok(()));
        let drawn = texts(screens::view(&model), (NARROWEST, 2400.0));
        assert!(
            drawn.iter().any(|(text, _)| text == crate::app::SHARED),
            "the page says the sheet was asked for"
        );
    }

    #[test]
    fn the_kept_copy_texts_state_the_release_rule() {
        // The copy beside the keystore stays until the account's next spend
        // and the release depth, so each text that mentions it says both.
        let depth = format!(
            "more than {} blocks",
            tawara_wallet_core::retained::FINAL_DEPTH
        );
        for text in [
            UNSAVED_ARTIFACT,
            SAVED_ARTIFACT,
            DISMISS_WARNING,
            report::KEPT_COPY,
        ] {
            assert!(text.contains("the account spends again"), "{text}");
            assert!(text.contains(&depth), "{text}");
            assert!(!text.contains("is final"), "{text}");
        }
    }

    #[test]
    fn w6_offers_dismiss_only_for_unsaved_bytes_and_asks_before_it_drops_them() {
        // Dismiss is for bytes not saved, and wipes the only copy Tawara holds,
        // so it is confirmed first, with what is lost said beside it.
        let drawn = |saved: Option<Result<std::path::PathBuf, String>>, dismissing: bool| {
            let sent = SentPage {
                sent: sample::sent_view(),
                order: 0,
                resigned: false,
                saved,
                dismissing,
            };
            let model = phone(
                Page::Send(SendPage {
                    from: Some(accounts()[0]),
                    form: typed(),
                    stage: SendStage::Sent(Box::new(sent)),
                }),
                Some(ReportKey::Sent),
            );
            assert_eq!(fits(screens::view(&model), NARROWEST), Ok(()));
            texts(screens::view(&model), (NARROWEST, 3000.0))
                .into_iter()
                .map(|(text, _)| text)
                .collect::<Vec<_>>()
        };
        let has = |texts: &[String], what: &str| texts.iter().any(|t| t == what);

        let unsaved = drawn(None, false);
        assert!(has(&unsaved, "Dismiss"), "{unsaved:?}");
        assert!(has(&unsaved, UNSAVED_ARTIFACT));
        assert!(!has(&unsaved, DISMISS_WARNING));

        let asked = drawn(Some(Err(crate::app::SHARED.to_owned())), true);
        assert!(has(&asked, DISMISS_WARNING), "{asked:?}");
        assert!(has(&asked, "Dismiss the bytes"));
        assert!(has(&asked, "Keep them"));

        let saved = drawn(
            Some(Ok(std::path::PathBuf::from(
                "/home/you/Downloads/spend.hex",
            ))),
            false,
        );
        assert!(!has(&saved, "Dismiss"), "{saved:?}");
        assert!(!has(&saved, "Dismiss the bytes"));
        assert!(has(&saved, SAVED_ARTIFACT));
        assert!(!has(&saved, UNSAVED_ARTIFACT));
    }

    #[test]
    fn w8_fits_a_phone_screen() {
        // The re-sign page at both phone sizes, bare and with a shell's scanner:
        // its Paste stands beside each field and its Scan beneath it, and nothing
        // passes the edge.
        let page = || {
            Page::Resign(ResignPage {
                account: accounts()[1],
                form: typed(),
            })
        };
        for (width, height) in PHONES {
            let bare = phone_at(page(), width);
            let scanned = with_scanner(phone_at(page(), width), ScanPreview::Shell);
            for model in [bare, scanned] {
                assert_eq!(
                    fits_at(screens::view(&model), width, height),
                    Ok(()),
                    "at {width} by {height}"
                );
            }
            // The first destination keeps the width its card gives less its Paste.
            let model = phone_at(page(), width);
            let field = focusable_bounds_at(screens::view(&model), width, height)[0];
            assert!(
                field.width >= 200.0,
                "at {width} by {height} the destination is {field:?}"
            );
        }
    }

    #[test]
    fn w9_fits_a_phone_screen() {
        let model = phone(
            Page::Submit(SubmitPage {
                hex: sample::sent_view().artifact_hex,
                result: Some((true, sample::submitted_text())),
                ..SubmitPage::default()
            }),
            None,
        );
        assert_eq!(fits(screens::view(&model), NARROWEST), Ok(()));
    }

    #[test]
    fn w9_in_resubmit_mode_says_what_is_accepted_and_what_a_write_establishes() {
        // The page says that only the identical
        // saved copy is accepted, under the library's two conditions, and the
        // written copy's report says that the write establishes no more than
        // that. Both fit a phone.
        let written = "The saved copy was sent to the API.";
        let model = phone(
            Page::Submit(SubmitPage {
                hex: sample::sent_view().artifact_hex,
                mode: SubmitMode::Resubmit(accounts()[2]),
                result: Some((true, written.to_owned())),
            }),
            Some(ReportKey::Submitted),
        );
        assert_eq!(fits(screens::view(&model), NARROWEST), Ok(()));
        let drawn: Vec<String> = texts(screens::view(&model), (NARROWEST, 844.0))
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        // The report is its title alone, which says what a write establishes;
        // its summary is not drawn.
        for line in [
            "Only the identical saved copy is accepted, and only while the balance at its source \
             has not moved and its block-to-live has not passed. The library checks the copy \
             before anything is written.",
            "Sent to the API: not yet accepted by the network",
        ] {
            assert!(drawn.iter().any(|text| text == line), "{line}: {drawn:?}");
        }
        assert!(
            !drawn
                .iter()
                .any(|text| text.starts_with("Only the write is confirmed here")),
            "{drawn:?}"
        );
    }

    #[test]
    fn w9_of_an_open_reservation_keeps_its_copy_and_no_resubmit_copy() {
        // The copy follows the mode: the
        // bytes of an open reservation keep the copy they had. The command each
        // mode sends is tested with `submit_command`, in app/wallet.rs.
        let model = phone(
            Page::Submit(SubmitPage {
                hex: sample::sent_view().artifact_hex,
                ..SubmitPage::default()
            }),
            None,
        );
        let drawn: Vec<String> = texts(screens::view(&model), (NARROWEST, 844.0))
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert!(
            drawn.iter().any(|text| text
                == "Only the bytes' layout is checked here; the network validates the rest."),
            "{drawn:?}"
        );
        assert!(
            !drawn
                .iter()
                .any(|text| text.starts_with("Only the identical saved copy")),
            "{drawn:?}"
        );
    }

    #[test]
    fn on_a_phone_the_first_destination_takes_typing() {
        // Tab reaches the first destination's field, and what is typed into it
        // is that destination's.
        let model = phone_at(
            stage_page(SendStage::To(BTreeMap::new()), SpendForm::default()),
            NARROWEST,
        );
        let mut driver = Driver::new(screens::view(&model), (360, 844));
        driver.focus_next();
        let (messages, _, _) = driver.send(&[press(Key::Character("a".into()), Some("a"))]);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Wallet(WalletMsg::DestinationTo(0, to))] if to == "a"
            ),
            "{messages:?}"
        );
    }

    /// Where each Paste button's label lies in `model`'s window at the
    /// phone's narrowest width.
    fn pastes(model: &Model) -> Vec<Rectangle> {
        pastes_at(model, NARROWEST)
    }

    /// Where each Paste button's label lies in `model`'s window `width` wide.
    fn pastes_at(model: &Model, width: f32) -> Vec<Rectangle> {
        labels_at(model, width, "Paste")
    }

    /// Where each Scan button's label lies in `model`'s window `width` wide.
    fn scans_at(model: &Model, width: f32) -> Vec<Rectangle> {
        labels_at(model, width, "Scan")
    }

    /// Where each button labelled `label` lies in `model`'s window `width` wide,
    /// top to bottom.
    fn labels_at(model: &Model, width: f32, label: &str) -> Vec<Rectangle> {
        texts(screens::view(model), (width, 844.0))
            .into_iter()
            .filter(|(text, _)| text == label)
            .map(|(_, at)| at)
            .collect()
    }

    /// `model` as a phone whose shell gives a scanner, which draws the camera's
    /// preview in the application when `preview` says so.
    fn with_scanner(mut model: Model, preview: ScanPreview) -> Model {
        model.scanner = Some(crate::harness::scanner(preview));
        model
    }

    /// `model` with a scan of destination `row` under way, begun from the
    /// sample's form as the page shows it.
    fn scanning(mut model: Model, row: usize) -> Model {
        model.scanning = Some(Scanning {
            ask: 1,
            row,
            form: FormOf::Send(Some(accounts()[0])),
            rows: typed().rows.len(),
            typed: typed().rows[row].clone(),
        });
        model
    }

    /// The phone's two sizes as the scanner's page is held to them: the phone's
    /// insets, which are the status bar's above and the home indicator's below.
    const PHONE_INSETS: Padding = Padding {
        top: 47.0,
        right: 0.0,
        bottom: 34.0,
        left: 0.0,
    };

    #[test]
    fn on_a_phone_a_destination_takes_the_width_of_its_card_beside_its_paste() {
        // The destination has a block of its own, so its field is as wide as the
        // card's inside less the Paste button: a share of a row with the amount
        // and the reference would be well under the 200 pixels asked for here.
        let model = phone_at(
            stage_page(SendStage::To(BTreeMap::new()), typed()),
            NARROWEST,
        );
        let fields = focusable_bounds(screens::view(&model));
        let field = fields.first().copied().expect("the first destination");
        let paste = pastes(&model)[0];
        assert!(field.width >= 200.0, "{fields:?}");
        assert!(
            paste.x > field.x + field.width && paste.y < field.y + field.height,
            "the Paste button stands to the field's right: {paste:?}, {field:?}"
        );
    }

    #[test]
    fn on_a_phone_a_destination_beside_its_paste_and_its_scan_beneath_it() {
        // With a scanner, the Paste button stands beside the field and the Scan
        // button beneath it, on a line of its own, so the destination keeps the width
        // its card gives less the Paste, at 360, at 390, and on a phone held sideways
        // narrower than the table's 844, at 667 and 780.
        for width in [NARROWEST, 390.0, 667.0, 780.0] {
            let model = with_scanner(
                phone_at(stage_page(SendStage::To(BTreeMap::new()), typed()), width),
                ScanPreview::Shell,
            );
            let fields = focusable_bounds_at(screens::view(&model), width, 844.0);
            let field = fields.first().copied().expect("the first destination");
            let paste = pastes_at(&model, width)[0];
            let scan = scans_at(&model, width)[0];
            assert!(
                field.width >= 200.0,
                "at {width} wide the destination is {field:?}"
            );
            assert!(
                field.x + field.width < paste.x && paste.y < field.y + field.height,
                "the Paste stands beside the field: {paste:?}, {field:?}"
            );
            assert!(
                scan.y > field.y + field.height && scan.x < paste.x,
                "the Scan stands beneath the field: {scan:?}, {field:?}"
            );
        }
    }

    #[test]
    fn on_a_phone_the_signed_bytes_field_takes_its_row_beside_its_paste() {
        let model = phone_at(
            Page::Submit(SubmitPage {
                hex: sample::sent_view().artifact_hex,
                result: None,
                ..SubmitPage::default()
            }),
            NARROWEST,
        );
        let fields = focusable_bounds(screens::view(&model));
        let field = fields.first().copied().expect("the signed-bytes field");
        let paste = pastes(&model)[0];
        assert!(field.width >= 200.0, "{fields:?}");
        assert!(
            paste.x > field.x + field.width && paste.y < field.y + field.height,
            "{paste:?}, {field:?}"
        );
    }

    #[test]
    fn a_paste_button_stands_by_each_pasted_field_on_a_phone_and_none_on_the_desktop() {
        // Page 1 has a Paste for each destination, and the amounts, the review
        // and the signing have none. The re-sign and saved-artifact pages keep
        // theirs.
        let pages = || {
            [
                (stage_page(SendStage::To(BTreeMap::new()), typed()), 2),
                (stage_page(SendStage::Amount, typed()), 0),
                (stage_page(SendStage::Review(plan()), typed()), 0),
                (stage_page(SendStage::Sign(plan()), typed()), 0),
                (
                    Page::Resign(ResignPage {
                        account: accounts()[1],
                        form: typed(),
                    }),
                    2,
                ),
                (Page::Submit(SubmitPage::default()), 1),
            ]
        };
        // A phone pastes at every width, held sideways as upright, and in a
        // table or in blocks.
        for width in [NARROWEST, 390.0, 600.0, 667.0, 780.0, 844.0, 1023.0] {
            for (page, fields) in pages() {
                assert_eq!(
                    pastes_at(&phone_at(page, width), width).len(),
                    fields,
                    "at {width} wide"
                );
            }
        }
        for width in [1024.0, 1440.0] {
            for (page, _) in pages() {
                let drawn = drawn_on(&desktop_at(page, width));
                assert!(
                    !has(&drawn, "Paste"),
                    "the desktop pastes from the keyboard, at {width} wide"
                );
            }
        }
    }

    #[test]
    fn a_scan_button_stands_beneath_each_field_and_none_without_a_scanner() {
        // A Scan button for each destination row, on page 1 and on the re-sign
        // form. In a block, on a phone narrower than 844, the Paste stands
        // beside the field and the Scan on a line of its own beneath it; in a
        // table row, from 844 wide held sideways, the Paste and the Scan both
        // stand beneath the field. A phone whose shell gives no scanner has none.
        let pages = || {
            [
                stage_page(SendStage::To(BTreeMap::new()), typed()),
                Page::Resign(ResignPage {
                    account: accounts()[1],
                    form: typed(),
                }),
            ]
        };
        for width in [NARROWEST, 390.0, 600.0, 667.0, 780.0, 844.0, 1023.0] {
            for page in pages() {
                let model = with_scanner(phone_at(page.clone(), width), ScanPreview::Shell);
                let pastes = pastes_at(&model, width);
                let scans = scans_at(&model, width);
                assert_eq!(scans.len(), 2, "at {width} wide");
                assert_eq!(pastes.len(), 2, "at {width} wide");
                let field = focusable_bounds_at(screens::view(&model), width, 844.0)[0];
                for (paste, scan) in pastes.iter().zip(&scans) {
                    if width < crate::app::PHONE_TABLE {
                        // In a block the Paste stands beside the field, and the Scan on
                        // a line of its own beneath it. The Scan's x is not compared.
                        assert!(
                            scan.y > paste.y + 20.0,
                            "the Scan stands beneath the Paste in a block at {width} \
                             wide: {scan:?}, {paste:?}"
                        );
                    } else {
                        // In a table row both stand beneath the field, on one
                        // line, so the field takes the cell's width.
                        assert!(
                            paste.y > field.y + field.height
                                && (scan.y - paste.y).abs() < 1.0
                                && scan.x > paste.x,
                            "the Paste and the Scan stand beneath the field at {width} \
                             wide: {paste:?}, {scan:?}, {field:?}"
                        );
                    }
                }
                assert_eq!(
                    fits(screens::view(&model), width),
                    Ok(()),
                    "at {width} wide"
                );
                let bare = phone_at(page, width);
                assert!(scans_at(&bare, width).is_empty(), "no scanner, no Scan");
                assert_eq!(pastes_at(&bare, width).len(), 2, "Paste stays without one");
            }
        }
    }

    /// Asserts that the labels `first` and `second` stand on one line, `second`
    /// to the right of `first`.
    fn assert_side_by_side(drawn: &[(String, Rectangle)], first: &str, second: &str) {
        let (a, b) = (at(drawn, first), at(drawn, second));
        assert!(
            (a.y - b.y).abs() < 1.0 && b.x > a.x,
            "{first} and {second} stand side by side: {a:?}, {b:?}"
        );
    }

    #[test]
    fn a_phone_held_sideways_draws_its_destinations_as_table_rows_with_paste_and_scan_beneath() {
        // From 844 wide, a phone held sideways draws its destinations as the
        // desktop's table rows, and its fee and block-to-live side by side. Its
        // Paste and Scan buttons both stand beneath each destination's field, on
        // one line, so that the field takes the cell's width. Its pages stay
        // pages.
        for (width, height) in [(844.0, 390.0), (1024.0, 600.0)] {
            let with_buttons = |page: Page| with_scanner(phone_at(page, width), ScanPreview::Shell);

            // Page 1: a table with its header, a Paste and a Scan for each row,
            // and no block. Both buttons stand beneath the field, the Paste
            // beside the Scan, and the field takes the cell's width: at least
            // 250 pixels.
            let to = with_buttons(stage_page(SendStage::To(BTreeMap::new()), typed()));
            let drawn = texts(screens::view(&to), (width, height));
            for head in ["#", "Destination", "Memo"] {
                assert!(has(&drawn, head), "{head} is a header at {width} wide");
            }
            assert!(!has(&drawn, "Destination 1"), "a table at {width} wide");
            assert_eq!(pastes_at(&to, width).len(), 2, "at {width} wide");
            assert_eq!(scans_at(&to, width).len(), 2, "at {width} wide");
            assert_eq!(fits_at(screens::view(&to), width, height), Ok(()));
            let field = focusable_bounds_at(screens::view(&to), width, height)[0];
            let (paste, scan) = (at(&drawn, "Paste"), at(&drawn, "Scan"));
            assert!(
                paste.y > field.y + field.height
                    && (scan.y - paste.y).abs() < 1.0
                    && paste.x < scan.x,
                "the Paste and the Scan stand beneath the field, on one line: \
                 {paste:?}, {scan:?}, {field:?}"
            );
            assert!(
                field.width >= 250.0,
                "the To page's field at {width} by {height} is {field:?}"
            );

            // Page 2: the amounts as rows, with no fee or block-to-live to give.
            let amounts = phone_at(stage_page(SendStage::Amount, typed()), width);
            let drawn = texts(screens::view(&amounts), (width, height));
            for head in ["#", "Destination", "Amount (MCM)"] {
                assert!(has(&drawn, head), "{head} is a header at {width} wide");
            }
            assert!(!has(&drawn, "Destination 1"), "a table at {width} wide");
            assert!(!has(&drawn, "Fee, nanoMCM in total") && !has(&drawn, "Block-to-live"));
            assert_eq!(fits_at(screens::view(&amounts), width, height), Ok(()));

            // The re-sign form: the same table with its Paste and Scan, and its fees
            // side by side.
            let resign = with_buttons(Page::Resign(ResignPage {
                account: accounts()[1],
                form: typed(),
            }));
            let drawn = texts(screens::view(&resign), (width, height));
            for head in ["#", "Destination", "Amount (MCM)", "Memo"] {
                assert!(has(&drawn, head), "{head} is a header at {width} wide");
            }
            assert!(!has(&drawn, "Destination 1"), "a table at {width} wide");
            assert_eq!(pastes_at(&resign, width).len(), 2, "at {width} wide");
            assert_eq!(scans_at(&resign, width).len(), 2, "at {width} wide");
            assert_side_by_side(&drawn, "Fee, nanoMCM in total", "Block-to-live");
            assert_eq!(fits_at(screens::view(&resign), width, height), Ok(()));
            // The re-sign page's destination field, with both buttons beneath
            // it: at least 150 pixels.
            let field = focusable_bounds_at(screens::view(&resign), width, height)[0];
            let (paste, scan) = (at(&drawn, "Paste"), at(&drawn, "Scan"));
            assert!(
                paste.y > field.y + field.height && (scan.y - paste.y).abs() < 1.0,
                "the send form's Paste and Scan stand beneath its field: {paste:?}, {scan:?}, {field:?}"
            );
            assert!(
                field.width >= 150.0,
                "the send form's destination field at {width} by {height} is {field:?}"
            );
        }
    }

    #[test]
    fn a_phone_narrower_than_844_held_sideways_keeps_its_blocks() {
        // A phone held sideways narrower than the table's 844 draws its
        // destinations as the blocks it draws upright, with its fees one over
        // the other: an iPhone SE at 667 by 375, and a phone at 780 by 360. Its
        // Paste stands beside each field and its Scan beneath it.
        for (width, height) in [(667.0, 375.0), (780.0, 360.0)] {
            let with_buttons = |page: Page| with_scanner(phone_at(page, width), ScanPreview::Shell);

            let to = with_buttons(stage_page(SendStage::To(BTreeMap::new()), typed()));
            let drawn = texts(screens::view(&to), (width, height));
            assert!(has(&drawn, "Destination 1"), "a block at {width} wide");
            assert!(!has(&drawn, "#"), "no table at {width} wide");
            assert_eq!(pastes_at(&to, width).len(), 2, "at {width} wide");
            assert_eq!(scans_at(&to, width).len(), 2, "at {width} wide");
            assert_eq!(fits_at(screens::view(&to), width, height), Ok(()));
            let field = focusable_bounds_at(screens::view(&to), width, height)[0];
            let (paste, scan) = (at(&drawn, "Paste"), at(&drawn, "Scan"));
            assert!(
                paste.x > field.x + field.width && paste.y < field.y + field.height,
                "the Paste stands beside the field at {width} wide: {paste:?}, {field:?}"
            );
            assert!(
                scan.y > field.y + field.height && scan.x < paste.x,
                "the Scan stands beneath the field at {width} wide: {scan:?}, {field:?}"
            );

            let amounts = phone_at(stage_page(SendStage::Amount, typed()), width);
            let drawn = texts(screens::view(&amounts), (width, height));
            assert!(has(&drawn, "Destination 1"), "a block at {width} wide");
            assert!(!has(&drawn, "#"), "no table at {width} wide");
            assert!(has(&drawn, "Max") && !has(&drawn, "Fee, nanoMCM in total"));
            assert_eq!(fits_at(screens::view(&amounts), width, height), Ok(()));

            let resign = with_buttons(Page::Resign(ResignPage {
                account: accounts()[1],
                form: typed(),
            }));
            let drawn = texts(screens::view(&resign), (width, height));
            assert!(has(&drawn, "Destination 1"), "a block at {width} wide");
            assert!(!has(&drawn, "#"), "no table at {width} wide");
            let (fee, btl) = (
                at(&drawn, "Fee, nanoMCM in total"),
                at(&drawn, "Block-to-live"),
            );
            assert!(
                btl.y > fee.y + 1.0,
                "the send form's block-to-live is under its fee at {width} wide: {fee:?}, {btl:?}"
            );
            assert_eq!(fits_at(screens::view(&resign), width, height), Ok(()));
        }
    }

    #[test]
    fn a_phone_review_page_draws_its_destinations_as_blocks_at_every_width() {
        // Page 3 of a phone's Send is `review_page`, and it draws each
        // destination as a block at every width, sideways included: the table
        // is on page 1 and on the re-sign page alone, from 844 wide.
        for (width, height) in [
            (NARROWEST, 640.0),
            (667.0, 375.0),
            (780.0, 360.0),
            (844.0, 390.0),
            (1024.0, 600.0),
        ] {
            let model = phone_at(stage_page(SendStage::Review(plan()), typed()), width);
            let drawn = texts(screens::view(&model), (width, height));
            assert!(has(&drawn, "3 of 4"), "page 3 at {width} wide");
            assert!(has(&drawn, "#1"), "a block at {width} wide");
            assert!(!has(&drawn, "#"), "no table header at {width} wide");
            assert!(has(&drawn, "Continue to signing"), "page 3 at {width} wide");
            assert_eq!(fits_at(screens::view(&model), width, height), Ok(()));
        }
    }

    #[test]
    fn a_phone_held_upright_keeps_its_blocks_and_its_fees_one_over_the_other() {
        // A phone held upright draws blocks, with no table, and its fees one
        // over the other.
        for width in [NARROWEST, 390.0] {
            let to = drawn_on(&phone_at(
                stage_page(SendStage::To(BTreeMap::new()), typed()),
                width,
            ));
            assert!(has(&to, "Destination 1"), "a block at {width} wide");
            assert!(!has(&to, "#"), "no table at {width} wide");

            let amounts = drawn_on(&phone_at(stage_page(SendStage::Amount, typed()), width));
            assert!(has(&amounts, "Destination 1"), "a block at {width} wide");
            assert!(has(&amounts, "Max") && !has(&amounts, "Fee, nanoMCM in total"));

            // The re-sign page alone keeps the fee and the block-to-live, one
            // over the other.
            let resign = drawn_on(&phone_at(
                Page::Resign(ResignPage {
                    account: accounts()[1],
                    form: typed(),
                }),
                width,
            ));
            let (fee, btl) = (
                at(&resign, "Fee, nanoMCM in total"),
                at(&resign, "Block-to-live"),
            );
            assert!(
                btl.y > fee.y + 1.0,
                "the block-to-live is under the fee at {width} wide: {fee:?}, {btl:?}"
            );
        }
    }

    #[test]
    fn the_desktop_draws_its_table_with_its_fees_side_by_side_and_neither_paste_nor_scan() {
        // The desktop's send and re-sign forms are tables, with the fees side
        // by side, and no Paste or Scan, from 600 wide up: the desktop's table
        // is not the phone's, and starts at COMPACT.
        for width in [600.0, 700.0, 843.0, 1024.0, 1440.0] {
            for (page, exact) in [
                (compose(typed()), false),
                (
                    Page::Resign(ResignPage {
                        account: accounts()[1],
                        form: typed(),
                    }),
                    true,
                ),
            ] {
                let model = with_scanner(desktop_at(page, width), ScanPreview::Shell);
                let drawn = drawn_on(&model);
                for head in ["#", "Destination", "Amount (MCM)", "Memo"] {
                    assert!(has(&drawn, head), "{head} at {width} wide");
                }
                assert!(!has(&drawn, "Destination 1"), "a table at {width} wide");
                assert!(!has(&drawn, "Paste") && !has(&drawn, "Scan"));
                if exact {
                    assert_side_by_side(&drawn, "Fee, nanoMCM in total", "Block-to-live");
                } else {
                    assert!(!has(&drawn, "Fee, nanoMCM in total") && !has(&drawn, "Block-to-live"));
                }
            }
        }
    }

    #[test]
    fn the_signed_bytes_field_keeps_its_paste_and_stands_submit_beside_its_helper_sideways() {
        // The saved-artifact field follows the width as the destinations do:
        // Submit stands beside its helper in a wide window, and above it
        // upright; the Paste beside the field stays on a phone at any width.
        const HELPER: &str = "Only the bytes' layout is checked here; the network validates the \
                              rest.";
        let page = || {
            Page::Submit(SubmitPage {
                hex: sample::sent_view().artifact_hex,
                result: None,
                ..SubmitPage::default()
            })
        };
        for (width, height) in [
            (NARROWEST, 844.0),
            (390.0, 844.0),
            (844.0, 390.0),
            (1024.0, 600.0),
        ] {
            let model = phone_at(page(), width);
            let drawn = texts(screens::view(&model), (width, height));
            let (submit, helper) = (at(&drawn, "Submit"), at(&drawn, HELPER));
            assert_eq!(pastes_at(&model, width).len(), 1, "at {width} wide");
            assert_eq!(
                helper.y < submit.y + submit.height,
                width >= crate::app::COMPACT,
                "Submit beside its helper only in a wide window, at {width} wide: \
                 {submit:?}, {helper:?}"
            );
            assert_eq!(fits_at(screens::view(&model), width, height), Ok(()));
        }
    }

    #[test]
    fn the_desktop_draws_neither_paste_nor_scan_even_where_a_scanner_is_given() {
        // Only a phone's shell gives a scanner, and the desktop's pages draw
        // neither button.
        for width in [1024.0, 1440.0] {
            for page in [
                compose(typed()),
                Page::Resign(ResignPage {
                    account: accounts()[1],
                    form: typed(),
                }),
            ] {
                let model = with_scanner(desktop_at(page, width), ScanPreview::Shell);
                let drawn = drawn_on(&model);
                assert!(!has(&drawn, "Paste"), "{drawn:?}");
                assert!(!has(&drawn, "Scan"), "{drawn:?}");
            }
        }
    }

    #[test]
    fn a_tap_on_a_scan_button_asks_for_its_own_row_and_none_is_asked_while_a_scan_is_under_way() {
        let idle = with_scanner(
            phone_at(
                stage_page(SendStage::To(BTreeMap::new()), typed()),
                NARROWEST,
            ),
            ScanPreview::Shell,
        );
        let second = scans_at(&idle, NARROWEST)[1];
        let mut driver = Driver::new(screens::view(&idle), (360, 844));
        let (messages, _, _) = driver.send(&[
            mouse_move(second.center_x(), second.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(messages.as_slice(), [Message::Scan(1)]),
            "{messages:?}"
        );

        // A scan under way disables every Scan button: a second scan is not begun.
        let under_way = scanning(
            with_scanner(
                phone_at(
                    stage_page(SendStage::To(BTreeMap::new()), typed()),
                    NARROWEST,
                ),
                ScanPreview::Shell,
            ),
            0,
        );
        let second = scans_at(&under_way, NARROWEST)[1];
        let mut driver = Driver::new(screens::view(&under_way), (360, 844));
        let (messages, _, _) = driver.send(&[
            mouse_move(second.center_x(), second.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn the_scan_page_draws_its_frame_a_line_and_cancel_and_fits_a_phone() {
        // The camera's preview is drawn in the application: the page shows the
        // latest frame, what to point at, and Cancel, whole at 360 by 640 and at
        // 390 by 844.
        for (width, height) in PHONES {
            let mut model = scanning(
                with_scanner(
                    phone_at(stage_page(SendStage::To(BTreeMap::new()), typed()), width),
                    ScanPreview::Application,
                ),
                0,
            );
            // The phone's status bar and home indicator cover the window's edges,
            // so the page is drawn between them, as the phone draws it.
            model.safe_area = SafeArea::new(PHONE_INSETS);
            model.scan_preview = Some(iced::widget::image::Handle::from_rgba(
                4,
                3,
                vec![0x40; 4 * 3 * 4],
            ));
            let problems = crate::harness::fits_the_viewport(
                screens::view(&model),
                (width, height),
                PHONE_INSETS,
            );
            assert!(problems.is_empty(), "at {width} by {height}: {problems:?}");
            assert!(
                past_the_edge(screens::view(&model), width, height).is_empty(),
                "at {width} by {height}"
            );
            let drawn = texts(screens::view(&model), (width, height));
            assert!(has(&drawn, "Cancel"), "{drawn:?}");
            assert!(
                has(
                    &drawn,
                    "Point the camera at a QR code with a destination in it."
                ),
                "{drawn:?}"
            );
            assert!(!has(&drawn, "Paste") && !has(&drawn, "Scan"), "{drawn:?}");
        }
    }

    #[test]
    fn a_tap_on_cancel_on_the_scan_page_asks_to_stop_the_scan() {
        let model = scanning(
            with_scanner(
                phone_at(
                    stage_page(SendStage::To(BTreeMap::new()), typed()),
                    NARROWEST,
                ),
                ScanPreview::Application,
            ),
            0,
        );
        let cancel = texts(screens::view(&model), (NARROWEST, 640.0))
            .into_iter()
            .find(|(text, _)| text == "Cancel")
            .map(|(_, at)| at)
            .expect("Cancel is drawn");
        let mut driver = Driver::new(screens::view(&model), (360, 640));
        let (messages, _, _) = driver.send(&[
            mouse_move(cancel.center_x(), cancel.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(messages.as_slice(), [Message::CancelScan]),
            "{messages:?}"
        );
    }

    #[test]
    fn a_tap_on_a_paste_button_asks_for_its_own_fields_paste() {
        let form = typed();
        let rows = form.rows.len();
        let model = phone_at(stage_page(SendStage::To(BTreeMap::new()), form), NARROWEST);
        let second = pastes(&model)[1];
        let mut driver = Driver::new(screens::view(&model), (360, 844));
        let (messages, _, _) = driver.send(&[
            mouse_move(second.center_x(), second.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Paste(PasteInto::Destination {
                    form: crate::app::FormOf::Send(Some(from)),
                    row: 1,
                    rows: r,
                })] if *from == accounts()[0] && *r == rows
            ),
            "{messages:?}"
        );

        let model = phone_at(Page::Submit(SubmitPage::default()), NARROWEST);
        let at = pastes(&model)[0];
        let mut driver = Driver::new(screens::view(&model), (360, 844));
        let (messages, _, _) = driver.send(&[
            mouse_move(at.center_x(), at.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Paste(PasteInto::SignedBytes)]
            ),
            "{messages:?}"
        );
    }

    /// The bounds of the children of `root` as the layout gives them at the
    /// phone's narrowest width, each relative to `root`. A row gives each child
    /// no more than the room left in it, so a child that does not fit is drawn
    /// narrower, not past the edge: `fits` cannot see that, and this can.
    fn child_bounds(mut root: Element<'_, Message>) -> Vec<Rectangle> {
        let renderer = crate::harness::renderer();
        let mut tree = Tree::new(root.as_widget());
        let limits = layout::Limits::new(iced::Size::ZERO, iced::Size::new(NARROWEST, 844.0));
        let node = root.as_widget_mut().layout(&mut tree, &renderer, &limits);
        node.children().iter().map(|child| child.bounds()).collect()
    }

    #[test]
    fn a_row_of_buttons_wraps_on_a_phone_and_stays_one_row_on_the_desktop() {
        // Two buttons 200 pixels across, which 360 cannot hold in one line: on a
        // phone the second goes onto a line of its own and keeps its width; the
        // desktop's row stays one row.
        let wide = || -> Element<'static, Message> {
            space()
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(40.0))
                .into()
        };
        let phone = child_bounds(button_row(true, vec![wide(), wide()]));
        assert!(
            phone[1].y > phone[0].y && phone.iter().all(|b| b.width == 200.0),
            "{phone:?}"
        );
        let desktop = child_bounds(button_row(false, vec![wide(), wide()]));
        assert_eq!(desktop[1].y, desktop[0].y, "{desktop:?}");
    }

    #[test]
    fn every_memo_example_the_send_page_gives_is_one_the_library_accepts() {
        for (i, memo) in MEMO_EXAMPLES.iter().enumerate() {
            assert!(MEMO_RULE.contains(&format!("'{memo}'")), "{memo}");
            assert!(
                tawara_wallet_core::spend::parse_reference(memo, i + 1).is_ok(),
                "{memo} is refused"
            );
        }
        assert_eq!(fee_line(), "Network fee is .0000005 MCM per destination.");
    }

    #[test]
    fn the_send_page_asks_for_no_fee_no_block_to_live_and_nothing_to_send_everything() {
        let mut desktop = desktop_at(compose(typed()), 1440.0);
        desktop.wallet = Some(two_can_spend());
        let mut phone_two = phone_at(stage_page(SendStage::Amount, typed()), NARROWEST);
        phone_two.wallet = Some(two_can_spend());
        for model in [&desktop, &phone_two] {
            let drawn = drawn_on(model);
            for gone in [
                "Fee, nanoMCM in total",
                "Block-to-live",
                "Reference",
                "Reserve",
                "Submit",
                "Settle",
                "Send everything: the whole balance less the fee, to this one destination.",
            ] {
                assert!(!has(&drawn, gone), "{gone:?} is drawn");
            }
            assert!(
                !drawn
                    .iter()
                    .any(|(text, _)| text.starts_with("Signs with one-time key")
                        || text.starts_with("Change to key")
                        || text.starts_with("Nothing is signed yet")),
                "{drawn:?}"
            );
            assert!(has(&drawn, "Change"), "{drawn:?}");
            assert!(has(&drawn, "Max"), "{drawn:?}");
            assert!(has(&drawn, &fee_line()), "{drawn:?}");
        }
        assert!(has(&drawn_on(&desktop), "Memo"));
        assert!(has(&drawn_on(&desktop), MEMO_RULE));
    }

    #[test]
    fn max_stands_on_the_left_of_each_amount_and_fills_it() {
        let mut model = desktop_at(compose(typed()), 1440.0);
        model.wallet = Some(two_can_spend());
        let drawn = drawn_on(&model);
        let max = drawn
            .iter()
            .filter(|(text, _)| text == "Max")
            .map(|(_, at)| *at)
            .collect::<Vec<_>>();
        assert_eq!(max.len(), typed().rows.len(), "one Max a destination");
        let root = screens::view(&model);
        let cell = crate::harness::bounds_of(root, (1440, 2400), &max_id(0)).unwrap();
        assert!(
            (max[0].x - cell.x) < 24.0,
            "Max stands at the cell's left: {max:?} {cell:?}"
        );

        let mut driver = Driver::new(screens::view(&model), (1440, 2400));
        let (messages, _, _) = driver.send(&[
            mouse_move(max[0].center_x(), max[0].center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(messages.as_slice(), [Message::Wallet(WalletMsg::Max(0))]),
            "{messages:?}"
        );
    }
}
