// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The worker's staking commands against the scripted node: the reads, the
//! network check, the library's checks before a key is reserved, and a stake
//! planned, signed and submitted.

mod support;

use std::time::{Duration, Instant};

use mochimo_crypto::tx::wire::Transaction;
use support::*;
use tawara_wallet_core::staking::{MAINNET, StakeAction, StakePlanView, StakeRequest, StakeView};
use tawara_wallet_core::view::{AccountId, AccountState};
use tawara_wallet_core::{Command, Refusal, RefusalKind, Reply};

/// A tip above mainnet's trigger, so the v4.1 rules apply to the next block.
const TIP: u64 = 1_143_700;
const MCM: u64 = 1_000_000_000;
const FUNDS: u64 = 1_000 * MCM;

fn account0() -> AccountId {
    AccountId::from_tag(tag(0))
}

fn refusal(reply: Reply) -> Refusal {
    match reply {
        Reply::Refused(r) => r,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn info(reply: Reply) -> Box<StakeView> {
    match reply {
        Reply::StakeInfo(v) => v,
        other => panic!("expected the stake page's reads, got {other:?}"),
    }
}

fn plan(reply: Reply) -> Box<StakePlanView> {
    match reply {
        Reply::StakePlanned(p) => p,
        other => panic!("expected a staking plan, got {other:?}"),
    }
}

fn request(action: StakeAction) -> Command {
    Command::PlanStake {
        request: StakeRequest {
            account: account0(),
            action,
        },
    }
}

/// A wallet open on mainnet's chain past the trigger, account 0 holding
/// `FUNDS`, set up before it opens by `script`.
fn staking(scratch: &Scratch, script: impl FnOnce(&Chain)) -> Harness {
    let mut h = Harness::with_node();
    h.chain.set_tip(TIP);
    h.chain.serve_staking(MAINNET.t41);
    h.chain.hold(tag(0), address(0, 0), FUNDS);
    script(&h.chain);
    match h.create_from_phrase(&scratch.store()) {
        Reply::Created {
            opened: Ok(view), ..
        } => assert!(view.opened, "the wallet opens: {view:?}"),
        other => panic!("the store was not made and opened: {other:?}"),
    }
    h
}

#[test]
fn the_stake_page_reads_the_account_and_the_network_and_spaces_its_reward_reads() {
    let scratch = Scratch::new("stake-info");
    let mut h = staking(&scratch, |_| {});
    let v = info(h.call(Command::StakeInfo {
        account: account0(),
    }));
    assert_eq!(v.height, TIP + 1);
    assert_eq!(v.minimum, 500 * MCM);
    assert_eq!(v.tiers.len(), 6);
    assert_eq!(v.tiers[5].coefficient, 6);
    let a = v.account_read.as_ref().expect("the account was read");
    assert_eq!(a.spendable, FUNDS);
    assert!(a.stake.is_none());
    assert_eq!(a.reward, Ok(0));
    // The library's own page for the same reads.
    assert!(v.text.contains("staking on mainnet"), "{}", v.text);
    assert_eq!(h.chain.entitlement_calls(), 1);

    // A second read asks `/account/entitlement` again only once the node's
    // ten seconds have passed.
    let start = Instant::now();
    info(h.call(Command::StakeInfo {
        account: account0(),
    }));
    assert!(
        start.elapsed() >= Duration::from_secs(10),
        "the second reward read waited out the API's limit: {:?}",
        start.elapsed()
    );
    assert_eq!(h.chain.entitlement_calls(), 2);
    assert!(h.chain.submits().is_empty());
}

#[test]
fn a_node_serving_another_chain_is_refused_before_anything_is_laid_out() {
    let scratch = Scratch::new("stake-network");
    let mut h = staking(&scratch, |c| c.serve_staking(1_128_960));
    let r = refusal(h.call(request(StakeAction::Stake {
        amount: 500 * MCM,
        tier: 0,
    })));
    assert_eq!(r.kind, RefusalKind::Staking);
    assert!(r.text.contains("1128960"), "{}", r.text);
    assert!(r.text.contains("no key was used"), "{}", r.text);
    let r = refusal(h.call(Command::StakeInfo {
        account: account0(),
    }));
    assert_eq!(r.kind, RefusalKind::Staking);
    assert!(h.chain.submits().is_empty());
}

#[test]
fn a_stake_below_the_minimum_is_refused_in_the_librarys_words() {
    let scratch = Scratch::new("stake-minimum");
    let mut h = staking(&scratch, |_| {});
    let r = refusal(h.call(request(StakeAction::Stake {
        amount: 10 * MCM,
        tier: 0,
    })));
    assert_eq!(r.kind, RefusalKind::Staking);
    assert!(r.text.contains("below the minimum stake"), "{}", r.text);
    assert!(r.text.contains("the key is unused"), "{}", r.text);
    assert!(h.chain.submits().is_empty());
}

#[test]
fn a_stake_is_laid_out_then_signed_and_submitted_as_a_spend_is() {
    let scratch = Scratch::new("stake-sign");
    let mut h = staking(&scratch, |_| {});
    let p = plan(h.call(request(StakeAction::Stake {
        amount: 500 * MCM,
        tier: 5,
    })));
    assert_eq!(p.height, TIP + 1);
    assert_eq!(p.expiry, mochimo_crypto::staking::expiry(TIP + 1, 5));
    assert_eq!(p.fee_total, 500);
    assert!(p.text.contains("FORFEITS"), "{}", p.text);
    assert!(
        h.chain.submits().is_empty(),
        "nothing is signed by laying out"
    );

    let s = match h.call(Command::ConfirmStake { plan: p.plan }) {
        Reply::Sent(s) => s,
        other => panic!("expected a signed stake, got {other:?}"),
    };
    let submits = h.chain.submits();
    assert_eq!(submits.len(), 1);
    assert!(s.submitted);
    assert_eq!(s.artifact_hex, hex_of(&submits[0]));
    Transaction::from_wire(&submits[0]).expect("the API was sent a transaction");
    // The library's page for a staking transaction sent.
    assert!(s.text.contains("Dust-proof form"), "{}", s.text);
    assert!(
        matches!(
            s.view.accounts[0].state,
            AccountState::SpendOutstanding { .. }
        ),
        "the stake holds the account's reservation: {:?}",
        s.view.accounts[0].state
    );

    // Confirming it again finds nothing waiting.
    let r = refusal(h.call(Command::ConfirmStake { plan: p.plan }));
    assert_eq!(r.kind, RefusalKind::NoSuchPlan);
    assert_eq!(h.chain.submits().len(), 1);
}

#[test]
fn a_locked_stake_is_unstaken_only_with_its_forfeit_confirmed() {
    let scratch = Scratch::new("stake-forfeit");
    let commit = TIP - 100;
    let expiry = mochimo_crypto::staking::expiry(commit, 0).expect("tier 0");
    let mut h = staking(&scratch, |c| {
        c.hold(tag(0), address(0, 0), 100 * MCM);
        c.stake(tag(0), 500 * MCM, commit, expiry, 7 * MCM);
    });
    let r = refusal(h.call(request(StakeAction::Unstake {
        forfeit: false,
        lock_ends: None,
    })));
    assert_eq!(r.kind, RefusalKind::Staking);
    assert!(r.text.contains("LOCKED"), "{}", r.text);
    let p = plan(h.call(request(StakeAction::Unstake {
        forfeit: true,
        lock_ends: None,
    })));
    assert_eq!(p.staked, 500 * MCM);
    assert_eq!(p.forfeits, Some(7 * MCM));
    assert_eq!(p.expiry, Some(expiry));
    assert!(p.text.contains("FORFEITS"), "{}", p.text);
    assert!(h.chain.submits().is_empty());
}

#[test]
fn a_lock_drops_a_staking_plan() {
    let scratch = Scratch::new("stake-lock");
    let mut h = staking(&scratch, |_| {});
    let p = plan(h.call(request(StakeAction::Stake {
        amount: 500 * MCM,
        tier: 0,
    })));
    match h.call(Command::Lock) {
        Reply::Locked => {}
        other => panic!("expected the store locked, got {other:?}"),
    }
    let _ = h.unlock(&scratch.store(), PASSWORD);
    let r = refusal(h.call(Command::ConfirmStake { plan: p.plan }));
    assert_eq!(r.kind, RefusalKind::NoSuchPlan);
    assert!(h.chain.submits().is_empty());
}
