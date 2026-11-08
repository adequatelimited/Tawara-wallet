// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Staking on the v4.1 chain: the requests the interface makes and the views
//! the worker returns.
//!
//! The rules are the library's. A stake, an unstake and a claim are checked by
//! `cli::staking::checked` (the checks the command line's `stake`, `unstake`
//! and `claim` make before a key is reserved) and laid out by
//! `Wallet::plan_dust_proof`. A refusal is `Outcome::StakingRefused` and a
//! signed transaction `Outcome::StakingSent`, rendered as the command line
//! renders them. An account's stake is read from `/account/balance` and
//! `/account/entitlement`, and the network's from `/stats/staking`, as
//! `stake-info` reads them.
//!
//! The worker adds a review step before signing, which the library does not
//! have, and the network check below.
//!
//! # The network check
//!
//! A staking transaction carries the network it is signed for, and the chain
//! refuses one signed for another. A testnet forked from mainnet's ledger
//! shares its accounts, so a transaction sent to the wrong network spends a
//! one-time key for nothing, and that key must not be used again. Before
//! anything is laid out or signed, the node's `/stats/staking` must report the
//! trigger of the network this build signs for ([`crate::Config::staking`],
//! mainnet by default). A node that reports another, or none, is refused.

use mochimo_crypto::staking;

use crate::command::PlanId;
use crate::view::AccountId;

/// The chain the staking verbs sign for: mainnet.
pub const MAINNET: staking::Network = staking::Network::MAINNET;

/// How many lock tiers there are: 0 to 5.
pub const TIERS: u8 = staking::TIERS;

/// The fee every staking transaction pays, in nanoMCM: the command line's
/// default (`MFEE`).
pub const FEE: u64 = mochimo_crypto::consts::MFEE;

/// Heights per hour, for "about N days" (one block a minute).
pub const BLOCKS_PER_HOUR: u64 = staking::BLOCKS_PER_HOUR;

/// What a staking transaction is asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StakeAction {
    /// Stake `amount` nanoMCM at lock tier `tier` (0 to 5).
    Stake { amount: u64, tier: u8 },
    /// End the stake. `forfeit` confirms that a still-locked stake gives up
    /// its earned reward to the other stakers; without it a locked stake is
    /// refused, as the command line refuses `unstake` without `--forfeit`.
    /// `lock_ends` is the end height the review showed for a lock that has
    /// ended; the library refuses the unstake when the node now reports a
    /// different one.
    Unstake {
        forfeit: bool,
        lock_ends: Option<u64>,
    },
    /// Release the whole reward into the spendable balance. After a lock has
    /// ended this also ends the stake.
    Claim,
}

/// A staking transaction to lay out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StakeRequest {
    pub account: AccountId,
    pub action: StakeAction,
}

/// One lock tier, as it applies to a stake made in the next block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierChoice {
    pub tier: u8,
    /// The reward weight's multiplier, `tier + 1`.
    pub coefficient: u64,
    /// The height the lock would end at, a neogenesis height.
    pub expiry: u64,
    /// How many heights from the next block that is.
    pub heights: u64,
}

/// The lock tiers for a stake in the block at `height`.
#[must_use]
pub fn tiers_at(height: u64) -> Vec<TierChoice> {
    (0..TIERS)
        .filter_map(|tier| {
            staking::expiry(height, tier).map(|expiry| TierChoice {
                tier,
                coefficient: staking::coefficient(tier),
                expiry,
                heights: expiry.saturating_sub(height),
            })
        })
        .collect()
}

/// `heights` as whole days and hours, at one block a minute.
#[must_use]
pub fn days_and_hours(heights: u64) -> (u64, u64) {
    staking::days_and_hours(heights)
}

/// An account's stake, as `/account/balance` reported it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StakeHeld {
    /// The staked principal, nanoMCM.
    pub staked: u64,
    pub commit: u64,
    /// The height the lock ends at.
    pub expiry: u64,
    /// Whether the lock holds at the next block.
    pub locked: bool,
    pub tier: Option<u8>,
    pub coefficient: Option<u64>,
    /// `(staked + reward) x coefficient`.
    pub weight: Option<u128>,
}

/// What an account holds for staking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountStake {
    /// What a send spends, nanoMCM. Staked coins and the reward are not in it.
    pub spendable: u64,
    /// The stake, when there is one.
    pub stake: Option<StakeHeld>,
    /// The reward the account holds (entitlement), nanoMCM, or why it could
    /// not be read.
    pub reward: Result<u64, String>,
    /// The height the figures were read at.
    pub read_at: u64,
}

/// The network's staking, from `/stats/staking`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkStake {
    /// How many tags stake, and how much, from the middleware's ledger cache.
    pub stakers: Option<u64>,
    pub staked: Option<u64>,
    /// The last block's staker pool and its height.
    pub last_pool: Option<(u64, u64)>,
}

/// What the stake page shows for one account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StakeView {
    pub account: AccountId,
    /// The height a transaction sent now is expected in: the tip plus one.
    pub height: u64,
    /// The minimum stake in that block, nanoMCM.
    pub minimum: u64,
    /// The next height the minimum falls at, and to what.
    pub minimum_next: Option<(u64, u64)>,
    /// The tiers a stake made now would get.
    pub tiers: Vec<TierChoice>,
    /// The account's figures, or why they could not be read.
    pub account_read: Result<AccountStake, String>,
    /// The network's figures, or why they could not be read.
    pub network_read: Result<NetworkStake, String>,
    /// The library's `stake-info` page for the same reads, verbatim.
    pub text: String,
}

/// A staking transaction laid out and not signed, waiting for
/// [`crate::Command::ConfirmStake`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StakePlanView {
    pub plan: PlanId,
    pub account: AccountId,
    pub action: StakeAction,
    pub fee_total: u64,
    /// The height the checks were made for.
    pub height: u64,
    /// The spendable balance the fee is paid from.
    pub spendable: u64,
    /// The stake as it stands, when there is one.
    pub staked: u64,
    /// For a stake, the height its lock would end at if included at
    /// `height`; for an unstake or a claim, the current lock's end.
    pub expiry: Option<u64>,
    /// The reward an early unstake gives up to the other stakers.
    pub forfeits: Option<u64>,
    /// The reward a claim releases.
    pub releases: Option<u64>,
    /// A description of what signing does.
    pub text: String,
}
