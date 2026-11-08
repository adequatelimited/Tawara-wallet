// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The wallet: the shell (the sidebar, its network panel and the lock, or the
//! phone's bars) and its pages, one module each.

mod account;
mod activity;
mod add_account;
mod dashboard;
mod explorer;
mod licences;
mod receive;
mod recovery;
mod report;
mod send;
mod settings;
mod stake;

use iced::widget::{column, container, row, rule, scrollable, space};
use iced::{Alignment, Element, Length, Padding};
use tawara_wallet_core::view::{
    AccountId, AccountKind, AccountRow, AccountState, ReservationState,
};

use crate::app::{
    Back, Busy, Go, Message, Model, Page, ReportKey, SentPage, To, WalletMsg, WalletPage,
};
use crate::host::DeviceCheck;
use crate::icon::{self, Icon};
use crate::tap::tap;
use crate::theme::{self, color, space as sp};
use crate::ui::{self, Size, t, ty};

/// The sidebar's items, in order, and where each leads.
const NAV: [(Icon, &str, Option<To>); 7] = [
    (Icon::Wallet, "Wallet", Some(To::Dashboard)),
    (Icon::Send, "Send", Some(To::Send(None))),
    (Icon::Receive, "Receive", Some(To::Receive(None))),
    (Icon::Layers, "Stake", Some(To::Stake(None))),
    (Icon::Activity, "Activity", Some(To::Activity)),
    (Icon::Cube, "Explorer", Some(To::Explorer)),
    (Icon::Sliders, "Settings", Some(To::Settings)),
];

/// The wallet's page with the sidebar.
pub fn view<'a>(model: &'a Model, page: &'a WalletPage) -> Element<'a, Message> {
    let content: Element<'a, Message> = match &page.page {
        // A scan that draws its camera's preview in the application stands its
        // page in place of the page it began on, which is back once it ends.
        _ if model.previewing() => send::scanner(model, page),
        Page::Dashboard => dashboard::view(model, page),
        Page::Receive(r) => receive::view(model, page, r),
        Page::AddAccount(a) => add_account::view(model, page, a),
        Page::Send(s) => send::view(model, page, s),
        Page::Resign(r) => send::resign(model, page, r),
        Page::Submit(s) => send::submit(model, page, s),
        Page::Account(a) => account::view(model, page, a),
        Page::Activity(a) => activity::view(model, page, a),
        Page::Settings(s) => settings::view(model, page, s),
        Page::Licences => licences::view(model, page),
        Page::Recovery(r) => recovery::view(model, page, r),
        Page::Explorer(e) => explorer::overview(model, page, e),
        Page::Block(b) => explorer::block(model, page, b),
        Page::Tag(t) => explorer::tag(model, page, t),
        Page::Transaction(t) => explorer::transaction(model, page, t),
        Page::Queue(q) => explorer::queue_page(model, page, q),
        Page::Stake(s) => stake::view(model, page, s),
    };
    if model.compact() {
        return compact_frame(model, page, content);
    }
    row![
        sidebar(model, page),
        rule::vertical(1).style(theme::divider),
        scrollable(
            container(content)
                .max_width(1240.0)
                .padding(Padding {
                    top: sp::S32,
                    right: sp::S40,
                    bottom: sp::S48,
                    left: sp::S40,
                })
                .center_x(Length::Fill),
        )
        // The page has the id the phone's page has, so that entering a review
        // scrolls it to its top on the desktop too.
        .id(super::page_id())
        .style(theme::scroll)
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}

/// The sidebar: the wordmark, the items, and the
/// network panel with the lock at its foot. While the page waits on the
/// worker, no item leads anywhere.
fn sidebar<'a>(model: &'a Model, page: &'a WalletPage) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let here = page.page.nav();
    let mut nav = column![].spacing(sp::S4);
    for (i, (glyph, name, to)) in NAV.into_iter().enumerate() {
        let active = i == here;
        let ink = if active {
            color::ACCENT
        } else if to.is_some() {
            color::TEXT_SECONDARY
        } else {
            color::TEXT_MUTED
        };
        nav = nav.push(
            iced::widget::button(
                container(
                    row![
                        icon::icon(glyph, 20.0, 2.0, ink),
                        ui::label(name, if active { ty::NAV_ACTIVE } else { ty::NAV }),
                    ]
                    .spacing(sp::S12)
                    .align_y(Alignment::Center),
                )
                .center_y(Length::Fill),
            )
            .height(Length::Fixed(44.0))
            .width(Length::Fill)
            .padding(Padding::from([0.0, sp::S12]))
            .style(theme::button(theme::Button::Nav { active }))
            .on_press_maybe(to.filter(|_| idle).map(|to| WalletMsg::Open(to).into())),
        );
    }
    container(
        column![
            container(t("TAWARA", ty::WORDMARK, color::TEXT_PRIMARY))
                .padding(Padding::from([0.0, sp::S10])),
            nav,
            space().height(Length::Fill),
            network_panel(model),
        ]
        .spacing(sp::S32)
        .height(Length::Fill),
    )
    .padding(Padding::from([sp::S28, sp::S16]))
    .width(Length::Fixed(232.0))
    .height(Length::Fill)
    .style(theme::sidebar)
    .into()
}

/// The network panel: which node, its tip, how long it took to
/// answer, a way to change it, and the lock.
fn network_panel(model: &Model) -> Element<'_, Message> {
    let host = model
        .node
        .url
        .as_deref()
        .map(|u| {
            u.trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/')
        })
        .unwrap_or("No API chosen");
    let (dot, block, latency) = match (&model.node.tip, &model.node.error) {
        (Some((tip, took)), None) => (
            color::ACCENT,
            ui::group(&tip.to_string()),
            format!("{} ms", took.as_millis()),
        ),
        (_, Some(_)) => (color::WARNING, "—".to_owned(), "not answering".to_owned()),
        _ => (color::TEXT_MUTED, "—".to_owned(), "—".to_owned()),
    };
    let line = |name: &'static str, value: String| {
        row![
            t(name, ty::NOTE, color::TEXT_SECONDARY),
            space().width(Length::Fill),
            t(value, ty::MONO_SMALL, color::TEXT_PRIMARY),
        ]
        .align_y(Alignment::Center)
    };
    let change: Element<'_, Message> = if model.busy.is_none() {
        ui::link(
            "Change API",
            ty::LINK_SMALL,
            Message::Go(Go::Node(Back::Wallet)),
        )
        .into()
    } else {
        t("Change API", ty::LINK_SMALL, color::TEXT_MUTED).into()
    };
    container(
        column![
            row![
                ui::dot(dot, 8.0),
                t(host, ty::TABLE_NAME, color::TEXT_PRIMARY).width(Length::Fill),
            ]
            .spacing(sp::S8)
            .align_y(Alignment::Center),
            line("Block", block),
            line("Latency", latency),
            change,
            container(ui::button_with(
                "Lock wallet",
                theme::Button::Secondary,
                Size::Small,
                Some(Icon::Lock),
                Some(Message::Lock),
            ))
            .center_x(Length::Fill)
            .padding(Padding {
                top: sp::S4,
                ..Padding::ZERO
            }),
        ]
        .spacing(sp::S8),
    )
    .padding(sp::S14)
    .width(Length::Fill)
    .style(theme::sidebar_panel)
    .into()
}

/// The phone's wallet: a bar at the top, the page between margins of 16, and,
/// while the keyboard is not up, a bar at the foot with the six items. The
/// page is a scrollable with [`super::page_id`], so the screenshot example
/// can scroll it to its end.
fn compact_frame<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    let scrolled = scrollable(
        container(content)
            .padding(Padding {
                top: sp::S16,
                right: sp::S16,
                bottom: sp::S24,
                left: sp::S16,
            })
            .width(Length::Fill),
    )
    .id(super::page_id())
    .style(theme::scroll)
    .width(Length::Fill)
    .height(Length::Fill);
    let mut shell = column![
        top_bar(model),
        rule::horizontal(1).style(theme::divider),
        container(scrolled)
            .style(theme::page)
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .height(Length::Fill);
    // The keyboard takes the foot of the window: the bar is not drawn under it.
    if model.safe_area.keyboard <= 0.0 {
        shell = shell
            .push(rule::horizontal(1).style(theme::divider))
            .push(tab_bar(model, page));
    }
    shell.into()
}

/// The phone's top bar: the wordmark, the node chip, and Lock.
fn top_bar(model: &Model) -> Element<'_, Message> {
    container(
        row![
            t("TAWARA", ty::WORDMARK, color::TEXT_PRIMARY),
            space().width(Length::Fill),
            node_chip(model),
            ui::button_with(
                "Lock",
                theme::Button::Secondary,
                Size::Small,
                Some(Icon::Lock),
                Some(Message::Lock),
            ),
        ]
        .spacing(sp::S8)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0.0, sp::S16]))
    .width(Length::Fill)
    .center_y(Length::Fixed(56.0))
    .style(theme::sidebar)
    .into()
}

/// The node chip: the node's dot and its
/// chain tip, grouped, or a dash. It goes to the node screen while the wallet
/// is idle, as the sidebar's "Change API" does.
fn node_chip(model: &Model) -> Element<'_, Message> {
    let (dot, tip) = match (&model.node.tip, &model.node.error) {
        (Some((tip, _)), None) => (color::ACCENT, ui::group(&tip.to_string())),
        (_, Some(_)) => (color::WARNING, "—".to_owned()),
        _ => (color::TEXT_MUTED, "—".to_owned()),
    };
    iced::widget::button(
        row![
            ui::dot(dot, 8.0),
            t(tip, ty::MONO_SMALL, color::TEXT_PRIMARY)
        ]
        .spacing(sp::S8)
        .align_y(Alignment::Center),
    )
    .height(Length::Fixed(36.0))
    .padding(Padding::from([0.0, sp::S8]))
    .style(theme::button(theme::Button::Ghost))
    .on_press_maybe(
        model
            .busy
            .is_none()
            .then_some(Message::Go(Go::Node(Back::Wallet))),
    )
    .into()
}

/// The phone's bar at the foot: the six
/// items of the sidebar, each an icon over its name, sharing the width. They
/// lead where the sidebar's do, and only while the wallet is idle.
fn tab_bar<'a>(model: &'a Model, page: &'a WalletPage) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let here = page.page.nav();
    let mut items = row![].width(Length::Fill).height(Length::Fill);
    for (i, (glyph, name, to)) in NAV.into_iter().enumerate() {
        let active = i == here;
        let ink = if active {
            color::ACCENT
        } else if to.is_some() {
            color::TEXT_SECONDARY
        } else {
            color::TEXT_MUTED
        };
        let label_type = if active {
            ty::NAV_TAB_ACTIVE
        } else {
            ty::NAV_TAB
        };
        items = items.push(
            iced::widget::button(
                container(
                    column![
                        icon::icon(glyph, 20.0, 2.0, ink),
                        ui::label(name, label_type),
                    ]
                    .spacing(sp::S4)
                    .align_x(Alignment::Center),
                )
                .center(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::ZERO)
            .style(theme::button(theme::Button::Nav { active }))
            .on_press_maybe(to.filter(|_| idle).map(|to| WalletMsg::Open(to).into())),
        );
    }
    container(items)
        .width(Length::Fill)
        .height(Length::Fixed(56.0))
        .style(theme::sidebar)
        .into()
}

/// A page: its title and subtitle with the page's own controls beside
/// them, what the worker is doing for it while it waits, the refusal its
/// last command met, then the page.
fn frame<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    title: &str,
    subtitle: impl iced::widget::text::IntoFragment<'a>,
    actions: Vec<Element<'a, Message>>,
    body: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    framed(model, page, title, subtitle, actions, body, true)
}

/// [`frame`] for a page whose buttons draw what their own command met, beside
/// them, with [`beside`]: the busy card and the refusal are not drawn at the
/// top of it.
fn frame_beside<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    title: &str,
    subtitle: impl iced::widget::text::IntoFragment<'a>,
    actions: Vec<Element<'a, Message>>,
    body: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    framed(model, page, title, subtitle, actions, body, false)
}

/// [`frame`], or [`frame_beside`] when `top` is false: whether the busy card
/// and the refusal are drawn under the title.
fn framed<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    title: &str,
    subtitle: impl iced::widget::text::IntoFragment<'a>,
    actions: Vec<Element<'a, Message>>,
    body: Vec<Element<'a, Message>>,
    top: bool,
) -> Element<'a, Message> {
    let compact = model.compact();
    // The title takes what the controls leave, its subtitle wrapping, so the
    // controls keep their size at the smallest window. On a phone the title
    // has the width, and the controls wrap beneath it.
    let header: Element<'a, Message> = if compact {
        let mut head = column![ui::page_header(title, subtitle)];
        if !actions.is_empty() {
            head = head.push(
                iced::widget::Row::with_children(actions)
                    .spacing(sp::S8)
                    .wrap()
                    .vertical_spacing(sp::S8),
            );
        }
        head.spacing(sp::S16).into()
    } else {
        let mut header = row![container(ui::page_header(title, subtitle)).width(Length::Fill)]
            .spacing(sp::S10)
            .align_y(Alignment::Center);
        for action in actions {
            header = header.push(action);
        }
        header.into()
    };
    let mut stack = column![header].spacing(if compact { sp::S16 } else { sp::S24 });
    if let Some(busy) = model.busy.as_ref().filter(|_| top) {
        stack = stack.push(working(compact, busy));
    }
    // The offer of the device unlock stands until it is answered or the store
    // locks, and what the device unlock last found stands under it.
    if let (true, Some(device)) = (model.device_offer, &model.device) {
        stack = stack.push(device_offer(device.check()));
    }
    if let Some(note) = &model.device_note {
        stack = stack.push(ui::refusal(note.as_str()));
    }
    if let Some(e) = page.error.as_deref().filter(|_| top) {
        stack = stack.push(refusal_of(compact, page, e));
    }
    for part in body {
        stack = stack.push(part);
    }
    stack.into()
}

/// The offer of the device unlock, made once after a password unlock opened
/// the store: "Turn on" keeps the password on this phone, and "Not now" keeps
/// nothing.
fn device_offer<'a>(check: DeviceCheck) -> Element<'a, Message> {
    let name = check.name();
    ui::accent_callout_with_actions(
        Icon::Lock,
        format!("Unlock with {name} next time?"),
        format!(
            "Keep this wallet's password on this phone, to unlock the wallet with {name} \
             instead of typing it. Anyone who knows this phone's passcode can open the \
             wallet then. Turning the unlock off in Settings deletes the password."
        ),
        row![
            ui::button_with(
                "Turn on",
                theme::Button::Primary,
                Size::Medium,
                None,
                Some(Message::DeviceOffer(true)),
            ),
            ui::button_with(
                "Not now",
                theme::Button::Secondary,
                Size::Medium,
                None,
                Some(Message::DeviceOffer(false)),
            ),
        ]
        .spacing(sp::S10),
    )
}

/// What a page draws beside the button whose command it is for: the busy card
/// while the worker answers, then the refusal that command met, each when
/// there is one. Empty when neither holds.
fn beside<'a>(model: &'a Model, page: &'a WalletPage) -> Vec<Element<'a, Message>> {
    let compact = model.compact();
    let mut out = Vec::new();
    if let Some(busy) = &model.busy {
        out.push(working(compact, busy));
    }
    if let Some(e) = page.error.as_deref() {
        out.push(refusal_of(compact, page, e));
    }
    out
}

/// A refusal in the library's words as a report when it is long, or as a
/// refusal card when it reads as a line.
fn refusal_of<'a>(compact: bool, page: &'a WalletPage, e: &'a str) -> Element<'a, Message> {
    match report::refused(e) {
        Some(r) => report::show_in(compact, page, ReportKey::Refused, r),
        None => ui::refusal(e),
    }
}

/// The stopped screen's spends signed this run (see [`Model::signed`]):
/// with the worker gone nothing can be unlocked, so each is offered to
/// save or copy here. `None` when there are none.
pub fn kept_after_stop(model: &Model) -> Option<Element<'_, Message>> {
    if model.signed.is_empty() {
        return None;
    }
    let compact = model.compact();
    let mut list = column![
        ui::section_label("Spends signed in this run"),
        t(
            "Until each is saved, its signed bytes are kept, and they go when Tawara closes: \
             save them first.",
            ty::BODY_SMALL,
            color::TEXT_SECONDARY,
        ),
    ]
    .spacing(sp::S12);
    for kept in &model.signed {
        let sent = &kept.sent.sent;
        let what = match (&sent.tx_id, sent.submitted) {
            (Some(id), _) => format!("Transaction id {}", short(id)),
            (None, true) => "Sent to the API".to_owned(),
            (None, false) => "Not sent to the API".to_owned(),
        };
        let copied = kept.copied;
        let details = column![
            t(name(sent.from), ty::ROW_TITLE, color::TEXT_PRIMARY),
            t(what, ty::MONO_SMALL, color::TEXT_MUTED),
        ]
        .spacing(sp::S2);
        let save = ui::button_with(
            "Save artifact",
            theme::Button::Primary,
            Size::Small,
            Some(Icon::Download),
            Some(WalletMsg::SaveSigned(sent.artifact_hex.clone()).into()),
        );
        let copy = ui::button_with(
            if copied { "Copied" } else { "Copy hex" },
            theme::Button::Secondary,
            Size::Small,
            Some(if copied { Icon::Check } else { Icon::Copy }),
            Some(WalletMsg::Copy(sent.artifact_hex.clone()).into()),
        );
        // On a phone the buttons wrap beneath the details.
        let line: Element<'_, Message> = if compact {
            column![
                details,
                row![save, copy]
                    .spacing(sp::S10)
                    .wrap()
                    .vertical_spacing(sp::S8),
            ]
            .spacing(sp::S8)
            .into()
        } else {
            row![details.width(Length::Fill), save, copy]
                .spacing(sp::S10)
                .align_y(Alignment::Center)
                .into()
        };
        let mut item = column![line].spacing(sp::S8);
        if let Some(note) = saved_note(&kept.sent) {
            item = item.push(note);
        }
        list = list.push(ui::divider()).push(item);
    }
    Some(ui::card_for(compact, list).into())
}

/// What became of Save artifact for a signed spend's page: where the bytes
/// were written, why they were not, or that the share sheet was asked for.
/// The sheet's outcome is not known, so the spend is still not saved, but
/// nothing was refused, so it is a note in the page's own colours rather than
/// a refusal.
fn saved_note<'a>(page: &'a SentPage) -> Option<Element<'a, Message>> {
    match &page.saved {
        Some(Ok(path)) => Some(
            t(
                format!("Saved to {}", path.display()),
                ty::MONO_SMALL,
                color::ACCENT,
            )
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
            .into(),
        ),
        Some(Err(e)) if page.shared() => Some(t(e, ty::BODY_SMALL, color::TEXT_SECONDARY).into()),
        Some(Err(e)) => Some(ui::refusal(e)),
        None => None,
    }
}

/// What the worker is doing for the page, in the opening screen's words, how
/// far it has got, and its Cancel. Until it answers, the page's controls wait.
fn working(compact: bool, busy: &Busy) -> Element<'_, Message> {
    let (title, what) = super::activity(busy);
    let mut text = column![
        t(title, ty::CARD_TITLE, color::TEXT_PRIMARY),
        t(what, ty::BODY_SMALL, color::TEXT_SECONDARY),
    ]
    .spacing(sp::S6);
    if let Some(detail) = super::progress(busy) {
        text = text.push(detail);
    }
    let cancel = ui::button_with(
        "Cancel",
        theme::Button::Secondary,
        Size::Medium,
        None,
        Some(Message::Cancel),
    );
    // On a phone Cancel sits beneath the progress, not beside it.
    let inside: Element<'_, Message> = if compact {
        column![text.width(Length::Fill), cancel]
            .spacing(sp::S16)
            .into()
    } else {
        row![text.width(Length::Fill), cancel]
            .spacing(sp::S24)
            .align_y(Alignment::Center)
            .into()
    };
    ui::card_for(compact, inside).into()
}

/// Two cards side by side in a wide window, `left` parts of the width to
/// `right`; stacked below [`crate::app::WIDE`].
fn pair<'a>(
    model: &Model,
    first: Element<'a, Message>,
    second: Element<'a, Message>,
    left: u16,
    right: u16,
) -> Element<'a, Message> {
    if model.width >= crate::app::WIDE {
        row![
            container(first).width(Length::FillPortion(left)),
            container(second).width(Length::FillPortion(right)),
        ]
        .spacing(sp::S24)
        .into()
    } else {
        column![first, second]
            .spacing(if model.compact() { sp::S16 } else { sp::S24 })
            .into()
    }
}

/// A button the page enables only while it waits on nothing.
fn action<'a>(
    model: &Model,
    content: &'a str,
    variant: theme::Button,
    size: Size,
    lead: Option<Icon>,
    on_press: impl Into<Message>,
) -> Element<'a, Message> {
    ui::button_with(
        content,
        variant,
        size,
        lead,
        model.busy.is_none().then(|| on_press.into()),
    )
    .into()
}

/// A button that puts `text` on the clipboard: `label`, or "Copied" once
/// `text` is what the page last copied.
fn copy<'a>(page: &WalletPage, text: &str, label: &'a str) -> Element<'a, Message> {
    let copied = page.copied.as_deref() == Some(text);
    ui::button_with(
        if copied { "Copied" } else { label },
        theme::Button::Secondary,
        Size::Small,
        Some(if copied { Icon::Check } else { Icon::Copy }),
        Some(WalletMsg::Copy(text.to_owned()).into()),
    )
    .into()
}

/// IBM Plex Mono's advance, which every glyph of it shares: 600 units of its
/// 1,000 to the em (`fonts::MONO`, `fonts::MONO_MEDIUM`).
const MONO_ADVANCE: f32 = 0.6;

/// The room a Copy button is given beside a destination: wider than the
/// button as drawn, "Copy" (87 logical pixels) or "Copied", so that a
/// destination set to its own width never pushes it past the edge.
const COPY_ROOM: f32 = 104.0;

/// A destination `shown` in `face`, a face of IBM Plex Mono, with its Copy
/// button straight after it. Where the room allows, the destination is set
/// to its own width and the button follows it; otherwise the destination
/// takes what the button leaves and breaks between characters. A row cannot
/// do this alone, since it gives a filling child its whole share, so the room
/// is read with `responsive`. The button's container has `id`.
fn destination_with_copy<'a>(
    page: &'a WalletPage,
    shown: String,
    face: ui::Type,
    ink: iced::Color,
    id: iced::widget::Id,
) -> Element<'a, Message> {
    let natural = (shown.chars().count() as f32 * face.size * MONO_ADVANCE).ceil() + 1.0;
    iced::widget::responsive(move |room| {
        let fits = room.width >= natural + sp::S8 + COPY_ROOM;
        let address = container(
            t(shown.clone(), face, ink).wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        )
        .width(if fits { Length::Shrink } else { Length::Fill });
        row![
            address,
            container(copy(page, &shown, "Copy")).id(id.clone())
        ]
        .spacing(sp::S8)
        .align_y(Alignment::Center)
        .into()
    })
    .height(Length::Shrink)
    .into()
}

/// The account's destination, or its tag in hex when the library cannot
/// render one (it documents that as unreachable).
fn destination(id: AccountId) -> String {
    id.destination().unwrap_or_else(|| id.hex())
}

/// An account's name: its destination's first and last six characters.
fn name(id: AccountId) -> String {
    short(&destination(id))
}

/// A destination shortened to its first and last six characters
/// ("9xQmT4…3cYdAf").
fn short(destination: &str) -> String {
    let chars: Vec<char> = destination.chars().collect();
    if chars.len() <= 14 {
        return destination.to_owned();
    }
    let head: String = chars[..6].iter().collect();
    let tail: String = chars[chars.len() - 6..].iter().collect();
    format!("{head}…{tail}")
}

/// Where an account's keys come from, set against the other kind:
/// "Recovery phrase · account 3", or
/// "Recovery phrase" while its number is not known, against "Imported", an
/// account whose keys are not the recovery phrase's. The number is the
/// command line's name for a derived account (`restore --account N`).
fn kind(row: &AccountRow) -> String {
    match (row.kind, row.number) {
        (AccountKind::Derived, Some(n)) => format!("Recovery phrase · account {n}"),
        (AccountKind::Derived, None) => "Recovery phrase".to_owned(),
        (AccountKind::Imported, _) => "Imported".to_owned(),
    }
}

/// The dashboard's line for a wallet whose accounts are all new.
const NOTHING_RECEIVED: &str = "Your wallet has not yet received any coins.";
/// The balance card's line for the same wallet.
const NOTHING_YET: &str =
    "Once you've received coins into your account we'll be able to see the balance here.";
/// The account page's line for a new account.
const NEW_ACCOUNT: &str = "This account has not yet received any coins. Share its destination above, or use Receive, \
     to be paid.";

/// The balance an account is shown with: the node's figure, or nothing yet
/// for a new account (`AccountRow::is_new`), which is shown as zero rather
/// than as unknown.
fn shown_balance(row: &AccountRow) -> Option<u64> {
    row.state.balance().or(row.is_new().then_some(0))
}

/// What an account's balance reads as: [`CHECKING`] while a balance check
/// waits on the API for it, its
/// balance, with the unit's name when `unit_named`, or `unknown`.
fn balance_text(model: &Model, row: &AccountRow, unknown: &str, unit_named: bool) -> String {
    if model.balance_checking.contains(&row.id) {
        return CHECKING.to_owned();
    }
    let unit = model.prefs.unit;
    shown_balance(row).map_or_else(
        || unknown.to_owned(),
        |b| {
            if unit_named {
                format!("{} {}", ui::amount(b, unit), ui::unit_name(unit))
            } else {
                ui::amount(b, unit)
            }
        },
    )
}

/// What a balance reads while its check waits on the API, and on Receive
/// before its first answer.
pub(crate) const CHECKING: &str = "Checking…";

/// An account's state in a word or two, and its colour. The library's report
/// for a state that needs one is on the account's own page. A new account is
/// "New Account", not paused.
fn status(row: &AccountRow) -> (&'static str, iced::Color) {
    if row.is_new() {
        return ("New Account", color::TEXT_SECONDARY);
    }
    state_status(&row.state)
}

/// [`status`] for a state read without its row.
fn state_status(state: &AccountState) -> (&'static str, iced::Color) {
    match state {
        AccountState::InSync { .. } => ("Address confirmed", color::ACCENT),
        AccountState::SpendOutstanding {
            reservation: ReservationState::Dead { .. },
            ..
        } => ("Reservation cannot land", color::WARNING),
        AccountState::SpendOutstanding { .. } => ("Spend outstanding", color::WARNING),
        AccountState::SpendLanded { .. } => ("Landed: settle it", color::WARNING),
        // Its last spend, which a reorganisation reverted, is sent again from
        // the kept copy until it lands.
        state if crate::app::rolled_back(state) => ("Re-sending last spend", color::WARNING),
        AccountState::Diverged { .. } => ("No Coins Available", color::WARNING),
        AccountState::NotReconciled => ("Not yet confirmed", color::TEXT_MUTED),
    }
}

/// The account in the open store with this id.
fn row_of(model: &Model, id: AccountId) -> Option<&AccountRow> {
    model.wallet.as_ref()?.accounts.iter().find(|a| a.id == id)
}

/// A choice of account ("From account" on Send): one row each, with its name,
/// destination and balance, `selected` marked. The next key's index is not
/// shown: it is nothing a person chooses an account by. While the page waits
/// on the worker, the choice stands.
fn account_choice<'a>(
    model: &'a Model,
    ids: impl Iterator<Item = AccountId>,
    selected: Option<AccountId>,
    on_pick: impl Fn(AccountId) -> Message,
) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let mut list = column![].spacing(sp::S8);
    for id in ids {
        let Some(account) = row_of(model, id) else {
            continue;
        };
        let chosen = selected == Some(id);
        let balance = balance_text(model, account, "balance not known", true);
        let card = if model.compact() {
            // The phone: the name, the destination, then the balance.
            container(
                column![
                    t(name(id), ty::RADIO_TITLE, color::TEXT_PRIMARY),
                    t(destination(id), ty::MONO_TINY, color::TEXT_MUTED)
                        .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
                    t(balance, ty::TABLE_AMOUNT, color::TEXT_PRIMARY),
                ]
                .spacing(sp::S4),
            )
            .padding(iced::Padding::from([sp::S12, sp::S16]))
            .width(Length::Fill)
            .style(theme::radio_card(chosen))
        } else {
            container(
                row![
                    column![
                        t(name(id), ty::RADIO_TITLE, color::TEXT_PRIMARY),
                        t(destination(id), ty::MONO_TINY, color::TEXT_MUTED)
                            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
                    ]
                    .spacing(sp::S2)
                    .width(Length::Fill),
                    t(balance, ty::TABLE_AMOUNT, color::TEXT_PRIMARY),
                ]
                .spacing(sp::S12)
                .align_y(Alignment::Center),
            )
            .padding(iced::Padding::from([sp::S12, sp::S16]))
            .width(Length::Fill)
            .style(theme::radio_card(chosen))
        };
        // A row acts on release, so a drag through the list scrolls it.
        list = list.push(tap(card).on_tap_maybe((idle && !chosen).then(|| on_pick(id))));
    }
    list.into()
}

/// A labelled figure on one line, as the summary cards set them.
fn figure<'a>(
    name: impl iced::widget::text::IntoFragment<'a>,
    value: String,
) -> Element<'a, Message> {
    row![
        t(name, ty::BODY_SMALL, color::TEXT_SECONDARY).width(Length::Fill),
        t(value, ty::TABLE_AMOUNT, color::TEXT_PRIMARY),
    ]
    .spacing(sp::S12)
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{NARROWEST, Screen};
    use crate::harness::Driver;
    use tawara_wallet_core::sample;

    #[test]
    fn destinations_shorten_to_their_ends() {
        assert_eq!(short("9xQmT4vRbN2kWe7HpLsZ3cYdAf"), "9xQmT4…3cYdAf");
        assert_eq!(short("short"), "short");
    }

    #[test]
    fn the_phone_shell_fits_in_360_with_nothing_in_its_page() {
        // The bars and the page's margins at the narrowest width: nothing is
        // cut off, nothing scrolls sideways. The screens' own content has its
        // own checks.
        use crate::app::{NARROWEST, Page};
        use tawara_wallet_core::preferences::Preferences;
        let mut model = Model::new(Preferences::default(), None);
        model.width = NARROWEST;
        model.safe_area = iced::mobile::SafeArea::new(iced::Padding {
            top: 47.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        });
        model.wallet = Some(tawara_wallet_core::sample::wallet_view());
        let page = WalletPage {
            page: Page::Dashboard,
            ..WalletPage::default()
        };
        let shell = compact_frame(&model, &page, iced::widget::column![].into());
        assert_eq!(crate::harness::fits(shell, NARROWEST), Ok(()));
    }

    #[test]
    fn an_asked_for_share_sheet_is_a_note_and_a_refused_save_a_refusal() {
        // The sheet's outcome is not known, so the spend is not saved, but
        // nothing was refused. The note is drawn
        // as the page's own text; a save that failed keeps its refusal, here
        // the one Save artifact gives when the platform was not asked for the
        // sheet.
        use crate::app::SHARED;
        use crate::harness::Driver;
        const NOT_SHOWN: &str = "The bytes could not be shared: no window. Copy the hex \
                                 instead and keep it somewhere safe.";
        let page = |saved: &str| SentPage {
            sent: tawara_wallet_core::sample::sent_view(),
            order: 0,
            resigned: false,
            saved: Some(Err(saved.to_owned())),
            dismissing: false,
        };
        let size = (360, 200);
        let draw = |page: &SentPage| {
            let note: Element<'_, Message> = saved_note(page).expect("a note is drawn");
            Driver::new(note, size).draw()
        };
        let plain =
            Driver::<Message>::new(t(SHARED, ty::BODY_SMALL, color::TEXT_SECONDARY), size).draw();
        let refused = Driver::<Message>::new(ui::refusal(NOT_SHOWN), size).draw();
        assert!(
            draw(&page(SHARED)) == plain,
            "a share sheet asked for is a note"
        );
        assert!(
            draw(&page(NOT_SHOWN)) == refused,
            "a failed save is a refusal"
        );
    }

    /// The dashboard of a phone, as the wallet shows it once a password unlock
    /// opened the store: with the device unlock offered by its shell or not, and
    /// with the offer on the page or not.
    fn phone_wallet(device: bool, offer: bool, width: f32) -> Model {
        use tawara_wallet_core::preferences::Preferences;
        let mut model = Model::new(Preferences::default(), None);
        model.width = width;
        model.wallet = Some(sample::wallet_view());
        model.screen = Screen::Wallet(WalletPage {
            page: Page::Dashboard,
            ..WalletPage::default()
        });
        if device {
            model.device = Some(crate::harness::face_id_device());
        }
        model.device_offer = offer;
        model
    }

    /// The texts the wallet page draws on a phone of `width` by 844.
    fn wallet_texts(model: &Model, width: f32) -> Vec<String> {
        let page = match &model.screen {
            Screen::Wallet(page) => page,
            _ => panic!("the wallet's page"),
        };
        crate::harness::texts(view(model, page), (width, 844.0))
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    #[test]
    fn the_offer_stands_on_the_wallet_only_while_the_device_unlock_is_offered_and_being_made() {
        // The offer comes once a password unlock opened the store, and stays until
        // it is answered or the store locks.
        let offer = "Unlock with Face ID next time?";
        let on = wallet_texts(&phone_wallet(true, true, NARROWEST), NARROWEST);
        assert!(on.iter().any(|t| t == offer), "{on:?}");
        assert!(on.iter().any(|t| t == "Turn on"));
        assert!(on.iter().any(|t| t == "Not now"));
        let none = wallet_texts(&phone_wallet(true, false, NARROWEST), NARROWEST);
        assert!(!none.iter().any(|t| t == offer), "no offer is being made");
        let unshown = wallet_texts(&phone_wallet(false, true, NARROWEST), NARROWEST);
        assert!(!unshown.iter().any(|t| t == offer), "no shell offers it");
    }

    #[test]
    fn the_offer_fits_a_phone_of_360_by_640_and_390_by_844() {
        for (width, height) in [(NARROWEST, 640.0), (390.0, 844.0)] {
            let model = phone_wallet(true, true, width);
            let page = match &model.screen {
                Screen::Wallet(page) => page,
                _ => unreachable!(),
            };
            assert_eq!(
                crate::harness::fits_at(view(&model, page), width, height),
                Ok(()),
                "the wallet with the offer, at {width} by {height}"
            );
        }
    }

    #[test]
    fn a_tap_on_the_offers_buttons_answers_it_yes_and_no() {
        let model = phone_wallet(true, true, NARROWEST);
        let size = (NARROWEST, 844.0);
        let answer = |text: &str| {
            let page = match &model.screen {
                Screen::Wallet(page) => page,
                _ => unreachable!(),
            };
            let (_, bounds) = crate::harness::texts(view(&model, page), size)
                .into_iter()
                .find(|(drawn, _)| drawn == text)
                .unwrap_or_else(|| panic!("no {text:?}"));
            let page = match &model.screen {
                Screen::Wallet(page) => page,
                _ => unreachable!(),
            };
            let mut driver = Driver::new(view(&model, page), (360, 844));
            let (x, y) = (
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            );
            let (messages, _, _) = driver.send(&[
                crate::harness::mouse_move(x, y),
                crate::harness::mouse_press(),
                crate::harness::mouse_release(),
            ]);
            messages
        };
        assert!(
            answer("Turn on")
                .iter()
                .any(|m| matches!(m, Message::DeviceOffer(true))),
            "Turn on keeps the password"
        );
        assert!(
            answer("Not now")
                .iter()
                .any(|m| matches!(m, Message::DeviceOffer(false))),
            "Not now keeps nothing"
        );
    }

    #[test]
    fn what_the_device_unlock_last_found_stands_on_the_wallet_page() {
        let mut model = phone_wallet(true, false, NARROWEST);
        model.device_note = Some(
            "The password could not be kept on this device (the keychain refused).".to_owned(),
        );
        let texts = wallet_texts(&model, NARROWEST);
        assert!(
            texts
                .iter()
                .any(|t| t
                    == "The password could not be kept on this device (the keychain refused)."),
            "{texts:?}"
        );
    }
}
