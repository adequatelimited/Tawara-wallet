// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Stake: an account's stake and the network's, and a stake, an unstake or a
//! claim from it, each laid out and reviewed before it is signed.
//!
//! The figures are the node's, read by the worker as the command line's
//! `stake-info` reads them, with the library's page for them under the cards.
//! The checks before signing are the library's: what this page does not
//! offer (a stake below the minimum, a second stake, a claim while locked)
//! the library would refuse anyway.

use iced::widget::text::Wrapping;
use iced::widget::{Column, column, row};
use iced::{Alignment, Element, Length};
use tawara_wallet_core::preferences::AmountUnit;
use tawara_wallet_core::staking::{
    FEE, StakeAction, StakePlanView, StakeView, TierChoice, days_and_hours,
};

use super::{account_choice, frame};
use crate::app::{Message, Model, StakePage, StakeVerb, WalletMsg, WalletPage};
use crate::icon::Icon;
use crate::theme::{self, color, space as sp};
use crate::ui::{self, Size, t, ty};

/// An amount in the chosen unit, with the unit's name.
fn money(unit: AmountUnit, nano: impl Into<u128>) -> String {
    format!("{} {}", ui::amount(nano, unit), ui::unit_name(unit))
}

/// `heights` at one block a minute, as a lock length: "about 45 days and 12
/// hours".
fn lock_length(heights: u64) -> String {
    let plural = |n: u64, one: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {one}s")
        }
    };
    let (days, hours) = days_and_hours(heights);
    format!(
        "about {} and {}",
        plural(days, "day"),
        plural(hours, "hour")
    )
}

/// What signing a stake does, with the stake's amount, tier, weight and lock.
/// Stakers share half of each block's reward, the pool the node pays
/// (`S_h = B_h / 2 + F_h`).
fn stake_says(unit: AmountUnit, amount: u64, tier: u8, lock: Option<u64>) -> String {
    let tier_named = if tier == 0 {
        "the base tier".to_owned()
    } else {
        format!("tier {tier}")
    };
    let locked = lock.map_or_else(
        || "The stake will be locked until its lock ends.".to_owned(),
        |heights| format!("The stake will be locked for {}.", lock_length(heights)),
    );
    format!(
        "Clicking \"Sign & submit\" below stakes {} at {tier_named} with a {}x stake weight \
         multiplier. {locked} Stakers share half of each block reward proportionally with all \
         other stakers for any given block, based on their stake weight. The staked rewards \
         compound each block as long as the coins remain staked. Even after the lock period \
         expires, the staked coins will continue to accrue rewards at the same staking tier \
         until you either unstake or claim your rewards.",
        money(unit, amount),
        u64::from(tier) + 1,
    )
}

/// The note under what signing a stake does: what an early unstake costs,
/// with the fee.
fn stake_note() -> String {
    let fee = tawara_wallet_core::amount::format_mcm_short(FEE);
    format!(
        "Unstaking before the lock period expires will FORFEIT your rewards to be split among \
         the remaining stakers, but your principal will be returned to you less the {} MCM \
         transaction fee. Please ensure you are prepared to leave these coins locked for the \
         full staking period to avoid forfeiting your accrued rewards.",
        fee.strip_prefix('0').unwrap_or(&fee)
    )
}

/// A block height with its digits grouped, as the other pages show one.
fn at(height: u64) -> String {
    ui::group(&height.to_string())
}

/// `heights` at one block a minute.
fn about(heights: u64) -> String {
    let plural = |n: u64, one: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {one}s")
        }
    };
    match days_and_hours(heights) {
        (0, 0) => "in under an hour".to_owned(),
        (0, h) => format!("in about {}", plural(h, "hour")),
        (d, 0) => format!("in about {}", plural(d, "day")),
        (d, h) => format!("in about {} and {}", plural(d, "day"), plural(h, "hour")),
    }
}

/// "until height 1,402,624 (in about 179 days and 19 hours)", counted from
/// `height`, the next block's.
fn until(expiry: u64, height: u64) -> String {
    format!(
        "until height {} ({})",
        at(expiry),
        about(expiry.saturating_sub(height))
    )
}

/// A figure and what it is, one line.
fn line<'a>(what: &'a str, value: String) -> Element<'a, Message> {
    row![
        t(what, ty::FORM_LABEL, color::TEXT_SECONDARY).width(Length::Fixed(150.0)),
        // An address is one word wider than a phone's column, so it breaks
        // between characters rather than run past the card.
        t(value, ty::BODY, color::TEXT_PRIMARY)
            .wrapping(Wrapping::WordOrGlyph)
            .width(Length::Fill),
    ]
    .spacing(sp::S12)
    .align_y(Alignment::Start)
    .into()
}

pub fn view<'a>(model: &'a Model, page: &'a WalletPage, s: &'a StakePage) -> Element<'a, Message> {
    if let Some(plan) = &s.review {
        return review(model, page, plan);
    }
    let compact = model.compact();
    let idle = model.busy.is_none();
    let accounts = model
        .wallet
        .as_ref()
        .map_or(&[][..], |w| w.accounts.as_slice());
    let mut body: Vec<Element<'a, Message>> = vec![
        ui::card_for(
            compact,
            column![
                ui::section_label("Account"),
                account_choice(model, accounts.iter().map(|a| a.id), s.account, |id| {
                    WalletMsg::StakeFor(id).into()
                }),
            ]
            .spacing(sp::S12),
        )
        .into(),
    ];
    match &s.read {
        Some(v) => read_cards(model, s, v, &mut body),
        None if s.reading => body.push(ui::helper("Reading…").into()),
        None => {}
    }
    let actions = vec![
        ui::button_with(
            "Read again",
            theme::Button::Secondary,
            Size::Header,
            Some(Icon::Refresh),
            (idle && s.account.is_some()).then(|| WalletMsg::StakeRead.into()),
        )
        .into(),
    ];
    frame(
        model,
        page,
        "Stake",
        "Stake coins for a share of each block's reward. The reward compounds inside \
         the lock.",
        actions,
        body,
    )
}

/// The account's card, the network's, and what can be done.
fn read_cards<'a>(
    model: &'a Model,
    s: &'a StakePage,
    v: &'a StakeView,
    body: &mut Vec<Element<'a, Message>>,
) {
    let compact = model.compact();
    let unit = model.prefs.unit;
    let mut account: Column<'a, Message> =
        column![ui::section_label("This account")].spacing(sp::S10);
    match &v.account_read {
        Ok(a) => {
            account = account.push(line("Spendable", money(unit, a.spendable)));
            match &a.stake {
                Some(k) => {
                    account = account.push(line("Staked", money(unit, k.staked)));
                    account = account.push(line(
                        "Reward",
                        match &a.reward {
                            Ok(r) => money(unit, *r),
                            Err(e) => format!("not read: {e}"),
                        },
                    ));
                    account = account.push(line(
                        "Tier",
                        match (k.tier, k.coefficient) {
                            (Some(tier), Some(c)) => format!("{tier}, reward weight x{c}"),
                            (Some(tier), None) => tier.to_string(),
                            _ => "none matches the stake's heights".to_owned(),
                        },
                    ));
                    if k.expiry > 0 {
                        account = account.push(line(
                            "Lock",
                            if k.locked {
                                format!("LOCKED {}", until(k.expiry, v.height))
                            } else {
                                format!("ended at height {}", at(k.expiry))
                            },
                        ));
                    }
                    if let Some(w) = k.weight {
                        account = account.push(line("Weight", money(unit, w)));
                    }
                }
                None => {
                    account = account.push(line("Staked", "nothing".to_owned()));
                    if let Ok(r) = &a.reward
                        && *r > 0
                    {
                        account = account.push(line("Reward", money(unit, *r)));
                    }
                }
            }
            account = account.push(t(
                format!("Read at height {}.", at(a.read_at)),
                ty::NOTE,
                color::TEXT_MUTED,
            ));
        }
        Err(e) => {
            account = account.push(ui::helper(format!("Not read: {e}.")));
        }
    }
    body.push(ui::card_for(compact, account).into());

    let mut network: Column<'a, Message> =
        column![ui::section_label("The network")].spacing(sp::S10);
    network = network.push(line("Minimum stake", money(unit, v.minimum)));
    if let Some((from, next)) = v.minimum_next {
        network = network.push(line(
            "Next minimum",
            format!(
                "{} {}",
                money(unit, next),
                about(from.saturating_sub(v.height))
            ),
        ));
    }
    match &v.network_read {
        Ok(n) => {
            if let Some(count) = n.stakers {
                network = network.push(line("Stakers", count.to_string()));
            }
            if let Some(staked) = n.staked {
                network = network.push(line("Staked in all", money(unit, staked)));
            }
            // What the stakers shared at the last block that paid them.
            if let Some((pool, _)) = n.last_pool {
                network = network.push(line(
                    "Last reward received",
                    format!("{} for the most recent block", money(unit, pool)),
                ));
            }
        }
        Err(e) => network = network.push(ui::helper(format!("Not read: {e}."))),
    }
    body.push(ui::card_for(compact, network).into());

    if let Ok(a) = &v.account_read {
        let reward = a.reward.as_ref().ok().copied().unwrap_or(0);
        match &a.stake {
            None if reward > 0 => body.push(claim_first(model, reward)),
            None => body.push(stake_form(model, s, v)),
            Some(k) if k.locked => {
                body.push(locked(model, v, k.expiry, a.reward.as_ref().ok().copied()))
            }
            Some(_) => body.push(ended(model, a.reward.as_ref().ok().copied())),
        }
    }
}

/// A tag holding reward and no stake: claim first.
fn claim_first<'a>(model: &'a Model, reward: u64) -> Element<'a, Message> {
    let idle = model.busy.is_none();
    ui::card_for(
        model.compact(),
        column![
            ui::warning_callout(
                "Claim the reward first",
                format!(
                    "This account holds {} of reward. A stake from an account with unclaimed \
                     reward is included for its fee and binds nothing, so the wallet will not lay \
                     one out. Claiming releases the reward into the spendable balance.",
                    money(model.prefs.unit, reward)
                ),
            ),
            ui::button_with(
                "Review claim",
                theme::Button::Primary,
                Size::Header,
                None,
                idle.then(|| WalletMsg::StakeReview(StakeVerb::Claim).into()),
            ),
        ]
        .spacing(sp::S16),
    )
    .into()
}

/// The form for a new stake: the amount and the lock.
fn stake_form<'a>(model: &'a Model, s: &'a StakePage, v: &'a StakeView) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let idle = model.busy.is_none();
    let review = WalletMsg::StakeReview(StakeVerb::Stake).into();
    let mut tiers = row![].spacing(sp::S8);
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    for (i, choice) in v.tiers.iter().enumerate() {
        let selected = choice.tier == s.tier;
        tiers = tiers.push(ui::button_with(
            format!(
                "{} days",
                choice.heights / (24 * tawara_wallet_core::staking::BLOCKS_PER_HOUR)
            ),
            theme::Button::Segment { selected },
            Size::Small,
            None,
            idle.then(|| WalletMsg::StakeTier(choice.tier).into()),
        ));
        // Three to a row on a phone, all six on one beside the sidebar.
        if model.compact() && i % 3 == 2 {
            rows.push(core::mem::replace(&mut tiers, row![].spacing(sp::S8)).into());
        }
    }
    rows.push(tiers.into());
    let chosen: Option<&TierChoice> = v.tiers.iter().find(|c| c.tier == s.tier);
    let detail = chosen.map_or_else(
        || "No lock could be computed for this tier.".to_owned(),
        |c| {
            format!(
                "Tier {}: locked {}. Reward weight x{}: the longer the lock, the larger the \
                 share.",
                c.tier,
                until(c.expiry, v.height),
                c.coefficient
            )
        },
    );
    ui::card_for(
        model.compact(),
        column![
            ui::section_label("New stake"),
            ui::field(
                format!("Amount ({})", ui::unit_name(AmountUnit::Mcm)),
                "0.000000000",
                &s.amount,
                ty::FIELD,
                false,
                |text| WalletMsg::StakeAmount(text).into(),
                Some(WalletMsg::StakeReview(StakeVerb::Stake).into()),
            ),
            row![
                ui::link("The most it can take", ty::LINK_SMALL, WalletMsg::StakeMax.into()),
            ],
            ui::helper(format!(
                "At least {}. The fee, {FEE} nanoMCM, comes from the spendable balance, and one more \
                 fee's worth stays spendable so that the account can unstake later. A stake has no \
                 top-ups: a larger one needs this one ended first.",
                money(unit, v.minimum)
            )),
            ui::section_label("Lock"),
            Column::with_children(rows).spacing(sp::S8),
            ui::helper(detail),
            ui::helper(
                "Unstaking before the lock ends FORFEITS the whole reward to the other stakers. \
                 After it ends, the stake keeps earning until it is unstaken or claimed.",
            ),
            ui::button_with(
                "Review stake",
                theme::Button::Primary,
                Size::Header,
                None,
                idle.then_some(review),
            ),
        ]
        .spacing(sp::S12),
    )
    .into()
}

/// A stake still locked: unstaking forfeits the reward.
fn locked<'a>(
    model: &'a Model,
    v: &StakeView,
    expiry: u64,
    reward: Option<u64>,
) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let idle = model.busy.is_none();
    let reward = reward.map_or_else(
        || "an amount the API did not report".to_owned(),
        |r| money(unit, r),
    );
    ui::card_for(
        model.compact(),
        column![
            ui::warning_callout(
                "The lock has not ended",
                format!(
                    "The stake is locked {}. Unstaking before then FORFEITS its whole reward, \
                     {reward}, to the other stakers, and that cannot be undone. Waiting until the \
                     lock ends keeps the reward.",
                    until(expiry, v.height)
                ),
            ),
            ui::button_with(
                "Review unstake and forfeit",
                theme::Button::WarningPrimary,
                Size::Header,
                None,
                idle.then(|| WalletMsg::StakeReview(StakeVerb::Unstake).into()),
            ),
        ]
        .spacing(sp::S16),
    )
    .into()
}

/// A stake whose lock has ended.
fn ended<'a>(model: &'a Model, reward: Option<u64>) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let idle = model.busy.is_none();
    let reward = reward.map_or_else(|| "its reward".to_owned(), |r| money(unit, r));
    ui::card_for(
        model.compact(),
        column![
            ui::accent_callout(
                Icon::Check,
                "The lock has ended",
                format!(
                    "Nothing is forfeited now. Claim releases the reward, {reward}, and returns the \
                     stake in one transaction. Unstake returns the stake and leaves the reward on \
                     the account, earning nothing more, to claim later."
                ),
            ),
            row![
                ui::button_with(
                    "Review claim and unstake",
                    theme::Button::Primary,
                    Size::Header,
                    None,
                    idle.then(|| WalletMsg::StakeReview(StakeVerb::Claim).into()),
                ),
                ui::button_with(
                    "Review unstake",
                    theme::Button::Secondary,
                    Size::Header,
                    None,
                    idle.then(|| WalletMsg::StakeReview(StakeVerb::Unstake).into()),
                ),
            ]
            .spacing(sp::S12),
        ]
        .spacing(sp::S16),
    )
    .into()
}

/// The transaction laid out, before it is signed.
fn review<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    plan: &'a StakePlanView,
) -> Element<'a, Message> {
    let compact = model.compact();
    let unit = model.prefs.unit;
    let idle = model.busy.is_none();
    let (title, what) = match plan.action {
        StakeAction::Stake { amount, tier } => (
            "Review stake",
            format!("Stake {} at tier {tier}", money(unit, amount)),
        ),
        StakeAction::Unstake { .. } => (
            "Review unstake",
            format!("Unstake {}", money(unit, plan.staked)),
        ),
        StakeAction::Claim => (
            "Review claim",
            match plan.releases {
                Some(r) => format!("Claim {}", money(unit, r)),
                None => "Claim the whole reward".to_owned(),
            },
        ),
    };
    let forfeits = plan.forfeits.filter(|f| *f > 0);
    let mut card: Column<'a, Message> =
        column![ui::section_label("What will be signed")].spacing(sp::S10);
    card = card.push(line(
        "From",
        plan.account
            .destination()
            .unwrap_or_else(|| plan.account.hex()),
    ));
    card = card.push(line("Transaction", what));
    if let Some(e) = plan.expiry {
        card = card.push(line(
            "Lock",
            match plan.action {
                StakeAction::Stake { .. } => until(e, plan.height),
                _ if plan.height < e => format!("LOCKED {}", until(e, plan.height)),
                _ => format!("ended at height {}", at(e)),
            },
        ));
    }
    card = card.push(line("Fee", format!("{} nanoMCM", plan.fee_total)));
    card = card.push(line("Spendable now", money(unit, plan.spendable)));
    let mut body: Vec<Element<'a, Message>> = vec![ui::card_for(compact, card).into()];
    if let Some(f) = forfeits {
        body.push(ui::warning_callout(
            "This forfeits the reward",
            format!(
                "{} goes to the other stakers when you unstake. You will forfeit and lose these coins \
                 if you unstake. This cannot be undone after you Sign & submit.",
                money(unit, f)
            ),
        ));
    }
    // A stake is described by `stake_says`, with its note on an early
    // unstake; an unstake or a claim uses the worker's description.
    let says = match plan.action {
        StakeAction::Stake { amount, tier } => stake_says(
            unit,
            amount,
            tier,
            plan.expiry.map(|e| e.saturating_sub(plan.height)),
        ),
        _ => plan.text.clone(),
    };
    body.push(
        ui::card_for(
            compact,
            column![
                ui::section_label("What signing does"),
                t(says, ty::BODY_SMALL, color::TEXT_SECONDARY),
            ]
            .spacing(sp::S10),
        )
        .into(),
    );
    if matches!(plan.action, StakeAction::Stake { .. }) {
        body.push(ui::warning_callout("Note", stake_note()));
    }
    let sign = if forfeits.is_some() {
        theme::Button::WarningPrimary
    } else {
        theme::Button::Primary
    };
    body.push(
        row![
            ui::button_with(
                "Edit",
                theme::Button::Secondary,
                Size::Header,
                None,
                idle.then(|| WalletMsg::StakeEdit.into()),
            ),
            ui::button_with(
                "Sign & submit",
                sign,
                Size::Header,
                None,
                idle.then(|| WalletMsg::StakeSign.into()),
            ),
        ]
        .spacing(sp::S12)
        .into(),
    );
    // What holds until the transaction is signed.
    let until = match plan.action {
        StakeAction::Stake { .. } => "Nothing is staked until you click \"Sign & submit\".",
        StakeAction::Unstake { .. } => "You are still staking until you click \"Sign & submit\".",
        StakeAction::Claim => "Nothing is claimed until you click \"Sign & submit\".",
    };
    frame(model, page, title, until, Vec::new(), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stake_says_its_tier_weight_and_lock_in_the_owners_words() {
        // 45 days and 12 hours at one block a minute.
        let base = stake_says(AmountUnit::Mcm, 500_000_000_000, 0, Some(65_520));
        assert!(
            base.starts_with(
                "Clicking \"Sign & submit\" below stakes 500 MCM at the base tier with a 1x \
                 stake weight multiplier. The stake will be locked for about 45 days and 12 hours."
            ),
            "{base}"
        );
        assert!(
            base.contains("Stakers share half of each block reward"),
            "{base}"
        );
        let longer = stake_says(AmountUnit::Mcm, 1, 2, Some(262_144));
        assert!(
            longer.contains("at tier 2 with a 3x stake weight multiplier"),
            "{longer}"
        );
        assert!(
            stake_note().contains("less the .0000005 MCM transaction fee"),
            "{}",
            stake_note()
        );
    }
}
