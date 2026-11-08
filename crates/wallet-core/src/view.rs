// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! View models: what the worker reports, as plain data that holds no secret.
//!
//! Every type here is `Clone` and `Debug`, because nothing in it is
//! sensitive: tags, destinations, indexes, balances, and the library's own
//! report text. The interface reads these and never a library type.

use core::fmt;
use std::collections::BTreeMap;
use std::path::PathBuf;

use mochimo_crypto::account::AccountKind as LibraryKind;
use mochimo_crypto::addr::Tag;
use mochimo_crypto::recon::{AccountStatus, ChainPosition, Divergence, Expiry, Reservation};

/// An account's tag: the twenty bytes that name it on the ledger. Public,
/// not secret. Commands name accounts by this.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountId(Tag);

impl AccountId {
    /// The account with this tag. Any twenty bytes make an id; a command
    /// that names a tag the store does not hold is refused by the library.
    #[must_use]
    pub fn from_tag(tag: [u8; 20]) -> AccountId {
        AccountId(tag)
    }

    pub(crate) fn tag(self) -> Tag {
        self.0
    }

    /// The tag in hex with `0x`, the form the library uses when it names a
    /// tag it is not offering as somewhere to send funds.
    #[must_use]
    pub fn hex(self) -> String {
        let mut out = String::with_capacity(42);
        out.push_str("0x");
        for b in self.0 {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }

    /// The destination: Base58 over the tag and its CRC16, the form every
    /// Mochimo wallet takes. `None` only if the library cannot render it,
    /// which it documents as unreachable for a tag; a caller shows nothing
    /// rather than a second form (the library's `cli::destination` argues
    /// why).
    #[must_use]
    pub fn destination(self) -> Option<String> {
        mochimo_crypto::addr::tag_to_base58(&self.0).ok()
    }
}

impl fmt::Debug for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountId({})", self.hex())
    }
}

/// Where an account's keys come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    /// Derived from the store's master seed.
    Derived,
    /// An imported root, held in the store.
    Imported,
}

impl From<LibraryKind> for AccountKind {
    fn from(kind: LibraryKind) -> AccountKind {
        match kind {
            LibraryKind::Derived => AccountKind::Derived,
            LibraryKind::Imported => AccountKind::Imported,
            // A Mochimo SDK account: its seed is held in the store like an
            // imported root's. This application never makes one; a store the
            // command line's `import --sdk` wrote can hold one.
            LibraryKind::Sdk => AccountKind::Imported,
        }
    }
}

/// What reconciliation made of an open reservation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReservationState {
    /// The store records no figures for it (a store from before they were
    /// recorded).
    Unrecorded,
    /// It can still land.
    Live,
    /// It can no longer be accepted: the balance it was built on moved, or
    /// its block-to-live passed. Re-signing reproduces bytes the ledger will
    /// refuse.
    Dead { balance_moved: bool, expired: bool },
    /// Its expiry could not be read; the library's report says why.
    Unclassified,
}

impl From<&Reservation> for ReservationState {
    fn from(r: &Reservation) -> ReservationState {
        match r {
            Reservation::Unrecorded => ReservationState::Unrecorded,
            Reservation::Recorded(d) => {
                let expired = matches!(d.expiry, Expiry::Reached { .. });
                if d.balance_moved || expired {
                    ReservationState::Dead {
                        balance_moved: d.balance_moved,
                        expired,
                    }
                } else if matches!(d.expiry, Expiry::Unreadable { .. }) {
                    ReservationState::Unclassified
                } else {
                    ReservationState::Live
                }
            }
        }
    }
}

/// An account's state, as last observed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountState {
    /// The node holds the address this store derives at its index.
    InSync { balance: u64 },
    /// A spend is reserved and has not landed.
    SpendOutstanding {
        balance: u64,
        spent_index: u32,
        reservation: ReservationState,
    },
    /// The spend landed; settling records it and frees the account.
    SpendLanded {
        balance: u64,
        spent_index: u32,
        settled_index: u32,
    },
    /// Reconciliation could not explain the account, and no operation on it
    /// is permitted. `report` is the library's report, word for word.
    /// `advance_to` is the index an advance would move it to, when the chain
    /// holds it ahead; `rollback_to` the index a rollback would move it back
    /// to, when the chain holds it behind.
    Diverged {
        /// What was found, for the interface to say in its own words; the
        /// report says it in full.
        kind: DivergenceKind,
        report: String,
        advance_to: Option<u32>,
        rollback_to: Option<u32>,
        /// Whether the chain holds the account one key behind, at the index
        /// of a settle this store retained: the case that
        /// [`crate::Command::ResubmitSaved`] serves. The library checks its
        /// other conditions when the saved copy is sent
        /// (`recon::resubmit_reverted_settle`).
        reverted_settle: bool,
    },
    /// Nothing has asked a node about it: no node is chosen.
    NotReconciled,
}

impl AccountState {
    pub(crate) fn from_status(status: &AccountStatus) -> AccountState {
        match status {
            AccountStatus::InSync { balance, .. } => AccountState::InSync { balance: *balance },
            AccountStatus::SpendOutstanding {
                spent_index,
                balance,
                reservation,
            } => AccountState::SpendOutstanding {
                balance: *balance,
                spent_index: spent_index.get(),
                reservation: reservation.into(),
            },
            AccountStatus::SpendLanded {
                spent_index,
                settled_index,
                balance,
            } => AccountState::SpendLanded {
                balance: *balance,
                spent_index: spent_index.get(),
                settled_index: settled_index.get(),
            },
        }
    }

    pub(crate) fn from_divergence(d: &Divergence) -> AccountState {
        AccountState::Diverged {
            kind: DivergenceKind::of(d),
            report: d.to_string(),
            advance_to: d.advance_target().map(|i| i.get()),
            rollback_to: d.rollback_target().map(|i| i.get()),
            reverted_settle: matches!(
                d,
                Divergence::IndexMismatch {
                    found: ChainPosition::Behind { gap: 1, .. },
                    reverted_settle: Some(_),
                    ..
                }
            ),
        }
    }

    /// The balance last observed, when one was.
    #[must_use]
    pub fn balance(&self) -> Option<u64> {
        match self {
            AccountState::InSync { balance }
            | AccountState::SpendOutstanding { balance, .. }
            | AccountState::SpendLanded { balance, .. } => Some(*balance),
            AccountState::Diverged { .. } | AccountState::NotReconciled => None,
        }
    }

    /// Whether this state is what a read leaves when it failed transiently,
    /// rather than what the node showed: the node did not answer for the
    /// account, or the expiry of an outstanding spend could not be read. Such
    /// a read says nothing about whether the spend has landed or can still
    /// land, so the polls that wait for it carry on (see [`Watch`]).
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            AccountState::Diverged {
                kind: DivergenceKind::Unreachable,
                ..
            } | AccountState::SpendOutstanding {
                reservation: ReservationState::Unclassified,
                ..
            }
        )
    }

    /// Whether the store holds an open reservation for the account, as far as
    /// this state says. A read that reached the node and the store says: the
    /// library reads an outstanding or landed spend, and an unexplained
    /// reservation, only when one is open, and an index out of step or an
    /// account in sync only when none is. A read that failed before that point
    /// (the node did not answer, answered "account not found", or the store
    /// could not be read), and one that asked no node, do not say, so `None`.
    /// [`AccountRow::set_state`] keeps the row's last word in that case, which
    /// is what lets the polls start for an account whose first read failed.
    #[must_use]
    pub fn reservation_held(&self) -> Option<bool> {
        match self {
            AccountState::SpendOutstanding { .. }
            | AccountState::SpendLanded { .. }
            | AccountState::Diverged {
                kind: DivergenceKind::ReservationUnexplained,
                ..
            } => Some(true),
            AccountState::InSync { .. }
            | AccountState::Diverged {
                kind:
                    DivergenceKind::Ahead { .. }
                    | DivergenceKind::Behind { .. }
                    | DivergenceKind::Unlocated,
                ..
            } => Some(false),
            AccountState::Diverged {
                kind:
                    DivergenceKind::NotFound
                    | DivergenceKind::Unreachable
                    | DivergenceKind::NoMaster
                    | DivergenceKind::Failed,
                ..
            }
            | AccountState::NotReconciled => None,
        }
    }

    /// The one-time key index of the reservation the state says is open: an
    /// outstanding or a landed spend's `spent_index`, the index that spend was
    /// signed with. `None` for a state that does not say which reservation it
    /// holds, as an unexplained one does not.
    #[must_use]
    pub fn reserved_index(&self) -> Option<u32> {
        match self {
            AccountState::SpendOutstanding { spent_index, .. }
            | AccountState::SpendLanded { spent_index, .. } => Some(*spent_index),
            AccountState::InSync { .. }
            | AccountState::Diverged { .. }
            | AccountState::NotReconciled => None,
        }
    }
}

/// What reconciliation found for an account it could not explain, as the
/// library tells the cases apart, so the interface can summarize the report
/// in its own words and name the causes that fit.
/// The report is the library's, whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DivergenceKind {
    /// The chain holds the account at this seed's key `gap` places ahead of
    /// the store's index: a spend landed that the store does not record.
    Ahead { gap: u32 },
    /// The chain holds it at this seed's key `gap` places behind the
    /// store's index.
    Behind { gap: u32 },
    /// None of the keys the search around the store's index reached is the
    /// address the chain holds.
    Unlocated,
    /// A spend is reserved, and the chain holds the account at neither the
    /// key that signed nor the change key.
    ReservationUnexplained,
    /// The node answered "account not found": no entry, a zero balance, or
    /// a lookup that failed, and it does not say which.
    NotFound,
    /// The node could not be reached for it.
    Unreachable,
    /// A derived account, and no master seed to derive its keys from.
    NoMaster,
    /// Reconciling it failed: a store error, an answer that did not parse.
    Failed,
}

impl DivergenceKind {
    fn of(d: &Divergence) -> DivergenceKind {
        let position = |found: &ChainPosition| match found {
            ChainPosition::Ahead { gap, .. } => DivergenceKind::Ahead { gap: *gap },
            ChainPosition::Behind { gap, .. } => DivergenceKind::Behind { gap: *gap },
            ChainPosition::Unlocated { .. } => DivergenceKind::Unlocated,
        };
        match d {
            Divergence::IndexMismatch { found, .. } => position(found),
            Divergence::ReservationUnexplained { .. } => DivergenceKind::ReservationUnexplained,
            Divergence::TagUnresolved { .. } => DivergenceKind::NotFound,
            Divergence::ChainUnreachable { .. } => DivergenceKind::Unreachable,
            Divergence::NoMasterForDerivedAccount { .. } => DivergenceKind::NoMaster,
            Divergence::CannotReconcile { .. } => DivergenceKind::Failed,
        }
    }
}

/// How many polls in a row a [`Watch`] sends about an account whose reads
/// leave its spend in doubt (see [`Watch`]): two, so that the third such read
/// in a row ends the polls. A doubtful read may be a failed lookup that the
/// next minute mends, so one is not enough to stop; but an account spent to
/// zero reads "account not found" every time, and a settle the library keeps
/// refusing leaves a landed spend every time, so the polls must end on their
/// own.
pub const DOUBTFUL_POLLS: u8 = 2;

/// What one read of an account means for its polls ([`Watch`]).
#[derive(Clone, Copy)]
enum Read {
    /// The reservation is live: the account is polled.
    Live,
    /// The read failed and says nothing: the node did not answer, or the
    /// expiry could not be read. The polls go on as they were.
    Silent,
    /// The read leaves the spend in doubt: the node answered "account not
    /// found", which the Mesh also answers for a lookup that failed (the
    /// library's `recon` module, fact 3), or the spend landed and the
    /// automatic settle did not make it. Polled again, a bounded number of
    /// times ([`DOUBTFUL_POLLS`]).
    Doubtful,
    /// No node was asked: nothing changes.
    Unasked,
    /// The read settles the question: the account is in sync, its
    /// reservation is dead or unrecorded, or it diverged in a way that is for
    /// the person to look at. The polls end until a live read.
    Ended,
}

impl Read {
    fn of(state: &AccountState) -> Read {
        match state {
            AccountState::SpendOutstanding {
                reservation: ReservationState::Live,
                ..
            } => Read::Live,
            _ if state.is_transient() => Read::Silent,
            AccountState::Diverged {
                kind: DivergenceKind::NotFound,
                ..
            }
            | AccountState::SpendLanded { .. } => Read::Doubtful,
            AccountState::NotReconciled => Read::Unasked,
            _ => Read::Ended,
        }
    }
}

/// Where one account's polls stand ([`Watch`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Polls {
    /// Polled. `sent` is the number of polls sent about it while its reads
    /// left the spend in doubt, since its last live read or sign
    /// ([`Watch::signed`]); `doubting` is whether its latest read that asked
    /// a node and said something was in doubt. A silent read changes neither,
    /// so a poll sent after a silent answer within a run in doubt counts: a
    /// poll counts when it is sent.
    Asking { sent: u8, doubting: bool },
    /// A read ended its polls, and only a live read starts them again.
    Ended,
}

/// The accounts an answer reads afresh, so that the polls of a [`Watch`] follow
/// those reads alone ([`Watch::read_answer`]). A check or a poll reads its
/// account; the read after a send reads the account the spend came from, and a
/// settle the account it settled; an opened or refreshed store reads every
/// account. The rows an answer carries for the other accounts are the store
/// as it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reads {
    /// No account: the answer carries the store as it was.
    Nothing,
    /// One account, read on its own.
    One(AccountId),
    /// Every account of the store.
    Every,
}

impl Reads {
    fn covers(self, account: AccountId) -> bool {
        match self {
            Reads::Nothing => false,
            Reads::One(read) => read == account,
            Reads::Every => true,
        }
    }
}

/// The accounts a poll asks the node about, as the reads so far say. Polling
/// continues through an outage, so that the polls that settle a spend do not
/// stop:
///
/// - An account is asked about while its row shows a live reservation.
/// - A spend signed for an account, by a sign or a re-sign, starts its polls
///   afresh, whatever they were ([`Watch::signed`]). The reads the signing's
///   reply carries are noted after that, and the rules below decide the polls
///   from them.
/// - It is still asked about while its reads fail and say nothing (the node
///   did not answer, or the expiry could not be read; see
///   [`AccountState::is_transient`]), once a read showed its reservation live,
///   or when the store holds a reservation for it ([`AccountRow::reserved`]):
///   a wallet opened during an outage has never read the spend live.
/// - It is asked about while its reads leave the spend in doubt (the node
///   answered "account not found", or the spend landed and the automatic
///   settle did not make it), in the same cases, but for at most
///   [`DOUBTFUL_POLLS`] polls in a row: the third doubtful read ends them.
/// - Any other read ends its polls until a live read: the account settled,
///   the reservation is dead or unrecorded, or the account diverged in a way
///   that is for the person to look at. A read that asked no node changes
///   nothing.
/// - A reply notes only the reads it carries ([`Reads`]). A row it carries for
///   any other account is the store as it was, and moves nothing: a doubtful
///   read ends the polls only as that account's own read.
/// - A lock forgets every account ([`Watch::clear`]), and a view forgets the
///   accounts it does not hold.
///
/// A poll is never the person's input, so nothing here restarts the idle
/// period.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watch {
    accounts: BTreeMap<AccountId, Polls>,
}

impl Watch {
    /// Whether a poll asks the node about `account`, whose row shows `state`.
    #[must_use]
    pub fn asks(&self, account: AccountId, state: &AccountState) -> bool {
        match Read::of(state) {
            Read::Live => true,
            Read::Silent => matches!(self.accounts.get(&account), Some(Polls::Asking { .. })),
            Read::Doubtful => matches!(
                self.accounts.get(&account),
                Some(Polls::Asking { sent, .. }) if *sent < DOUBTFUL_POLLS
            ),
            Read::Unasked | Read::Ended => false,
        }
    }

    /// Notes a read of `account` that showed `state`, as a check or a poll
    /// answers it, which does not say whether the store holds a reservation
    /// (see [`Watch::read_row`]).
    pub fn read(&mut self, account: AccountId, state: &AccountState) {
        self.note(account, state, false);
    }

    /// Notes the read a row shows, with what the store holds for it: a row
    /// whose store holds a reservation starts the polls on a failed read, as
    /// a live read does.
    pub fn read_row(&mut self, row: &AccountRow) {
        self.note(row.id, &row.state, row.reserved);
    }

    fn note(&mut self, account: AccountId, state: &AccountState, reserved: bool) {
        // The same read noted again leaves the count as it was, so only polls
        // advance it.
        let read = Read::of(state);
        match (read, self.accounts.get(&account).copied()) {
            (Read::Live, _) => {
                self.accounts.insert(
                    account,
                    Polls::Asking {
                        sent: 0,
                        doubting: false,
                    },
                );
            }
            // A doubtful read once the polls in doubt are spent ends them, so a
            // silent read after it asks nothing either.
            (Read::Doubtful, Some(Polls::Asking { sent, .. })) if sent >= DOUBTFUL_POLLS => {
                self.accounts.insert(account, Polls::Ended);
            }
            // A doubtful read within the polls in doubt opens a run in doubt,
            // or goes on with it: the polls sent from now count.
            (Read::Doubtful, Some(Polls::Asking { sent, .. })) => {
                self.accounts.insert(
                    account,
                    Polls::Asking {
                        sent,
                        doubting: true,
                    },
                );
            }
            (Read::Silent | Read::Doubtful, None)
                if reserved || state.reservation_held() == Some(true) =>
            {
                self.accounts.insert(
                    account,
                    Polls::Asking {
                        sent: 0,
                        doubting: matches!(read, Read::Doubtful),
                    },
                );
            }
            (Read::Ended, _) => {
                self.accounts.insert(account, Polls::Ended);
            }
            (Read::Silent | Read::Doubtful | Read::Unasked, _) => {}
        }
    }

    /// Notes that a poll asked the node about `account`, whose row showed
    /// `state`: a poll sent while the spend is in doubt counts towards
    /// [`DOUBTFUL_POLLS`]. It is in doubt from a doubtful read until a live
    /// one: a silent answer between them leaves it in doubt, so the next poll
    /// counts too. A silent read neither counts nor restarts the count, and a
    /// poll counts when it is sent.
    pub fn asked(&mut self, account: AccountId, state: &AccountState) {
        if let Some(Polls::Asking { sent, doubting }) = self.accounts.get_mut(&account)
            && (*doubting || matches!(Read::of(state), Read::Doubtful))
        {
            *sent = sent.saturating_add(1);
        }
    }

    /// Notes the reads an answer carries, as `reads` names them (see
    /// [`Reads`] and [`Watch::read_row`]), and forgets the accounts `view`
    /// does not hold. A row the answer does not read is the store as it was,
    /// not a read: noting it again could end another account's watch before
    /// that account's own answer has come.
    pub fn read_answer(&mut self, view: &WalletView, reads: Reads) {
        self.accounts
            .retain(|id, _| view.accounts.iter().any(|row| row.id == *id));
        for row in view.accounts.iter().filter(|row| reads.covers(row.id)) {
            self.read_row(row);
        }
    }

    /// Notes that a spend was signed for `account`, by a sign or a re-sign:
    /// the account holds a reservation from now on, so its polls start
    /// afresh, with no doubtful poll counted. Without this, an account an
    /// unlock found in sync would go unpolled through a first read that
    /// failed, and through a settle the node interrupted. The reads the reply
    /// carries are noted after this ([`Watch::read_answer`]), so a read that
    /// settles the spend or shows it dead still ends the polls.
    pub fn signed(&mut self, account: AccountId) {
        self.accounts.insert(
            account,
            Polls::Asking {
                sent: 0,
                doubting: false,
            },
        );
    }

    /// Forgets every account: the wallet locked, so its next reads start over.
    pub fn clear(&mut self) {
        self.accounts.clear();
    }
}

/// What a store's notice is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    /// No node is chosen (the worker's words).
    NoNode,
    /// The node was changed and nothing is reconciled against the new one
    /// yet (the worker's words).
    NodeChanged,
    /// The node did not answer (the worker's words, then the library's).
    NodeSilent,
    /// Reconciling was cancelled (the worker's words).
    Cancelled,
    /// The node could not be used (a refusal, in the library's or the
    /// worker's words).
    NodeRefused,
    /// Some accounts could not be reconciled and are set aside: the
    /// library's "THIS STORE IS NOT WHOLE". Their rows say how.
    NotWhole,
    /// The library would not open the wallet: its "WALLET WILL NOT START".
    WillNotStart,
}

/// The store's notice: what it is about, and its text, whole. It reads as
/// its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

impl core::ops::Deref for Notice {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// One account in the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRow {
    pub id: AccountId,
    pub kind: AccountKind,
    /// The one-time key index the store holds for it.
    pub index: u32,
    pub state: AccountState,
    /// Whether a spend may be planned from it now: the wallet opened, the
    /// account reconciled when it did, and it is in sync.
    pub spendable: bool,
    /// A derived account's number, as the command line's `restore
    /// --account N` names it, found by deriving from the store's seed
    /// (`crate::worker`, "Account numbers"). `None` for an imported
    /// account, for a store without its seed, and for a derived account
    /// not found within [`crate::DISCOVER_MAX_TO`].
    pub number: Option<u32>,
    /// Whether the store holds an open reservation for it (the library's
    /// `AccountView::pending`), as the store said when the row was made and as
    /// each read since has said ([`AccountRow::set_state`]). A read that failed
    /// does not say whether a spend is outstanding, and this does, so that the
    /// polls carry on for a wallet opened during an outage (see [`Watch`]).
    pub reserved: bool,
}

impl AccountRow {
    /// Whether this is a new account: the node answered "account not found",
    /// the store has never signed with it (its key index is still the first)
    /// and holds no reservation for it. That is what an account looks like
    /// until it is first paid, so the interface says "New Account" and counts
    /// it as holding nothing rather than reporting it as paused. An account
    /// the store has signed from is never new: not found, it may be emptied,
    /// or the node may be wrong.
    #[must_use]
    pub fn is_new(&self) -> bool {
        self.index == 0
            && !self.reserved
            && matches!(
                self.state,
                AccountState::Diverged {
                    kind: DivergenceKind::NotFound,
                    ..
                }
            )
    }

    /// The row as a read left it: its state, and whether a reservation is
    /// held when the state says (see [`AccountState::reservation_held`]). A
    /// state that does not say leaves `reserved` as the last word was.
    pub fn set_state(&mut self, state: AccountState) {
        if let Some(held) = state.reservation_held() {
            self.reserved = held;
        }
        self.state = state;
    }
}

/// The unlocked store, as the interface shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalletView {
    /// Where the store is.
    pub dir: PathBuf,
    pub accounts: Vec<AccountRow>,
    /// Whether the store opened as a wallet: every account reconciled
    /// against the node or was set aside as diverged, and spending is
    /// possible from the ones that reconciled. `false` when the wallet could
    /// not open (no node, an unreachable node, no account on the ledger yet,
    /// or every account diverged); the store is still open and its
    /// addresses can be shown.
    pub opened: bool,
    /// What the store as a whole needs said, when anything does: the
    /// library's notice that it is not whole, why the wallet would not open,
    /// or the worker's word that no node is chosen or answered. The
    /// interface says it in its own words and shows the text whole on
    /// request.
    pub notice: Option<Notice>,
    /// The accounts the worker settled on its own in the command that
    /// answered with this view: a spend that had landed, which a read the
    /// person asked for showed. Empty when nothing was settled, and for the
    /// person's own Settle, whose reply says what it did. The interface shows
    /// its banner from this list rather than guessing from a change of state.
    pub settled: Vec<SettledAccount>,
}

/// An account the worker settled on its own, with the library's page for the
/// settlement: the page the command line's `settle` prints, which the
/// interface shows on request from its banner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettledAccount {
    pub account: AccountId,
    pub text: String,
}

/// What can be said about a store's total balance. An account that diverged
/// or was never reconciled has no known balance, so the sum of the others is
/// not the store's total and is never offered as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Total {
    /// Every account's balance is known, and this is their sum.
    Whole(u128),
    /// Only some are: `known` is the sum of those, and `unknown` accounts
    /// are left out of it.
    Partial { known: u128, unknown: usize },
    /// No account's balance is known.
    Unknown,
    /// Every account is new ([`AccountRow::is_new`]): the wallet has not
    /// yet received any coins.
    NothingReceived,
}

impl WalletView {
    /// The store's total balance, as far as it is known.
    #[must_use]
    pub fn total(&self) -> Total {
        let mut known = 0u128;
        let mut unknown = 0;
        let mut new = 0;
        for account in &self.accounts {
            match account.state.balance() {
                Some(balance) => known += u128::from(balance),
                // A new account holds nothing yet.
                None if account.is_new() => new += 1,
                None => unknown += 1,
            }
        }
        if new > 0 && new == self.accounts.len() {
            Total::NothingReceived
        } else if unknown == 0 {
            Total::Whole(known)
        } else if unknown == self.accounts.len() {
            Total::Unknown
        } else {
            Total::Partial { known, unknown }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_debug_show_the_tag() {
        let id = AccountId::from_tag([0xab; 20]);
        assert_eq!(id.hex(), format!("0x{}", "ab".repeat(20)));
        assert_eq!(
            format!("{id:?}"),
            format!("AccountId(0x{})", "ab".repeat(20))
        );
    }

    #[test]
    fn destination_is_base58_with_checksum() {
        let id = AccountId::from_tag([0x05; 20]);
        let d = id.destination().expect("renders");
        assert_eq!(
            mochimo_crypto::addr::tag_from_base58(&d).ok(),
            Some([0x05; 20])
        );
    }

    #[test]
    fn a_total_never_counts_an_unknown_balance_as_zero() {
        let row = |state| AccountRow {
            id: AccountId::from_tag([1; 20]),
            kind: AccountKind::Derived,
            index: 0,
            state,
            spendable: false,
            number: Some(0),
            reserved: false,
        };
        let view = WalletView {
            dir: PathBuf::new(),
            accounts: vec![
                row(AccountState::InSync { balance: u64::MAX }),
                row(AccountState::SpendLanded {
                    balance: 5,
                    spent_index: 1,
                    settled_index: 2,
                }),
                row(AccountState::NotReconciled),
            ],
            opened: false,
            notice: None,
            settled: Vec::new(),
        };
        // One account has no known balance: the sum of the others is not
        // the total.
        assert_eq!(
            view.total(),
            Total::Partial {
                known: u128::from(u64::MAX) + 5,
                unknown: 1,
            }
        );
        let mut whole = view.clone();
        whole.accounts.pop();
        assert_eq!(whole.total(), Total::Whole(u128::from(u64::MAX) + 5));
        let mut none = view.clone();
        none.accounts.retain(|a| a.state.balance().is_none());
        assert_eq!(none.total(), Total::Unknown);

        // A new account holds nothing yet: counted as zero beside
        // others, and a wallet of new accounts alone has received nothing.
        let unfound = || AccountState::Diverged {
            kind: DivergenceKind::NotFound,
            report: String::new(),
            advance_to: None,
            rollback_to: None,
            reverted_settle: false,
        };
        let mut fresh = whole.clone();
        fresh.accounts.push(row(unfound()));
        assert!(fresh.accounts[2].is_new());
        assert_eq!(fresh.total(), Total::Whole(u128::from(u64::MAX) + 5));
        let mut empty = view.clone();
        empty.accounts = vec![row(unfound()), row(unfound())];
        assert_eq!(empty.total(), Total::NothingReceived);
        // Not found after the store signed from it, or with a reservation:
        // not new, and still unknown.
        let mut spent = row(unfound());
        spent.index = 3;
        assert!(!spent.is_new());
        let mut held = row(unfound());
        held.reserved = true;
        assert!(!held.is_new());
        empty.accounts = vec![spent, held];
        assert_eq!(empty.total(), Total::Unknown);
    }

    fn outstanding(reservation: ReservationState) -> AccountState {
        AccountState::SpendOutstanding {
            balance: 1,
            spent_index: 2,
            reservation,
        }
    }

    fn diverged(kind: DivergenceKind) -> AccountState {
        AccountState::Diverged {
            kind,
            report: "the library's report".to_owned(),
            advance_to: None,
            rollback_to: None,
            reverted_settle: false,
        }
    }

    fn landed() -> AccountState {
        AccountState::SpendLanded {
            balance: 1,
            spent_index: 2,
            settled_index: 3,
        }
    }

    #[test]
    fn a_watch_asks_through_transient_reads_after_a_live_one_and_stops_on_any_other() {
        let id = AccountId::from_tag([7; 20]);
        let live = outstanding(ReservationState::Live);
        let silent = diverged(DivergenceKind::Unreachable);
        let unclassified = outstanding(ReservationState::Unclassified);
        // A live reservation is asked about whatever the reads said before.
        assert!(Watch::default().asks(id, &live));
        // Nothing is asked about a failed read before a read is noted.
        let mut watch = Watch::default();
        assert!(!watch.asks(id, &silent));
        assert!(!watch.asks(id, &unclassified));
        // A node that did not answer does not say a spend is outstanding, so
        // that read alone starts nothing.
        watch.read(id, &silent);
        assert!(!watch.asks(id, &silent));
        // A live read asks, and each transient read after it keeps asking, for
        // as long as they last: they are not counted.
        watch.read(id, &live);
        for _ in 0..5 {
            watch.read(id, &silent);
            assert!(watch.asks(id, &silent));
            watch.asked(id, &silent);
            watch.read(id, &unclassified);
            assert!(watch.asks(id, &unclassified));
            watch.asked(id, &unclassified);
        }
        // A read that asked no node says nothing either way.
        watch.read(id, &AccountState::NotReconciled);
        assert!(watch.asks(id, &silent));
        // Any other read ends the polls, and a transient read after it does
        // not bring them back, even on a row whose store holds a reservation.
        let others = [
            outstanding(ReservationState::Dead {
                balance_moved: true,
                expired: false,
            }),
            outstanding(ReservationState::Dead {
                balance_moved: false,
                expired: true,
            }),
            outstanding(ReservationState::Unrecorded),
            AccountState::InSync { balance: 1 },
            diverged(DivergenceKind::ReservationUnexplained),
            diverged(DivergenceKind::Ahead { gap: 1 }),
            diverged(DivergenceKind::Behind { gap: 1 }),
            diverged(DivergenceKind::Unlocated),
            diverged(DivergenceKind::NoMaster),
            diverged(DivergenceKind::Failed),
        ];
        for other in others {
            let mut watch = Watch::default();
            watch.read(id, &live);
            watch.read(id, &other);
            assert!(!watch.asks(id, &other), "{other:?}");
            for after in [&silent, &unclassified] {
                let mut row = AccountRow {
                    id,
                    kind: AccountKind::Derived,
                    index: 0,
                    state: after.clone(),
                    spendable: false,
                    number: None,
                    reserved: true,
                };
                watch.read_row(&row);
                assert!(!watch.asks(id, after), "{other:?}, then {after:?}");
                row.state = landed();
                watch.read_row(&row);
                assert!(!watch.asks(id, &landed()), "{other:?}, then landed");
            }
            // A live read starts them again.
            watch.read(id, &live);
            watch.read(id, &silent);
            assert!(watch.asks(id, &silent), "{other:?}, then live");
        }
    }

    #[test]
    fn a_watch_asks_about_a_doubtful_read_twice_in_a_row_and_the_third_ends_it() {
        let id = AccountId::from_tag([7; 20]);
        let live = outstanding(ReservationState::Live);
        let silent = diverged(DivergenceKind::Unreachable);
        for doubtful in [diverged(DivergenceKind::NotFound), landed()] {
            let mut watch = Watch::default();
            watch.read(id, &live);
            // One doubtful read and then a live one: the count starts over.
            watch.read(id, &doubtful);
            assert!(watch.asks(id, &doubtful), "{doubtful:?}");
            watch.asked(id, &doubtful);
            watch.read(id, &live);
            // Two polls are sent while the reads stay in doubt, and noting the
            // same read again counts nothing.
            for n in 0..DOUBTFUL_POLLS {
                watch.read(id, &doubtful);
                watch.read(id, &doubtful);
                assert!(watch.asks(id, &doubtful), "{doubtful:?}, poll {n}");
                watch.asked(id, &doubtful);
                // A node that does not answer between them neither counts nor
                // starts the count over.
                watch.read(id, &silent);
                assert!(watch.asks(id, &silent), "{doubtful:?}, poll {n}");
            }
            // The third doubtful read in a row asks nothing, nor does a silent
            // one after it.
            watch.read(id, &doubtful);
            assert!(!watch.asks(id, &doubtful), "{doubtful:?}");
            watch.read(id, &silent);
            assert!(!watch.asks(id, &silent), "{doubtful:?}");
            // A live read starts the polls again.
            watch.read(id, &live);
            watch.read(id, &doubtful);
            assert!(watch.asks(id, &doubtful), "{doubtful:?}");
        }
    }

    #[test]
    fn a_watch_starts_on_a_failed_first_read_when_a_reservation_is_held() {
        let id = AccountId::from_tag([7; 20]);
        let row = |state: AccountState, reserved| AccountRow {
            id,
            kind: AccountKind::Derived,
            index: 0,
            state,
            spendable: false,
            number: None,
            reserved,
        };
        let failed = [
            diverged(DivergenceKind::Unreachable),
            diverged(DivergenceKind::NotFound),
        ];
        // A failed lookup on a row whose store holds a reservation is polled,
        // as a wallet opened during an outage shows it.
        for state in &failed {
            let mut watch = Watch::default();
            watch.read_row(&row(state.clone(), true));
            assert!(watch.asks(id, state), "{state:?}");
            // One whose store holds none is not.
            let mut watch = Watch::default();
            watch.read_row(&row(state.clone(), false));
            assert!(!watch.asks(id, state), "{state:?}");
        }
        // An unreadable expiry and a landed spend each say that a reservation
        // is held, so either starts the polls on its own.
        for state in [outstanding(ReservationState::Unclassified), landed()] {
            let mut watch = Watch::default();
            watch.read(id, &state);
            assert!(watch.asks(id, &state), "{state:?}");
        }
    }

    #[test]
    fn a_row_keeps_its_reservation_through_a_read_that_does_not_say() {
        let mut row = AccountRow {
            id: AccountId::from_tag([7; 20]),
            kind: AccountKind::Derived,
            index: 0,
            state: AccountState::NotReconciled,
            spendable: false,
            number: None,
            reserved: true,
        };
        for state in [
            diverged(DivergenceKind::Unreachable),
            diverged(DivergenceKind::NotFound),
            diverged(DivergenceKind::NoMaster),
            diverged(DivergenceKind::Failed),
            AccountState::NotReconciled,
        ] {
            row.set_state(state.clone());
            assert!(row.reserved, "{state:?}");
            assert_eq!(row.state, state);
        }
        for (state, held) in [
            (AccountState::InSync { balance: 1 }, false),
            (outstanding(ReservationState::Live), true),
            (diverged(DivergenceKind::Ahead { gap: 1 }), false),
            (landed(), true),
            (diverged(DivergenceKind::Unlocated), false),
            (diverged(DivergenceKind::ReservationUnexplained), true),
        ] {
            row.set_state(state.clone());
            assert_eq!(row.reserved, held, "{state:?}");
        }
    }

    #[test]
    fn a_watch_reads_each_row_of_a_view_and_forgets_what_the_view_leaves_out_or_a_lock() {
        let row = |n: u8, state| AccountRow {
            id: AccountId::from_tag([n; 20]),
            kind: AccountKind::Derived,
            index: 0,
            state,
            spendable: false,
            number: None,
            reserved: false,
        };
        let live = outstanding(ReservationState::Live);
        let silent = diverged(DivergenceKind::Unreachable);
        let (one, two) = (AccountId::from_tag([1; 20]), AccountId::from_tag([2; 20]));
        let mut view = WalletView {
            dir: PathBuf::new(),
            accounts: vec![row(1, live.clone()), row(2, live.clone())],
            opened: true,
            notice: None,
            settled: Vec::new(),
        };
        let mut watch = Watch::default();
        watch.read_answer(&view, Reads::Every);
        assert!(watch.asks(one, &silent));
        assert!(watch.asks(two, &silent));
        // A view that leaves an account out forgets it, so a later silent read
        // of it does not ask.
        view.accounts.retain(|r| r.id == one);
        watch.read_answer(&view, Reads::Every);
        assert!(watch.asks(one, &silent));
        view.accounts = vec![row(2, silent.clone())];
        watch.read_answer(&view, Reads::Every);
        assert!(!watch.asks(one, &silent));
        assert!(!watch.asks(two, &silent));
        // A lock forgets every account.
        watch.read(one, &live);
        watch.clear();
        assert!(!watch.asks(one, &silent));
    }

    #[test]
    fn a_spend_signed_starts_its_accounts_polls_whatever_they_were() {
        let id = AccountId::from_tag([7; 20]);
        let silent = diverged(DivergenceKind::Unreachable);
        let unclassified = outstanding(ReservationState::Unclassified);
        let live = outstanding(ReservationState::Live);
        // An unlock that finds the account in sync ends its polls, and a failed
        // read after it does not start them.
        let mut watch = Watch::default();
        watch.read(id, &AccountState::InSync { balance: 1 });
        watch.read(id, &unclassified);
        assert!(!watch.asks(id, &unclassified));
        // Signed: the spend's first read fails, and the polls go on through the
        // outage and the recovery.
        watch.signed(id);
        watch.read(id, &unclassified);
        assert!(watch.asks(id, &unclassified));
        watch.asked(id, &unclassified);
        watch.read(id, &silent);
        assert!(watch.asks(id, &silent));
        watch.read(id, &live);
        assert!(watch.asks(id, &live));
        // A spend in doubt whose polls are spent is asked about again once it
        // is signed, from a fresh count.
        let spent_in_doubt = landed();
        watch.read(id, &spent_in_doubt);
        for _ in 0..DOUBTFUL_POLLS {
            assert!(watch.asks(id, &spent_in_doubt));
            watch.asked(id, &spent_in_doubt);
        }
        watch.read(id, &spent_in_doubt);
        assert!(!watch.asks(id, &spent_in_doubt));
        watch.signed(id);
        assert!(watch.asks(id, &spent_in_doubt));
    }

    #[test]
    fn a_poll_sent_after_a_silent_answer_in_doubt_counts() {
        // A silent read neither counts nor restarts the count, and a poll
        // counts when it is sent. A poll sent after a silent answer, within a
        // run of doubtful reads, is one of the two that follow the first
        // doubtful read.
        let id = AccountId::from_tag([7; 20]);
        let live = outstanding(ReservationState::Live);
        let silent = diverged(DivergenceKind::Unreachable);
        let doubtful = diverged(DivergenceKind::NotFound);
        let mut watch = Watch::default();
        watch.read(id, &live);
        watch.read(id, &doubtful);
        assert!(watch.asks(id, &doubtful));
        watch.asked(id, &doubtful);
        watch.read(id, &silent);
        assert!(watch.asks(id, &silent), "a silent answer keeps the polls");
        watch.asked(id, &silent);
        // Two polls have followed the doubtful read: the next doubtful read
        // ends them.
        watch.read(id, &doubtful);
        assert!(
            !watch.asks(id, &doubtful),
            "the poll after the silent answer counted"
        );

        // After a live read, silent answers count nothing, however many.
        watch.read(id, &live);
        for _ in 0..5 {
            watch.read(id, &silent);
            assert!(watch.asks(id, &silent));
            watch.asked(id, &silent);
        }
        watch.read(id, &doubtful);
        assert!(watch.asks(id, &doubtful), "no poll was sent in doubt");
    }

    #[test]
    fn a_reply_notes_only_the_row_it_reads_so_a_watch_ends_on_its_own_account() {
        // Two accounts are polled; B is on its final follow-up poll; A's reply
        // comes first and carries B's cached row, which is not a read of B; B's
        // own reply then decides its watch, and a silent one continues it.
        let (a, b) = (AccountId::from_tag([1; 20]), AccountId::from_tag([2; 20]));
        let row = |id, state| AccountRow {
            id,
            kind: AccountKind::Derived,
            index: 0,
            state,
            spendable: false,
            number: None,
            reserved: true,
        };
        let live = outstanding(ReservationState::Live);
        let doubtful = diverged(DivergenceKind::NotFound);
        let view = |a_state: AccountState, b_state: AccountState| WalletView {
            dir: PathBuf::new(),
            accounts: vec![row(a, a_state), row(b, b_state)],
            opened: true,
            notice: None,
            settled: Vec::new(),
        };
        for own in [
            diverged(DivergenceKind::Unreachable),
            outstanding(ReservationState::Unclassified),
        ] {
            let mut watch = Watch::default();
            watch.read_answer(&view(live.clone(), live.clone()), Reads::Every);
            // B reads "account not found", and each poll sent about it in doubt
            // is answered the same way: the second poll is its final follow-up,
            // and it is still on its way.
            watch.read_answer(&view(live.clone(), doubtful.clone()), Reads::One(b));
            watch.asked(b, &doubtful);
            watch.read_answer(&view(live.clone(), doubtful.clone()), Reads::One(b));
            watch.asked(b, &doubtful);
            // A's reply comes first, with B's cached row: that is not a read of
            // B, so B is still watched.
            watch.read_answer(&view(live.clone(), doubtful.clone()), Reads::One(a));
            assert!(watch.asks(b, &own), "{own:?}: B's own reply has not come");
            // B's own reply is silent: the watch continues, and the count stays.
            watch.read_answer(&view(live.clone(), own.clone()), Reads::One(b));
            assert!(
                watch.asks(b, &own),
                "{own:?}: a silent read continues the watch"
            );
            watch.asked(b, &own);
            // B's own third doubtful read in a row ends the polls.
            watch.read_answer(&view(live.clone(), doubtful.clone()), Reads::One(b));
            assert!(
                !watch.asks(b, &doubtful),
                "{own:?}: the third doubtful read ends them"
            );
            assert!(
                !watch.asks(b, &own),
                "{own:?}: and a silent one does not restart them"
            );
        }
    }
}
