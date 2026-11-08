// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Sample view models, for drawing the screens without a store or a node,
//! as the screenshots do.
//!
//! Every text in them is the library's own, rendered from sample values by
//! the same code that renders a real one, so a screenshot shows the words a
//! person will read. The tags are made-up bytes that belong to no one, and
//! the views are plain data: nothing here opens a store, asks a node or
//! holds a secret.

use std::path::PathBuf;

use mochimo_crypto::account::{AccountKind as LibraryKind, StreamId, WotsIndex};
use mochimo_crypto::cli::discover::{Sighting, Sweep};
use mochimo_crypto::cli::outcome::{MempoolRow, Outcome, Shipped, StakeInfoPage, StakingContext};
use mochimo_crypto::entitlement::{Entitlement, Origin};
use mochimo_crypto::mesh::codec::{
    BlockMetadata, MeshBlock, MeshTransaction, OP_DESTINATION, OP_FEE, OP_REWARD, OP_SOURCE,
    Operation, SearchPage, StakeAt, StakeFields, StakeSetSummary, StakeSource, StakeTerms,
    StakingLedger, StakingStats,
};
use mochimo_crypto::mesh::spend::DustProofOp;
use mochimo_crypto::mesh::{ChainTip, LedgerEntry, TxId};
use mochimo_crypto::recon::{AccountStatus, ChainPosition, Divergence, Reservation};
use mochimo_crypto::tx::wire::Destination;

use crate::command::PlanId;
use crate::event::{
    AccountReport, Discovered, PlanView, PlannedDestination, ReceiveView, SentView,
};
use crate::explorer::{
    AccountHistory, BlockDetail, BlockSummary, BlocksView, ChainView, ExplorerRefusal, IndexState,
    LedgerRead, MempoolView, PendingRow, PendingView, TagView, TransactionView,
};
use crate::staking::{StakeAction, StakePlanView, StakeRequest, StakeView};
use crate::text;
use crate::view::{
    AccountId, AccountKind, AccountRow, AccountState, Notice, NoticeKind, ReservationState,
    WalletView,
};
use crate::worker::hex;

/// A made-up tag: twenty bytes from `seed`.
fn tag(seed: u8) -> [u8; 20] {
    let mut t = [0u8; 20];
    for (i, b) in t.iter_mut().enumerate() {
        *b = seed
            .wrapping_mul(31)
            .wrapping_add(u8::try_from(i).unwrap_or(0).wrapping_mul(17))
            | 1;
    }
    t
}

fn index(n: u32) -> WotsIndex {
    let mut i = WotsIndex::ZERO;
    for _ in 0..n {
        i = i.advanced().unwrap_or(i);
    }
    i
}

/// An account two keys behind the chain: the library's report for it, and
/// the index its acknowledged advance would move it to.
fn behind(t: [u8; 20], local: u32, balance: u64) -> Divergence {
    let mut local_address = [0u8; 40];
    local_address[..20].copy_from_slice(&t);
    local_address[20..].fill(0x5a);
    let mut chain_address = local_address;
    chain_address[20..].fill(0xb7);
    Divergence::IndexMismatch {
        tag: t,
        local: index(local),
        local_address,
        chain_address,
        balance,
        stream: StreamId::from_bytes(t),
        found: ChainPosition::Ahead {
            index: index(local + 2),
            gap: 2,
        },
        reverted_settle: None,
    }
}

/// The sample store's diverged account: the chain shows it two keys ahead.
fn diverged() -> Divergence {
    behind(tag(3), 31, 85_135_096_906)
}

/// The ledger address a made-up tag holds at some key: the tag, then
/// twenty bytes of `fill` standing for the key's half.
fn address(t: [u8; 20], fill: u8) -> [u8; 40] {
    let mut a = [0u8; 40];
    a[..20].copy_from_slice(&t);
    a[20..].fill(fill);
    a
}

/// The first account's balance in the sample store.
const FIRST_BALANCE: u64 = 9_215_402_118_000;

/// An open store of three accounts: one reconciled, one with a spend
/// outstanding, and one the chain shows two keys ahead, set aside with the
/// library's report; and the library's notice that the store is not whole.
#[must_use]
pub fn wallet_view() -> WalletView {
    let diverged = diverged();
    let rows = vec![
        AccountRow {
            id: AccountId::from_tag(tag(1)),
            kind: AccountKind::Derived,
            index: 49,
            state: AccountState::InSync {
                balance: FIRST_BALANCE,
            },
            spendable: true,
            number: Some(0),
            reserved: false,
        },
        AccountRow {
            id: AccountId::from_tag(tag(2)),
            kind: AccountKind::Derived,
            index: 6,
            state: AccountState::SpendOutstanding {
                balance: 3_180_000_000_000,
                spent_index: 6,
                reservation: ReservationState::Live,
            },
            spendable: false,
            number: Some(1),
            reserved: true,
        },
        AccountRow {
            id: AccountId::from_tag(tag(3)),
            kind: AccountKind::Imported,
            index: 31,
            state: AccountState::from_divergence(&diverged),
            spendable: false,
            number: None,
            reserved: false,
        },
    ];
    WalletView {
        dir: PathBuf::from("/home/you/.local/share/tawara/keystore"),
        accounts: rows,
        opened: true,
        notice: crate::text::standing_notice(&[diverged]).map(|text| Notice {
            kind: NoticeKind::NotWhole,
            text,
        }),
        settled: Vec::new(),
    }
}

/// The sample store with the second account's spend landed: the chain holds
/// that account at its change key, and the account reads `SpendLanded` until
/// the worker settles it. Its other accounts are as in [`wallet_view`].
#[must_use]
pub fn landed_view() -> WalletView {
    let mut view = wallet_view();
    let row = &mut view.accounts[1];
    row.index = 7;
    row.state = AccountState::SpendLanded {
        balance: 2_640_000_000_000,
        spent_index: 6,
        settled_index: 7,
    };
    row.spendable = false;
    view
}

/// A wallet just made, its one account never paid: the node answers
/// "account not found" for it, which is what a new account is.
#[must_use]
pub fn new_view() -> WalletView {
    let unfound = Divergence::TagUnresolved {
        tag: tag(1),
        local: WotsIndex::ZERO,
    };
    WalletView {
        dir: PathBuf::from("/home/you/.local/share/tawara/keystore"),
        accounts: vec![AccountRow {
            id: AccountId::from_tag(tag(1)),
            kind: AccountKind::Derived,
            index: 0,
            state: AccountState::from_divergence(&unfound),
            spendable: false,
            number: Some(0),
            reserved: false,
        }],
        opened: true,
        notice: crate::text::standing_notice(&[unfound]).map(|text| Notice {
            kind: NoticeKind::NotWhole,
            text,
        }),
        settled: Vec::new(),
    }
}

/// The same store opened while its node did not answer: no account was
/// reconciled, so no balance is known, and the worker's notice says why.
#[must_use]
pub fn unreconciled_view() -> WalletView {
    let mut view = wallet_view();
    for row in &mut view.accounts {
        row.state = AccountState::NotReconciled;
        row.spendable = false;
    }
    view.opened = false;
    view.notice = Some(Notice {
        kind: NoticeKind::NodeSilent,
        text: crate::worker::NODE_SILENT.to_owned(),
    });
    view
}

/// The first account's receive page: its destination, its key index, the
/// ledger address that index makes, and the library's words for the two.
#[must_use]
pub fn receive_view() -> ReceiveView {
    let t = tag(1);
    let held = address(t, 0x3c);
    ReceiveView {
        account: AccountId::from_tag(t),
        destination: AccountId::from_tag(t).destination(),
        index: 49,
        ledger_address: hex(&held),
        text: text::page(
            &[],
            Outcome::Address {
                tag: t,
                address: held,
                index: index(49),
            },
        ),
    }
}

/// A reference in its sixteen-byte field, zero-filled.
fn reference(text: &str) -> [u8; 16] {
    let mut field = [0u8; 16];
    field[..text.len()].copy_from_slice(text.as_bytes());
    field
}

/// What the sample spend sends: two payees, each with its reference.
fn payees() -> Vec<(Destination, &'static str)> {
    vec![
        (
            Destination {
                tag: tag(7),
                reference: reference("PAYROLL-11"),
                amount: 420_000_000_000,
            },
            "PAYROLL-11",
        ),
        (
            Destination {
                tag: tag(8),
                reference: reference("PAYROLL-12"),
                amount: 380_000_000_000,
            },
            "PAYROLL-12",
        ),
    ]
}

/// The sample spend's totals: sent, the fee at the node's floor, and the
/// change back to the first account's next key.
fn totals() -> (u64, u64, u64) {
    let send: u64 = payees().iter().map(|(d, _)| d.amount).sum();
    let fee = 2 * mochimo_crypto::consts::MFEE;
    (send, fee, FIRST_BALANCE - send - fee)
}

/// A spend from the first account to two payees, laid out and not signed:
/// the library's "not signed" page, with the store's standing divergence in
/// front of it as the worker writes it.
#[must_use]
pub fn plan_view() -> PlanView {
    let (send_total, fee_total, change_total) = totals();
    let destinations = payees();
    let text = text::page(
        &[diverged()],
        Outcome::Planned {
            source: tag(1),
            destinations: destinations.iter().map(|(d, _)| d.clone()).collect(),
            send_total,
            fee_total,
            change_total,
            blk_to_live: 0,
        },
    );
    PlanView {
        plan: PlanId::example(),
        from: AccountId::from_tag(tag(1)),
        destinations: destinations
            .iter()
            .map(|(d, r)| PlannedDestination {
                to: AccountId::from_tag(d.tag),
                destination: AccountId::from_tag(d.tag).destination().unwrap_or_default(),
                amount: d.amount,
                reference: (*r).to_owned(),
            })
            .collect(),
        send_total,
        fee_total,
        change_total,
        balance: FIRST_BALANCE,
        blk_to_live: 0,
        empties_account: false,
        text,
    }
}

/// Made-up bytes the size of a signed spend, standing for one: the sample
/// shows how much the retry artifact is, not one that could be sent.
fn signed_bytes() -> Vec<u8> {
    (0u32..2_412)
        .map(|i| u8::try_from((i.wrapping_mul(2_654_435_761) >> 13) & 0xff).unwrap_or(0))
        .collect()
}

/// The sample spend signed and written to the node's socket: the library's
/// page with its three facts, the retry artifact, and the store afterwards,
/// the first account now holding a reservation.
#[must_use]
pub fn sent_view() -> SentView {
    let (send_total, fee_total, change_total) = totals();
    let wire = signed_bytes();
    let id = TxId([0x5e; 32]);
    let text = text::page(
        &[diverged()],
        Outcome::Sent {
            shipped: Shipped {
                source: tag(1),
                destinations: payees().into_iter().map(|(d, _)| d).collect(),
                blk_to_live: 0,
                wire: wire.clone(),
                submitted: Ok(id),
            },
            send_total,
            fee_total,
            change_total,
            upgraded: None,
        },
    );
    let mut view = wallet_view();
    view.accounts[0].set_state(AccountState::SpendOutstanding {
        balance: FIRST_BALANCE,
        spent_index: 49,
        reservation: ReservationState::Live,
    });
    view.accounts[0].spendable = false;
    view.accounts[0].index = 50;
    SentView {
        from: AccountId::from_tag(tag(1)),
        spent_index: Some(49),
        submitted: true,
        tx_id: Some(hex(&id.0)),
        artifact_hex: hex(&wire),
        text,
        view,
    }
}

/// A sweep of derived accounts 0 to 4: the two the store holds, a third
/// the chain holds and the store does not, and two the node does not know.
/// The library's page, and the rows the worker makes of it.
#[must_use]
pub fn discovered() -> (String, Vec<Discovered>) {
    let seen = |account: u32, t: [u8; 20], balance: Option<u64>, held: Option<u32>| Sighting {
        account,
        tag: t,
        entry: balance.map(|balance| LedgerEntry {
            address: address(t, 0x2d),
            balance,
        }),
        held: held.map(|i| (LibraryKind::Derived, index(i))),
    };
    let sweep = Sweep {
        to: 4,
        sightings: vec![
            seen(0, tag(1), Some(FIRST_BALANCE), Some(49)),
            seen(1, tag(2), Some(3_180_000_000_000), Some(6)),
            seen(2, tag(4), Some(25_000_000_000), None),
            seen(3, tag(5), None, None),
            seen(4, tag(6), None, None),
        ],
    };
    let rows = sweep
        .sightings
        .iter()
        .map(|s| Discovered {
            account_index: s.account,
            id: AccountId::from_tag(s.tag),
            ledger_balance: s.entry.map(|e| e.balance),
            held: s.held.is_some(),
        })
        .collect();
    (text::page(&[], Outcome::Discovered { sweep }), rows)
}

/// The library's page for the sample spend's saved bytes, submitted again
/// with no store open.
#[must_use]
pub fn submitted_text() -> String {
    text::page(
        &[],
        Outcome::Submitted {
            source: tag(1),
            bytes: signed_bytes(),
            submitted: Ok(TxId([0x5e; 32])),
        },
    )
}

/// One transaction as the node's index spells it: `ops` are (kind, tag or
/// none, amount in nanoMCM).
fn indexed(
    id: u8,
    block: u64,
    at_ms: i64,
    ops: &[(&str, Option<[u8; 20]>, i128)],
) -> MeshTransaction {
    MeshTransaction {
        hash: [id; 32],
        block: Some(ChainTip {
            index: block,
            hash: [u8::try_from(block % 251).unwrap_or(0); 32],
        }),
        timestamp_ms: Some(at_ms),
        operations: ops
            .iter()
            .enumerate()
            .map(|(i, (kind, t, amount))| Operation {
                index: i as u64,
                kind: (*kind).to_owned(),
                address: t.map(|t| format!("0x{}", hex(&t))).unwrap_or_default(),
                amount: *amount,
                memo: String::new(),
            })
            .collect(),
        metadata: Vec::new(),
    }
}

/// A spend from `from` as the index lists it: the source debited gross, the
/// payees, the change back to `from`, and the fee at the node's floor.
fn spend(
    id: u8,
    block: u64,
    at_ms: i64,
    from: [u8; 20],
    to: &[([u8; 20], i128)],
    change: i128,
) -> MeshTransaction {
    let fee = i128::from(mochimo_crypto::consts::MFEE) * to.len() as i128;
    let sent: i128 = to.iter().map(|(_, a)| a).sum();
    let mut ops = vec![(OP_SOURCE, Some(from), -(sent + fee + change))];
    ops.extend(to.iter().map(|(t, a)| (OP_DESTINATION, Some(*t), *a)));
    ops.push((OP_DESTINATION, Some(from), change));
    ops.push((OP_FEE, None, fee));
    indexed(id, block, at_ms, &ops)
}

const MCM: i128 = 1_000_000_000;

/// `tx` with `reference` on its destination to `to`, as the indexer stores
/// one: the whole sixteen-byte field, padded with NULs.
fn referenced(mut tx: MeshTransaction, to: [u8; 20], reference: &str) -> MeshTransaction {
    let address = format!("0x{}", hex(&to));
    for op in &mut tx.operations {
        if op.kind == OP_DESTINATION && op.address == address {
            op.memo = format!("{reference:\0<16}");
        }
    }
    tx
}

/// What the node's index holds for each account of the sample store, newest
/// first, and the library's page for each. Some rows carry a reference on a
/// destination, as the index does when the spend gave one.
#[must_use]
pub fn activity() -> Vec<AccountHistory> {
    let own = spend(
        0x4b,
        869_130,
        1_790_439_640_000,
        tag(2),
        &[(tag(1), 500 * MCM)],
        2_680 * MCM,
    );
    let rows = [
        (
            tag(1),
            vec![
                referenced(
                    spend(
                        0x91,
                        870_668,
                        1_791_036_200_000,
                        tag(1),
                        &[(tag(7), 420 * MCM), (tag(8), 380 * MCM)],
                        9_215 * MCM,
                    ),
                    tag(7),
                    "INV-0412",
                ),
                referenced(
                    spend(
                        0x37,
                        870_645,
                        1_791_029_405_000,
                        tag(9),
                        &[(tag(1), 420 * MCM)],
                        1_200 * MCM,
                    ),
                    tag(1),
                    "ORDER-77",
                ),
                spend(
                    0x62,
                    870_190,
                    1_790_850_600_000,
                    tag(1),
                    &[(tag(10), 300 * MCM)],
                    10_015 * MCM,
                ),
                own.clone(),
            ],
        ),
        (
            tag(2),
            vec![
                own,
                spend(
                    0x2c,
                    867_780,
                    1_789_989_330_000,
                    tag(11),
                    &[(tag(2), 1_200 * MCM)],
                    40 * MCM,
                ),
                spend(
                    0x15,
                    865_850,
                    1_789_411_622_000,
                    tag(2),
                    &[(tag(12), 410 * MCM)],
                    2_000 * MCM,
                ),
            ],
        ),
        (
            tag(3),
            vec![indexed(
                0x7e,
                868_870,
                1_790_323_331_000,
                &[(OP_REWARD, Some(tag(3)), 85_135_096_906)],
            )],
        ),
    ];
    rows.into_iter()
        .map(|(t, transactions)| {
            let page = SearchPage {
                total_count: transactions.len() as u64,
                transactions,
                next_offset: None,
            };
            let text = text::page(
                &[],
                Outcome::RecentTransactions {
                    tag: t,
                    from: 0,
                    page: Box::new(page.clone()),
                },
            );
            AccountHistory::of(AccountId::from_tag(t), &page, 0, text)
        })
        .collect()
}

/// The time of the sample chain's tip, in milliseconds since the epoch.
pub const TIP_MS: i64 = 1_791_186_252_000;

/// The six newest blocks of the sample chain, read down from its tip, and
/// the library's page for them. Block 871,168 is a neogenesis block (its
/// number's low byte is zero) and 871,171 a pseudo-block (it carries no
/// transactions); the others are normal, their reward among the
/// transactions `/block` lists and not among those their figures count.
#[must_use]
pub fn blocks() -> BlocksView {
    let tip = 871_173;
    let gaps = [0, 312, 289, 405, 251, 338];
    let counts: [u32; 6] = [3, 2, 0, 5, 2, 0];
    let mut at = TIP_MS;
    let mut rows = Vec::new();
    for (n, (gap, count)) in gaps.iter().zip(counts).enumerate() {
        at -= gap * 1_000;
        let index = tip - n as u64;
        let seed = u8::try_from(index % 251).unwrap_or(0);
        let metadata = BlockMetadata {
            block_size: 164 + 2_408 * u64::from(count),
            difficulty: 37,
            fee: 500,
            haiku: "a quiet frost / falls on the sleeping cedar / the moon holds its breath"
                .to_owned(),
            nonce: [seed ^ 0x5a; 32],
            root: [seed ^ 0xa5; 32],
            stime_ms: at,
            tx_count: count.saturating_sub(1),
        };
        rows.push(MeshBlock {
            block: ChainTip {
                index,
                hash: [u8::try_from(index % 251).unwrap_or(0); 32],
            },
            parent: ChainTip {
                index: index - 1,
                hash: [u8::try_from((index - 1) % 251).unwrap_or(0); 32],
            },
            timestamp_ms: at,
            transactions: (0..count)
                .map(|i| MeshTransaction {
                    hash: [u8::try_from(i).unwrap_or(0); 32],
                    block: None,
                    timestamp_ms: None,
                    operations: Vec::new(),
                    metadata: Vec::new(),
                })
                .collect(),
            metadata: Some(metadata),
        });
    }
    let blocks = rows.iter().map(BlockSummary::of).collect();
    let tip = ChainTip {
        index: tip,
        hash: [u8::try_from(tip % 251).unwrap_or(0); 32],
    };
    BlocksView {
        tip: tip.index,
        blocks,
        text: text::page(
            &[],
            Outcome::Blocks {
                count: crate::explorer::CARD_BLOCKS,
                tip,
                rows,
            },
        ),
    }
}

/// A search of the sample node's index refused, with the library's page
/// for it: the route's 404 for a node that runs no index, the middleware's
/// code 2 for one whose index did not answer.
#[must_use]
pub fn index_refusal(state: IndexState) -> ExplorerRefusal {
    let cause = match state {
        IndexState::Absent => mochimo_crypto::Error::HttpStatus { status: 404 },
        IndexState::Unavailable => mochimo_crypto::Error::Mesh {
            code: 2,
            retriable: true,
        },
    };
    ExplorerRefusal {
        index: Some(state),
        text: text::page(&[], Outcome::SearchFailed { cause }),
    }
}

/// The sample node's queue: fourteen transactions waiting, none read whole.
#[must_use]
pub fn mempool() -> MempoolView {
    MempoolView {
        waiting: 14,
        text: text::page(
            &[],
            Outcome::Mempool {
                count: 0,
                total: 14,
                rows: Vec::new(),
            },
        ),
    }
}

/// The miners of the sample chain: made-up tags.
const MINERS: [u8; 4] = [41, 42, 43, 44];

/// What the sample chain pays a miner for a normal block, in nanoMCM.
const REWARD: i128 = 5_084_197_712;

/// The reward transaction of block `index`, paid to `miner`.
fn reward(index: u64, miner: [u8; 20]) -> MeshTransaction {
    let mut id = [0u8; 32];
    id[..8].copy_from_slice(&index.to_be_bytes());
    id[31] = 0xee;
    MeshTransaction {
        hash: id,
        block: None,
        timestamp_ms: None,
        operations: vec![Operation {
            index: 0,
            kind: OP_REWARD.to_owned(),
            address: format!("0x{}", hex(&miner)),
            amount: REWARD,
            memo: String::new(),
        }],
        metadata: Vec::new(),
    }
}

/// A spend as `/block` and `/mempool/transaction` list one: the source
/// debited what left it net of its change, each payee, and the fee at the
/// node's floor for each destination. The change is not an operation.
fn listed(id: [u8; 32], from: [u8; 20], to: &[([u8; 20], i128, &str)]) -> MeshTransaction {
    let fee = i128::from(mochimo_crypto::consts::MFEE) * to.len() as i128;
    let sent: i128 = to.iter().map(|(_, a, _)| a).sum();
    let op = |i: usize, kind: &str, t: Option<[u8; 20]>, amount: i128, memo: &str| Operation {
        index: i as u64,
        kind: kind.to_owned(),
        address: t.map(|t| format!("0x{}", hex(&t))).unwrap_or_default(),
        amount,
        memo: if memo.is_empty() {
            String::new()
        } else {
            format!("{memo:\0<16}")
        },
    };
    let mut ops = vec![op(0, OP_SOURCE, Some(from), -(sent + fee), "")];
    for (i, (t, a, memo)) in to.iter().enumerate() {
        ops.push(op(i + 1, OP_DESTINATION, Some(*t), *a, memo));
    }
    ops.push(op(to.len() + 1, OP_FEE, None, fee, ""));
    MeshTransaction {
        hash: id,
        block: None,
        timestamp_ms: None,
        operations: ops,
        metadata: Vec::new(),
    }
}

/// A made-up transaction id.
fn txid(seed: u8) -> [u8; 32] {
    let mut id = [0u8; 32];
    for (i, b) in id.iter_mut().enumerate() {
        *b = seed
            .wrapping_mul(73)
            .wrapping_add(u8::try_from(i).unwrap_or(0).wrapping_mul(29));
    }
    id
}

/// Block `index` of the sample chain, made at `at` with `spends` and its
/// reward paid to `miner`; a pseudo-block and a neogenesis block carry no
/// reward. Its figures are the node's, with `haiku` for a normal block.
fn sample_block(
    index: u64,
    at: i64,
    kind: &str,
    miner: [u8; 20],
    spends: Vec<MeshTransaction>,
    haiku: &str,
) -> MeshBlock {
    let normal = kind == "normal";
    let counted = if normal {
        u32::try_from(spends.len()).unwrap_or(u32::MAX)
    } else {
        0
    };
    let mut transactions = Vec::new();
    if normal {
        transactions.push(reward(index, miner));
        transactions.extend(spends);
    }
    let seed = u8::try_from(index % 251).unwrap_or(0);
    MeshBlock {
        block: ChainTip {
            index,
            hash: [seed; 32],
        },
        parent: ChainTip {
            index: index - 1,
            hash: [u8::try_from((index - 1) % 251).unwrap_or(0); 32],
        },
        timestamp_ms: at,
        transactions,
        metadata: Some(BlockMetadata {
            block_size: 164 + 2_408 * u64::from(counted),
            difficulty: 37,
            fee: 500,
            haiku: haiku.to_owned(),
            nonce: [seed ^ 0x5a; 32],
            root: [seed ^ 0xa5; 32],
            stime_ms: at,
            tx_count: counted,
        }),
    }
}

/// `n` spends of a block, made up, none of them the sample store's.
fn filler(n: usize, block: u64) -> Vec<MeshTransaction> {
    (0..n)
        .map(|i| {
            let seed = u8::try_from((block as usize + i) % 200).unwrap_or(0);
            listed(
                txid(seed),
                tag(seed.wrapping_add(50)),
                &[(tag(seed.wrapping_add(90)), 1_000 * MCM, "")],
            )
        })
        .collect()
}

/// The sample chain as the explorer shows it: the tip, the ten newest
/// blocks with each one's solve time, and the average over a hundred.
/// Block 871,171 is a pseudo-block and 871,168 a neogenesis block, as in
/// [`blocks`].
#[must_use]
pub fn chain() -> ChainView {
    let tip: u64 = 871_173;
    // Each block's solve time in seconds, newest first; the eleventh is read
    // only for the tenth's time.
    let solves = [66, 78, 949, 122, 91, 75, 72, 58, 107, 82, 70];
    let counts = [126, 211, 0, 94, 158, 0, 173, 88, 142, 119, 97];
    let mut at = TIP_MS;
    let mut rows = Vec::new();
    for (n, (solve, count)) in solves.iter().zip(counts).enumerate() {
        let index = tip - n as u64;
        let kind = match index {
            871_171 => "pseudo",
            871_168 => "neogenesis",
            _ => "normal",
        };
        let miner = tag(MINERS[n % MINERS.len()]);
        rows.push(sample_block(
            index,
            at,
            kind,
            miner,
            filler(count, index),
            "",
        ));
        at -= solve * 1_000;
    }
    let mut blocks = BlockSummary::of_walk(&rows);
    let shown = usize::try_from(crate::explorer::LATEST_BLOCKS).unwrap_or(usize::MAX);
    blocks.truncate(shown);
    rows.truncate(shown);
    let tip = ChainTip {
        index: tip,
        hash: [u8::try_from(tip % 251).unwrap_or(0); 32],
    };
    ChainView {
        tip: tip.index,
        blocks,
        average_ms: Some(86_000),
        text: text::page(
            &[],
            Outcome::Blocks {
                count: crate::explorer::LATEST_BLOCKS,
                tip,
                rows,
            },
        ),
    }
}

/// The sample node's queue: fourteen transactions waiting, the first five
/// read whole, as the explorer's overview reads them.
#[must_use]
pub fn pending() -> PendingView {
    queue_read(crate::explorer::PENDING_ROWS)
}

/// The sample node's whole queue, every one of its fourteen transactions
/// read whole, as "View all pending" reads them.
#[must_use]
pub fn queue() -> PendingView {
    queue_read(crate::explorer::QUEUE_ROWS)
}

/// The sample node's queue with its first `count` transactions read whole.
fn queue_read(count: u64) -> PendingView {
    // Each payee an even share in thousandths of an MCM, the last what makes
    // the total.
    let payees = |n: i128, total: i128| -> Vec<([u8; 20], i128, &'static str)> {
        let each = total / n / 1_000_000 * 1_000_000;
        (0..n)
            .map(|i| {
                let amount = if i + 1 == n {
                    total - each * (n - 1)
                } else {
                    each
                };
                (tag(120 + u8::try_from(i).unwrap_or(0)), amount, "")
            })
            .collect()
    };
    let waiting = [
        (txid(1), tag(60), payees(3, 96_400_000_000)),
        (txid(2), tag(61), payees(1, 12 * MCM)),
        (txid(3), tag(62), payees(18, 1_804_250_000_000)),
        (txid(4), tag(63), payees(1, MCM / 2)),
        (txid(5), tag(64), payees(2, 310 * MCM)),
        (txid(6), tag(65), payees(1, 75 * MCM)),
        (txid(7), tag(66), payees(4, 2_000 * MCM)),
        (txid(8), tag(67), payees(1, 1_250_500_000)),
        (txid(9), tag(2), payees(1, 40 * MCM)),
        (txid(10), tag(68), payees(2, 18 * MCM)),
        (txid(11), tag(69), payees(1, 640 * MCM)),
        (txid(12), tag(70), payees(6, 3_600 * MCM)),
        (txid(13), tag(71), payees(1, 9 * MCM)),
        (txid(14), tag(72), payees(1, 220 * MCM)),
    ];
    let total = waiting.len();
    let rows: Vec<MempoolRow> = waiting
        .into_iter()
        .take(usize::try_from(count).unwrap_or(usize::MAX))
        .map(|(id, from, to)| MempoolRow {
            id,
            transaction: Some(Box::new(listed(id, from, &to))),
        })
        .collect();
    PendingView {
        waiting: total,
        rows: rows
            .iter()
            .map(|r| PendingRow {
                id: hex(&r.id),
                transaction: r.transaction.as_deref().map(TransactionView::of),
            })
            .collect(),
        text: text::page(&[], Outcome::Mempool { count, total, rows }),
    }
}

/// Block 871,172 of the sample chain, whole: a normal block with its haiku,
/// twelve spends, the first from the sample store's first account, and two
/// blocks standing on it.
#[must_use]
pub fn block() -> BlockDetail {
    let index = 871_172;
    let mut spends = vec![listed(
        txid(9),
        tag(1),
        &[
            (tag(7), 1_000 * MCM, "INV-2048"),
            (tag(8), 250 * MCM, "INV-2049"),
        ],
    )];
    spends.extend(filler(11, index));
    let at = TIP_MS - 66_000;
    let block = sample_block(
        index,
        at,
        "normal",
        tag(MINERS[1]),
        spends,
        "patient winter moon\nthe river carries the frost\nquiet as a crane",
    );
    let text = text::page(
        &[],
        Outcome::Block {
            block: Box::new(block.clone()),
        },
    );
    BlockDetail::of(&block, Some(at - 78_000), Some(871_173), text)
}

/// The sample store's first account as the explorer shows it: the ledger's
/// entry for it and its history from the index ([`activity`]'s).
#[must_use]
pub fn tag_view() -> TagView {
    let account = AccountId::from_tag(tag(1));
    TagView {
        account,
        ledger: LedgerRead::Held {
            address: format!("0x{}", hex(&address(tag(1), 0xb7))),
            balance: FIRST_BALANCE,
        },
        history: activity()
            .into_iter()
            .find(|h| h.account == account)
            .ok_or_else(|| index_refusal(IndexState::Absent)),
    }
}

/// A payment received by the sample store's first account, found in the
/// index by its id, and the library's page for it.
#[must_use]
pub fn transaction() -> (TransactionView, String) {
    let tx = referenced(
        spend(
            0x37,
            870_645,
            1_791_029_405_000,
            tag(9),
            &[(tag(1), 420 * MCM)],
            1_200 * MCM,
        ),
        tag(1),
        "ORDER-77",
    );
    let view = TransactionView::of(&tx);
    let text = text::page(
        &[],
        Outcome::LookedUpTransaction {
            page: Box::new(SearchPage {
                transactions: vec![tx],
                total_count: 1,
                next_offset: None,
            }),
        },
    );
    (view, text)
}

/// The sample node's answers for a hash that names nothing: its index holds
/// no transaction with that id, and it serves no block with that hash.
#[must_use]
pub fn not_found(hash: [u8; 32]) -> (ExplorerRefusal, ExplorerRefusal) {
    (
        ExplorerRefusal {
            index: None,
            text: text::page(&[], Outcome::TransactionNotFound { hash }),
        },
        ExplorerRefusal {
            index: None,
            text: text::page(
                &[],
                Outcome::BlockNotServed {
                    by_hash: true,
                    cause: mochimo_crypto::Error::Mesh {
                        code: 2,
                        retriable: false,
                    },
                },
            ),
        },
    )
}

/// Every account of the sample store reconciled now, as account recovery
/// shows them first: the library's report for each.
#[must_use]
pub fn review() -> Vec<AccountReport> {
    let found: Vec<([u8; 20], Result<AccountStatus, Divergence>)> = vec![
        (
            tag(1),
            Ok(AccountStatus::InSync {
                index: index(49),
                address: address(tag(1), 0x3c),
                balance: FIRST_BALANCE,
            }),
        ),
        (
            tag(2),
            Ok(AccountStatus::SpendOutstanding {
                spent_index: index(6),
                balance: 3_180_000_000_000,
                reservation: Reservation::Unrecorded,
            }),
        ),
        (tag(3), Err(diverged())),
    ];
    found
        .into_iter()
        .map(|(t, result)| {
            let state = match &result {
                Ok(status) => AccountState::from_status(status),
                Err(d) => AccountState::from_divergence(d),
            };
            AccountReport {
                account: AccountId::from_tag(t),
                spendable: matches!(state, AccountState::InSync { .. }),
                state,
                text: text::page(&[], mochimo_crypto::cli::status_outcome(&t, result)),
            }
        })
        .collect()
}

/// The stake page's samples' tip: a mainnet height past the v4.1 trigger.
const STAKE_TIP: u64 = 1_143_700;

/// One MCM in nanoMCM.
const NANO: u64 = 1_000_000_000;

fn stake_tip(index: u64) -> ChainTip {
    ChainTip {
        index,
        hash: [0x6e; 32],
    }
}

/// The network's staking summary the samples show.
fn staking_stats() -> StakingStats {
    StakingStats {
        tip: stake_tip(STAKE_TIP),
        trigger: Some(crate::staking::MAINNET.t41),
        active: true,
        min_stake: 500 * NANO,
        stake_set: Some(StakeSetSummary {
            block: stake_tip(STAKE_TIP),
            count: 14,
            pool: 2_500_000_000,
            forfeit: 0,
            dust: 3,
            weight: 151_200 * u128::from(NANO),
        }),
        ledger: Some(StakingLedger {
            block: STAKE_TIP,
            stakers: 14,
            staked: 41_250 * NANO,
            weight: 151_200 * u128::from(NANO),
        }),
    }
}

/// The locked sample stake: 2,000 NANO at tier 2, committed some days ago,
/// with 3.2 NANO of reward earned.
fn locked_stake() -> StakeFields {
    let commit = 1_140_250;
    StakeFields {
        staked: 2_000 * NANO,
        source: StakeSource::Node,
        block: STAKE_TIP,
        entitlement: Some(3_200_000_000),
        total: Some(1_180 * NANO + 2_000 * NANO + 3_200_000_000),
        terms: Some(StakeTerms {
            commit,
            expiry: mochimo_crypto::staking::expiry(commit, 2).unwrap_or(commit),
            locked: true,
            tier: Some(2),
            coefficient: Some(3),
            weight: Some(u128::from(2_000 * NANO + 3_200_000_000) * 3),
        }),
        consent: None,
    }
}

/// The stake page for the first account, which does not stake: its
/// spendable balance, no reward, the network's figures, and the library's
/// `stake-info` page.
#[must_use]
pub fn stake_view() -> StakeView {
    let t = tag(1);
    crate::worker::stake_view(
        AccountId::from_tag(t),
        crate::staking::MAINNET,
        STAKE_TIP + 1,
        StakeInfoPage {
            network: crate::staking::MAINNET,
            tip: Ok(stake_tip(STAKE_TIP)),
            account: Some((
                t,
                Ok(StakeAt {
                    tip: stake_tip(STAKE_TIP),
                    spendable: FIRST_BALANCE,
                    stake: Some(StakeFields {
                        staked: 0,
                        source: StakeSource::Node,
                        block: STAKE_TIP,
                        entitlement: None,
                        total: None,
                        terms: None,
                        consent: None,
                    }),
                }),
            )),
            entitlement: Some(Ok(Entitlement {
                amount: 0,
                origin: Origin::Endpoint { block: STAKE_TIP },
            })),
            stats: Ok(staking_stats()),
        },
    )
}

/// The stake page for the second account, whose stake is still locked.
#[must_use]
pub fn stake_view_locked() -> StakeView {
    let t = tag(2);
    crate::worker::stake_view(
        AccountId::from_tag(t),
        crate::staking::MAINNET,
        STAKE_TIP + 1,
        StakeInfoPage {
            network: crate::staking::MAINNET,
            tip: Ok(stake_tip(STAKE_TIP)),
            account: Some((
                t,
                Ok(StakeAt {
                    tip: stake_tip(STAKE_TIP),
                    spendable: 1_180 * NANO,
                    stake: Some(locked_stake()),
                }),
            )),
            entitlement: None,
            stats: Ok(staking_stats()),
        },
    )
}

/// The review of unstaking the locked sample stake, which forfeits its
/// reward: the worker's text, as [`crate::Command::PlanStake`] answers.
#[must_use]
pub fn stake_plan_view() -> StakePlanView {
    let network = crate::staking::MAINNET;
    crate::worker::plan_view(
        PlanId::example(),
        StakeRequest {
            account: AccountId::from_tag(tag(2)),
            action: StakeAction::Unstake {
                forfeit: true,
                lock_ends: None,
            },
        },
        &DustProofOp::Unstake {
            network: network.id,
        },
        &StakingContext {
            network,
            height: STAKE_TIP + 1,
            spendable: 1_180 * NANO,
            stake: Some(locked_stake()),
            entitlement: None,
        },
        crate::staking::FEE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_explorer_samples_carry_the_librarys_pages() {
        use crate::explorer::Direction;
        let histories = activity();
        assert_eq!(histories.len(), 3);
        let first = AccountId::from_tag(tag(1));
        let batch = &histories[0].transactions[0];
        assert_eq!(batch.direction(first), Direction::Both);
        assert_eq!(
            batch.net(first),
            -(800 * MCM + 2 * i128::from(mochimo_crypto::consts::MFEE))
        );
        assert_eq!(batch.paid_out(first).count(), 2);
        assert!(histories[2].transactions[0].rewards(AccountId::from_tag(tag(3))));
        assert!(
            histories
                .iter()
                .all(|h| h.text.contains("recent transactions"))
        );

        let b = blocks();
        assert_eq!(b.blocks.len(), 6);
        assert_eq!(b.blocks[0].index, b.tip);
        assert_eq!(b.blocks[0].time_ms, TIP_MS);

        let c = chain();
        assert_eq!(c.blocks.len(), 10);
        assert_eq!(c.tip, b.tip, "the explorer's chain is the network card's");
        assert_eq!(c.blocks[0].solve_ms, Some(66_000));
        assert_eq!(c.blocks[9].solve_ms, Some(82_000), "read one further");
        assert!(c.blocks[2].reward.is_none(), "871,171 is a pseudo-block");
        assert!(c.text.contains("the 10 newest"), "{}", c.text);

        let p = pending();
        assert_eq!((p.waiting, p.rows.len()), (14, 5));
        assert!(p.text.contains("14 transaction(s) waiting"), "{}", p.text);

        let block = block();
        assert_eq!(block.confirmations(), Some(2));
        assert_eq!(block.summary.solve_ms, Some(78_000));
        assert_eq!(block.spends.len(), 12);
        assert_eq!(
            block.spends[0].source(),
            Some(&crate::explorer::Party::Account(first))
        );
        assert_eq!(block.figures.as_ref().map(|f| f.haiku.len()), Some(3));
        assert!(block.text.contains("block 871172"), "{}", block.text);

        let tag_view = tag_view();
        assert_eq!(tag_view.account, first);
        assert!(tag_view.history.is_ok());

        let (tx, text) = transaction();
        assert_eq!(tx.block, Some(870_645));
        assert!(!text.is_empty());

        let r = review();
        assert_eq!(r.len(), 3);
        assert!(matches!(
            r[2].state,
            AccountState::Diverged {
                advance_to: Some(33),
                ..
            }
        ));
        assert!(r.iter().all(|a| !a.text.is_empty()));
    }

    #[test]
    fn the_wallet_samples_carry_the_librarys_pages() {
        let receive = receive_view();
        assert_eq!(receive.index, 49);
        assert!(receive.destination.is_some());
        assert!(!receive.text.is_empty());

        let plan = plan_view();
        assert_eq!(plan.destinations.len(), 2);
        assert_eq!(
            plan.send_total + plan.fee_total + plan.change_total,
            plan.balance
        );
        assert!(
            plan.text.contains("THIS STORE IS NOT WHOLE"),
            "{}",
            plan.text
        );

        let sent = sent_view();
        assert!(sent.submitted);
        assert!(
            sent.text.contains(&sent.artifact_hex),
            "the page carries the artifact"
        );
        assert!(matches!(
            sent.view.accounts[0].state,
            AccountState::SpendOutstanding { .. }
        ));

        let (text, rows) = discovered();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows.iter().filter(|r| r.held).count(), 2);
        assert!(!text.is_empty());
        assert!(!submitted_text().is_empty());
    }

    #[test]
    fn the_sample_carries_the_librarys_words() {
        let view = wallet_view();
        assert_eq!(view.accounts.len(), 3);
        let notice = view.notice.expect("a store that is not whole says so");
        assert!(notice.starts_with("THIS STORE IS NOT WHOLE"), "{notice}");
        assert!(
            matches!(
                &view.accounts[2].state,
                AccountState::Diverged {
                    advance_to: Some(33),
                    ..
                }
            ),
            "{:?}",
            view.accounts[2].state
        );
    }
}
