// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! What the interface asks the worker to do.
//!
//! Every [`Command`] is answered by exactly one [`crate::Event::Done`]
//! carrying the same [`RequestId`]. A command that takes time (a key
//! derivation, a node) is preceded by an [`crate::Event::Busy`]. Commands
//! run one at a time, in the order they were sent.
//!
//! Commands that carry a secret take a [`SecretText`], which the interface
//! builds with [`SecretText::take`] so the text field is emptied as the
//! command is made. A command is not `Clone`, so a secret in one is never
//! duplicated by the channel.

use std::path::PathBuf;

use crate::explorer::BlockAt;
use crate::secret::SecretText;
use crate::spend::SpendRequest;
use crate::staking::StakeRequest;
use crate::view::AccountId;

/// Which request an event answers. Assigned by the handle, in sending order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestId(pub(crate) u64);

impl RequestId {
    /// An id for sample data such as screenshots. The worker numbers requests
    /// from 1, so this one never answers anything.
    #[must_use]
    pub const fn example() -> RequestId {
        RequestId(0)
    }
}

/// A planned spend waiting for confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlanId(pub(crate) u64);

impl PlanId {
    /// An id for sample data, as [`RequestId::example`]. The worker numbers
    /// plans from 1, so confirming this one is always refused.
    #[must_use]
    pub const fn example() -> PlanId {
        PlanId(0)
    }
}

/// What the interface asks for.
#[derive(Debug)]
pub enum Command {
    /// Choose the node to ask. The application sends the node it remembers
    /// here at start-up, which is [`crate::preferences::DEFAULT_NODE`] until
    /// the person chooses another. `https://` only,
    /// or `http://` to the loopback interface. Takes effect for the next
    /// command that asks a node. A different node detaches an open store from
    /// the previous one, dropping a pending plan; nothing can be sent until
    /// [`Command::Refresh`] reconciles the store against the new node.
    SetNode { url: String },
    /// Forget the node. Nothing asks a node until another is set, and an
    /// open store is detached from it as for [`Command::SetNode`]. The
    /// application keeps this across a restart, so the default does not come
    /// back on its own.
    ClearNode,

    /// Start a new store in `dir`: check that nothing is there and that the
    /// password is long enough and typed the same twice, then make a
    /// recovery phrase and answer with it, to be shown once
    /// ([`crate::Reply::CreatePhrase`]). **Nothing is written yet.**
    CreateBegin {
        dir: PathBuf,
        password: SecretText,
        password_again: SecretText,
    },
    /// The words at the confirmation positions, separated by spaces. When
    /// they match, the store is written, account 0 is put in it, and it is
    /// opened with the password from [`Command::CreateBegin`]. When they do
    /// not, nothing is written and the phrase stays pending, so the
    /// interface can show it again. It stays pending too when the library
    /// refuses to write the store (an unsafe folder, a store there since),
    /// so the folder can be put right and the words given again; only a
    /// written store, [`Command::CreateAbandon`] or a lock drops it.
    CreateConfirm { answer: SecretText },
    /// Drop a pending phrase; nothing is written.
    CreateAbandon,
    /// Make a store from a recovery phrase the person already has (12 to 24
    /// words), and open it. The interface says which wallets' phrases are
    /// compatible before the phrase is typed.
    CreateFromPhrase {
        dir: PathBuf,
        password: SecretText,
        password_again: SecretText,
        phrase: SecretText,
    },

    /// Open the store in `dir` with its password, and reconcile it against
    /// the node when one is chosen. Any store already open is locked first.
    /// When the library will not open the wallet, the store stays open on
    /// its own with the library's page as its notice; nothing needs the
    /// password twice.
    Unlock { dir: PathBuf, password: SecretText },
    /// Lock: drop the store, its secret and its lock, and any pending phrase
    /// or plan. Like every command it runs after the ones sent before it;
    /// [`crate::WorkerHandle::cancel`] first stops those, as
    /// [`crate::WorkerHandle::background`] does.
    Lock,
    /// Ask the node about every account again. With the wallet open, each
    /// account's status is read fresh; with only the store open, opening
    /// the wallet is tried again.
    Refresh,

    /// An account's destination, and the address the ledger will hold for
    /// it at its current index. No node is asked.
    Receive { account: AccountId },

    /// Lay out a spend and answer with what it would do. Nothing is
    /// reserved or signed.
    PlanSend { spend: SpendRequest },
    /// Reserve the key the plan names, sign, and submit. The plan is
    /// checked again against the store, the tip and a fresh ledger read
    /// first; a refusal leaves the key unused.
    ConfirmSend { plan: PlanId },
    /// Forget a planned spend.
    DiscardPlan,
    /// Settle an account's reservation when the chain shows it landed.
    Settle { account: AccountId },
    /// Reproduce a reserved spend's signed bytes from its parameters (the
    /// destinations, amounts, references, fee and block-to-live used when
    /// it was made) and submit them: the one recovery for lost bytes.
    Resign { spend: SpendRequest },
    /// Submit signed bytes saved earlier, as hex. Opens no store.
    SubmitArtifact { artifact_hex: String },
    /// Send again a saved copy, as hex, of a settle the chain has reverted,
    /// for an account the chain holds one key behind with the settle retained.
    /// Needs the wallet open. The library
    /// writes the bytes only when every check holds: their signed part hashes
    /// to the retained digest, their signature recovers to the source key, the
    /// balance at the source is the one the settle was built against, and a
    /// non-zero block-to-live has not been reached. Nothing is signed and no
    /// key index moves. The account is read again after the write, so its row
    /// moves when the copy lands. After a refusal the caller reads the account,
    /// as it does after a sign the library refused.
    ResubmitSaved {
        account: AccountId,
        artifact_hex: String,
    },
    /// Send again, as [`Command::ResubmitSaved`] does, the copy the worker
    /// kept of the spend from `account` a reorganisation reverted
    /// ([`crate::retained`]). Nothing is signed. Refused
    /// when no copy is kept for it.
    ResendRetained { account: AccountId },

    /// Reconcile one account now and report, whatever its state. `scan_to`
    /// sets how far the search for where the chain holds it goes (key
    /// indices `0..=scan_to`, beside the window around the store's index),
    /// at most [`crate::MAX_KEY_INDEX`]. Every index walked costs one key
    /// derivation, so a far one is a long wait; it can be cancelled.
    Status {
        account: AccountId,
        scan_to: Option<u32>,
    },
    /// The acknowledged advance for a diverged account: move it to
    /// `advance_to` only if the library's live report names exactly that
    /// index. The whole store is reconciled and reported first.
    /// `advance_to` is at most [`crate::MAX_KEY_INDEX`]. It can be cancelled
    /// until it writes, and a cancel writes nothing.
    Reconcile { account: AccountId, advance_to: u32 },
    /// The confirmed rollback for an account the chain holds behind the
    /// store: move it back to `rollback_to` only if the library's live
    /// report names exactly that index. As [`Command::Reconcile`] otherwise.
    Rollback {
        account: AccountId,
        rollback_to: u32,
    },
    /// Find where derived account `account_index` sits on the chain and put
    /// it in the store at that index. `scan_to`, at most
    /// [`crate::MAX_KEY_INDEX`], widens the search as for
    /// [`Command::Status`]. It can be cancelled until it writes, and a
    /// cancel writes nothing.
    Restore {
        account_index: u32,
        scan_to: Option<u32>,
    },
    /// Ask the node about derived accounts `0..=to`, `to` from 1 to
    /// [`crate::DISCOVER_MAX_TO`] as at the command line; nothing is
    /// written. A cancel stops it between two requests, and it then reports
    /// nothing found: a sweep that stopped short is not a sweep.
    Discover { to: u32 },

    /// The node's chain tip, when it was solved, and the Mesh middleware's
    /// sync state: one request to `/network/status`.
    NetworkStatus,
    /// The networks the node serves, by name. No store is needed.
    Networks,
    /// The newest [`crate::explorer::CARD_BLOCKS`] blocks, read down from
    /// the tip: the command line's `blocks`. No store is needed.
    Blocks,
    /// How many transactions wait in the node's queue: the command line's
    /// `mempool` with none read whole. No store is needed.
    Mempool,
    /// Every account's transactions from the node's index, newest first, the
    /// newest [`crate::explorer::HISTORY_ROWS`] each: the command line's
    /// `recent-transactions` for each account. One request per account; a
    /// cancel stops it between two, and it then reports nothing.
    Activity,
    /// The next older page of each account named, from the offset given
    /// (its [`crate::explorer::AccountHistory::next`]): the command line's
    /// `recent-transactions --from`. An account the store does not hold is
    /// not read. Cancelled as [`Command::Activity`] is.
    OlderActivity(Vec<(AccountId, u64)>),
    /// The explorer's chain: the tip, the
    /// [`crate::explorer::LATEST_BLOCKS`] newest blocks with each one's
    /// solve time, and the average solve over
    /// [`crate::explorer::SOLVE_SPAN`] blocks. The command line's `blocks`
    /// walk, one block further down so the last one shown has its solve
    /// time, then the block that span below the tip. No store is needed. A
    /// cancel, a lock or the idle period stops it between two requests, and
    /// it then reports nothing.
    Chain,
    /// The node's queue: how many transactions wait, and the first `count`
    /// read whole, as the command line's `mempool --count` reads them; at
    /// most [`crate::explorer::QUEUE_ROWS`], and a larger count reads that
    /// many. No store is needed; stopped between two requests as
    /// [`Command::Chain`] is.
    Pending { count: u64 },
    /// One block, whole: the command line's `block`. The block below it is
    /// read too, for its solve time, and the tip, for how many blocks stand
    /// on it. No store is needed; stopped between two requests as
    /// [`Command::Chain`] is.
    Block(BlockAt),
    /// What the ledger holds for a tag, and the newest
    /// [`crate::explorer::HISTORY_ROWS`] of its transactions from the
    /// node's index: the command line's `recent-transactions`. No store is
    /// needed; stopped between the two requests.
    Tag(AccountId),
    /// An older page of a tag's transactions, from the offset given (its
    /// [`crate::explorer::AccountHistory::next`]): the command line's
    /// `recent-transactions --from`. No store is needed.
    TagHistory { account: AccountId, from: u64 },
    /// What a hash names. The node's index is searched for a transaction
    /// with that id (the command line's `transaction`), and when it holds
    /// none, or cannot be searched, the node is asked for a block with that
    /// hash (`block <hash>`). No store is needed; stopped between two
    /// requests as [`Command::Chain`] is.
    Find([u8; 32]),
    /// The stake page's reads for one account: the
    /// tip, the account's stake and reward, and the network's staking, as
    /// the command line's `stake-info` reads them. No store is needed.
    StakeInfo { account: AccountId },
    /// Lay out a stake, an unstake or a claim and answer with what it would
    /// do: the library's checks are made, and nothing is reserved or signed.
    /// The pending plan replaces any other, a spend's included.
    PlanStake { request: StakeRequest },
    /// Check the staking transaction laid out again, then reserve the key,
    /// sign and submit it. A check that no longer holds, or a transaction it
    /// would now make differently, is refused with the key unused.
    ConfirmStake { plan: PlanId },

    /// Reconcile every account in the store now and report each, whatever
    /// its state: what account recovery shows before anything else. As
    /// [`Command::Status`] for each account, in the store's order; a cancel
    /// stops it between accounts or inside a walk, and it then reports
    /// nothing.
    Review,
}
