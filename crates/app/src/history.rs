// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Activity's rows: what the node's index holds for each of the store's
//! accounts, one row per transaction, and what each did to those accounts.
//!
//! A transaction's amounts come from wallet-core (`TransactionView::net` and
//! `direction`, the command line's sums). This module classifies it as sent,
//! received, between the store's own accounts, or a mining reward. A
//! reference is the one the index's row carries on a destination, and a spend
//! still settling comes from the store's own record, with no amount, never
//! from the index.

use tawara_wallet_core::explorer::{
    AccountHistory, Direction, OperationKind, OperationView, Party, TransactionView,
};
use tawara_wallet_core::view::{AccountId, AccountRow, AccountState};

/// What a transaction was, to the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Value left one of the store's accounts for someone else's.
    Sent,
    /// Value arrived at one of the store's accounts from someone else's.
    Received,
    /// Value moved between two of the store's accounts.
    Own,
    /// A mining reward to one of the store's accounts.
    Reward,
    /// It names the account and moved nothing either way.
    Other,
}

/// Which row Activity shows beside the list: a transaction and the
/// account it is seen from, since a payment from elsewhere to several of
/// the store's accounts is a row for each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowId {
    pub tx: String,
    pub account: AccountId,
}

/// One transaction as Activity lists it, seen from `account`, the store's
/// account it left (or reached, when it only arrived).
#[derive(Clone, Debug)]
pub struct Row<'a> {
    pub tx: &'a TransactionView,
    pub account: AccountId,
    pub kind: Kind,
    /// In nanoMCM: what it did to `account`, net of any change, for a spend
    /// to someone else or a receipt; what moved, for a transfer between the
    /// store's own accounts.
    pub amount: i128,
    /// The history it was read from, for the library's page.
    pub history: &'a AccountHistory,
}

impl Row<'_> {
    #[must_use]
    pub fn id(&self) -> RowId {
        RowId {
            tx: self.tx.id.clone(),
            account: self.account,
        }
    }

    /// Whether it is the row `id` names.
    #[must_use]
    pub fn is(&self, id: &RowId) -> bool {
        self.tx.id == id.tx && self.account == id.account
    }

    /// Where a spend went, change left out.
    pub fn payees(&self) -> impl Iterator<Item = &OperationView> {
        self.tx.paid_out(self.account)
    }

    /// Who paid.
    pub fn payers(&self) -> impl Iterator<Item = &OperationView> {
        self.tx.paid_by(self.account)
    }

    /// The references the index's row carries that are this row's to show:
    /// those on the destinations that reached the account, for a payment
    /// received; those on the destinations it paid, for anything else.
    pub fn references(&self) -> Vec<&str> {
        let to_me = |o: &&OperationView| o.party == Party::Account(self.account);
        let carried = self
            .tx
            .operations
            .iter()
            .filter(|o| o.kind == OperationKind::Destination);
        let mine: Vec<&OperationView> = if self.kind == Kind::Received {
            carried.filter(to_me).collect()
        } else {
            carried.filter(|o| !to_me(o)).collect()
        };
        mine.into_iter()
            .map(|o| o.memo.as_str())
            .filter(|m| !m.is_empty())
            .collect()
    }

    /// Whether `query` (already lowercase) is in its id, its block, a
    /// party's destination or tag, or a reference.
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let mut hay = vec![self.tx.id.clone()];
        if let Some(b) = self.tx.block {
            hay.push(b.to_string());
        }
        for op in &self.tx.operations {
            match &op.party {
                Party::Account(id) => {
                    hay.push(id.hex());
                    hay.extend(id.destination());
                }
                Party::Ledger(text) | Party::Other(text) => hay.push(text.clone()),
            }
            hay.push(op.memo.clone());
        }
        hay.iter().any(|h| h.to_lowercase().contains(query))
    }
}

fn names_own(op: &OperationView, own: &[AccountId]) -> bool {
    matches!(&op.party, Party::Account(a) if own.contains(a))
}

/// The store's account `tx` left, by its source operation.
fn left_from(tx: &TransactionView, own: &[AccountId]) -> Option<AccountId> {
    tx.operations.iter().find_map(|o| match &o.party {
        Party::Account(a) if o.kind == OperationKind::Source && own.contains(a) => Some(*a),
        _ => None,
    })
}

/// What `tx` was, seen from `account`, given the store's accounts `own`.
fn classify<'a>(
    tx: &'a TransactionView,
    account: AccountId,
    own: &[AccountId],
    history: &'a AccountHistory,
) -> Row<'a> {
    let net = tx.net(account);
    let (kind, amount) = if tx.rewards(account) {
        (Kind::Reward, net)
    } else {
        match tx.direction(account) {
            Direction::Out | Direction::Both => {
                let payees: Vec<_> = tx.paid_out(account).collect();
                if !payees.is_empty() && payees.iter().all(|o| names_own(o, own)) {
                    (Kind::Own, payees.iter().map(|o| o.amount).sum())
                } else {
                    (Kind::Sent, net)
                }
            }
            Direction::In => {
                if tx.paid_by(account).any(|o| names_own(o, own)) {
                    (Kind::Own, net)
                } else {
                    (Kind::Received, net)
                }
            }
            Direction::Neither => (Kind::Other, net),
        }
    };
    Row {
        tx,
        account,
        kind,
        amount,
        history,
    }
}

/// Every transaction in `histories`, newest first. One that left one of
/// the store's accounts is listed once, from the account it left, though
/// the histories of the store's accounts it paid hold it too: a transfer
/// between them, or a spend that paid one of them among others. One that
/// left none of them (a payment from elsewhere to several of them) is
/// listed once for each account it reached, with what it did to that
/// account and that account's references.
///
/// The account it left is read from its own operations, not from whose
/// history it was found in, so a row is the same whichever pages have been
/// read: a send from an account whose older pages are not read yet is still
/// that account's send when only its payee's page holds it.
#[must_use]
pub fn rows(histories: &[AccountHistory]) -> Vec<Row<'_>> {
    let own: Vec<AccountId> = histories.iter().map(|h| h.account).collect();
    let mut out: Vec<Row<'_>> = Vec::new();
    for history in histories {
        for tx in &history.transactions {
            let account = left_from(tx, &own).unwrap_or(history.account);
            match out
                .iter()
                .position(|r| r.tx.id == tx.id && r.account == account)
            {
                // Its library page is the one of the account it is seen
                // from, once that account's history holds it.
                Some(at) if history.account == account => out[at].history = history,
                Some(_) => {}
                None => out.push(classify(tx, account, &own, history)),
            }
        }
    }
    out.sort_by_key(|r| std::cmp::Reverse((r.tx.block, r.tx.time_ms)));
    out
}

/// Which rows Activity shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    /// Value that left an account: spends, and transfers between the
    /// store's own accounts.
    Sent,
    /// Value that arrived: receipts, rewards, and transfers between the
    /// store's own accounts.
    Received,
    /// Spends the store has reserved and not settled, from its own record.
    Pending,
}

impl Filter {
    /// Whether a row of the index belongs under it. Pending holds none:
    /// its rows are the store's, not the index's.
    #[must_use]
    pub fn admits(self, kind: Kind) -> bool {
        match self {
            Filter::All => true,
            Filter::Sent => matches!(kind, Kind::Sent | Kind::Own),
            Filter::Received => matches!(kind, Kind::Received | Kind::Reward | Kind::Own),
            Filter::Pending => false,
        }
    }

    /// Whether the store's own settling spends are listed under it.
    #[must_use]
    pub fn shows_pending(self) -> bool {
        matches!(self, Filter::All | Filter::Pending)
    }
}

/// The store's accounts with a spend reserved and not yet settled: the
/// store keeps no amount for one, so none is shown.
pub fn pending(accounts: &[AccountRow]) -> impl Iterator<Item = &AccountRow> {
    accounts.iter().filter(|a| {
        matches!(
            a.state,
            AccountState::SpendOutstanding { .. } | AccountState::SpendLanded { .. }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tawara_wallet_core::sample;

    #[test]
    fn the_sample_index_reads_as_the_store_would_say_it() {
        let histories = sample::activity();
        let rows = rows(&histories);
        // The transfer between the first two accounts is in both histories
        // and listed once.
        let total: usize = histories.iter().map(|h| h.transactions.len()).sum();
        assert_eq!(rows.len(), total - 1);
        let kinds: Vec<Kind> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds[0], Kind::Sent, "newest first: the batch send");
        assert_eq!(rows[0].payees().count(), 2);
        assert!(rows[0].amount < 0);
        assert_eq!(
            rows[0].references(),
            ["INV-0412"],
            "the payee's reference, its padding left off"
        );
        let received = rows
            .iter()
            .find(|r| r.kind == Kind::Received)
            .expect("a payment received");
        assert_eq!(received.references(), ["ORDER-77"]);
        // Seen from the other payee, who was given none, the batch shows no
        // reference: the first payee's is not theirs.
        let payee = match &rows[0].tx.operations[2].party {
            Party::Account(id) => *id,
            other => panic!("expected an account, got {other:?}"),
        };
        let seen = Row {
            tx: rows[0].tx,
            account: payee,
            kind: Kind::Received,
            amount: 0,
            history: rows[0].history,
        };
        assert!(seen.references().is_empty(), "the other payee's has none");
        assert!(kinds.contains(&Kind::Reward));
        let own = rows
            .iter()
            .find(|r| r.kind == Kind::Own)
            .expect("an own transfer");
        assert_eq!(own.amount, 500_000_000_000, "what moved, not the fee");
        assert_eq!(
            own.account, histories[1].account,
            "listed from the account it left"
        );
        for pair in rows.windows(2) {
            assert!(pair[0].tx.block >= pair[1].tx.block);
        }
    }

    /// A transaction with `ops`, as `(kind, account, amount, reference)`.
    fn tx(id: &str, block: u64, ops: &[(OperationKind, AccountId, i128, &str)]) -> TransactionView {
        TransactionView {
            id: id.to_owned(),
            block: Some(block),
            time_ms: Some(1_790_000_000_000),
            operations: ops
                .iter()
                .map(|(kind, account, amount, memo)| OperationView {
                    kind: kind.clone(),
                    party: Party::Account(*account),
                    amount: *amount,
                    memo: (*memo).to_owned(),
                })
                .collect(),
        }
    }

    fn history(account: AccountId, transactions: Vec<TransactionView>) -> AccountHistory {
        AccountHistory {
            account,
            total: transactions.len() as u64,
            next: transactions.len() as u64,
            transactions,
            text: String::new(),
        }
    }

    #[test]
    fn a_payment_from_elsewhere_to_two_accounts_is_a_row_for_each() {
        use OperationKind::{Destination, Source};
        let (a, b, c) = (
            AccountId::from_tag([0xa1; 20]),
            AccountId::from_tag([0xb2; 20]),
            AccountId::from_tag([0xc3; 20]),
        );
        let outside = AccountId::from_tag([0xee; 20]);
        // Someone else pays A 10 and B 20 in one transaction.
        let batch = tx(
            "batch",
            900,
            &[
                (Source, outside, -40, ""),
                (Destination, a, 10, "FOR-A"),
                (Destination, b, 20, "FOR-B"),
                (Destination, outside, 10, ""),
            ],
        );
        // A pays B 5, with its change: a transfer between two of them.
        let transfer = tx(
            "transfer",
            800,
            &[
                (Source, a, -20, ""),
                (Destination, b, 5, ""),
                (Destination, a, 15, ""),
            ],
        );
        // B pays C and someone else: a spend that paid one of them.
        let spend = tx(
            "spend",
            700,
            &[
                (Source, b, -30, ""),
                (Destination, c, 7, ""),
                (Destination, outside, 8, ""),
                (Destination, b, 15, ""),
            ],
        );
        let histories = [
            history(a, vec![batch.clone(), transfer.clone()]),
            history(b, vec![batch, transfer, spend.clone()]),
            history(c, vec![spend]),
        ];
        let rows = rows(&histories);
        let seen: Vec<(&str, AccountId, Kind, i128)> = rows
            .iter()
            .map(|r| (r.tx.id.as_str(), r.account, r.kind, r.amount))
            .collect();
        assert_eq!(
            seen,
            [
                ("batch", a, Kind::Received, 10),
                ("batch", b, Kind::Received, 20),
                ("transfer", a, Kind::Own, 5),
                ("spend", b, Kind::Sent, -15),
            ]
        );
        assert_eq!(rows[0].references(), ["FOR-A"]);
        assert_eq!(
            rows[1].references(),
            ["FOR-B"],
            "each account's own reference"
        );
        assert!(rows[1].is(&rows[1].id()) && !rows[0].is(&rows[1].id()));
    }

    #[test]
    fn a_send_is_its_senders_whichever_page_holds_it() {
        use OperationKind::{Destination, Fee, Source};
        let (a, b) = (
            AccountId::from_tag([0xa1; 20]),
            AccountId::from_tag([0xb2; 20]),
        );
        let outside = AccountId::from_tag([0xee; 20]);
        // A pays B 7 and someone else 8, with 14 back as change and a fee
        // of 1: A is 16 down.
        let batch = tx(
            "batch",
            900,
            &[
                (Source, a, -30, ""),
                (Destination, b, 7, ""),
                (Destination, outside, 8, ""),
                (Destination, a, 14, ""),
                (Fee, outside, 1, ""),
            ],
        );
        let seen = |histories: &[AccountHistory]| {
            rows(histories)
                .iter()
                .map(|r| {
                    (
                        r.tx.id.clone(),
                        r.account,
                        r.kind,
                        r.amount,
                        r.history.account,
                    )
                })
                .collect::<Vec<_>>()
        };
        let sent = ("batch".to_owned(), a, Kind::Sent, -16);
        // Only B's page holds it: A's copy is on a page not read yet.
        let unread = [history(a, vec![]), history(b, vec![batch.clone()])];
        assert_eq!(seen(&unread), [(sent.0.clone(), sent.1, sent.2, sent.3, b)]);
        // Both pages hold it, in either order: the same row, from A's page.
        let both = [
            history(a, vec![batch.clone()]),
            history(b, vec![batch.clone()]),
        ];
        assert_eq!(seen(&both), [(sent.0.clone(), sent.1, sent.2, sent.3, a)]);
        let reversed = [history(b, vec![batch.clone()]), history(a, vec![batch])];
        assert_eq!(seen(&reversed), [(sent.0, sent.1, sent.2, sent.3, a)]);
    }

    #[test]
    fn filters_and_search_narrow_the_rows() {
        let histories = sample::activity();
        let rows = rows(&histories);
        let count = |f: Filter| rows.iter().filter(|r| f.admits(r.kind)).count();
        assert_eq!(count(Filter::All), rows.len());
        assert_eq!(count(Filter::Pending), 0);
        assert!(count(Filter::Sent) > 0 && count(Filter::Sent) < rows.len());
        assert!(count(Filter::Received) > 0 && count(Filter::Received) < rows.len());
        let id = rows[0].tx.id[..12].to_lowercase();
        assert_eq!(rows.iter().filter(|r| r.matches(&id)).count(), 1);
        assert!(rows.iter().all(|r| r.matches("")));
        assert!(!rows[0].matches("nothing-like-this"));
        assert_eq!(pending(&sample::wallet_view().accounts).count(), 1);
    }
}
