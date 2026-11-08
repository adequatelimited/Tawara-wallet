// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! What the worker reports.
//!
//! Plain data, apart from [`Reply::CreatePhrase`], which carries the new
//! store's recovery phrase as a [`PhraseForDisplay`]: it zeroizes when dropped
//! and prints nothing in `Debug`. An `Event` is therefore not `Clone`; the
//! interface moves it into its state rather than copying it.

use core::fmt;
use std::path::PathBuf;

use crate::command::{PlanId, RequestId};
use crate::explorer::{
    AccountHistory, BlockDetail, BlocksView, ChainView, ExplorerRefusal, Found, MempoolView,
    PendingView, TagView,
};
use crate::node::{NetworkName, SyncState};
use crate::secret::PhraseForDisplay;
use crate::spend::SpendInputError;
use crate::view::{AccountId, AccountState, SettledAccount, WalletView};

/// Something the worker reports.
#[derive(Debug)]
pub enum Event {
    /// A command has started something that takes time.
    Busy { id: RequestId, activity: Activity },
    /// How far a long command has got: opening the wallet, a restore, an
    /// acknowledged advance or a discovery sweep. Sent between `Busy` and
    /// `Done`, before each account and every few hundred key positions.
    Progress { id: RequestId, progress: Progress },
    /// A command finished. Every command gets exactly one. The reply is
    /// boxed to keep every event small (clippy's `large_enum_variant` limit
    /// is crossed on some platforms otherwise).
    Done { id: RequestId, reply: Box<Reply> },
    /// The store was closed: its secret is dropped and its lock released.
    /// Sent whenever an open store closes, for any reason, including in
    /// answer to [`crate::Command::Lock`], and whenever a recovery phrase
    /// waiting for its confirmation is dropped by locking.
    Locked { reason: LockReason },
    /// The app moved to the background ([`crate::WorkerHandle::background`],
    /// which returned `seq`). Sent on every such move, also when nothing
    /// was open, once the session and everything pending are dropped: after
    /// the answer to the running command and after `Locked` when one is due.
    /// An interface that has taken this event has taken every event before
    /// it, any recovery phrase included.
    Backgrounded { seq: u64 },
    /// The worker has stopped and will answer nothing more. `panicked` is
    /// true when it stopped because of a fault rather than a shutdown; the
    /// release profile unwinds, so every secret it held was zeroized on the
    /// way out.
    Stopped { panicked: bool },
}

/// What a slow command is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Deriving the store's key from the password (Argon2id at the
    /// library's recommended cost: 64 MiB over three passes).
    DerivingKey,
    /// Waiting on the node.
    AskingNode,
    /// Reading the node's transaction index, one request per account.
    ReadingIndex,
}

/// How far a long command has got, as the library counts it: never an
/// estimate.
///
/// `account` of `accounts` is the account it is on, counted from zero (for
/// a sweep, the account index it is about to ask the node about, of the
/// `to + 1` it asks about). `position` of `ceiling` is how many key
/// positions that account's walk has reached of the most it can reach. A
/// walk that finds what it looks for stops short of `ceiling`, so `ceiling`
/// is no promise of how long is left; an account in sync walks nothing, and
/// a sweep walks nothing (its `position` and `ceiling` are zero).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub account: u32,
    pub accounts: u32,
    pub position: u32,
    pub ceiling: u32,
}

impl Progress {
    /// The library's count, as plain data that names no library type.
    pub(crate) fn of(p: mochimo_crypto::recon::Progress) -> Progress {
        Progress {
            account: p.account,
            accounts: p.accounts,
            position: p.position,
            ceiling: p.ceiling,
        }
    }
}

/// Why the store closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockReason {
    /// [`crate::Command::Lock`].
    Asked,
    /// Nothing was done for the idle period.
    Idle,
    /// The app moved to the background.
    Background,
    /// Another store was unlocked in its place.
    Replaced,
    /// The store was reopened after an operation (a refresh against a new
    /// node, an advance, a restore) and could not be read back; the
    /// command's refusal says why. A wallet the library refuses to open
    /// does not close the store: it stays open on its own, with the
    /// library's page as its notice. Also sent, after the command's answer,
    /// when an automatic settle's write failed and left the store's handle
    /// unusable: the store is closed so that unlocking it reads disk again.
    ReopenFailed,
    /// The worker is shutting down.
    Shutdown,
}

/// The answer to one command.
#[derive(Debug)]
pub enum Reply {
    /// The node was set. When it is a different node, an open store was
    /// detached from the previous one: `view` is the store afterwards, open
    /// but not reconciled against the new node until
    /// [`crate::Command::Refresh`].
    NodeSet {
        url: String,
        view: Option<WalletView>,
    },
    /// The node was forgotten, and an open store detached from it: `view`
    /// is the store afterwards.
    NodeCleared { view: Option<WalletView> },
    /// A new store's recovery phrase, to be shown once and then confirmed
    /// by the words at `confirm_positions` (one-based). Nothing is written.
    CreatePhrase {
        phrase: PhraseForDisplay,
        confirm_positions: [usize; 3],
    },
    /// A store was written in `dir` with `first` (account 0) in it, and
    /// opened with the same password: `opened` is the open store's view, or
    /// why it could not be opened (the store is written either way).
    Created {
        dir: PathBuf,
        first: AccountId,
        opened: Result<WalletView, Refusal>,
    },
    /// A pending phrase was dropped.
    CreateAbandoned,
    /// The store opened. When a cancel stopped the reconciliation that
    /// follows, the store is open on its own and its notice says so.
    Unlocked(WalletView),
    /// The store is closed (the [`Event::Locked`] says why).
    Locked,
    /// The store's state, after a refresh.
    Wallet(WalletView),
    /// An account's destination.
    Receive(ReceiveView),
    /// A spend, laid out and not yet signed.
    Planned(PlanView),
    /// The pending plan was dropped.
    PlanDiscarded,
    /// The stake page's reads ([`crate::Command::StakeInfo`]).
    StakeInfo(Box<crate::staking::StakeView>),
    /// A staking transaction, laid out and not yet signed. Signing it answers
    /// with [`Reply::Sent`], as a spend does: its bytes are the retry artifact
    /// in the same way.
    StakePlanned(Box<crate::staking::StakePlanView>),
    /// A spend was signed and submitted, or a reserved one re-signed and
    /// submitted.
    Sent(SentView),
    /// What settling found.
    Settled { text: String, view: WalletView },
    /// What the submitted artifact did.
    Submitted { accepted: bool, text: String },
    /// A saved copy was written to the node, as [`crate::Command::ResubmitSaved`]
    /// asks: `tx_id` is the transaction id the node echoed, and `text` says
    /// what that write establishes. `view` is the wallet with the account read
    /// again.
    Resubmitted {
        tx_id: String,
        text: String,
        view: WalletView,
    },
    /// One account, reconciled now. `spendable` is whether a spend may be
    /// planned from it now, as [`crate::view::AccountRow::spendable`] says:
    /// it reconciled, and the wallet held it when it opened (an account set
    /// aside then stays aside until the store is opened again). `settled` is
    /// set when the read showed the spend landed and the worker settled the
    /// account on its own; `state`, `spendable` and
    /// `text` are then what the settle left, with the library's settled
    /// page as `text`.
    Status {
        account: AccountId,
        state: AccountState,
        spendable: bool,
        text: String,
        settled: Option<SettledAccount>,
    },
    /// What an acknowledged advance did. `ok` is whether the library counts
    /// it as done (the command line's exit status 0); `advanced_to` is the
    /// new index when it moved. `opened` is the store reopened afterwards
    /// (the wallet when the library opens it, the store alone with the
    /// library's page as its notice when it refuses), or why the store could
    /// not be read back: then it is closed ([`Event::Locked`]). The advance
    /// stands either way.
    ///
    /// A cancel that stops it answers here too, with `ok` false, no
    /// `advanced_to`, and `text` saying nothing was written: the store was
    /// taken out of its session to be written, so `opened` is how it is
    /// now, open on its own and not reconciled until
    /// [`crate::Command::Refresh`].
    Reconciled {
        ok: bool,
        advanced_to: Option<u32>,
        text: String,
        opened: Result<WalletView, Refusal>,
    },
    /// What a restore did, with `ok` and `opened` as for
    /// [`Reply::Reconciled`], and a cancel answered the same way.
    Restored {
        ok: bool,
        text: String,
        opened: Result<WalletView, Refusal>,
    },
    /// What a discovery sweep found.
    Discovered {
        text: String,
        accounts: Vec<Discovered>,
    },
    /// The node's chain tip, when it was solved, in milliseconds since the
    /// epoch, and the Mesh middleware's own sync state, when it sent one.
    Network {
        tip_index: u64,
        tip_hash: String,
        tip_time_ms: i64,
        sync: Option<SyncState>,
    },
    /// The networks the node serves, by name.
    Networks(Vec<NetworkName>),
    /// The newest blocks, or why the node did not serve them.
    Blocks(Result<BlocksView, ExplorerRefusal>),
    /// How many transactions wait in the node's queue, or why it did not
    /// say.
    Mempool(Result<MempoolView, ExplorerRefusal>),
    /// The accounts' transactions, in the store's order, one page each for
    /// those asked about, or why the node did not serve them: a refusal for
    /// one account is the answer for all, since every read goes to the same
    /// index.
    Activity(Result<Vec<AccountHistory>, ExplorerRefusal>),
    /// The explorer's chain, or why the node did not serve it.
    Chain(Result<ChainView, ExplorerRefusal>),
    /// The node's queue, with its first transactions read whole, or why the
    /// node did not serve it.
    Pending(Result<PendingView, ExplorerRefusal>),
    /// One block, or why the node did not serve it.
    Block(Result<Box<BlockDetail>, ExplorerRefusal>),
    /// What the ledger and the node's index hold for a tag; each says for
    /// itself when the node did not answer.
    Tag(Box<TagView>),
    /// An older page of a tag's transactions, or why the index did not
    /// serve it.
    TagHistory(Result<AccountHistory, ExplorerRefusal>),
    /// What a hash names.
    Found(Box<Found>),
    /// Every account reconciled now, in the store's order.
    Reviewed(Vec<AccountReport>),
    /// The command was refused; nothing it would have changed was changed
    /// unless `text` says otherwise.
    Refused(Refusal),
}

/// One account, reconciled now: what [`Reply::Status`] says of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountReport {
    pub account: AccountId,
    pub state: AccountState,
    /// As [`Reply::Status`]'s.
    pub spendable: bool,
    /// The library's page for it, word for word.
    pub text: String,
}

/// An account's destination, for receiving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiveView {
    pub account: AccountId,
    /// Base58 over the tag and its CRC16: what to give a payer. `None` only
    /// if it cannot be rendered (see [`AccountId::destination`]).
    pub destination: Option<String>,
    /// The account's one-time key index.
    pub index: u32,
    /// The forty-byte entry the ledger will hold for the account at this
    /// index, in hex. Not a destination: no wallet takes it.
    pub ledger_address: String,
    /// The library's explanation of the two, word for word.
    pub text: String,
}

/// One destination of a planned spend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedDestination {
    pub to: AccountId,
    /// Rendered, whichever form was typed, to be compared against the payee.
    pub destination: String,
    pub amount: u64,
    /// The reference field's text; empty for none.
    pub reference: String,
}

/// A spend laid out, waiting for [`crate::Command::ConfirmSend`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanView {
    pub plan: PlanId,
    pub from: AccountId,
    /// In the order the person gave them.
    pub destinations: Vec<PlannedDestination>,
    pub send_total: u64,
    pub fee_total: u64,
    /// What returns to the account's next key.
    pub change_total: u64,
    /// The balance the plan was built against, from the ledger read it made.
    pub balance: u64,
    pub blk_to_live: u64,
    /// The change is zero: once this lands the node reports the account as
    /// not found, and every operation on it refuses until it is paid again.
    /// The words to show for it are the library's, in `text`.
    pub empties_account: bool,
    /// The library's page for the spend before it is signed, word for word:
    /// its destinations in the order that goes on the wire, the fee, the
    /// change and the block-to-live, the request to check each destination
    /// against its payee, and, when the change is zero, its warning that
    /// this empties the account.
    pub text: String,
}

/// A spend signed and submitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentView {
    pub from: AccountId,
    /// The one-time key index the spend was signed with, which is the index of
    /// the reservation it holds (`AccountState::SpendOutstanding::spent_index`
    /// while that reservation is open). A re-sign is for the spend that holds
    /// the account's open reservation, so the interface finds the spend by it.
    /// `None` only when the store could not say which reservation it holds.
    pub spent_index: Option<u32>,
    /// Whether the node's socket took the bytes. **A socket write, not a
    /// verdict**: whether the spend landed is settling's question.
    pub submitted: bool,
    /// The transaction id the node echoed, in hex, when it took them.
    pub tx_id: Option<String>,
    /// The signed bytes in hex: the retry artifact. The wallet does not keep
    /// them, and they are the only bytes that can move these funds while the
    /// reservation is open, so the interface offers to save them as a file
    /// ([`crate::save_artifact`]).
    pub artifact_hex: String,
    /// The library's page, with the three facts it never softens.
    pub text: String,
    /// The store's state afterwards.
    pub view: WalletView,
}

/// One derived account a discovery sweep asked about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovered {
    pub account_index: u32,
    pub id: AccountId,
    /// The balance the node reported, or `None` when it answered "account
    /// not found" (never funded, emptied, or a failed lookup: it cannot say
    /// which).
    pub ledger_balance: Option<u64>,
    /// Whether the store already holds it.
    pub held: bool,
}

/// A refused command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub kind: RefusalKind,
    /// What to show: the library's words where the library refused, the
    /// worker's otherwise.
    pub text: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Why a command was refused, for the interface to choose a screen by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefusalKind {
    /// The command needs an open store.
    NotUnlocked,
    /// The command needs the wallet open (reconciled), and only the store
    /// is.
    WalletNotOpen,
    /// The command needs a node and none is chosen.
    NoNode,
    /// The node URL is not one this wallet uses.
    NodeRefused,
    /// Another process (another window, or the command-line wallet) holds
    /// the store's lock.
    StoreInUse,
    /// The password does not open the store.
    WrongPassword,
    /// There is no store in the directory.
    NoStore,
    /// The library refused the directory: its owner, mode or access list, or
    /// a symbolic link.
    UnsafeDirectory,
    /// Creating: a store already exists there.
    Occupied,
    /// Creating: the password is shorter than the library's floor.
    PasswordTooShort,
    /// Creating: the two passwords differ.
    PasswordsDiffer,
    /// Creating: the confirmation words did not match.
    ConfirmationWrong,
    /// Creating: there is no pending phrase to confirm.
    NothingToConfirm,
    /// The spend as entered breaks one of the command line's rules.
    Spend(SpendInputError),
    /// The plan named is not the pending one.
    NoSuchPlan,
    /// The account diverged; no operation on it is permitted.
    Diverged,
    /// The operating system's generator failed.
    Entropy,
    /// A cancel stopped it ([`crate::WorkerHandle::cancel`], a move to the
    /// background, a shutdown), or the idle period passed while it ran.
    /// Nothing it would have changed was changed.
    Cancelled,
    /// A number is outside what the worker takes: a key index past
    /// [`crate::MAX_KEY_INDEX`] for a scan or an advance, or a discovery
    /// bound outside `1..=`[`crate::DISCOVER_MAX_TO`].
    OutOfRange,
    /// A staking check refused before any key was reserved: `text` is the
    /// library's page, or the worker's when the node serves another network.
    Staking,
    /// Any other refusal from the library; `text` is its own.
    Library,
}
