// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Settings: the accounts and their one-time keys, the keystore, the node,
//! the display unit and this build's details.
//!
//! A paused account offers "Review", which opens account recovery; this page
//! never moves a key index itself. The recovery phrase is not shown again and
//! the keystore is not exported, since the phrase is shown once and a copy of
//! the store is a rollback that would reuse a one-time key.
//!
//! On a phone the accounts are stacked items rather than a table, each
//! control sits under its label, the units share the width, and the
//! auto-lock periods wrap onto a second line (see `auto_lock`).

use iced::widget::text::Wrapping;
use iced::widget::{Row, column, container, row, space, text_input};
use iced::{Alignment, Element, Length, Padding};
use tawara_wallet_core::preferences::{AmountUnit, DEFAULT_NODE, DeviceUnlock, IDLE_LOCK_MINUTES};
use tawara_wallet_core::view::{AccountRow, AccountState, DivergenceKind};

use super::{action, frame, kind, name, pair, status};
use crate::app::{HELD_BODY, HELD_TITLE, Message, Model, SettingsPage, To, WalletMsg, WalletPage};
use crate::host::Device;
use crate::icon::{self, Icon};
use crate::theme::{self, color, radius, space as sp};
use crate::ui::{self, Size, t, ty};

pub fn view<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    s: &'a SettingsPage,
) -> Element<'a, Message> {
    let mut body = Vec::new();
    if let Some(wallet) = &model.wallet {
        body.push(accounts(model, &wallet.accounts));
    }
    body.push(pair(model, keystore(model), node(model, s), 1, 1));
    body.push(pair(model, display(model), about(model), 1, 1));
    frame(
        model,
        page,
        "Settings",
        "Keystore, one-time keys and the API this wallet talks to",
        Vec::new(),
        body,
    )
}

/// The key index the chain shows for an account, when its state gives one:
/// the store's index when they agree, the signing key while a spend is
/// outstanding, the change key once it has landed, or where a divergence
/// found it.
pub(super) fn on_ledger(account: &AccountRow) -> Option<u32> {
    match &account.state {
        AccountState::InSync { .. } => Some(account.index),
        AccountState::SpendOutstanding { spent_index, .. } => Some(*spent_index),
        AccountState::SpendLanded { settled_index, .. } => Some(*settled_index),
        AccountState::Diverged {
            kind: DivergenceKind::Ahead { gap },
            ..
        } => account.index.checked_add(*gap),
        AccountState::Diverged {
            kind: DivergenceKind::Behind { gap },
            ..
        } => account.index.checked_sub(*gap),
        AccountState::Diverged { .. } | AccountState::NotReconciled => None,
    }
}

/// Accounts and one-time keys: each account's key on this device and on the
/// ledger, compared at every unlock and refresh.
fn accounts<'a>(model: &'a Model, rows: &'a [AccountRow]) -> Element<'a, Message> {
    let compact = model.compact();
    let list: Element<'a, Message> = if compact {
        stacked_accounts(model, rows).into()
    } else {
        table(model, rows).into()
    };
    let words = column![
        t(
            "Accounts & one-time keys",
            ty::CARD_TITLE,
            color::TEXT_PRIMARY
        ),
        t(
            "Each stored key index is compared with the ledger at every unlock and refresh.",
            ty::NOTE,
            color::TEXT_MUTED
        ),
    ]
    .spacing(sp::S4)
    .width(Length::Fill);
    let check = action(
        model,
        "Check again",
        theme::Button::Secondary,
        Size::Small,
        Some(Icon::Refresh),
        Message::Refresh,
    );
    // On a phone the note has the full width, and the button comes beneath it.
    let head: Element<'a, Message> = if compact {
        column![words, check].spacing(sp::S12).into()
    } else {
        row![words, check]
            .spacing(sp::S16)
            .align_y(Alignment::Center)
            .into()
    };
    ui::card_for(compact, column![head, list].spacing(sp::S12)).into()
}

/// The accounts as a table with its column heads, on the desktop.
fn table<'a>(model: &'a Model, rows: &'a [AccountRow]) -> iced::widget::Column<'a, Message> {
    let head = |label: &'static str, portion: u16| {
        t(label, ty::TABLE_HEADER, color::TEXT_MUTED).width(Length::FillPortion(portion))
    };
    let mut table = column![
        row![
            head("Account", 4),
            head("Tag (hex)", 5),
            head("On this device", 3),
            head("On ledger", 3),
            head("Status", 4),
            space().width(Length::Fixed(96.0)),
        ]
        .spacing(sp::S12)
        .padding(Padding::from([sp::S10, sp::S12])),
        ui::divider(),
    ]
    .spacing(sp::S4);
    for account in rows {
        let (word, ink) = status(account);
        let paused = matches!(account.state, AccountState::Diverged { .. });
        let ledger = on_ledger(account).map_or_else(|| "—".to_owned(), |i| format!("#{i}"));
        let hex = account.id.hex();
        let tag = format!("{}…{}", &hex[..10], &hex[hex.len() - 8..]);
        let act: Element<'a, Message> = if paused {
            action(
                model,
                "Review",
                theme::Button::WarningTonal,
                Size::Small,
                None,
                WalletMsg::Open(To::Recovery),
            )
        } else {
            space().into()
        };
        let line = row![
            column![
                t(name(account.id), ty::TABLE_NAME, color::TEXT_PRIMARY),
                t(kind(account), ty::TINY, color::TEXT_MUTED),
            ]
            .spacing(sp::S2)
            .width(Length::FillPortion(4)),
            t(tag, ty::MONO_SMALL, color::TEXT_SECONDARY)
                .wrapping(Wrapping::WordOrGlyph)
                .width(Length::FillPortion(5)),
            t(format!("#{}", account.index), ty::MONO, color::TEXT_PRIMARY)
                .width(Length::FillPortion(3)),
            t(
                ledger,
                ty::MONO,
                if paused {
                    color::WARNING
                } else {
                    color::TEXT_PRIMARY
                }
            )
            .width(Length::FillPortion(3)),
            t(word, ty::TABLE_BODY, ink).width(Length::FillPortion(4)),
            container(act)
                .width(Length::Fixed(96.0))
                .align_x(Alignment::End),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center);
        let boxed = container(line)
            .padding(Padding::from([sp::S12, sp::S12]))
            .width(Length::Fill);
        table = table.push(if paused {
            boxed.style(theme::row_warning)
        } else {
            boxed
        });
    }
    table
}

/// The accounts on a phone: one stacked item each, with a divider between
/// each pair.
fn stacked_accounts<'a>(
    model: &'a Model,
    rows: &'a [AccountRow],
) -> iced::widget::Column<'a, Message> {
    let mut list = column![];
    for (n, account) in rows.iter().enumerate() {
        if n > 0 {
            list = list.push(ui::divider());
        }
        list = list.push(account_item(model, account));
    }
    list
}

/// One account on a phone: its name and kind with its status, its tag, the
/// key index on this device and the one on the ledger, and Review where the
/// account is paused.
fn account_item<'a>(model: &'a Model, account: &'a AccountRow) -> Element<'a, Message> {
    let (word, ink) = status(account);
    let paused = matches!(account.state, AccountState::Diverged { .. });
    let ledger = on_ledger(account).map_or_else(|| "—".to_owned(), |i| format!("#{i}"));
    let hex = account.id.hex();
    let tag = format!("{}…{}", &hex[..10], &hex[hex.len() - 8..]);
    let ledger_ink = if paused {
        color::WARNING
    } else {
        color::TEXT_SECONDARY
    };
    let mut item = column![
        row![
            column![
                t(name(account.id), ty::TABLE_NAME, color::TEXT_PRIMARY),
                t(kind(account), ty::TINY, color::TEXT_MUTED),
            ]
            .spacing(sp::S2)
            .width(Length::Fill),
            t(word, ty::TABLE_BODY, ink),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center),
        t(tag, ty::MONO_SMALL, color::TEXT_SECONDARY).wrapping(Wrapping::WordOrGlyph),
        row![
            t(
                format!("On this device #{}", account.index),
                ty::NOTE,
                color::TEXT_MUTED
            ),
            t(format!("· On ledger {ledger}"), ty::NOTE, ledger_ink),
        ]
        .spacing(sp::S6),
    ]
    .spacing(sp::S8);
    if paused {
        item = item.push(
            container(action(
                model,
                "Review",
                theme::Button::WarningTonal,
                Size::Small,
                None,
                WalletMsg::Open(To::Recovery),
            ))
            .width(Length::Fill)
            .align_x(Alignment::End),
        );
    }
    let boxed = container(item)
        .padding(Padding::from([sp::S12, sp::S12]))
        .width(Length::Fill);
    if paused {
        boxed.style(theme::row_warning).into()
    } else {
        boxed.into()
    }
}

fn card_head<'a>(lead: Icon, title: &'a str, subtitle: String) -> Element<'a, Message> {
    row![
        container(icon::icon(lead, 20.0, 2.0, color::TEXT_ON_ACCENT))
            .center(Length::Fixed(40.0))
            .style(theme::icon_tile(color::ACCENT, color::TEXT_ON_ACCENT)),
        column![
            t(title, ty::CARD_TITLE, color::TEXT_PRIMARY),
            t(subtitle, ty::NOTE, color::TEXT_MUTED),
        ]
        .spacing(sp::S2),
    ]
    .spacing(sp::S14)
    .align_y(Alignment::Center)
    .into()
}

/// A labelled line of a settings card, its control or value beside it. On a
/// phone the control sits under the label, and a line without a control is
/// only its label and note.
fn setting<'a>(
    compact: bool,
    label: impl iced::widget::text::IntoFragment<'a>,
    note: Option<String>,
    control: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut words = column![t(label, ty::BODY, color::TEXT_PRIMARY)].spacing(sp::S2);
    if let Some(note) = note {
        words = words.push(t(note, ty::NOTE, color::TEXT_MUTED).wrapping(Wrapping::WordOrGlyph));
    }
    if compact {
        let mut line = column![words.width(Length::Fill)].spacing(sp::S10);
        if let Some(control) = control {
            line = line.push(control);
        }
        return line.into();
    }
    row![
        words.width(Length::Fill),
        control.unwrap_or_else(|| space().width(Length::Shrink).into()),
    ]
    .spacing(sp::S16)
    .align_y(Alignment::Center)
    .into()
}

/// The auto-lock periods, one segment each, hugging their labels. On a phone
/// they wrap onto a second line rather than share the width: five segments
/// with 14 pixels of padding either side are wider than a phone, and a
/// segment sharing the width would break its label across two lines.
fn auto_lock(model: &Model) -> Element<'_, Message> {
    let compact = model.compact();
    let mut periods = row![].spacing(if compact { sp::S6 } else { sp::S4 });
    for m in IDLE_LOCK_MINUTES {
        let label = match m {
            1 => "1 min",
            2 => "2 min",
            5 => "5 min",
            10 => "10 min",
            _ => "15 min",
        };
        let on_tap: Option<Message> = Some(WalletMsg::AutoLock(m).into());
        periods = periods.push(ui::segment(
            label,
            model.prefs.idle_lock.as_secs() == m * 60,
            Length::Shrink,
            on_tap,
        ));
    }
    if compact {
        return periods.wrap().vertical_spacing(sp::S6).into();
    }
    periods.into()
}

/// The keystore: its location, how its key is derived, the auto-lock, and the
/// device unlock on a phone whose shell offers it.
fn keystore(model: &Model) -> Element<'_, Message> {
    let compact = model.compact();
    let kdf = tawara_wallet_core::NEW_STORE_KDF;
    let dir = model
        .wallet
        .as_ref()
        .map_or_else(String::new, |w| w.dir.display().to_string());
    let mut parts = column![
        card_head(
            Icon::Lock,
            "Keystore",
            "Argon2id and ChaCha20-Poly1305 · on this device only".to_owned()
        ),
        setting(compact, "Folder", Some(dir), None),
        ui::divider(),
        setting(
            compact,
            "Key derivation",
            Some(format!(
                "Argon2id at {} MiB over {} passes and {} lane, for a new keystore; a keystore \
                 made elsewhere keeps its own.",
                kdf.memory_kib / 1024,
                kdf.passes,
                kdf.lanes
            )),
            None,
        ),
        ui::divider(),
        setting(
            compact,
            "Auto-lock",
            Some("After this long with nothing done".to_owned()),
            Some(auto_lock(model)),
        ),
    ]
    .spacing(sp::S14);
    if let Some(device) = &model.device {
        parts = parts.push(ui::divider()).push(device_unlock(model, device));
    }
    parts = parts.push(ui::divider()).push(ui::helper(
        "The recovery phrase is shown once, when the keystore is made, and the keystore is \
         never exported.",
    ));
    ui::card_for(compact, parts).into()
}

/// The device unlock, on a phone whose shell offers it. When on, the password
/// is kept on this phone and released by the device's check, and it can be
/// turned off here. When off, it can be offered again at the next unlock with
/// the password.
fn device_unlock<'a>(model: &'a Model, device: &'a Device) -> Element<'a, Message> {
    let name = device.check().name();
    let (note, control) = match model.prefs.device_unlock {
        DeviceUnlock::On => (
            format!(
                "On. The password is kept on this phone, and {name} releases it to unlock the \
                 wallet. Turning it off deletes the password."
            ),
            Some(action(
                model,
                "Turn off",
                theme::Button::Secondary,
                Size::Small,
                None,
                Message::TurnOffDeviceUnlock,
            )),
        ),
        DeviceUnlock::Off => (
            "Off. Turn it on, and the next unlock with the password offers to keep the password \
             on this phone."
                .to_owned(),
            // If the wallet page already shows the offer, that is where to
            // turn it on.
            (!model.device_offer).then(|| {
                action(
                    model,
                    "Turn on",
                    theme::Button::Secondary,
                    Size::Small,
                    None,
                    Message::OfferDeviceUnlockAgain,
                )
            }),
        ),
        DeviceUnlock::Unoffered => (
            "Off. The next unlock with the password offers to keep the password on this phone."
                .to_owned(),
            None,
        ),
    };
    setting(
        model.compact(),
        format!("Unlock with {name}"),
        Some(note),
        control,
    )
}

/// The node card's note on the default node and what a node learns, as the
/// first-run node screen says it. With no node set (after Forget), nothing is
/// asked, so the note does not mention "the node it uses".
fn node_note(set: bool) -> String {
    if set {
        format!(
            "By default the wallet uses {DEFAULT_NODE}; you can choose another API, or none. The \
             API it uses learns which tags the wallet asks about, and so their balances, and when \
             the wallet is used; it can also learn your network address. Requests to \
             {DEFAULT_NODE} pass through Cloudflare, which can read them and sees your network \
             address."
        )
    } else {
        format!(
            "No API is set, so the wallet asks none until you choose one. The default is \
             {DEFAULT_NODE}, and requests to it pass through Cloudflare, which can read them and \
             sees your network address."
        )
    }
}

/// The node: its address, network, whether it answers, its tip and when that
/// was solved, and the Mesh middleware's sync state.
fn node<'a>(model: &'a Model, s: &'a SettingsPage) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    let mut field = text_input("https://", &s.node)
        .font(ty::MONO.font)
        .size(ty::MONO.size)
        .padding(Padding::from([sp::S12, sp::S14]))
        .width(Length::Fill)
        .style(theme::field(radius::R12));
    if idle {
        field = field
            .on_input(|u| WalletMsg::NodeTyped(u).into())
            .on_submit(WalletMsg::SaveNode.into());
    }
    let (state, ink) = match (&model.node.url, &model.node.tip, &model.node.error) {
        (None, ..) => ("None chosen".to_owned(), color::TEXT_MUTED),
        (Some(_), _, Some(_)) => ("Not answering".to_owned(), color::WARNING),
        (Some(_), Some((_, took)), None) => (
            format!("Answering · {} ms", took.as_millis()),
            color::ACCENT,
        ),
        (Some(_), None, None) => ("Not asked yet".to_owned(), color::TEXT_MUTED),
    };
    let tip = model
        .node
        .tip
        .map_or_else(|| "—".to_owned(), |(tip, _)| ui::group(&tip.to_string()));
    let solved = model
        .node
        .solved_ms
        .filter(|_| model.node.tip.is_some())
        .map(|at| format!("solved {}", ui::age(model.clock_ms, at)));
    let network = model.node.network.as_ref().map_or_else(
        || "—".to_owned(),
        |n| format!("{} · {}", n.blockchain, n.network),
    );
    // The middleware's description of its last refresh, verbatim.
    let (sync, sync_ink) = match (&model.node.sync, model.node.tip) {
        (Some(s), Some(_)) => (
            s.stage.clone(),
            if s.synced {
                color::ACCENT
            } else {
                color::WARNING
            },
        ),
        (None, Some(_)) => ("Not said".to_owned(), color::TEXT_MUTED),
        (_, None) => ("—".to_owned(), color::TEXT_MUTED),
    };
    let tile = |label: &'static str, value: String, ink: iced::Color, under: Option<String>| {
        let mut body = column![
            t(label, ty::NOTE, color::TEXT_MUTED),
            t(value, ty::MONO, ink).wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(sp::S4);
        if let Some(under) = under {
            body = body.push(t(under, ty::NOTE, color::TEXT_MUTED));
        }
        container(body)
            .padding(sp::S14)
            .width(Length::Fill)
            .style(theme::field_box)
    };
    // While the preferences file cannot be read, the card says why and offers
    // Forget beside Check now as the way out.
    let mut parts: Vec<Element<'a, Message>> = vec![card_head(
        Icon::Server,
        "API",
        "Balances, tag lookups, submits and the index".to_owned(),
    )];
    if model.prefs_held {
        parts.push(ui::warning_callout(HELD_TITLE, HELD_BODY));
    }
    parts.push(t("Address", ty::FORM_LABEL, color::TEXT_SECONDARY).into());
    parts.push(
        row![
            field,
            action(
                model,
                "Save",
                theme::Button::Primary,
                Size::Medium,
                None,
                WalletMsg::SaveNode,
            ),
        ]
        .spacing(sp::S10)
        .align_y(Alignment::Center)
        .into(),
    );
    parts.push(
        ui::helper(format!(
            "{} {}",
            node_note(model.node.url.is_some()),
            // The same rule, worded for a phone.
            if model.phone() {
                "https:// only, or http:// to this phone."
            } else {
                "https:// only, or http:// to this computer."
            }
        ))
        .into(),
    );
    parts.push(
        row![
            tile("Network", network, color::TEXT_PRIMARY, None),
            tile("Status", state, ink, None),
        ]
        .spacing(sp::S12)
        .into(),
    );
    parts.push(
        row![
            tile("Tip", tip, color::TEXT_PRIMARY, solved),
            tile("Middleware sync", sync, sync_ink, None),
        ]
        .spacing(sp::S12)
        .into(),
    );
    parts.push(
        ui::helper("Sync says whether the API's last read of the chain tip finished.").into(),
    );
    let check = action(
        model,
        "Check now",
        theme::Button::Secondary,
        Size::Small,
        Some(Icon::Refresh),
        WalletMsg::CheckNode,
    );
    parts.push(if model.prefs_held {
        row![
            check,
            action(
                model,
                "Forget the API",
                theme::Button::Secondary,
                Size::Small,
                None,
                Message::ForgetNode,
            ),
        ]
        .spacing(sp::S10)
        .into()
    } else {
        check
    });
    ui::card_for(model.compact(), column(parts).spacing(sp::S12)).into()
}

/// The amount units, one segment each. On the desktop they hug their labels;
/// on a phone they share the width.
fn units(model: &Model) -> Row<'_, Message> {
    let compact = model.compact();
    let (width, gap) = if compact {
        (Length::Fill, sp::S6)
    } else {
        (Length::Shrink, sp::S4)
    };
    let unit = |label: &'static str, u: AmountUnit| {
        let on_tap: Option<Message> = Some(WalletMsg::Unit(u).into());
        ui::segment(label, model.prefs.unit == u, width, on_tap)
    };
    row![
        unit("MCM", AmountUnit::Mcm),
        unit("nanoMCM", AmountUnit::NanoMcm)
    ]
    .spacing(gap)
}

/// Display: the unit amounts are shown in.
fn display(model: &Model) -> Element<'_, Message> {
    let compact = model.compact();
    ui::card_for(
        compact,
        column![
            t("Display", ty::CARD_TITLE, color::TEXT_PRIMARY),
            setting(
                compact,
                "Amounts",
                Some("Typed amounts are MCM whichever is shown.".to_owned()),
                Some(units(model).into()),
            ),
        ]
        .spacing(sp::S16),
    )
    .into()
}

/// About: this build's details, and the Licences row that opens the
/// licences page.
fn about<'a>(model: &'a Model) -> Element<'a, Message> {
    let compact = model.compact();
    let fact = |label: &'static str, value: String| -> Element<'a, Message> {
        let words = t(label, ty::BODY_SMALL, color::TEXT_MUTED);
        let value = t(value, ty::BODY_SMALL, color::TEXT_PRIMARY).wrapping(Wrapping::WordOrGlyph);
        if compact {
            return column![words, value.width(Length::Fill)]
                .spacing(sp::S2)
                .into();
        }
        row![
            words.width(Length::FillPortion(2)),
            value.width(Length::FillPortion(3)),
        ]
        .spacing(sp::S12)
        .into()
    };
    let to_licences: Option<Message> = model
        .busy
        .is_none()
        .then(|| WalletMsg::Open(To::Licences).into());
    let licences = setting(
        compact,
        "Licences",
        Some("The wallet's own licence, and the notices of the software it includes.".to_owned()),
        Some(
            ui::button_with(
                "Show the licences",
                theme::Button::Secondary,
                Size::Small,
                None,
                to_licences,
            )
            .into(),
        ),
    );
    ui::card_for(
        compact,
        column![
            t("About", ty::CARD_TITLE, color::TEXT_PRIMARY),
            fact("Signatures", "WOTS+ one-time keys".to_owned()),
            fact("Addresses", "20-byte tag · Base58 with CRC-16".to_owned()),
            fact("Tawara", format!("version {}", env!("CARGO_PKG_VERSION"))),
            fact(
                "Wallet library",
                format!(
                    "mochimo-crypto at {}",
                    &tawara_wallet_core::LIBRARY_REV[..7]
                )
            ),
            licences,
        ]
        .spacing(sp::S12),
    )
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
    use crate::app::{NARROWEST, NodeState, Page, Screen, WalletPage};
    use crate::harness::{Driver, fits, fits_at};
    use crate::screens;
    use crate::ui::Zone;

    /// The node and the folder the sample store is kept in, as the screenshot
    /// example names them.
    const NODE: &str = "https://node.example";
    const DIR: &str = "/home/you/.local/share/tawara/keystore";

    /// The sample store on the settings page, drawn `width` logical pixels
    /// across, built as the screenshot example builds it.
    fn sample_model(width: f32) -> Model {
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
        model.width = width;
        model.wallet = Some(sample::wallet_view());
        model.screen = Screen::Wallet(WalletPage {
            page: Page::Settings(SettingsPage {
                node: NODE.to_owned(),
            }),
            ..WalletPage::default()
        });
        model
    }

    /// The auto-lock periods as the desktop drew them before they were
    /// segments: buttons, in a row.
    fn old_auto_lock(model: &Model) -> Element<'_, Message> {
        let mut periods = row![].spacing(sp::S4);
        for m in IDLE_LOCK_MINUTES {
            let label = match m {
                1 => "1 min",
                2 => "2 min",
                5 => "5 min",
                10 => "10 min",
                _ => "15 min",
            };
            let on_press: Option<Message> = Some(WalletMsg::AutoLock(m).into());
            periods = periods.push(ui::button_with(
                label,
                theme::Button::Segment {
                    selected: model.prefs.idle_lock.as_secs() == m * 60,
                },
                ui::Size::Small,
                None,
                on_press,
            ));
        }
        periods.into()
    }

    /// The units as the desktop drew them before they were segments.
    fn old_units(model: &Model) -> Element<'_, Message> {
        let unit = |label: &'static str, u: AmountUnit| {
            let on_press: Option<Message> = Some(WalletMsg::Unit(u).into());
            ui::button_with(
                label,
                theme::Button::Segment {
                    selected: model.prefs.unit == u,
                },
                ui::Size::Small,
                None,
                on_press,
            )
        };
        row![
            unit("MCM", AmountUnit::Mcm),
            unit("nanoMCM", AmountUnit::NanoMcm)
        ]
        .spacing(sp::S4)
        .into()
    }

    #[test]
    fn the_settings_fit_a_phone_of_the_narrowest_width() {
        let model = sample_model(NARROWEST);
        assert_eq!(fits(screens::view(&model), NARROWEST), Ok(()));
    }

    #[test]
    fn the_settings_fit_a_phone_of_the_narrowest_width_with_the_preferences_held() {
        // The note, and Forget beside Check now, at the narrowest width.
        let mut model = sample_model(NARROWEST);
        model.node = NodeState::default();
        model.prefs_held = true;
        assert_eq!(fits(screens::view(&model), NARROWEST), Ok(()));
    }

    #[test]
    fn the_desktop_periods_and_units_draw_the_pixels_of_the_buttons_they_replace() {
        let model = sample_model(1440.0);
        let segments: Element<'_, Message> = auto_lock(&model);
        let drawn = Driver::new(segments, (420, 40)).draw();
        let buttons = Driver::new(old_auto_lock(&model), (420, 40)).draw();
        assert!(drawn == buttons, "the auto-lock periods' pixels differ");

        let segments: Element<'_, Message> = units(&model).into();
        let drawn = Driver::new(segments, (300, 40)).draw();
        let buttons = Driver::new(old_units(&model), (300, 40)).draw();
        assert!(drawn == buttons, "the units' pixels differ");
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

    #[test]
    fn on_a_phone_each_auto_lock_label_lies_on_one_line() {
        // A label that wraps is two lines, about 34 pixels tall, and one line
        // is about 17, so the height tells them apart.
        let model = sample_model(NARROWEST);
        let mut renderer = crate::harness::renderer();
        let root = screens::view(&model);
        let mut ui = iced_runtime::user_interface::UserInterface::build(
            root,
            iced::Size::new(NARROWEST, 844.0),
            iced_runtime::user_interface::Cache::default(),
            &mut renderer,
        );
        let mut texts = Texts::default();
        ui.operate(&renderer, &mut texts);
        for label in ["1 min", "2 min", "5 min", "10 min", "15 min"] {
            let lines: Vec<f32> = texts
                .0
                .iter()
                .filter(|(drawn, _)| drawn == label)
                .map(|(_, at)| at.height)
                .collect();
            assert_eq!(lines.len(), 1, "`{label}` is drawn {} times", lines.len());
            assert!(
                lines[0] < 20.0,
                "`{label}` is drawn over two lines: {lines:?}"
            );
        }
    }

    /// The texts `model` draws, in a window 1440 by 1160.
    fn drawn(model: &Model) -> Vec<String> {
        let mut renderer = crate::harness::renderer();
        let mut ui = iced_runtime::user_interface::UserInterface::build(
            screens::view(model),
            iced::Size::new(1440.0, 1160.0),
            iced_runtime::user_interface::Cache::default(),
            &mut renderer,
        );
        let mut texts = Texts::default();
        ui.operate(&renderer, &mut texts);
        texts.0.into_iter().map(|(text, _)| text).collect()
    }

    #[test]
    fn the_node_card_speaks_of_the_node_it_uses_only_when_one_is_set() {
        // A node is set: what it learns, and Cloudflare in front of the
        // default.
        let set = drawn(&sample_model(1440.0));
        assert!(
            set.iter()
                .any(|t| t.contains("The API it uses learns") && t.contains("Cloudflare")),
            "{set:?}"
        );
        // Forgotten: no node is asked, and the card says that instead.
        let mut model = sample_model(1440.0);
        model.node = NodeState::default();
        let none = drawn(&model);
        assert!(
            !none.iter().any(|t| t.contains("The API it uses")),
            "{none:?}"
        );
        assert!(
            none.iter()
                .any(|t| t.contains("No API is set") && t.contains(DEFAULT_NODE)),
            "{none:?}"
        );
    }

    /// The sample store's settings page on a phone whose shell offers Face ID,
    /// with the device unlock in `state`, and an offer on the page or not.
    fn phone_settings(state: DeviceUnlock, offer: bool) -> Model {
        let mut model = sample_model(NARROWEST);
        model.device = Some(crate::harness::face_id_device());
        model.prefs.device_unlock = state;
        model.device_offer = offer;
        model
    }

    /// The texts the phone's settings page draws, in a window of `height`.
    fn phone_texts(model: &Model, height: f32) -> Vec<String> {
        crate::harness::texts(screens::view(model), (NARROWEST, height))
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    #[test]
    fn the_device_row_is_drawn_only_where_a_shell_offers_the_device_unlock() {
        let plain = phone_texts(&sample_model(NARROWEST), 844.0);
        assert!(
            !plain.iter().any(|t| t.starts_with("Unlock with")),
            "{plain:?}"
        );
        let offered = phone_texts(&phone_settings(DeviceUnlock::On, false), 844.0);
        assert!(
            offered.iter().any(|t| t == "Unlock with Face ID"),
            "{offered:?}"
        );
        assert!(
            offered
                .iter()
                .any(|t| t.starts_with("On. The password is kept on this phone")),
            "{offered:?}"
        );
    }

    #[test]
    fn the_device_row_turns_the_unlock_off_when_on_and_on_again_when_off() {
        let on = phone_texts(&phone_settings(DeviceUnlock::On, false), 844.0);
        assert!(on.iter().any(|t| t == "Turn off"), "{on:?}");
        assert!(!on.iter().any(|t| t == "Turn on"), "{on:?}");

        let off = phone_texts(&phone_settings(DeviceUnlock::Off, false), 844.0);
        assert!(off.iter().any(|t| t == "Turn on"), "{off:?}");
        assert!(!off.iter().any(|t| t == "Turn off"), "{off:?}");

        // Not yet offered: the next unlock with the password offers it, and there
        // is nothing for the row to switch.
        let unoffered = phone_texts(&phone_settings(DeviceUnlock::Unoffered, false), 844.0);
        assert!(!unoffered.iter().any(|t| t == "Turn on"), "{unoffered:?}");
        assert!(!unoffered.iter().any(|t| t == "Turn off"), "{unoffered:?}");
        assert!(
            unoffered
                .iter()
                .any(|t| t.starts_with("Off. The next unlock with the password")),
            "{unoffered:?}"
        );
    }

    #[test]
    fn the_device_row_fits_a_phone_of_360_by_640_and_390_by_844() {
        for (width, height) in [(NARROWEST, 640.0), (390.0, 844.0)] {
            let mut model = phone_settings(DeviceUnlock::On, false);
            model.width = width;
            assert_eq!(
                fits_at(screens::view(&model), width, height),
                Ok(()),
                "the settings with the device row, at {width} by {height}"
            );
        }
    }

    #[test]
    fn a_tap_on_turn_off_turns_the_device_unlock_off() {
        // The page is drawn tall, so that the row's button is in the window.
        let model = phone_settings(DeviceUnlock::On, false);
        let size = (NARROWEST, 4000.0);
        let (_, bounds) = crate::harness::texts(screens::view(&model), size)
            .into_iter()
            .find(|(text, _)| text == "Turn off")
            .expect("the button");
        let (x, y) = (
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );
        let mut driver = Driver::new(screens::view(&model), (360, 4000));
        let (messages, _, _) = driver.send(&[
            crate::harness::mouse_move(x, y),
            crate::harness::mouse_press(),
            crate::harness::mouse_release(),
        ]);
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::TurnOffDeviceUnlock)),
            "{messages:?}"
        );
    }

    /// The messages a press on the drawn text `label` produces in a window of
    /// `size`, as a person presses it.
    fn pressed(model: &Model, label: &str, size: (u32, u32)) -> Vec<Message> {
        let mut driver = crate::harness::Driver::new(screens::view(model), size);
        let at = driver
            .text_bounds(label)
            .unwrap_or_else(|| panic!("`{label}` is not drawn"));
        let (messages, _, _) = driver.send(&[
            crate::harness::mouse_move(at.center_x(), at.center_y()),
            crate::harness::mouse_press(),
            crate::harness::mouse_release(),
        ]);
        messages
    }

    #[test]
    fn the_licences_row_in_about_opens_the_licences_on_the_desktop_and_a_phone() {
        // Each page is drawn tall enough that the row's button is in the window.
        for (size, model) in [
            ((1440, 2400), sample_model(1440.0)),
            ((360, 4000), sample_model(NARROWEST)),
        ] {
            let messages = pressed(&model, "Show the licences", size);
            assert!(
                matches!(
                    messages.as_slice(),
                    [Message::Wallet(WalletMsg::Open(To::Licences))]
                ),
                "{messages:?}"
            );
        }
    }
}
