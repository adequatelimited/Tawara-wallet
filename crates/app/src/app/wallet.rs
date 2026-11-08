// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The wallet's and the explorer's pages: what each holds, the messages that
//! change it, and how the worker's answers update it.
//!
//! Destinations, amounts and the library's pages are not secret. Copies of a
//! signed spend's bytes are wiped when dropped (see [`Signed`]); the store's
//! key never leaves the worker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::Task;
use iced::widget::qr_code;
use tawara_wallet_core::explorer::{
    BlockAt, BlockDetail, ExplorerRefusal, Found, PendingView, QUEUE_ROWS, Query, TagView,
    TransactionView,
};
use tawara_wallet_core::preferences::{AmountUnit, IDLE_LOCK_MINUTES};
use tawara_wallet_core::spend::{
    Amount, DestinationInput, SpendRequest, parse_destination, parse_reference,
};
use tawara_wallet_core::staking::{
    FEE as STAKE_FEE, StakeAction, StakePlanView, StakeRequest, StakeView, TIERS,
};
use tawara_wallet_core::view::{
    AccountId, AccountState, DivergenceKind, SettledAccount, WalletView,
};
use tawara_wallet_core::{AccountReport, amount};

use crate::history::{Filter, RowId};
use crate::host;
use crate::scan::{Code, destination};
use tawara_wallet_core::{
    Command, Discovered, MAX_DESTINATIONS, PlanView, ReceiveView, RefusalKind, Reply, SentView,
};

use super::{App, Message, Model, Purpose, Screen, WalletPage, wipe};

/// Which wallet page is shown.
#[derive(Clone, Debug, Default)]
pub enum Page {
    /// The wallet overview.
    #[default]
    Dashboard,
    /// Receive.
    Receive(ReceivePage),
    /// Add account.
    AddAccount(AddAccountPage),
    /// Send: the form, the review and the sent page; on a phone, four pages
    /// before the sent page.
    Send(SendPage),
    /// One account.
    Account(AccountPage),
    /// Re-sign.
    Resign(ResignPage),
    /// Submit saved bytes.
    Submit(SubmitPage),
    /// Activity.
    Activity(ActivityPage),
    /// Settings.
    Settings(SettingsPage),
    /// The licences, from Settings' About.
    Licences,
    /// Account recovery.
    Recovery(RecoveryPage),
    /// The explorer's overview.
    Explorer(ExplorerPage),
    /// One block.
    Block(BlockPage),
    /// One account, by its tag.
    Tag(TagPage),
    /// One transaction.
    Transaction(TransactionPage),
    /// The explorer's "View all pending": the node's whole queue.
    Queue(QueuePage),
    /// Staking.
    Stake(StakePage),
}

/// The staking page: an account's stake and the network's, and a stake, an
/// unstake or a claim from it.
#[derive(Clone, Debug, Default)]
pub struct StakePage {
    pub account: Option<AccountId>,
    /// The node's answer for `account`, once it has come.
    pub read: Option<Box<StakeView>>,
    /// Whether a read is on its way.
    pub reading: bool,
    /// The amount typed, in MCM.
    pub amount: String,
    /// The lock tier chosen, 0 to 5.
    pub tier: u8,
    /// The transaction laid out, while the worker holds it for confirmation.
    pub review: Option<Box<StakePlanView>>,
}

/// What the staking page's buttons lay out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StakeVerb {
    /// Stake the amount typed at the tier chosen.
    Stake,
    /// End the stake. A stake still locked is ended with its reward forfeited:
    /// the button says so, and so does the review before anything is signed.
    Unstake,
    /// Release the reward, and after a lock has ended the stake with it.
    Claim,
}

/// The node's whole queue, up to the command line's limit.
#[derive(Clone, Debug, Default)]
pub struct QueuePage {
    /// The node's answer, once it has come.
    pub read: Option<Result<PendingView, ExplorerRefusal>>,
}

/// The explorer's overview: the chain, the node's queue, and the search field.
#[derive(Clone, Debug, Default)]
pub struct ExplorerPage {
    /// What is typed in the search field.
    pub search: String,
    /// Why what was typed is none of the things the explorer finds.
    pub invalid: Option<String>,
    /// The hash being looked for. Only its answer is taken, not one for an
    /// earlier search.
    pub finding: Option<[u8; 32]>,
    /// What the last hash looked for named, when it was nothing.
    pub missed: Option<Box<Missed>>,
}

/// A hash that named nothing: the index's answer for a transaction with
/// that id, and the node's for a block with that hash.
#[derive(Clone, Debug)]
pub struct Missed {
    /// The index answered, and holds no such transaction.
    pub searched: bool,
    pub transaction: ExplorerRefusal,
    pub block: ExplorerRefusal,
}

/// How many of a block's transactions its page lists at a time.
pub const BLOCK_ROWS: usize = 10;

/// One block.
#[derive(Clone, Debug)]
pub struct BlockPage {
    pub at: BlockAt,
    /// The node's answer, once it has come.
    pub read: Option<Result<Box<BlockDetail>, ExplorerRefusal>>,
    /// The first of its transactions the page lists, [`BLOCK_ROWS`] at a
    /// time.
    pub rows_from: usize,
}

/// One account, by its tag.
#[derive(Clone, Debug)]
pub struct TagPage {
    pub account: AccountId,
    /// The node's answer, once it has come.
    pub read: Option<Box<TagView>>,
    /// An older page of its history is on its way.
    pub reading_older: bool,
    /// The person typed this account's address or tag into the explorer's
    /// search field. Only then is "Send to this account" offered: an
    /// account reached through a link came from the node, which could name
    /// its own account where the person expects another's.
    pub typed: bool,
}

/// One transaction, whole.
#[derive(Clone, Debug)]
pub struct TransactionPage {
    pub transaction: TransactionView,
    /// As the node's index lists it (its source at its gross amount, the
    /// change a destination of its own), rather than as a block or the
    /// queue lists it (its source at what left it net of the change, which
    /// is not listed).
    pub from_index: bool,
    /// The library's page for it, when it was looked up in the index; a
    /// transaction opened from its block's page or a tag's history is on
    /// that page's.
    pub text: Option<String>,
    /// The chain's tip read with the block page it was opened from, if any.
    /// Confirmations are counted from this or the explorer's tip, whichever
    /// is higher, so they never drop below the block page's count.
    pub tip: Option<u64>,
}

/// Activity: every account's transactions from the node's index.
#[derive(Clone, Debug, Default)]
pub struct ActivityPage {
    pub filter: Filter,
    /// What is typed in the search field.
    pub search: String,
    /// The row shown beside the list; the newest when none is chosen.
    pub selected: Option<RowId>,
}

/// Settings.
#[derive(Clone, Debug, Default)]
pub struct SettingsPage {
    /// The node's address as typed, before it is saved.
    pub node: String,
}

/// Account recovery: what the chain shows for every account, and for each
/// one the library can act on, the one action that applies.
#[derive(Clone, Debug, Default)]
pub struct RecoveryPage {
    /// Every account's report, once the review answers. Each opens to its
    /// summary.
    pub reports: Option<Vec<AccountReport>>,
    /// The rollback asked for and not yet confirmed: the account and the
    /// key index the chain shows it at.
    pub rollback: Option<(AccountId, u32)>,
    /// Whether the last resync or rollback is done, and the library's page
    /// for it.
    pub result: Option<(bool, String)>,
}

/// How far Find live index searches: key indices `0..=SEARCH_FURTHER_TO`,
/// about 1.6 ms each, so under three minutes, and it can be cancelled.
pub const SEARCH_FURTHER_TO: u32 = 100_000;

/// What can be done for an account on the recovery page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remedy {
    /// The advance to the index the report names: forward only, to where
    /// the chain shows the account.
    Advance(u32),
    /// The rollback to the earlier index the chain shows the account at,
    /// once the person confirms it.
    Rollback(u32),
    /// Its chain address was not among the keys searched: search further.
    SearchFurther,
}

/// What the recovery page offers for `state`, if anything: an advance only
/// where the library's report names the index, a further search only where
/// it found nothing in the keys searched.
#[must_use]
pub fn remedy(state: &AccountState) -> Option<Remedy> {
    match state {
        AccountState::Diverged {
            advance_to: Some(to),
            ..
        } => Some(Remedy::Advance(*to)),
        // A settle the chain reverted is sent again from its kept copy,
        // which signs nothing; a rollback would clear the record that copy
        // is checked against, so none is offered while it applies.
        AccountState::Diverged {
            rollback_to: Some(to),
            reverted_settle: false,
            ..
        } => Some(Remedy::Rollback(*to)),
        AccountState::Diverged {
            kind: DivergenceKind::Unlocated,
            ..
        } => Some(Remedy::SearchFurther),
        _ => None,
    }
}

/// Receive: one account's destination, and its QR code.
#[derive(Clone, Debug, Default)]
pub struct ReceivePage {
    pub account: Option<AccountId>,
    /// What the worker said about it, once it has.
    pub view: Option<ReceiveView>,
    /// The destination as a QR code, built when the worker answers rather than
    /// on every draw, because the widget borrows the code's data. `None`
    /// until the answer, and when only the hex form is known.
    pub code: Option<Arc<qr_code::Data>>,
}

impl ReceivePage {
    /// The text the code encodes: the destination the page prints and nothing
    /// else. `None` when only the hex form is known.
    #[must_use]
    pub fn code_text(view: &ReceiveView) -> Option<&str> {
        view.destination.as_deref()
    }

    /// The page once the worker has answered `view` for `account`. The code
    /// is QR version 3 at error correction Q, which holds 32 bytes even in
    /// byte mode; a destination is at most 31 characters (a 22-byte payload
    /// in Base58). The encoder picks the shortest modes. A destination that
    /// does not encode gets no code, never a different one.
    #[must_use]
    pub fn answered(account: AccountId, view: ReceiveView) -> ReceivePage {
        let code = Self::code_text(&view)
            .and_then(|text| {
                qr_code::Data::with_version(
                    text.as_bytes(),
                    qr_code::Version::Normal(3),
                    qr_code::ErrorCorrection::Quartile,
                )
                .ok()
            })
            .map(Arc::new);
        ReceivePage {
            account: Some(account),
            view: Some(view),
            code,
        }
    }
}

/// Add account: a discovery sweep, and adding what it found.
#[derive(Clone, Debug)]
pub struct AddAccountPage {
    /// How far to search, as typed: derived accounts `0..=to`.
    pub to: String,
    /// The library's page for the last sweep, and its rows.
    pub found: Option<(String, Vec<Discovered>)>,
    /// Whether the last account asked for was added, and the library's
    /// page for it.
    pub added: Option<(bool, String)>,
    /// The additions a restore from a phrase makes by itself.
    pub auto: Option<AutoAdd>,
}

impl Default for AddAccountPage {
    fn default() -> AddAccountPage {
        AddAccountPage {
            to: tawara_wallet_core::DISCOVER_DEFAULT_TO.to_string(),
            found: None,
            added: None,
            auto: None,
        }
    }
}

/// After a restore from a phrase: a sweep of the default range, then each
/// account it found with a balance that the store does not hold, added one
/// at a time by the same command as the page's Add. A refusal, a cancel or
/// leaving the page ends it, and the page is then used as it always is.
#[derive(Clone, Debug, Default)]
pub struct AutoAdd {
    /// Whether it is still going.
    pub running: bool,
    /// Whether the sweep has answered and the accounts to add were taken
    /// from it.
    pub swept: bool,
    /// The accounts still to add, in order.
    pub queue: Vec<u32>,
    /// The account being added now.
    pub current: Option<u32>,
    /// The accounts added.
    pub added: Vec<u32>,
    /// The accounts the library did not add.
    pub not_added: Vec<u32>,
}

impl AutoAdd {
    /// The accounts the sweep found with a balance that the store does not
    /// hold.
    pub fn funded(rows: &[Discovered]) -> Vec<u32> {
        rows.iter()
            .filter(|r| !r.held && r.ledger_balance.is_some_and(|b| b > 0))
            .map(|r| r.account_index)
            .collect()
    }
}

/// One destination as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DestinationRow {
    pub to: String,
    /// In MCM.
    pub amount: String,
    pub reference: String,
    /// The focus has left the memo field with a memo in it, so the page shows
    /// whether its format is valid. Cleared when the memo is emptied.
    pub memo_checked: bool,
}

/// A spend as typed: the Send form, and the Re-sign form, which must repeat
/// the reserved spend's figures exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpendForm {
    pub rows: Vec<DestinationRow>,
    /// The fee in nanoMCM, in total; empty for the fee per destination.
    /// Only the Re-sign form has a field for it: Send always pays the fee
    /// per destination.
    pub fee: String,
    /// The block after which the spend may not be included; empty for
    /// none. Only the Re-sign form has a field for it: a spend from Send has
    /// none.
    pub blk_to_live: String,
}

impl Default for SpendForm {
    fn default() -> SpendForm {
        SpendForm {
            rows: vec![DestinationRow::default()],
            fee: String::new(),
            blk_to_live: String::new(),
        }
    }
}

/// Which spend form a destination's Paste button was drawn on, so that a
/// read which comes back once another form is shown is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormOf {
    /// Send, spending from this account.
    Send(Option<AccountId>),
    /// Re-sign, re-signing this account's spend.
    Resign(AccountId),
}

/// Send: composing a spend (pages 1 and 2 on a phone), reviewing the
/// library's page for it (page 3), signing it (page 4), and the sent page.
#[derive(Clone, Debug, Default)]
pub struct SendPage {
    pub from: Option<AccountId>,
    pub form: SpendForm,
    pub stage: SendStage,
}

/// A field of a destination row, for what "Continue" finds wrong with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefusedField {
    /// The address.
    Address,
    /// The reference.
    Reference,
}

/// Where a spend is. On a desktop: the form on one page, the review, and the
/// sent page. On a phone: four pages (the destinations, the amounts and fee,
/// the review, and signing), then the sent page. A desktop never uses the
/// phone's pages, and a phone never uses [`SendStage::Compose`].
#[derive(Clone, Debug, Default)]
pub enum SendStage {
    /// On a desktop: the whole form on one page.
    #[default]
    Compose,
    /// Page 1 of 4 on a phone: the destinations. The map holds what "Continue"
    /// found wrong with an address or a reference, by its row and its field,
    /// to show under that field.
    To(BTreeMap<(usize, RefusedField), String>),
    /// Page 2 of 4 on a phone: the amounts and the fee, and "Review spend".
    Amount,
    /// The review (page 3 of 4 on a phone): laid out and not signed; the
    /// worker holds the plan.
    Review(Box<PlanView>),
    /// Page 4 of 4 on a phone: the plan, to sign and submit. The worker holds
    /// it, as it does on the review.
    Sign(Box<PlanView>),
    /// The sent page: signed, and written to the node's socket or not.
    Sent(Box<SentPage>),
}

impl SendStage {
    /// The plan the worker holds for this page, if it holds one: the review's,
    /// and the signing page's.
    fn plan(&self) -> Option<&PlanView> {
        match self {
            SendStage::Review(plan) | SendStage::Sign(plan) => Some(plan),
            _ => None,
        }
    }
}

/// A QR scan under way, for the destination row it fills. It holds the form
/// and row as they were when the scan began: a reply that comes once either
/// has changed is dropped.
#[derive(Clone, Debug)]
pub struct Scanning {
    /// The number of this scan, as its reply carries it.
    pub ask: u64,
    /// The destination row the text is for, zero based.
    pub row: usize,
    /// The form the row is on.
    pub form: FormOf,
    /// How many rows the form had.
    pub rows: usize,
    /// The row as it was when the scan began.
    pub typed: DestinationRow,
}

/// The sent page.
#[derive(Clone, Debug)]
pub struct SentPage {
    pub sent: SentView,
    /// The spend's place in the run's signing history, the order
    /// [`Model::signed`] keeps spends in. A re-sign gives the same bytes again
    /// and keeps the place of the spend it re-signs.
    pub order: u64,
    /// Re-signed rather than signed for the first time.
    pub resigned: bool,
    /// Where "Save artifact" wrote the bytes. An `Err` holds why they are not
    /// known to be saved: a refusal, or on a phone that they were offered to
    /// the share sheet, which cannot report whether they were kept. Only an
    /// `Ok` is a save.
    pub saved: Option<Result<PathBuf, String>>,
    /// "Dismiss" was pressed once for bytes not saved, and waits to be
    /// confirmed: the card says that dismissing wipes the only copy Tawara
    /// holds.
    pub dismissing: bool,
}

impl SentPage {
    /// Whether its bytes are in memory only: not saved to a file. Bytes
    /// offered to the share sheet still count as unsaved.
    #[must_use]
    pub fn unsaved(&self) -> bool {
        !matches!(self.saved, Some(Ok(_)))
    }

    /// On a phone, "Save artifact" asked the system to open the share sheet
    /// with the bytes: `saved` holds [`SHARED`]. The spend still counts as
    /// not saved, but nothing was refused, so the page shows a note rather
    /// than a refusal. Derived from `saved` so the two cannot disagree.
    #[must_use]
    pub fn shared(&self) -> bool {
        matches!(&self.saved, Some(Err(e)) if e == SHARED)
    }
}

/// One account, its state, and what that state allows.
#[derive(Clone, Debug)]
pub struct AccountPage {
    pub account: AccountId,
    /// What was last done here, and the library's page for it.
    pub report: Option<(Done, String)>,
}

/// What was done on an account's page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    Checked,
    Settled,
}

/// A report on a wallet page: shown as a slim banner, it opens into a
/// summary in Tawara's words, and from there into the library's full output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReportKey {
    /// The store's notice (overview).
    Notice,
    /// A diverged account's report (account page).
    Diverged,
    /// What a check or a settlement found (account page).
    Done,
    /// The spend laid out and not signed (review).
    Plan,
    /// The spend signed (sent page).
    Sent,
    /// An account's destination (Receive).
    Receive,
    /// A discovery sweep (Add account).
    Found,
    /// An account added (Add account).
    Added,
    /// Saved bytes submitted (Submit).
    Submitted,
    /// The refusal the page's last command met.
    Refused,
    /// The node's index for an account (Activity).
    History,
    /// What the index or the newest blocks could not be read for (overview,
    /// Activity).
    Explorer,
    /// The n-th account's report on the recovery page.
    Review(u16),
    /// What the last advance did (recovery page).
    Advanced,
    /// The library's page for what an explorer page shows.
    Read,
    /// The node's answer for a block, when a hash named nothing (explorer).
    Missed,
    /// An account the node did not resolve (tag page).
    Ledger,
}

/// How far a report is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    /// The banner alone.
    #[default]
    Closed,
    /// The summary, with its common causes.
    Summary,
    /// The summary and the library's full output.
    Full,
}

/// Re-sign: the reserved spend from one account, typed again.
#[derive(Clone, Debug)]
pub struct ResignPage {
    pub account: AccountId,
    pub form: SpendForm,
}

/// Submit: signed bytes saved earlier, submitted again.
#[derive(Clone, Debug, Default)]
pub struct SubmitPage {
    pub hex: String,
    /// What the bytes are sent as, and so what the page says of them.
    pub mode: SubmitMode,
    /// Whether the library counts it as written, and its page.
    pub result: Option<(bool, String)>,
}

/// What the Submit page sends its bytes as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubmitMode {
    /// The signed bytes of an open reservation, as the sent page saved them,
    /// sent with `SubmitArtifact`, which opens no store.
    #[default]
    Artifact,
    /// A saved copy of a settle the chain reverted, for this account, sent
    /// with `ResubmitSaved`. Reached from the account's page only.
    Resubmit(AccountId),
}

/// Where a wallet message goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    Dashboard,
    /// Receive, for this account or the first.
    Receive(Option<AccountId>),
    AddAccount,
    /// Send, from this account or the first that can spend.
    Send(Option<AccountId>),
    Account(AccountId),
    Resign(AccountId),
    Submit,
    /// Submit in resubmit mode, for this account's saved copy of a settle
    /// the chain reverted.
    Resubmit(AccountId),
    Activity,
    Settings,
    /// The licences, from Settings' About.
    Licences,
    /// Account recovery.
    Recovery,
    /// The explorer's overview.
    Explorer,
    /// One block.
    Block(BlockAt),
    /// One account, by its tag.
    Tag(AccountId),
    /// The explorer's "View all pending".
    Queue,
    /// Staking, for this account or the first that can spend.
    Stake(Option<AccountId>),
}

/// What the wallet's pages react to.
#[derive(Clone, Debug)]
pub enum WalletMsg {
    Open(To),
    /// Receive: show this account's destination.
    ReceiveFor(AccountId),
    /// Put this text on the clipboard: a destination, or the hex of a
    /// signed spend. Neither is secret.
    Copy(String),
    DiscoverTo(String),
    Discover,
    /// Add derived account N, as the sweep found it.
    Add(u32),
    From(AccountId),
    /// Page 1 of 4 on a phone: "Change" opens the account list under the From
    /// line, or closes it.
    ChooseAccount(bool),
    DestinationTo(usize, String),
    Amount(usize, String),
    Reference(usize, String),
    AddDestination,
    RemoveDestination(usize),
    /// Max beside destination N's amount: fill it with the most it can
    /// send.
    Max(usize),
    /// Overview: the Nth movement of Recent activity, opened as its
    /// transaction in the explorer, found on its account's page.
    OpenMovement(usize),
    /// Which memo field holds the focus after a click, a tap or Tab, if one
    /// does: a memo the focus has left is checked.
    MemoFocus(Option<usize>),
    Fee(String),
    BlockToLive(String),
    /// Form to review (page 2 to page 3 on a phone): lay the spend out.
    Review,
    /// Review back to the form: forget the plan, keep what was typed.
    Edit,
    /// Review (page 4 of 4 on a phone): reserve the key, sign and submit.
    Sign,
    /// Page 1 of 4 on a phone: "Continue", which checks the destinations and
    /// shows page 2 when they are valid.
    Continue,
    /// Page 3 of 4 on a phone: "Continue to signing", which shows page 4 with
    /// the plan the worker holds.
    ToSigning,
    /// Pages 2 to 4 on a phone: back one page, keeping what was typed.
    Back,
    SaveArtifact,
    /// Show again the kept spend whose bytes are these (see
    /// [`Model::signed`]).
    ShowSigned(String),
    /// Save the bytes of the kept spend whose bytes are these: the stopped
    /// screen's, with no page to show them on.
    SaveSigned(String),
    /// Sent page: "Dismiss", pressed once: the card asks for confirmation
    /// and says what is lost.
    AskDismiss,
    /// Sent page: "Keep them": the bytes are not dismissed after all.
    CancelDismiss,
    /// Sent page: the dismissal confirmed, for these unsaved bytes. They are
    /// wiped and no longer offered, and the page is left.
    DismissSigned(String),
    Settle(AccountId),
    /// Account page: reconcile the account now and report.
    Check(AccountId),
    /// Re-sign and submit.
    Resign,
    ArtifactHex(String),
    Submit,
    /// Open or close a report on the page.
    Report(ReportKey, Level),
    /// Overview on a phone: the account whose row is expanded into a card,
    /// or none.
    Expand(Option<AccountId>),
    /// Activity: the filter chosen.
    Filter(Filter),
    Search(String),
    Select(RowId),
    /// Activity: read the node's index again.
    ReadActivity,
    /// Activity: read the next older page of every account the index holds
    /// more of.
    ReadOlderActivity,
    /// Settings: the amount unit.
    Unit(AmountUnit),
    /// Settings: the auto-lock period, in minutes.
    AutoLock(u64),
    NodeTyped(String),
    SaveNode,
    /// Settings: ask the node for its tip now.
    CheckNode,
    /// Recovery: reconcile every account again.
    ReviewAll,
    /// Recovery: move the account to the key index its report names.
    Resync(AccountId),
    /// Recovery: search further for where the chain holds the account.
    SearchFurther(AccountId),
    /// Recovery: ask to roll the account back to the index the chain shows.
    Rollback(AccountId),
    /// Recovery: the rollback asked for, confirmed.
    ConfirmRollback,
    /// Recovery: the rollback asked for, not made.
    CancelRollback,
    /// Explorer: what is typed in the search field.
    ExplorerTyped(String),
    /// Explorer: find what is typed.
    ExplorerFind,
    /// Explorer: read the chain and the queue again.
    ReadChain,
    /// Explorer: turn auto-update on or off; turning it on reads at once.
    AutoRead(bool),
    /// Block page: list the block's transactions from this one on.
    BlockRows(usize),
    /// Open the block page's `n`-th transaction, whole.
    OpenSpend(usize),
    /// Open the tag page's `n`-th row, whole.
    OpenTagRow(usize),
    /// Open the `n`-th of the queue's transactions, whole.
    OpenPending(usize),
    /// Tag page: read the next older page of the tag's transactions.
    ReadOlderTag,
    /// From the tag page: the send form, with this account as its destination.
    SendTo(AccountId),
    /// Staking: show this account's stake.
    StakeFor(AccountId),
    /// Staking: read the account's stake again.
    StakeRead,
    StakeAmount(String),
    /// Staking: the most a stake can take, leaving one fee spendable after
    /// its own fee, as the library requires.
    StakeMax,
    StakeTier(u8),
    /// Staking: lay this out.
    StakeReview(StakeVerb),
    /// Staking: back from the review; the plan is forgotten.
    StakeEdit,
    /// Staking: reserve the key, sign and submit what was reviewed.
    StakeSign,
}

impl From<WalletMsg> for Message {
    fn from(m: WalletMsg) -> Message {
        Message::Wallet(m)
    }
}

impl Page {
    /// Which sidebar item it belongs to: 0 Wallet, 1 Send, 2 Receive, 3 Stake.
    #[must_use]
    pub fn nav(&self) -> usize {
        match self {
            Page::Send(_) | Page::Resign(_) | Page::Submit(_) => 1,
            Page::Receive(_) => 2,
            Page::Stake(_) => 3,
            Page::Activity(_) => 4,
            Page::Explorer(_)
            | Page::Block(_)
            | Page::Tag(_)
            | Page::Transaction(_)
            | Page::Queue(_) => 5,
            Page::Settings(_) | Page::Licences | Page::Recovery(_) => 6,
            Page::Dashboard | Page::AddAccount(_) | Page::Account(_) => 0,
        }
    }
}

/// The spend `form` asks for from `from`, or why it cannot be one yet. Only
/// what the application can check; the worker checks the rest by the
/// command line's rules.
pub fn spend_request(from: AccountId, form: &SpendForm) -> Result<SpendRequest, String> {
    let rows: Vec<&DestinationRow> = form
        .rows
        .iter()
        .filter(|r| **r != DestinationRow::default())
        .collect();
    if rows.is_empty() {
        return Err("Give at least one destination.".to_owned());
    }
    let mut destinations = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let amount = match amount::parse_mcm(&row.amount) {
            Ok(nano) => Amount::Nano(nano),
            Err(e) => return Err(format!("Destination {}: {e}.", i + 1)),
        };
        destinations.push(DestinationInput {
            to: row.to.trim().to_owned(),
            amount,
            reference: row.reference.trim().to_owned(),
        });
    }
    let fee = match form.fee.trim() {
        "" => None,
        typed => Some(amount::parse_nano(typed).map_err(|e| format!("The fee: {e}."))?),
    };
    let blk_to_live = match form.blk_to_live.trim() {
        "" => 0,
        typed => typed.parse::<u64>().map_err(|_| {
            "The block-to-live is a block number, in digits, or empty for none.".to_owned()
        })?,
    };
    Ok(SpendRequest {
        from,
        destinations,
        fee,
        blk_to_live,
    })
}

/// The newest transaction in an account's history that moved its balance by
/// `change` nanoMCM, net of any change back to it, as Activity counts it.
/// `None` when the history was not read, or no single transaction moved it
/// by that much (several seen as one movement, or one not yet indexed).
#[must_use]
pub fn movement_transaction(view: &TagView, change: i128) -> Option<TransactionView> {
    let history = view.history.as_ref().ok()?;
    crate::history::rows(std::slice::from_ref(history))
        .into_iter()
        .find(|r| r.account == view.account && r.amount == change)
        .map(|r| r.tx.clone())
}

/// Whether `memo` is one a spend may carry: none, or one the library's rule
/// accepts.
#[must_use]
pub fn memo_ok(memo: &str) -> bool {
    let memo = memo.trim();
    memo.is_empty() || parse_reference(memo, 1).is_ok()
}

/// Whether every memo of `form` is one a spend may carry: "Review spend",
/// "Continue" and "Re-sign & submit" wait until it is.
#[must_use]
pub fn memos_ok(form: &SpendForm) -> bool {
    form.rows.iter().all(|r| memo_ok(&r.reference))
}

/// What Max fills destination `row` of `form` with, from a balance of
/// `balance` nanoMCM: the balance less what every other destination sends
/// and less the fee. The fee is the form's own when it gives one (Re-sign),
/// otherwise the fee per destination for each destination typed, this one
/// included. An empty row is not a destination ([`spend_request`]) and pays
/// no fee. `None`, so Max is not offered, when the balance does not cover
/// the rest or another amount is not valid MCM.
#[must_use]
pub fn max_amount(form: &SpendForm, row: usize, balance: u64) -> Option<u64> {
    if row >= form.rows.len() {
        return None;
    }
    let mut others = 0u64;
    let mut count = 1u64;
    for (i, r) in form.rows.iter().enumerate() {
        if i == row || *r == DestinationRow::default() {
            continue;
        }
        count += 1;
        if !r.amount.trim().is_empty() {
            others = others.checked_add(amount::parse_mcm(&r.amount).ok()?)?;
        }
    }
    let fee = match form.fee.trim() {
        "" => tawara_wallet_core::FEE_PER_DESTINATION.checked_mul(count)?,
        typed => amount::parse_nano(typed).ok()?,
    };
    balance.checked_sub(others)?.checked_sub(fee)
}

/// What "Continue" on a phone's destinations page finds wrong: each address
/// that is not a destination, and each reference that breaks the node's
/// rule, by row and field, in the library's words. An empty row is not a
/// destination ([`spend_request`]), so a form with none is refused whole.
/// The amounts, the fee and the rest are checked by "Review spend" on the
/// next page.
fn destination_refusals(
    form: &SpendForm,
) -> Result<BTreeMap<(usize, RefusedField), String>, String> {
    let mut refused = BTreeMap::new();
    let mut typed = false;
    for (i, row) in form.rows.iter().enumerate() {
        if *row == DestinationRow::default() {
            continue;
        }
        typed = true;
        let address = if row.to.trim().is_empty() {
            Some("Give this destination's address.".to_owned())
        } else {
            parse_destination(&row.to, i + 1)
                .err()
                .map(|e| e.to_string())
        };
        if let Some(why) = address {
            refused.insert((i, RefusedField::Address), why);
        }
        if let Err(e) = parse_reference(row.reference.trim(), i + 1) {
            refused.insert((i, RefusedField::Reference), e.to_string());
        }
    }
    if !typed {
        return Err("Give at least one destination.".to_owned());
    }
    Ok(refused)
}

/// Puts a phone's page 1 of Send in `s`, with `refused` on it, and returns the
/// task that scrolls the first refusal into view: the buttons are at the foot
/// of a scrolling page, so a refusal above the view would go unseen.
fn to_destinations(
    s: &mut SendPage,
    refused: BTreeMap<(usize, RefusedField), String>,
) -> Task<Message> {
    let first = refused.keys().next().map(|&(i, field)| match field {
        RefusedField::Address => crate::screens::destination_id(i),
        RefusedField::Reference => crate::screens::reference_id(i),
    });
    s.stage = SendStage::To(refused);
    match first {
        Some(id) => iced::advanced::widget::operate(crate::screens::reveal_field(id)).discard(),
        None => Task::none(),
    }
}

/// The stage a spend's flow starts in: the form on a desktop, and page 1 of 4
/// on a phone.
fn first_stage(phone: bool) -> SendStage {
    if phone {
        SendStage::To(BTreeMap::new())
    } else {
        SendStage::Compose
    }
}

/// The stage a spend is composed in again once its plan is given up: the form
/// on a desktop, and page 2 of 4 on a phone, where "Review spend" is.
fn composing_stage(phone: bool) -> SendStage {
    if phone {
        SendStage::Amount
    } else {
        SendStage::Compose
    }
}

/// Why a spend is not laid out from the account chosen: it cannot spend
/// now (`AccountRow::spendable`).
const CANNOT_SPEND: &str = "This account cannot spend now. An account spends once the wallet \
                                has reconciled it on opening and it is in sync; one set aside \
                                when the wallet opened spends again after Refresh. Choose another \
                                account, or Refresh the wallet first.";

/// The accounts a spend may be planned from now.
fn spendable(wallet: Option<&WalletView>) -> impl Iterator<Item = AccountId> + '_ {
    wallet
        .into_iter()
        .flat_map(|w| w.accounts.iter())
        .filter(|a| a.spendable)
        .map(|a| a.id)
}

/// Whether two folders are one store's, however they were written.
fn same_store(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

/// A signed spend whose page is not shown, kept for the rest of the run
/// while its reservation may be open (see [`Model::signed`]).
#[derive(Clone, Debug)]
pub struct Signed {
    pub sent: SentPage,
    /// What was typed for it, which re-signing starts from.
    pub form: SpendForm,
    /// Which of its page's reports were open.
    pub open: BTreeMap<ReportKey, Level>,
    /// A lock closed its page while it was shown: the page comes back as
    /// it was left once its store is unlocked. A lock asked for, on idle, in
    /// the background or at shutdown keeps only saved bytes and wipes unsaved
    /// ones; a store the worker closed, or one replaced, keeps both.
    pub resume: bool,
    /// Its hex is what the stopped screen last copied.
    pub copied: bool,
}

impl Signed {
    /// The store it was signed from.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.sent.sent.view.dir
    }

    /// Whether its bytes are in memory only: not saved to a file.
    #[must_use]
    pub fn unsaved(&self) -> bool {
        self.sent.unsaved()
    }

    fn into_page(self) -> WalletPage {
        WalletPage {
            page: Page::Send(SendPage {
                from: Some(self.sent.sent.from),
                form: self.form,
                stage: SendStage::Sent(Box::new(self.sent)),
            }),
            open: self.open,
            ..WalletPage::default()
        }
    }
}

impl Model {
    /// Whether a Scan button is offered: only where a shell provides a
    /// scanner, which only phones do.
    #[must_use]
    pub fn scan_offered(&self) -> bool {
        self.scanner.is_some() && self.phone()
    }

    /// Whether a scan draws its preview in the application, and so takes the
    /// camera's frames.
    #[must_use]
    pub fn previewing(&self) -> bool {
        self.scanning.is_some()
            && self
                .scanner
                .as_ref()
                .is_some_and(|s| s.preview() == crate::host::ScanPreview::Application)
    }

    /// Ends the scan under way, if there is one: the shell's camera is stopped,
    /// a later reply finds no scan to fill, and the preview is dropped.
    pub fn stop_scan(&mut self) {
        self.scan_preview = None;
        if self.scanning.take().is_some()
            && let Some(scanner) = &self.scanner
        {
            scanner.cancel();
        }
    }

    /// Takes the camera's latest frame into the preview, while a scan draws its
    /// preview in the application. A frame that comes when no scan is under
    /// way is left where the shell put it.
    pub fn take_preview(&mut self) {
        if !self.previewing() {
            return;
        }
        if let Some(frame) = crate::host::take_frame() {
            self.scan_preview = Some(iced::widget::image::Handle::from_rgba(
                frame.width,
                frame.height,
                frame.rgba,
            ));
        }
    }

    /// The spend form shown, as [`App::form`] reaches it: on a phone the pages
    /// that hold it are the destinations and the amounts.
    fn shown_form(&self) -> Option<&SpendForm> {
        match &self.screen {
            Screen::Wallet(WalletPage {
                page:
                    Page::Send(SendPage {
                        form,
                        stage: SendStage::Compose | SendStage::To(_) | SendStage::Amount,
                        ..
                    })
                    | Page::Resign(ResignPage { form, .. }),
                ..
            }) => Some(form),
            _ => None,
        }
    }

    /// The spend form shown, if one is, and how many destination rows it
    /// has: what a Paste button records when pressed, and what its read is
    /// checked against when it comes back. Page 2 of a phone's flow has no
    /// destination to paste into, so a read that returns there is dropped.
    #[must_use]
    pub fn form_of(&self) -> Option<(FormOf, usize)> {
        match &self.screen {
            Screen::Wallet(WalletPage {
                page:
                    Page::Send(SendPage {
                        from,
                        form,
                        stage: SendStage::Compose | SendStage::To(_),
                    }),
                ..
            }) => Some((FormOf::Send(*from), form.rows.len())),
            Screen::Wallet(WalletPage {
                page: Page::Resign(ResignPage { account, form }),
                ..
            }) => Some((FormOf::Resign(*account), form.rows.len())),
            _ => None,
        }
    }

    /// What Max would fill destination `row` with on the spend form shown,
    /// from its account's known balance (`max_amount`); `None` where Max
    /// is not offered.
    #[must_use]
    pub fn max_of(&self, row: usize) -> Option<u64> {
        let (from, form) = match &self.screen {
            Screen::Wallet(WalletPage {
                page:
                    Page::Send(SendPage {
                        from: Some(from),
                        form,
                        stage: SendStage::Compose | SendStage::To(_) | SendStage::Amount,
                    }),
                ..
            }) => (*from, form),
            Screen::Wallet(WalletPage {
                page: Page::Resign(ResignPage { account, form }),
                ..
            }) => (*account, form),
            _ => return None,
        };
        let balance = self
            .wallet
            .as_ref()?
            .accounts
            .iter()
            .find(|a| a.id == from && a.spendable)?
            .state
            .balance()?;
        max_amount(form, row, balance)
    }

    /// Whether a phone's send flow is in progress: the page shown is one of
    /// the four before the sent page. Tapping the Send tab then keeps it.
    #[must_use]
    pub(crate) fn send_in_progress(&self) -> bool {
        self.phone()
            && matches!(
                &self.screen,
                Screen::Wallet(WalletPage {
                    page: Page::Send(SendPage {
                        stage: SendStage::To(_)
                            | SendStage::Amount
                            | SendStage::Review(_)
                            | SendStage::Sign(_),
                        ..
                    }),
                    ..
                })
            )
    }

    /// Whether the system's Back steps back a page of a phone's send flow
    /// rather than acting as elsewhere: true on pages 2 to 4, not on page 1.
    #[must_use]
    pub(crate) fn send_steps_back(&self) -> bool {
        self.send_in_progress()
            && !matches!(
                &self.screen,
                Screen::Wallet(WalletPage {
                    page: Page::Send(SendPage {
                        stage: SendStage::To(_),
                        ..
                    }),
                    ..
                })
            )
    }

    /// Before the screen changes: keep the signed spend's page it shows, if
    /// any (see [`Model::signed`]), with `resume` when a lock closes it.
    /// Returns whether a page was kept.
    pub(super) fn keep_signed(&mut self, resume: bool) -> bool {
        let Screen::Wallet(shown) = &mut self.screen else {
            return false;
        };
        let (form, mut sent, open) = match core::mem::take(shown) {
            WalletPage {
                page:
                    Page::Send(SendPage {
                        form,
                        stage: SendStage::Sent(sent),
                        ..
                    }),
                open,
                ..
            } => (form, *sent, open),
            other => {
                *shown = other;
                return false;
            }
        };
        // What the store showed goes; its folder says which store the spend
        // is from.
        sent.sent.view.accounts = Vec::new();
        sent.sent.view.notice = None;
        // A dismissal asked for and not confirmed is not kept with it.
        sent.dismissing = false;
        // A re-signed spend is the same bytes again: one spend, kept once. If
        // either copy was saved, the bytes are on disk, and the one kept says
        // so.
        if sent.unsaved()
            && let Some(path) = self.saved_copy(&sent.sent.artifact_hex)
        {
            sent.saved = Some(Ok(path));
        }
        self.discard_signed(|s| s.sent.sent.artifact_hex == sent.sent.artifact_hex);
        // Kept at its place in the signing history, so that leaving a page,
        // or showing it again, never reorders the kept spends.
        let at = self.signed.partition_point(|s| s.sent.order < sent.order);
        self.signed.insert(
            at,
            Signed {
                sent,
                form,
                open,
                resume,
                copied: false,
            },
        );
        true
    }

    /// Where a kept spend with the bytes `hex` was saved, if one was. A
    /// re-sign gives the same bytes again, and those are then already on
    /// disk.
    pub(super) fn saved_copy(&self, hex: &str) -> Option<PathBuf> {
        self.signed.iter().find_map(|s| match &s.sent.saved {
            Some(Ok(path)) if s.sent.sent.artifact_hex == hex => Some(path.clone()),
            _ => None,
        })
    }

    /// The place in the signing history of the kept spend with the bytes `hex`,
    /// if one is kept: a re-sign gives the same bytes again, and keeps the
    /// place.
    pub(super) fn kept_order(&self, hex: &str) -> Option<u64> {
        self.signed
            .iter()
            .find(|s| s.sent.sent.artifact_hex == hex)
            .map(|s| s.sent.order)
    }

    /// The place the next spend signed in this run takes in the signing
    /// history.
    pub(super) fn next_order(&mut self) -> u64 {
        let order = self.next_signing;
        self.next_signing += 1;
        order
    }

    /// Once a store is unlocked: the signed spend's page a lock closed for
    /// it, shown again as it was left.
    pub(super) fn bring_back_signed(&mut self) {
        let Some(view) = &self.wallet else {
            return;
        };
        let Some(at) = self
            .signed
            .iter()
            .position(|s| s.resume && same_store(s.dir(), &view.dir))
        else {
            return;
        };
        let mut kept = self.signed.remove(at);
        kept.sent.sent.view = view.clone();
        self.screen = Screen::Wallet(kept.into_page());
    }

    /// The banner for an account the worker settled, with the library's page
    /// of the settlement. It is shown on the account's page if that page is
    /// shown; otherwise it waits for that page's next opening, so it is not
    /// lost.
    pub(super) fn show_settled(&mut self, settled: SettledAccount) {
        let SettledAccount { account, text } = settled;
        self.settled_waiting.remove(&account);
        if let Screen::Wallet(WalletPage {
            page: Page::Account(a),
            ..
        }) = &mut self.screen
            && a.account == account
        {
            a.report = Some((Done::Settled, text));
        } else {
            self.settled_waiting.insert(account, text);
        }
    }

    /// Forget the saved spends whose account the open store now shows in
    /// sync: the reservation is settled, and the file holds the bytes. An
    /// unsaved one is kept even then, since a reorganisation can revert the
    /// settle and its bytes may be the only way to move the balance. It goes
    /// when it is saved, dismissed, or the wallet locks.
    pub(super) fn forget_settled(&mut self) {
        let Some(view) = &self.wallet else {
            return;
        };
        let dir = view.dir.clone();
        let synced: Vec<AccountId> = view
            .accounts
            .iter()
            .filter(|a| matches!(a.state, AccountState::InSync { .. }))
            .map(|a| a.id)
            .collect();
        self.discard_signed(|s| {
            !s.unsaved() && same_store(s.dir(), &dir) && synced.contains(&s.sent.sent.from)
        });
    }

    /// Dismiss the unsaved bytes `hex` of the open store: they are wiped and
    /// no longer offered. The spend's page, if shown, is left with them.
    /// Returns the dropped spends, wiped, for the caller to release.
    pub(super) fn dismiss_signed(&mut self, hex: &str) -> Vec<Signed> {
        let Some(dir) = self.wallet.as_ref().map(|w| w.dir.clone()) else {
            return Vec::new();
        };
        if self.shown_signed(|s| s.unsaved() && s.sent.artifact_hex == hex) {
            self.keep_signed(false);
        }
        self.discard_signed(|s| {
            s.unsaved() && same_store(s.dir(), &dir) && s.sent.sent.artifact_hex == hex
        })
    }

    /// The person leaves the sent page: its bytes, if not saved, are dropped
    /// and wiped as Dismiss drops them, and no banner mentions them after.
    /// Only the person's own navigation calls this: a lock wipes them by its
    /// own rule, and a stopped worker keeps them for the stopped screen.
    /// Returns the dropped spends, wiped, for the caller to release.
    pub(super) fn drop_left_unsaved(&mut self) -> Vec<Signed> {
        let hex = match &self.screen {
            Screen::Wallet(WalletPage {
                page:
                    Page::Send(SendPage {
                        stage: SendStage::Sent(sent),
                        ..
                    }),
                ..
            }) if sent.unsaved() => sent.sent.artifact_hex.clone(),
            _ => return Vec::new(),
        };
        self.keep_signed(false);
        self.discard_signed(|s| s.unsaved() && s.sent.sent.artifact_hex == hex)
    }

    /// The wallet locked, by a lock that releases unsaved bytes (asked for,
    /// on idle, in the background, or at shutdown; see `App::on_event`):
    /// every spend whose bytes are not saved is dropped and wiped, whether
    /// its page is shown or kept. Saved spends stay. Returns the dropped
    /// spends, wiped, for the caller to release.
    pub(super) fn drop_unsaved_at_lock(&mut self) -> Vec<Signed> {
        if self.shown_signed(SentPage::unsaved) {
            self.keep_signed(false);
        }
        self.discard_signed(Signed::unsaved)
    }

    /// Whether the page shown is a signed spend's, and `then` holds for it.
    fn shown_signed(&self, then: impl FnOnce(&SentPage) -> bool) -> bool {
        match &self.screen {
            Screen::Wallet(WalletPage {
                page:
                    Page::Send(SendPage {
                        stage: SendStage::Sent(sent),
                        ..
                    }),
                ..
            }) => then(sent),
            _ => false,
        }
    }

    /// Drop the kept spends that `is_gone` names, wiping their bytes as they
    /// go: the hex, and the library's page, which repeats it under "RETRY
    /// ARTIFACT". They are returned wiped, for the caller to release.
    fn discard_signed(&mut self, is_gone: impl FnMut(&Signed) -> bool) -> Vec<Signed> {
        let (mut gone, kept): (Vec<Signed>, Vec<Signed>) = core::mem::take(&mut self.signed)
            .into_iter()
            .partition(is_gone);
        self.signed = kept;
        for s in &mut gone {
            wipe(&mut s.sent.sent.artifact_hex);
            wipe(&mut s.sent.sent.text);
        }
        gone
    }

    /// The kept spends from the open store whose bytes are not saved.
    pub fn unsaved_signed(&self) -> impl Iterator<Item = &Signed> {
        let dir = self.wallet.as_ref().map(|w| w.dir.as_path());
        self.signed
            .iter()
            .filter(move |s| s.unsaved() && dir.is_some_and(|d| same_store(s.dir(), d)))
    }

    /// What was typed for the kept spend that holds the account's open
    /// reservation in the open store: the spend whose one-time key index is
    /// the reservation's. No other spend of the account is offered, since the
    /// library would refuse its parameters. `None` when no kept spend holds
    /// that reservation, or the account's state does not name it.
    fn kept_form(&self, account: AccountId) -> Option<SpendForm> {
        let view = self.wallet.as_ref()?;
        let held = view
            .accounts
            .iter()
            .find(|a| a.id == account)?
            .state
            .reserved_index()?;
        self.signed
            .iter()
            .rev()
            .find(|s| {
                s.sent.sent.from == account
                    && s.sent.sent.spent_index == Some(held)
                    && same_store(s.dir(), &view.dir)
            })
            .map(|s| s.form.clone())
    }
}

/// What a phone's "Save artifact" says once Tawara has asked the system to
/// open the share sheet. Tawara cannot tell what was done with the bytes
/// there, so they still count as not saved. The note says what Tawara asked
/// for, not that the sheet opened: the platform presents it later, and a
/// refusal then is not reported.
pub const SHARED: &str = "Tawara asked the system to open the share sheet. Tawara cannot tell \
                          whether the bytes were saved there, so keep them until you have them \
                          somewhere safe.";

/// "Save artifact" for a spend's page: the bytes go where [`save`] puts them,
/// `share` being the shell's share sheet, and the page keeps the outcome
/// (see [`SentPage::shared`]).
fn keep_bytes(
    downloads: Option<&Path>,
    share: fn(&str, &str) -> Option<Result<(), String>>,
    page: &mut SentPage,
) {
    page.saved = Some(save(downloads, share, &page.sent));
}

/// Write a signed spend's bytes to the Downloads folder under a new name, or
/// say why they were not. With no Downloads folder, the shell's share sheet
/// takes them, if it has one, under the same file name and with the same
/// text: the hex and a line break. A share sheet is not a save, since the
/// application cannot learn what was done with it, so it is an `Err` too.
fn save(
    downloads: Option<&Path>,
    share: fn(&str, &str) -> Option<Result<(), String>>,
    sent: &SentView,
) -> Result<PathBuf, String> {
    match downloads {
        Some(dir) => {
            tawara_wallet_core::save_artifact_in(dir, &artifact_stem(sent), &sent.artifact_hex)
                .map_err(|e| format!("The bytes could not be saved in {}: {e}.", dir.display()))
        }
        None => match share(
            &format!("{}.hex", artifact_stem(sent)),
            &format!("{}\n", sent.artifact_hex),
        ) {
            Some(Ok(())) => Err(SHARED.to_owned()),
            Some(Err(e)) => Err(format!(
                "The bytes could not be shared: {e}. Copy the hex instead and keep \
                 it somewhere safe."
            )),
            None => Err(
                "This system names no Downloads folder, so the bytes were not saved. \
                 Copy the hex instead and keep it somewhere safe."
                    .to_owned(),
            ),
        },
    }
}

/// A name for a saved artifact: the spend's id when the node echoed one,
/// so the file can be matched to the page that shows it.
fn artifact_stem(sent: &SentView) -> String {
    match &sent.tx_id {
        Some(id) => format!("tawara-spend-{}", &id[..id.len().min(16)]),
        None => "tawara-spend-unsubmitted".to_owned(),
    }
}

impl App {
    /// The account's row as the worker's answer says it stands now: its state,
    /// and whether a spend may be planned from it (`AccountRow::spendable`).
    fn set_row(&mut self, account: AccountId, state: AccountState, spendable: bool) {
        if let Some(w) = &mut self.model.wallet
            && let Some(row) = w.accounts.iter_mut().find(|a| a.id == account)
        {
            row.spendable = spendable;
            row.set_state(state);
        }
    }

    /// The wallet page shown, when the wallet is.
    fn wallet_page(&mut self) -> Option<&mut WalletPage> {
        match &mut self.model.screen {
            Screen::Wallet(p) => Some(p),
            _ => None,
        }
    }

    /// Leave the page shown: a plan the worker holds for the review or the
    /// signing page is forgotten, so nothing waits to be signed that no screen
    /// shows; a signed spend's page is kept (see [`Model::signed`]).
    fn leave_page(&mut self) {
        // Unsaved bytes go with their page.
        drop(self.model.drop_left_unsaved());
        let reviewing = match self.wallet_page().map(|p| &p.page) {
            Some(Page::Send(s)) => s.stage.plan().is_some(),
            Some(Page::Stake(s)) => s.review.is_some(),
            _ => false,
        };
        if reviewing {
            self.send(Command::DiscardPlan, Purpose::DiscardPlan);
        }
        // Signed bytes pasted on the Submit page go with the page, wiped.
        if let Some(WalletPage {
            page: Page::Submit(s),
            ..
        }) = self.wallet_page()
        {
            wipe(&mut s.hex);
        }
        self.model.keep_signed(false);
    }

    /// Show again the kept spend whose bytes are `hex`, as it was left.
    /// Dismiss asked for (`asked`) or put off, on the sent page shown, while
    /// its bytes are not saved.
    fn ask_dismiss(&mut self, asked: bool) {
        if let Some(WalletPage {
            page:
                Page::Send(SendPage {
                    stage: SendStage::Sent(s),
                    ..
                }),
            ..
        }) = self.wallet_page()
            && s.unsaved()
        {
            s.dismissing = asked;
        }
    }

    fn show_signed(&mut self, hex: &str) {
        let Some(view) = self.model.wallet.clone() else {
            return;
        };
        self.leave_page();
        if let Some(at) = self
            .model
            .signed
            .iter()
            .position(|s| s.sent.sent.artifact_hex == hex && same_store(s.dir(), &view.dir))
        {
            let mut kept = self.model.signed.remove(at);
            kept.sent.sent.view = view;
            self.model.screen = Screen::Wallet(kept.into_page());
        } else {
            self.model.screen = Screen::Wallet(WalletPage::default());
        }
    }

    /// Show `page`, and ask the worker for what it needs first.
    fn open(&mut self, to: To) {
        // A movement's transaction is looked for only on the page opened for it.
        self.model.movement_lookup = None;
        // On a phone, tapping the Send tab mid-flow keeps the flow, its form
        // and any plan it holds, rather than building an empty form.
        if matches!(to, To::Send(_)) && self.model.send_in_progress() {
            return;
        }
        self.leave_page();
        let phone = self.model.phone();
        let wallet = self.model.wallet.as_ref();
        let first = wallet.and_then(|w| w.accounts.first()).map(|a| a.id);
        let mut receive = None;
        let page = match to {
            To::Dashboard => Page::Dashboard,
            To::Receive(account) => {
                receive = account.or(first);
                Page::Receive(ReceivePage {
                    account: receive,
                    view: None,
                    code: None,
                })
            }
            To::AddAccount => Page::AddAccount(AddAccountPage::default()),
            // An account asked for stays the one chosen, even one that
            // cannot spend now: the page says so, and never puts another in
            // its place. With none asked for, the first that can spend.
            To::Send(from) => Page::Send(SendPage {
                from: from.or_else(|| spendable(wallet).next()),
                stage: first_stage(phone),
                ..SendPage::default()
            }),
            // A settlement reported while this page was not shown is shown here
            // first.
            To::Account(account) => Page::Account(AccountPage {
                account,
                report: self
                    .model
                    .settled_waiting
                    .remove(&account)
                    .map(|text| (Done::Settled, text)),
            }),
            // The same spend exactly: what was typed for it, when this run
            // signed it.
            To::Resign(account) => Page::Resign(ResignPage {
                account,
                form: self.model.kept_form(account).unwrap_or_default(),
            }),
            To::Submit => Page::Submit(SubmitPage::default()),
            To::Resubmit(account) => Page::Submit(SubmitPage {
                mode: SubmitMode::Resubmit(account),
                ..SubmitPage::default()
            }),
            To::Activity => Page::Activity(ActivityPage::default()),
            To::Settings => Page::Settings(SettingsPage {
                node: self.model.node.url.clone().unwrap_or_default(),
            }),
            To::Licences => Page::Licences,
            To::Recovery => Page::Recovery(RecoveryPage::default()),
            To::Explorer => Page::Explorer(ExplorerPage::default()),
            To::Block(at) => Page::Block(BlockPage {
                at,
                read: None,
                rows_from: 0,
            }),
            To::Tag(account) => Page::Tag(TagPage {
                account,
                read: None,
                reading_older: false,
                typed: false,
            }),
            To::Queue => Page::Queue(QueuePage::default()),
            // An account asked for stays the one chosen, as on Send; with none
            // asked for, the first that can spend, or else the first.
            To::Stake(account) => Page::Stake(StakePage {
                account: account.or_else(|| spendable(wallet).next()).or(first),
                ..StakePage::default()
            }),
        };
        self.model.screen = Screen::Wallet(WalletPage {
            page,
            ..WalletPage::default()
        });
        if let Some(account) = receive {
            self.send(Command::Receive { account }, Purpose::Receive);
        }
        match to {
            // Read afresh when the page opens, behind anything already
            // asked.
            To::Activity => self.read_activity(),
            // Every account's report before anything else.
            To::Recovery => {
                self.send(Command::Review, Purpose::Review);
            }
            To::Explorer => self.read_chain(),
            To::Block(at) => {
                self.send(Command::Block(at), Purpose::Block(at));
            }
            To::Tag(account) => {
                self.send(Command::Tag(account), Purpose::Tag(account));
            }
            To::Queue => {
                self.send(Command::Pending { count: QUEUE_ROWS }, Purpose::Queue);
            }
            To::Stake(_) => self.read_stake(),
            _ => {}
        }
    }

    /// The staking page's account, read again from the node.
    fn read_stake(&mut self) {
        let account = match self.wallet_page() {
            Some(WalletPage {
                page: Page::Stake(s),
                ..
            }) => {
                if s.account.is_some() {
                    s.reading = true;
                }
                s.account
            }
            _ => None,
        };
        if let Some(account) = account {
            self.send(Command::StakeInfo { account }, Purpose::StakeInfo);
        }
    }

    /// The staking page, when it is the one shown.
    fn stake_page(&mut self) -> Option<&mut StakePage> {
        match self.wallet_page() {
            Some(WalletPage {
                page: Page::Stake(s),
                ..
            }) => Some(s),
            _ => None,
        }
    }

    /// Show `page`, read already: a transaction from the page that held it,
    /// or what a hash named.
    fn show(&mut self, page: Page) {
        self.leave_page();
        self.model.screen = Screen::Wallet(WalletPage {
            page,
            ..WalletPage::default()
        });
    }

    /// The recovery page shown.
    fn recovery(&mut self) -> Option<&mut RecoveryPage> {
        match &mut self.wallet_page()?.page {
            Page::Recovery(r) => Some(r),
            _ => None,
        }
    }

    /// The report the recovery page holds for `account`, as the last
    /// review or search left it.
    fn reviewed(&self, account: AccountId) -> Option<&AccountReport> {
        match &self.model.screen {
            Screen::Wallet(WalletPage {
                page: Page::Recovery(r),
                ..
            }) => r.reports.as_ref()?.iter().find(|a| a.account == account),
            _ => None,
        }
    }

    /// The send or re-sign form shown: on a phone, the pages that hold it are
    /// the destinations and the amounts.
    fn form(&mut self) -> Option<&mut SpendForm> {
        match &mut self.wallet_page()?.page {
            Page::Send(SendPage {
                form,
                stage: SendStage::Compose | SendStage::To(_) | SendStage::Amount,
                ..
            })
            | Page::Resign(ResignPage { form, .. }) => Some(form),
            _ => None,
        }
    }

    /// What "Continue" found wrong with the destinations, on page 1 of a
    /// phone's send flow, shown.
    fn refusals(&mut self) -> Option<&mut BTreeMap<(usize, RefusedField), String>> {
        match &mut self.wallet_page()?.page {
            Page::Send(SendPage {
                stage: SendStage::To(refused),
                ..
            }) => Some(refused),
            _ => None,
        }
    }

    /// Steps a phone's send flow back one page, keeping what was typed. Back
    /// from the review to the amounts gives up the plan, as Edit does; back
    /// from the signing page to the review keeps it.
    fn step_send_back(&mut self) {
        let Some(WalletPage {
            page: Page::Send(s),
            error,
            ..
        }) = self.wallet_page()
        else {
            return;
        };
        *error = None;
        match &s.stage {
            SendStage::Amount => s.stage = SendStage::To(BTreeMap::new()),
            SendStage::Sign(_) => {
                s.stage = match core::mem::take(&mut s.stage) {
                    SendStage::Sign(plan) => SendStage::Review(plan),
                    other => other,
                };
            }
            SendStage::Review(_) => {
                s.stage = SendStage::Amount;
                self.send(Command::DiscardPlan, Purpose::DiscardPlan);
            }
            _ => {}
        }
    }

    /// Whether `scanning` is still the scan the page shows: the same form, with
    /// the same number of rows, and the row it fills unchanged.
    pub(super) fn scan_fits(&self, scanning: &Scanning) -> bool {
        self.model.form_of() == Some((scanning.form, scanning.rows))
            && self
                .model
                .shown_form()
                .and_then(|form| form.rows.get(scanning.row))
                == Some(&scanning.typed)
    }

    /// Begins a scan of a QR code for destination row `row`, on a phone's
    /// destinations, where Paste is offered. The shell is asked to start its
    /// scanner, and its reply comes back as
    /// [`Message::Scanned`]. A scan that cannot start says why under the row.
    pub(super) fn begin_scan(&mut self, row: usize) -> Task<Message> {
        let m = &mut self.model;
        let Some(scanner) = m.scanner.clone() else {
            return Task::none();
        };
        // Only where the destination can be pasted too: on a phone, while
        // nothing is being done for the page, and for a row that is shown.
        if !m.phone() || m.busy.is_some() || m.scanning.is_some() {
            return Task::none();
        }
        let Some((form, rows)) = m.form_of() else {
            return Task::none();
        };
        let Some(typed) = m.shown_form().and_then(|f| f.rows.get(row)).cloned() else {
            return Task::none();
        };
        m.scan_asks += 1;
        let ask = m.scan_asks;
        m.scan_preview = None;
        if scanner.preview() == crate::host::ScanPreview::Application {
            // A frame left from an earlier scan is not the camera's now.
            let _ = crate::host::take_frame();
        }
        m.scanning = Some(Scanning {
            ask,
            row,
            form,
            rows,
            typed,
        });
        if let Err(why) = scanner.start(row, ask) {
            m.scanning = None;
            self.refuse_row(row, why);
        }
        Task::none()
    }

    /// The shell's reply to scan number `ask`, begun for row `row`. The text fills
    /// that row, and no other, once it is checked as a destination. A reply to a
    /// scan that is no longer under way, or whose page or row has changed since
    /// it began, is dropped.
    pub(super) fn scanned(&mut self, row: usize, ask: u64, text: Option<Code>) -> Task<Message> {
        let Some(scanning) = self.model.scanning.take_if(|s| s.ask == ask) else {
            return Task::none();
        };
        // The scan is over, so the shell's camera stops, whatever the reply holds.
        // A reply to an ended scan was dropped above, so this stops only the scan
        // this reply ends.
        if let Some(scanner) = &self.model.scanner {
            scanner.cancel();
        }
        // The scan is over, so its preview goes with it, whatever the reply holds.
        self.model.scan_preview = None;
        if scanning.row != row || !self.scan_fits(&scanning) || self.model.busy.is_some() {
            return Task::none();
        }
        // Cancelled, or refused by the camera: nothing is read.
        let Some(Code(bytes)) = text else {
            return Task::none();
        };
        match destination(&bytes, row) {
            Ok(to) => self.update(Message::Wallet(WalletMsg::DestinationTo(row, to))),
            Err(why) => {
                self.refuse_row(row, why);
                Task::none()
            }
        }
    }

    /// Shows `why` under the address of destination row `row`, as "Continue"
    /// shows a refused address. Where the page has no such field, it is shown
    /// on the page.
    fn refuse_row(&mut self, row: usize, why: String) {
        match self.refusals() {
            Some(refused) => {
                refused.insert((row, RefusedField::Address), why);
            }
            None => self.page_error(Some(why)),
        }
    }

    /// Show `error` on the page, or clear it.
    fn page_error(&mut self, error: Option<String>) {
        if let Some(p) = self.wallet_page() {
            p.error = error;
        }
    }

    pub(super) fn on_wallet(&mut self, msg: WalletMsg) -> Task<Message> {
        // While a command the page waits on runs, the page's controls are
        // shown disabled; a message sent before they were is let go.
        let busy = self.model.busy.is_some();
        match msg {
            WalletMsg::Open(to) => {
                if !busy && self.model.wallet.is_some() {
                    // A phone's send flow starts at the top of its first
                    // page, as a page drawn over another does (see `page_top`).
                    let entering = self.model.phone()
                        && matches!(to, To::Send(_))
                        && !self.model.send_in_progress();
                    // The licences open at their top, and so does Settings when
                    // Back returns from them: pages share one scrollable widget,
                    // so a page otherwise keeps its predecessor's scroll.
                    let licences = matches!(to, To::Licences)
                        || matches!(self.wallet_page().map(|p| &p.page), Some(Page::Licences));
                    self.open(to);
                    if entering || licences {
                        return crate::screens::page_top();
                    }
                }
            }
            WalletMsg::ReceiveFor(account) => {
                if let Some(WalletPage {
                    page: Page::Receive(r),
                    ..
                }) = self.wallet_page()
                {
                    // The code goes with the answer it was built from, so a
                    // new account never shows the last account's code.
                    *r = ReceivePage {
                        account: Some(account),
                        ..ReceivePage::default()
                    };
                    self.send(Command::Receive { account }, Purpose::Receive);
                }
            }
            WalletMsg::Copy(text) => {
                match self.wallet_page() {
                    Some(p) => p.copied = Some(text.clone()),
                    // The stopped screen: the kept spend with these bytes.
                    None => {
                        for s in &mut self.model.signed {
                            s.copied = s.sent.sent.artifact_hex == text;
                        }
                    }
                }
                // A mobile shell copies through its own write first, which sets
                // the clipboard protection that iced's write does not. On the
                // desktop there is no shell, so iced's write is used.
                if crate::host::copy(&text) {
                    return Task::none();
                }
                return iced::clipboard::write(text);
            }
            WalletMsg::DiscoverTo(to) => {
                if let Some(WalletPage {
                    page: Page::AddAccount(a),
                    ..
                }) = self.wallet_page()
                {
                    a.to = to;
                }
            }
            WalletMsg::Discover => {
                let Some(WalletPage {
                    page: Page::AddAccount(a),
                    ..
                }) = self.wallet_page()
                else {
                    return Task::none();
                };
                match a.to.trim().parse::<u32>() {
                    Ok(to) => {
                        a.found = None;
                        a.added = None;
                        self.page_error(None);
                        self.send(Command::Discover { to }, Purpose::Discover);
                    }
                    Err(_) => self.page_error(Some(format!(
                        "Search up to which account: a number from 1 to {}.",
                        tawara_wallet_core::DISCOVER_MAX_TO
                    ))),
                }
            }
            WalletMsg::Add(account_index) => {
                self.page_error(None);
                self.send(
                    Command::Restore {
                        account_index,
                        scan_to: None,
                    },
                    Purpose::Restore,
                );
            }
            WalletMsg::From(account) => {
                // Choosing an account closes the list it was chosen from.
                if let Some(WalletPage {
                    page: Page::Send(s),
                    choosing,
                    ..
                }) = self.wallet_page()
                {
                    s.from = Some(account);
                    *choosing = false;
                }
            }
            WalletMsg::ChooseAccount(open) => {
                if let Some(WalletPage {
                    page:
                        Page::Send(SendPage {
                            stage: SendStage::To(_),
                            ..
                        }),
                    choosing,
                    ..
                }) = self.wallet_page()
                    && !busy
                {
                    *choosing = open;
                }
            }
            WalletMsg::DestinationTo(i, text) => {
                if let Some(row) = self.form().and_then(|f| f.rows.get_mut(i)) {
                    row.to = text;
                    // "Continue" checks the address again, so what was wrong
                    // with the one typed before is not kept.
                    if let Some(refused) = self.refusals() {
                        refused.remove(&(i, RefusedField::Address));
                    }
                }
            }
            WalletMsg::Amount(i, text) => {
                if let Some(row) = self.form().and_then(|f| f.rows.get_mut(i)) {
                    row.amount = text;
                }
            }
            WalletMsg::OpenMovement(n) => {
                if let Some(m) = self.model.movements.get(n).cloned() {
                    self.open(To::Tag(m.account));
                    self.model.movement_lookup = Some((m.account, m.change));
                }
            }
            WalletMsg::MemoFocus(now) => {
                let before = std::mem::replace(&mut self.model.memo_focus, now);
                if let Some(i) = before.filter(|i| Some(*i) != now)
                    && let Some(row) = self.form().and_then(|f| f.rows.get_mut(i))
                    && !row.reference.trim().is_empty()
                {
                    row.memo_checked = true;
                }
            }
            WalletMsg::Reference(i, text) => {
                if let Some(row) = self.form().and_then(|f| f.rows.get_mut(i)) {
                    row.reference = text.to_uppercase();
                    if row.reference.trim().is_empty() {
                        row.memo_checked = false;
                    }
                    if let Some(refused) = self.refusals() {
                        refused.remove(&(i, RefusedField::Reference));
                    }
                }
            }
            WalletMsg::AddDestination => {
                if let Some(f) = self.form()
                    && f.rows.len() < usize::from(MAX_DESTINATIONS)
                {
                    f.rows.push(DestinationRow::default());
                }
            }
            WalletMsg::RemoveDestination(i) => {
                if let Some(f) = self.form()
                    && f.rows.len() > 1
                    && i < f.rows.len()
                {
                    f.rows.remove(i);
                    // The rows after it move up, so no refusal is kept for
                    // the row it was, and no memo is taken to hold the focus.
                    if let Some(refused) = self.refusals() {
                        refused.clear();
                    }
                    self.model.memo_focus = None;
                }
            }
            WalletMsg::Max(i) => {
                let most = self.model.max_of(i);
                if let Some((row, most)) = self.form().and_then(|f| f.rows.get_mut(i)).zip(most) {
                    row.amount = amount::format_mcm(most);
                }
            }
            WalletMsg::Fee(text) => {
                if let Some(f) = self.form() {
                    f.fee = text;
                }
            }
            WalletMsg::BlockToLive(text) => {
                if let Some(f) = self.form() {
                    f.blk_to_live = text;
                }
            }
            WalletMsg::StakeFor(account) => {
                self.leave_page();
                if let Some(s) = self.stake_page() {
                    *s = StakePage {
                        account: Some(account),
                        ..StakePage::default()
                    };
                }
                self.page_error(None);
                self.read_stake();
            }
            WalletMsg::StakeRead => self.read_stake(),
            WalletMsg::StakeAmount(text) => {
                if let Some(s) = self.stake_page() {
                    s.amount = text;
                }
            }
            WalletMsg::StakeMax => {
                if let Some(s) = self.stake_page()
                    && let Some(Ok(a)) = s.read.as_ref().map(|r| &r.account_read)
                {
                    s.amount = amount::format_mcm_short(a.spendable.saturating_sub(2 * STAKE_FEE));
                }
            }
            WalletMsg::StakeTier(tier) => {
                if let Some(s) = self.stake_page() {
                    s.tier = tier.min(TIERS - 1);
                }
            }
            WalletMsg::StakeReview(verb) => {
                let can: Vec<AccountId> = spendable(self.model.wallet.as_ref()).collect();
                let Some(s) = self.stake_page() else {
                    return Task::none();
                };
                let Some(account) = s.account else {
                    return Task::none();
                };
                let held = match s.read.as_ref().map(|r| &r.account_read) {
                    Some(Ok(a)) => a.stake,
                    _ => None,
                };
                let locked = held.is_some_and(|k| k.locked);
                let lock_ends = held.filter(|k| !k.locked).map(|k| k.expiry);
                let action = match verb {
                    StakeVerb::Stake => match amount::parse_mcm(&s.amount) {
                        Ok(amount) => StakeAction::Stake {
                            amount,
                            tier: s.tier,
                        },
                        Err(e) => {
                            self.page_error(Some(format!("The amount: {e}.")));
                            return Task::none();
                        }
                    },
                    StakeVerb::Unstake => StakeAction::Unstake {
                        forfeit: locked,
                        lock_ends,
                    },
                    StakeVerb::Claim => StakeAction::Claim,
                };
                if !can.contains(&account) {
                    self.page_error(Some(CANNOT_SPEND.to_owned()));
                    return Task::none();
                }
                self.page_error(None);
                self.send(
                    Command::PlanStake {
                        request: StakeRequest { account, action },
                    },
                    Purpose::PlanStake,
                );
            }
            WalletMsg::StakeEdit => {
                self.leave_page();
                if let Some(s) = self.stake_page() {
                    s.review = None;
                }
                return crate::screens::page_top();
            }
            WalletMsg::StakeSign => {
                let plan = self
                    .stake_page()
                    .and_then(|s| s.review.as_ref())
                    .map(|r| r.plan);
                if let Some(plan) = plan {
                    self.page_error(None);
                    self.send(Command::ConfirmStake { plan }, Purpose::ConfirmStake);
                }
            }
            WalletMsg::Review => {
                let can: Vec<AccountId> = spendable(self.model.wallet.as_ref()).collect();
                let phone = self.model.phone();
                let Some(WalletPage {
                    page:
                        Page::Send(SendPage {
                            from: Some(from),
                            form,
                            stage: SendStage::Compose | SendStage::Amount,
                        }),
                    ..
                }) = self.wallet_page()
                else {
                    return Task::none();
                };
                let from = *from;
                let request = spend_request(from, form);
                // On a phone, a destination given an amount on page 2 may have
                // no address, since "Continue" passed it while it was empty. The
                // address is checked here, and page 1 asks for it under its
                // field, as "Continue" does.
                let addresses = if phone {
                    destination_refusals(form)
                } else {
                    Ok(BTreeMap::new())
                };
                if !can.contains(&from) {
                    self.page_error(Some(CANNOT_SPEND.to_owned()));
                    return Task::none();
                }
                match addresses {
                    Ok(refused) if refused.is_empty() => match request {
                        Ok(spend) => {
                            self.page_error(None);
                            self.send(Command::PlanSend { spend }, Purpose::PlanSend);
                        }
                        Err(e) => self.page_error(Some(e)),
                    },
                    Ok(refused) => {
                        if let Some(WalletPage {
                            page: Page::Send(s),
                            error,
                            ..
                        }) = self.wallet_page()
                        {
                            *error = None;
                            return to_destinations(s, refused);
                        }
                    }
                    Err(why) => self.page_error(Some(why)),
                }
            }
            WalletMsg::Edit => {
                let stage = composing_stage(self.model.phone());
                self.leave_page();
                if let Some(WalletPage {
                    page: Page::Send(s),
                    ..
                }) = self.wallet_page()
                {
                    s.stage = stage;
                    // The form starts at the top of its page; drawn in the
                    // review's place, it would otherwise keep the review's
                    // scroll.
                    return crate::screens::page_top();
                }
            }
            WalletMsg::Sign => {
                // A desktop signs from its review, and a phone from its own
                // signing page, which the review leads to.
                let phone = self.model.phone();
                if let Some(WalletPage {
                    page: Page::Send(SendPage { stage, .. }),
                    ..
                }) = self.wallet_page()
                {
                    let plan = match stage {
                        SendStage::Review(plan) if !phone => plan.plan,
                        SendStage::Sign(plan) if phone => plan.plan,
                        _ => return Task::none(),
                    };
                    self.page_error(None);
                    self.send(Command::ConfirmSend { plan }, Purpose::ConfirmSend);
                }
            }
            WalletMsg::Continue => {
                let Some(WalletPage {
                    page: Page::Send(s),
                    error,
                    choosing,
                    ..
                }) = self.wallet_page()
                else {
                    return Task::none();
                };
                if !matches!(s.stage, SendStage::To(_)) {
                    return Task::none();
                }
                match destination_refusals(&s.form) {
                    Ok(refused) if refused.is_empty() => {
                        *error = None;
                        *choosing = false;
                        s.stage = SendStage::Amount;
                        return crate::screens::page_top();
                    }
                    Ok(refused) => {
                        *error = None;
                        return to_destinations(s, refused);
                    }
                    Err(why) => {
                        *error = Some(why);
                        s.stage = SendStage::To(BTreeMap::new());
                    }
                }
            }
            WalletMsg::ToSigning => {
                let phone = self.model.phone();
                let Some(WalletPage {
                    page: Page::Send(s),
                    ..
                }) = self.wallet_page()
                else {
                    return Task::none();
                };
                if !phone || !matches!(s.stage, SendStage::Review(_)) {
                    return Task::none();
                }
                s.stage = match core::mem::take(&mut s.stage) {
                    SendStage::Review(plan) => SendStage::Sign(plan),
                    other => other,
                };
                return crate::screens::page_top();
            }
            WalletMsg::Back => {
                // Not while the worker answers a command the page sent: the
                // answer is for the page it was sent from.
                if !busy && self.model.send_steps_back() {
                    self.step_send_back();
                    return crate::screens::page_top();
                }
            }
            WalletMsg::SaveArtifact => {
                let downloads = self.model.downloads.clone();
                if let Some(WalletPage {
                    page:
                        Page::Send(SendPage {
                            stage: SendStage::Sent(sent),
                            ..
                        }),
                    ..
                }) = self.wallet_page()
                {
                    keep_bytes(downloads.as_deref(), host::share, sent);
                }
            }
            WalletMsg::ShowSigned(hex) => {
                if !busy {
                    self.show_signed(&hex);
                }
            }
            WalletMsg::SaveSigned(hex) => {
                let downloads = self.model.downloads.clone();
                if let Some(s) = self
                    .model
                    .signed
                    .iter_mut()
                    .find(|s| s.sent.sent.artifact_hex == hex)
                {
                    keep_bytes(downloads.as_deref(), host::share, &mut s.sent);
                }
            }
            WalletMsg::AskDismiss => self.ask_dismiss(true),
            WalletMsg::CancelDismiss => self.ask_dismiss(false),
            WalletMsg::DismissSigned(hex) => {
                // The dismissal leaves the page shown with the bytes, if it is
                // that page (see `Model::dismiss_signed`).
                self.model.dismiss_signed(&hex);
            }
            WalletMsg::Settle(account) => {
                self.page_error(None);
                self.send(Command::Settle { account }, Purpose::Settle(account));
            }
            WalletMsg::Check(account) => {
                self.page_error(None);
                self.send(
                    Command::Status {
                        account,
                        scan_to: None,
                    },
                    Purpose::Status,
                );
            }
            WalletMsg::Resign => {
                let Some(WalletPage {
                    page: Page::Resign(r),
                    ..
                }) = self.wallet_page()
                else {
                    return Task::none();
                };
                match spend_request(r.account, &r.form) {
                    Ok(spend) => {
                        self.page_error(None);
                        self.send(Command::Resign { spend }, Purpose::Resign);
                    }
                    Err(e) => self.page_error(Some(e)),
                }
            }
            WalletMsg::Report(key, level) => {
                if let Some(p) = self.wallet_page() {
                    p.open.insert(key, level);
                }
            }
            WalletMsg::Expand(row) => {
                if let Some(p) = self.wallet_page() {
                    p.expanded = row;
                }
            }
            WalletMsg::ArtifactHex(hex) => {
                if let Some(WalletPage {
                    page: Page::Submit(s),
                    ..
                }) = self.wallet_page()
                {
                    wipe(&mut s.hex);
                    s.hex = hex;
                }
            }
            WalletMsg::Submit => {
                let Some(WalletPage {
                    page: Page::Submit(s),
                    ..
                }) = self.wallet_page()
                else {
                    return Task::none();
                };
                let artifact_hex = s.hex.trim().to_owned();
                if artifact_hex.is_empty() {
                    self.page_error(Some("Paste the signed bytes, as hex.".to_owned()));
                } else {
                    s.result = None;
                    let (command, purpose) = submit_command(s.mode, artifact_hex);
                    self.page_error(None);
                    self.send(command, purpose);
                }
            }
            WalletMsg::Filter(filter) => {
                if let Some(WalletPage {
                    page: Page::Activity(a),
                    ..
                }) = self.wallet_page()
                {
                    a.filter = filter;
                }
            }
            WalletMsg::Search(text) => {
                if let Some(WalletPage {
                    page: Page::Activity(a),
                    ..
                }) = self.wallet_page()
                {
                    a.search = text;
                }
            }
            WalletMsg::Select(id) => {
                if let Some(WalletPage {
                    page: Page::Activity(a),
                    ..
                }) = self.wallet_page()
                {
                    a.selected = Some(id);
                }
            }
            WalletMsg::ReadActivity => self.read_activity(),
            WalletMsg::ExplorerTyped(text) => {
                if let Some(WalletPage {
                    page: Page::Explorer(e),
                    ..
                }) = self.wallet_page()
                {
                    e.search = text;
                    e.invalid = None;
                }
            }
            WalletMsg::ExplorerFind => {
                let Some(WalletPage {
                    page: Page::Explorer(e),
                    ..
                }) = self.wallet_page()
                else {
                    return Task::none();
                };
                if e.finding.is_some() {
                    return Task::none();
                }
                match Query::parse(&e.search) {
                    Err(why) => e.invalid = Some(why),
                    Ok(Query::Block(n)) => self.open(To::Block(BlockAt::Number(n))),
                    Ok(Query::Tag(account)) => {
                        self.open(To::Tag(account));
                        if let Some(WalletPage {
                            page: Page::Tag(t), ..
                        }) = self.wallet_page()
                        {
                            t.typed = true;
                        }
                    }
                    Ok(Query::Hash(hash)) => {
                        e.missed = None;
                        if self
                            .send(Command::Find(hash), Purpose::Find(hash))
                            .is_some()
                            && let Some(WalletPage {
                                page: Page::Explorer(e),
                                ..
                            }) = self.wallet_page()
                        {
                            e.finding = Some(hash);
                        }
                    }
                }
            }
            WalletMsg::ReadChain => self.read_chain(),
            WalletMsg::AutoRead(on) => {
                self.model.explorer_auto = on;
                if on {
                    self.read_chain();
                }
            }
            WalletMsg::BlockRows(from) => {
                if let Some(WalletPage {
                    page: Page::Block(b),
                    ..
                }) = self.wallet_page()
                {
                    b.rows_from = from;
                }
            }
            WalletMsg::OpenSpend(n) => {
                let spend = match self.wallet_page().map(|p| &p.page) {
                    Some(Page::Block(BlockPage {
                        read: Some(Ok(block)),
                        ..
                    })) => block.spends.get(n).cloned().map(|t| (t, block.tip)),
                    _ => None,
                };
                if let Some((transaction, tip)) = spend {
                    self.show(Page::Transaction(TransactionPage {
                        transaction,
                        from_index: false,
                        text: None,
                        tip,
                    }));
                }
            }
            WalletMsg::OpenPending(n) => {
                // From the queue's own page, or from the overview's card.
                let read = match self.wallet_page().map(|p| &p.page) {
                    Some(Page::Queue(q)) => q.read.as_ref(),
                    _ => self.model.pending.last.as_ref(),
                };
                let row = match read {
                    Some(Ok(p)) => p.rows.get(n).and_then(|r| r.transaction.clone()),
                    _ => None,
                };
                if let Some(transaction) = row {
                    self.show(Page::Transaction(TransactionPage {
                        transaction,
                        from_index: false,
                        text: None,
                        tip: None,
                    }));
                }
            }
            WalletMsg::OpenTagRow(n) => {
                let row = match self.wallet_page().map(|p| &p.page) {
                    Some(Page::Tag(TagPage {
                        read: Some(view), ..
                    })) => view
                        .history
                        .as_ref()
                        .ok()
                        .and_then(|h| h.transactions.get(n).cloned()),
                    _ => None,
                };
                if let Some(transaction) = row {
                    self.show(Page::Transaction(TransactionPage {
                        transaction,
                        from_index: true,
                        text: None,
                        tip: None,
                    }));
                }
            }
            WalletMsg::ReadOlderTag => {
                let older = match self.wallet_page().map(|p| &p.page) {
                    Some(Page::Tag(TagPage {
                        account,
                        read: Some(view),
                        reading_older: false,
                        ..
                    })) => view
                        .history
                        .as_ref()
                        .ok()
                        .filter(|h| h.more())
                        .map(|h| (*account, h.next)),
                    _ => None,
                };
                if let Some((account, from)) = older
                    && self
                        .send(
                            Command::TagHistory { account, from },
                            Purpose::TagHistory(account),
                        )
                        .is_some()
                    && let Some(WalletPage {
                        page: Page::Tag(t), ..
                    }) = self.wallet_page()
                {
                    t.reading_older = true;
                }
            }
            WalletMsg::SendTo(account) => {
                // Only from the page of an account the person typed.
                let typed = matches!(
                    self.wallet_page().map(|p| &p.page),
                    Some(Page::Tag(t)) if t.typed && t.account == account
                );
                if busy || self.model.wallet.is_none() || !typed {
                    return Task::none();
                }
                self.open(To::Send(None));
                if let Some(WalletPage {
                    page: Page::Send(s),
                    ..
                }) = self.wallet_page()
                    && let Some(to) = account.destination()
                {
                    s.form.rows = vec![DestinationRow {
                        to,
                        ..DestinationRow::default()
                    }];
                }
                // A phone's send flow starts at the top of its first page.
                if self.model.phone() {
                    return crate::screens::page_top();
                }
            }
            WalletMsg::ReadOlderActivity => self.read_older_activity(),
            WalletMsg::Unit(unit) => {
                self.model.prefs.unit = unit;
                self.save_prefs();
            }
            WalletMsg::AutoLock(minutes) => {
                if IDLE_LOCK_MINUTES.contains(&minutes) {
                    let period = std::time::Duration::from_secs(minutes * 60);
                    self.model.prefs.idle_lock = period;
                    if let Some(w) = &self.worker {
                        w.handle.set_idle_lock(period);
                    }
                    self.save_prefs();
                }
            }
            WalletMsg::NodeTyped(url) => {
                if let Some(WalletPage {
                    page: Page::Settings(s),
                    ..
                }) = self.wallet_page()
                {
                    s.node = url;
                }
            }
            WalletMsg::SaveNode => {
                let Some(WalletPage {
                    page: Page::Settings(s),
                    ..
                }) = self.wallet_page()
                else {
                    return Task::none();
                };
                let url = s.node.trim().to_owned();
                if url.is_empty() {
                    self.page_error(Some(
                        "Type the API's address, starting https://.".to_owned(),
                    ));
                } else {
                    self.page_error(None);
                    self.send(Command::SetNode { url }, Purpose::SetNode { form: true });
                }
            }
            WalletMsg::CheckNode => self.ask_tip(),
            WalletMsg::ReviewAll => {
                if let Some(r) = self.recovery() {
                    r.result = None;
                }
                self.page_error(None);
                self.send(Command::Review, Purpose::Review);
            }
            WalletMsg::Resync(account) => {
                // The index is the one the live report names; the library
                // moves the account only to exactly that one, and only
                // forward.
                match self.reviewed(account).and_then(|a| remedy(&a.state)) {
                    Some(Remedy::Advance(advance_to)) => {
                        self.page_error(None);
                        self.send(
                            Command::Reconcile {
                                account,
                                advance_to,
                            },
                            Purpose::Reconcile,
                        );
                    }
                    _ => self.page_error(Some(
                        "The chain does not show this account ahead of this keystore: there is \
                         nothing to resync. Read again to see where it stands now."
                            .to_owned(),
                    )),
                }
            }
            WalletMsg::Rollback(account) => {
                let target = match self.reviewed(account).and_then(|a| remedy(&a.state)) {
                    Some(Remedy::Rollback(to)) => Some(to),
                    _ => None,
                };
                match target {
                    Some(to) => {
                        self.page_error(None);
                        if let Some(r) = self.recovery() {
                            r.rollback = Some((account, to));
                        }
                    }
                    None => self.page_error(Some(
                        "The chain does not show this account behind this keystore: there is \
                         nothing to roll back. Read again to see where it stands now."
                            .to_owned(),
                    )),
                }
            }
            WalletMsg::CancelRollback => {
                if let Some(r) = self.recovery() {
                    r.rollback = None;
                }
            }
            WalletMsg::ConfirmRollback => {
                let asked = self.recovery().and_then(|r| r.rollback.take());
                if let Some((account, rollback_to)) = asked {
                    self.page_error(None);
                    self.send(
                        Command::Rollback {
                            account,
                            rollback_to,
                        },
                        Purpose::Reconcile,
                    );
                }
            }
            WalletMsg::SearchFurther(account) => {
                self.page_error(None);
                self.send(
                    Command::Status {
                        account,
                        scan_to: Some(SEARCH_FURTHER_TO),
                    },
                    Purpose::Status,
                );
            }
        }
        Task::none()
    }

    /// After a restore from a phrase: the Add account page, with a sweep of
    /// the default range sent, and each account it finds with a balance
    /// added in turn ([`AutoAdd`]).
    pub(super) fn discover_after_restore(&mut self) {
        let Some(p) = self.wallet_page() else {
            return;
        };
        p.page = Page::AddAccount(AddAccountPage {
            auto: Some(AutoAdd {
                running: true,
                ..AutoAdd::default()
            }),
            ..AddAccountPage::default()
        });
        let to = tawara_wallet_core::DISCOVER_DEFAULT_TO;
        if self
            .send(Command::Discover { to }, Purpose::Discover)
            .is_none()
        {
            self.end_auto_add();
        }
    }

    /// The next account [`AutoAdd`] adds, sent; with none left, it ends.
    fn auto_add_next(&mut self) {
        let next = match self.wallet_page() {
            Some(WalletPage {
                page: Page::AddAccount(a),
                ..
            }) => match &mut a.auto {
                Some(auto) if auto.running && auto.swept && auto.current.is_none() => {
                    if auto.queue.is_empty() {
                        auto.running = false;
                        None
                    } else {
                        let index = auto.queue.remove(0);
                        auto.current = Some(index);
                        Some(index)
                    }
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(account_index) = next
            && self
                .send(
                    Command::Restore {
                        account_index,
                        scan_to: None,
                    },
                    Purpose::Restore,
                )
                .is_none()
        {
            self.end_auto_add();
        }
    }

    /// Ends [`AutoAdd`] where it is: the account being added, and those left,
    /// are not added.
    fn end_auto_add(&mut self) {
        if let Some(WalletPage {
            page: Page::AddAccount(a),
            ..
        }) = self.wallet_page()
            && let Some(auto) = &mut a.auto
            && auto.running
        {
            auto.running = false;
            auto.not_added.extend(auto.current.take());
            auto.not_added.append(&mut auto.queue);
        }
    }

    /// The worker's answer to a command a wallet page sent. Refusals are
    /// shown on the page by [`App::refused`], apart from the ones that move
    /// the page. The task returned is for the runtime to run: a review that
    /// is entered returns one that scrolls its page to the top.
    pub(super) fn on_wallet_reply(&mut self, purpose: Purpose, reply: Reply) -> Task<Message> {
        match (purpose, reply) {
            // An explorer page's answer goes to it while it shows what was
            // asked for; another page has nothing to do with it.
            (Purpose::Block(at), Reply::Block(read)) => {
                if let Some(WalletPage {
                    page: Page::Block(b),
                    ..
                }) = self.wallet_page()
                    && b.at == at
                {
                    b.read = Some(read);
                }
            }
            (Purpose::Tag(account), Reply::Tag(view)) => {
                if let Some(WalletPage {
                    page: Page::Tag(t), ..
                }) = self.wallet_page()
                    && t.account == account
                {
                    t.read = Some(view);
                }
                // A movement's account page, opened for its transaction: the
                // newest transaction in its history that moved the balance by
                // that much is opened; otherwise the page stays.
                if let Some((wanted, change)) = self.model.movement_lookup.take() {
                    let found = match self.wallet_page().map(|p| &p.page) {
                        Some(Page::Tag(TagPage {
                            account: shown,
                            read: Some(view),
                            ..
                        })) if *shown == wanted => movement_transaction(view, change),
                        _ => None,
                    };
                    if let Some(transaction) = found {
                        self.show(Page::Transaction(TransactionPage {
                            transaction,
                            from_index: true,
                            text: None,
                            tip: None,
                        }));
                    }
                }
            }
            // A page is added below the rows already read; a refusal leaves
            // them as they were, as Activity's does.
            (Purpose::TagHistory(account), reply) => {
                if let Some(WalletPage {
                    page: Page::Tag(t), ..
                }) = self.wallet_page()
                    && t.account == account
                {
                    t.reading_older = false;
                    if let Reply::TagHistory(Ok(older)) = reply
                        && let Some(view) = &mut t.read
                        && let Ok(history) = &mut view.history
                    {
                        history.extend(older);
                    }
                }
            }
            (Purpose::Find(hash), Reply::Found(found)) => {
                let Some(WalletPage {
                    page: Page::Explorer(e),
                    ..
                }) = self.wallet_page()
                else {
                    return Task::none();
                };
                if e.finding != Some(hash) {
                    return Task::none();
                }
                e.finding = None;
                match *found {
                    Found::Transaction { transaction, text } => {
                        self.show(Page::Transaction(TransactionPage {
                            transaction,
                            from_index: true,
                            text: Some(text),
                            tip: None,
                        }));
                    }
                    Found::Block(block) => self.show(Page::Block(BlockPage {
                        at: BlockAt::Number(block.summary.index),
                        read: Some(Ok(block)),
                        rows_from: 0,
                    })),
                    Found::Neither {
                        searched,
                        transaction,
                        block,
                    } => {
                        e.missed = Some(Box::new(Missed {
                            searched,
                            transaction,
                            block,
                        }));
                    }
                }
            }
            (Purpose::Queue, reply) => {
                if let Some(WalletPage {
                    page: Page::Queue(q),
                    error,
                    ..
                }) = self.wallet_page()
                {
                    match reply {
                        Reply::Pending(read) => q.read = Some(read),
                        Reply::Refused(r) => *error = Some(r.text),
                        _ => {}
                    }
                }
            }
            (Purpose::Find(hash), Reply::Refused(r)) => {
                if let Some(WalletPage {
                    page: Page::Explorer(e),
                    error,
                    ..
                }) = self.wallet_page()
                    && e.finding == Some(hash)
                {
                    e.finding = None;
                    *error = Some(r.text);
                }
            }
            (Purpose::Block(at), Reply::Refused(r)) => {
                if let Some(WalletPage {
                    page: Page::Block(b),
                    error,
                    ..
                }) = self.wallet_page()
                    && b.at == at
                {
                    *error = Some(r.text);
                }
            }
            (Purpose::Tag(account), Reply::Refused(r)) => {
                if let Some(WalletPage {
                    page: Page::Tag(t),
                    error,
                    ..
                }) = self.wallet_page()
                    && t.account == account
                {
                    *error = Some(r.text);
                }
            }
            (Purpose::Receive, Reply::Receive(view)) => {
                if let Some(WalletPage {
                    page: Page::Receive(r),
                    ..
                }) = self.wallet_page()
                    && r.account == Some(view.account)
                {
                    *r = ReceivePage::answered(view.account, view);
                }
            }
            (Purpose::Discover, Reply::Discovered { text, accounts }) => {
                if let Some(WalletPage {
                    page: Page::AddAccount(a),
                    ..
                }) = self.wallet_page()
                {
                    if let Some(auto) = &mut a.auto
                        && auto.running
                        && !auto.swept
                    {
                        auto.swept = true;
                        auto.queue = AutoAdd::funded(&accounts);
                    }
                    a.found = Some((text, accounts));
                }
                self.auto_add_next();
            }
            (Purpose::Restore, Reply::Restored { ok, text, opened }) => {
                let reopened = opened.is_ok();
                match opened {
                    Ok(view) => self.model.wallet = Some(view),
                    // The store could not be read back and is closed; the
                    // lock that follows says so.
                    Err(r) => self.page_error(Some(r.text)),
                }
                if let Some(WalletPage {
                    page: Page::AddAccount(a),
                    ..
                }) = self.wallet_page()
                {
                    match &mut a.auto {
                        Some(auto) if auto.running => {
                            // The sweep's rows stay, with the added account
                            // marked held, so the next one is added from
                            // them.
                            if let Some(index) = auto.current.take() {
                                if ok {
                                    auto.added.push(index);
                                    if let Some((_, rows)) = &mut a.found
                                        && let Some(row) =
                                            rows.iter_mut().find(|r| r.account_index == index)
                                    {
                                        row.held = true;
                                    }
                                } else {
                                    auto.not_added.push(index);
                                }
                            }
                            if !reopened {
                                auto.running = false;
                            }
                        }
                        // What the sweep showed is out of date now.
                        _ => a.found = None,
                    }
                    a.added = Some((ok, text));
                }
                self.auto_add_next();
            }
            (Purpose::Discover | Purpose::Restore, Reply::Refused(r)) => {
                self.end_auto_add();
                self.refused(r);
            }
            (Purpose::StakeInfo, Reply::StakeInfo(view)) => {
                if let Some(s) = self.stake_page()
                    && s.account == Some(view.account)
                {
                    s.reading = false;
                    s.read = Some(view);
                }
            }
            (Purpose::StakeInfo, Reply::Refused(r)) => {
                if let Some(s) = self.stake_page() {
                    s.reading = false;
                }
                self.refused(r);
            }
            (Purpose::PlanStake, Reply::StakePlanned(plan)) => {
                if let Some(s) = self.stake_page()
                    && s.review.is_none()
                    && s.account == Some(plan.account)
                {
                    s.review = Some(plan);
                    return crate::screens::page_top();
                }
                // The page was left while it was being laid out.
                self.send(Command::DiscardPlan, Purpose::DiscardPlan);
            }
            // Signing was refused, by a check before the key was reserved or by
            // the library: back to the stake page, with why, and the account
            // read again (as for a spend's refused sign, the library may have
            // recorded a reservation before refusing).
            (Purpose::ConfirmStake, Reply::Refused(r)) => {
                let account = self.stake_page().and_then(|s| {
                    s.review = None;
                    s.account
                });
                if let Some(account) = account.filter(|_| r.kind == RefusalKind::Library) {
                    self.send(
                        Command::Status {
                            account,
                            scan_to: None,
                        },
                        Purpose::Poll,
                    );
                }
                self.refused(r);
                self.read_stake();
                return crate::screens::page_top();
            }
            (Purpose::PlanSend, Reply::Planned(plan)) => {
                if let Some(WalletPage {
                    page: Page::Send(s),
                    ..
                }) = self.wallet_page()
                    && matches!(s.stage, SendStage::Compose | SendStage::Amount)
                {
                    s.stage = SendStage::Review(Box::new(plan));
                    // The review starts at the top of its page, so that the
                    // payees and the amount are read before "Sign & submit" is
                    // reached.
                    return crate::screens::page_top();
                } else {
                    // The page was left while it was being laid out, or is no
                    // longer one that lays a spend out.
                    self.send(Command::DiscardPlan, Purpose::DiscardPlan);
                }
            }
            (
                Purpose::ConfirmSend | Purpose::Resign | Purpose::ConfirmStake,
                Reply::Sent(mut sent),
            ) => {
                // The sent page is always shown: it carries the only copy of
                // the signed bytes.
                self.model.wallet = Some(sent.view.clone());
                let from = Some(sent.from);
                let form = match self.wallet_page().map(|p| &p.page) {
                    Some(Page::Send(s)) => s.form.clone(),
                    Some(Page::Resign(r)) => r.form.clone(),
                    _ => SpendForm::default(),
                };
                // Bytes a re-sign gives again that were saved before count as
                // saved.
                let saved = self.model.saved_copy(&sent.artifact_hex).map(Ok);
                // A re-sign gives the same bytes again, and keeps their place in
                // the signing history; a new spend takes the next place.
                let order = match purpose {
                    Purpose::Resign => self.model.kept_order(&sent.artifact_hex),
                    _ => None,
                }
                .unwrap_or_else(|| self.model.next_order());
                // The page shown is that spend now: its kept copy goes, so that
                // no banner offers it beside its own page. A re-sign whose read
                // of the store failed reports no index, and keeps the one its
                // kept copy had, for the Re-sign form.
                let earlier = match purpose {
                    Purpose::Resign => self
                        .model
                        .discard_signed(|s| s.sent.sent.artifact_hex == sent.artifact_hex),
                    _ => Vec::new(),
                };
                if sent.spent_index.is_none() {
                    sent.spent_index = earlier.iter().find_map(|s| s.sent.sent.spent_index);
                }
                self.model.screen = Screen::Wallet(WalletPage {
                    page: Page::Send(SendPage {
                        from,
                        form,
                        stage: SendStage::Sent(Box::new(SentPage {
                            saved,
                            sent,
                            order,
                            resigned: matches!(purpose, Purpose::Resign),
                            dismissing: false,
                        })),
                    }),
                    // The three facts are the page's point: their summary
                    // starts open.
                    open: [(ReportKey::Sent, Level::Summary)].into(),
                    ..WalletPage::default()
                });
                // The sent page starts at its top; drawn in the review's place,
                // it would otherwise keep the review's scroll, with the three
                // facts out of view.
                return crate::screens::page_top();
            }
            // Signing was refused; the worker dropped the plan. Back to the
            // form, with why: on a phone, to page 2, where "Review spend" is.
            (Purpose::ConfirmSend, Reply::Refused(r)) => {
                // The library records the reservation before it signs, so a
                // refused sign may have left one with no bytes. The account is
                // read again, quietly, as a poll is, so that its row follows
                // what the store holds.
                let from = match self.wallet_page().map(|p| &p.page) {
                    Some(Page::Send(s)) => s.stage.plan().map(|plan| plan.from),
                    _ => None,
                };
                let phone = self.model.phone();
                if let Some(account) = from.filter(|_| r.kind == RefusalKind::Library) {
                    self.send(
                        Command::Status {
                            account,
                            scan_to: None,
                        },
                        Purpose::Poll,
                    );
                }
                if let Some(WalletPage {
                    page: Page::Send(s),
                    error,
                    ..
                }) = self.wallet_page()
                {
                    s.stage = composing_stage(phone);
                    *error = Some(r.text);
                    // The refusal is shown beside the button that caused it. On
                    // a desktop that is at the form's top, so the form opens at
                    // its top rather than at the review's scroll. On a phone it
                    // is beside "Review spend" at the foot of page 2, so that
                    // page opens at its end.
                    return if phone {
                        crate::screens::page_end()
                    } else {
                        crate::screens::page_top()
                    };
                }
            }
            (Purpose::Settle(_), Reply::Settled { text, view }) => {
                self.model.wallet = Some(view);
                if let Some(WalletPage {
                    page: Page::Account(a),
                    ..
                }) = self.wallet_page()
                {
                    a.report = Some((Done::Settled, text));
                }
            }
            // A poll answers as a check does, but reports nothing of its own: a
            // poll that is refused or unanswered changes nothing, and the next
            // one asks again.
            (
                Purpose::Poll,
                Reply::Status {
                    account,
                    state,
                    spendable,
                    settled,
                    ..
                },
            ) => {
                self.set_row(account, state, spendable);
                if let Some(settled) = settled {
                    self.model.show_settled(settled);
                }
            }
            (Purpose::Poll, _) => {}
            // A balance check answers as a poll does, and its account's balance
            // no longer reads "Checking"; one refused or unanswered leaves it
            // so until one answers.
            (
                Purpose::Balance,
                Reply::Status {
                    account,
                    state,
                    spendable,
                    settled,
                    ..
                },
            ) => {
                super::trace(|| format!("balance check answered: {} {state:?}", account.hex()));
                self.model.balance_checking.remove(&account);
                self.set_row(account, state, spendable);
                if let Some(settled) = settled {
                    self.model.show_settled(settled);
                }
            }
            // A kept copy sent again in its turn: the account read after it,
            // quietly; a refusal leaves the account to the next turn.
            (Purpose::Resend(account), Reply::Resubmitted { view, .. }) => {
                super::trace(|| format!("kept copy sent again: {}", account.hex()));
                self.model.balance_checking.remove(&account);
                self.model.wallet = Some(view);
            }
            (Purpose::Resend(account), other) => {
                super::trace(|| format!("kept copy not sent: {} {other:?}", account.hex()));
                self.model.balance_checking.remove(&account);
            }
            (Purpose::Balance, other) => {
                super::trace(|| format!("balance check refused: {other:?}"));
            }
            (
                Purpose::Status,
                Reply::Status {
                    account,
                    state,
                    spendable,
                    text,
                    settled,
                },
            ) => {
                // The worker's word on spending, not one worked out here: a
                // check that failed and then one that reconciled leave an
                // account the wallet held spendable again, and one set aside
                // when it opened stays aside.
                self.set_row(account, state.clone(), spendable);
                // A check that settled the account says so, with the page of
                // its settlement, in place of what the check found.
                let was_settled = settled.is_some();
                if let Some(settled) = settled {
                    self.model.show_settled(settled);
                }
                let Some(p) = self.wallet_page() else {
                    return Task::none();
                };
                match &mut p.page {
                    Page::Account(a) if a.account == account && !was_settled => {
                        a.report = Some((Done::Checked, text));
                    }
                    // A further search on the recovery page: the account's
                    // report, as it stands now, folded to its summary.
                    Page::Recovery(r) => {
                        if let Some((n, entry)) = r
                            .reports
                            .iter_mut()
                            .flatten()
                            .enumerate()
                            .find(|(_, a)| a.account == account)
                        {
                            *entry = AccountReport {
                                account,
                                state,
                                spendable,
                                text,
                            };
                            p.open.insert(
                                ReportKey::Review(u16::try_from(n).unwrap_or(u16::MAX)),
                                Level::Summary,
                            );
                        }
                    }
                    _ => {}
                }
            }
            (Purpose::Review, Reply::Reviewed(reports)) => {
                // The worker applied what it found; the view follows.
                if let Some(w) = &mut self.model.wallet {
                    for found in &reports {
                        if let Some(row) = w.accounts.iter_mut().find(|a| a.id == found.account) {
                            row.set_state(found.state.clone());
                            row.spendable = found.spendable;
                        }
                    }
                }
                if let Some(p) = self.wallet_page()
                    && let Page::Recovery(r) = &mut p.page
                {
                    // Each opens to its summary.
                    for n in 0..reports.len() {
                        p.open.insert(
                            ReportKey::Review(u16::try_from(n).unwrap_or(u16::MAX)),
                            Level::Summary,
                        );
                    }
                    r.reports = Some(reports);
                }
            }
            (
                Purpose::Reconcile,
                Reply::Reconciled {
                    ok, text, opened, ..
                },
            ) => {
                let reopened = match opened {
                    Ok(view) => {
                        self.model.wallet = Some(view);
                        true
                    }
                    // The store could not be read back and is closed; the
                    // lock that follows says so.
                    Err(r) => {
                        self.page_error(Some(r.text));
                        false
                    }
                };
                if let Some(p) = self.wallet_page()
                    && let Page::Recovery(r) = &mut p.page
                {
                    r.result = Some((ok, text));
                    p.open.insert(ReportKey::Advanced, Level::Summary);
                }
                // The reports above are from before it: read them again.
                if reopened {
                    self.send(Command::Review, Purpose::Review);
                }
            }
            (Purpose::SubmitArtifact, Reply::Submitted { accepted, text }) => {
                if let Some(WalletPage {
                    page: Page::Submit(s),
                    ..
                }) = self.wallet_page()
                {
                    s.result = Some((accepted, text));
                }
            }
            // A saved copy written to the node. The view is the worker's read
            // after the write, so the account's row is the store as it stands
            // now. A refusal is the page's error, as any other.
            (Purpose::Resubmit(_), Reply::Resubmitted { text, view, .. }) => {
                self.model.wallet = Some(view);
                if let Some(WalletPage {
                    page: Page::Submit(s),
                    ..
                }) = self.wallet_page()
                {
                    s.result = Some((true, text));
                }
            }
            // A saved copy the library refused. Its write may have reached the
            // node before the refusal, so the account is read again, quietly, as
            // after a refused sign (ConfirmSend above). The read is a poll: what
            // it finds, and any settlement, is shown as a poll's is.
            (Purpose::Resubmit(account), Reply::Refused(r)) => {
                if r.kind == RefusalKind::Library {
                    self.send(
                        Command::Status {
                            account,
                            scan_to: None,
                        },
                        Purpose::Poll,
                    );
                }
                self.refused(r);
            }
            (_, Reply::Refused(r)) => self.refused(r),
            _ => {}
        }
        Task::none()
    }
}

/// What the Submit page sends for `artifact_hex` in `mode`: the bytes of an
/// open reservation go with `SubmitArtifact`, and a saved copy of a reverted
/// settle with `ResubmitSaved`, for its account.
fn submit_command(mode: SubmitMode, artifact_hex: String) -> (Command, Purpose) {
    match mode {
        SubmitMode::Artifact => (
            Command::SubmitArtifact { artifact_hex },
            Purpose::SubmitArtifact,
        ),
        SubmitMode::Resubmit(account) => (
            Command::ResubmitSaved {
                account,
                artifact_hex,
            },
            Purpose::Resubmit(account),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> AccountId {
        AccountId::from_tag([7; 20])
    }

    #[test]
    fn w9_sends_a_saved_copy_of_a_reverted_settle_as_resubmit_and_the_rest_as_before() {
        // The mode decides the command: an open reservation's bytes go as
        // SubmitArtifact, and only a saved copy goes as ResubmitSaved, for its
        // account.
        let account = account();
        let (command, purpose) = submit_command(SubmitMode::Artifact, "4f2a".to_owned());
        assert!(
            matches!(&command, Command::SubmitArtifact { artifact_hex } if artifact_hex == "4f2a"),
            "{command:?}"
        );
        assert!(matches!(purpose, Purpose::SubmitArtifact));
        let (command, purpose) = submit_command(SubmitMode::Resubmit(account), "4f2a".to_owned());
        assert!(
            matches!(
                &command,
                Command::ResubmitSaved { account: a, artifact_hex }
                    if *a == account && artifact_hex == "4f2a"
            ),
            "{command:?}"
        );
        assert!(matches!(purpose, Purpose::Resubmit(p) if p == account));
    }

    fn row(to: &str, amount: &str, reference: &str) -> DestinationRow {
        DestinationRow {
            to: to.to_owned(),
            amount: amount.to_owned(),
            reference: reference.to_owned(),
            memo_checked: false,
        }
    }

    #[test]
    fn a_spend_is_read_exactly_from_what_was_typed() {
        let form = SpendForm {
            rows: vec![
                row(" dest-a ", "420.5", "PAYROLL-11"),
                DestinationRow::default(),
                row("dest-b", "0.000000001", ""),
            ],
            fee: "1000".to_owned(),
            blk_to_live: "871200".to_owned(),
        };
        let spend = spend_request(account(), &form).unwrap();
        assert_eq!(spend.from, account());
        assert_eq!(spend.fee, Some(1000));
        assert_eq!(spend.blk_to_live, 871_200);
        assert_eq!(
            spend.destinations,
            vec![
                DestinationInput {
                    to: "dest-a".to_owned(),
                    amount: Amount::Nano(420_500_000_000),
                    reference: "PAYROLL-11".to_owned(),
                },
                DestinationInput {
                    to: "dest-b".to_owned(),
                    amount: Amount::Nano(1),
                    reference: String::new(),
                },
            ],
            "an empty row is left out, and nothing is rounded"
        );
    }

    #[test]
    fn the_send_pages_spend_pays_the_fee_per_destination_and_has_no_block_to_live() {
        let form = SpendForm {
            rows: vec![row("dest", "1", "")],
            ..SpendForm::default()
        };
        let spend = spend_request(account(), &form).unwrap();
        assert_eq!(spend.destinations[0].amount, Amount::Nano(1_000_000_000));
        assert_eq!(spend.fee, None, "the fee per destination");
        assert_eq!(spend.blk_to_live, 0, "none");

        // An amount is always required.
        let blank = SpendForm {
            rows: vec![row("dest", "", "")],
            ..SpendForm::default()
        };
        assert!(spend_request(account(), &blank).is_err());
    }

    #[test]
    fn max_is_the_balance_less_every_other_amount_and_every_destinations_fee() {
        const MCM: u64 = 1_000_000_000;
        let fee = tawara_wallet_core::FEE_PER_DESTINATION;
        let mut form = SpendForm {
            rows: vec![row("dest-a", "", "")],
            ..SpendForm::default()
        };
        // One destination: the balance less its fee, so the change is zero.
        assert_eq!(max_amount(&form, 0, 10 * MCM), Some(10 * MCM - fee));

        // A second: less the first's amount and both fees.
        form.rows[0].amount = "3".to_owned();
        form.rows.push(row("dest-b", "", ""));
        assert_eq!(max_amount(&form, 1, 10 * MCM), Some(7 * MCM - 2 * fee));

        // Max on the first with the second's amount typed: the same rule.
        form.rows[1].amount = "2".to_owned();
        assert_eq!(max_amount(&form, 0, 10 * MCM), Some(8 * MCM - 2 * fee));

        // An empty row is no destination and pays no fee.
        form.rows.push(DestinationRow::default());
        assert_eq!(max_amount(&form, 0, 10 * MCM), Some(8 * MCM - 2 * fee));

        // The Re-sign form's own fee, when given, is the fee.
        form.fee = "1000".to_owned();
        assert_eq!(max_amount(&form, 0, 10 * MCM), Some(8 * MCM - 1000));
        form.fee.clear();

        // Not offered when the rest does not fit, or another amount is not MCM.
        assert_eq!(max_amount(&form, 0, 2 * MCM), None);
        form.rows[1].amount = "two".to_owned();
        assert_eq!(max_amount(&form, 0, 10 * MCM), None);
        assert_eq!(max_amount(&form, 9, 10 * MCM), None);
    }

    #[test]
    fn what_cannot_be_a_spend_is_said_before_the_worker_is_asked() {
        let empty = SpendForm::default();
        assert!(spend_request(account(), &empty).is_err());
        let bad_amount = SpendForm {
            rows: vec![row("dest", "1.0000000001", "")],
            ..SpendForm::default()
        };
        let e = spend_request(account(), &bad_amount).unwrap_err();
        assert!(e.starts_with("Destination 1:"), "{e}");
        let bad_fee = SpendForm {
            rows: vec![row("dest", "1", "")],
            fee: "0.5".to_owned(),
            ..SpendForm::default()
        };
        assert!(
            spend_request(account(), &bad_fee)
                .unwrap_err()
                .starts_with("The fee")
        );
        let bad_btl = SpendForm {
            rows: vec![row("dest", "1", "")],
            blk_to_live: "soon".to_owned(),
            ..SpendForm::default()
        };
        assert!(spend_request(account(), &bad_btl).is_err());
    }

    /// A share sheet that must not open: with a Downloads folder the bytes
    /// are written there, and the sheet is not consulted at all.
    fn sheet_not_wanted(_: &str, _: &str) -> Option<Result<(), String>> {
        panic!("the share sheet was consulted with a Downloads folder")
    }

    #[test]
    fn save_artifact_writes_the_file_when_there_is_a_downloads_folder() {
        let sent = tawara_wallet_core::sample::sent_view();
        let downloads = std::env::temp_dir().join(format!(
            "tawara-test-{}-save-artifact-downloads",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&downloads);
        let path = save(Some(&downloads), sheet_not_wanted, &sent).expect("the bytes are saved");
        let name = format!("{}.hex", artifact_stem(&sent));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some(name.as_str())
        );
        assert_eq!(
            std::fs::read_to_string(&path).ok(),
            Some(format!("{}\n", sent.artifact_hex))
        );
        let _ = std::fs::remove_dir_all(&downloads);
    }

    #[test]
    fn save_artifact_asks_for_the_share_sheet_without_a_downloads_folder() {
        // What the sheet was last asked for: the file's name and its text.
        static SHEET: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);
        fn sheet(name: &str, text: &str) -> Option<Result<(), String>> {
            *SHEET.lock().expect("not poisoned") = Some((name.to_owned(), text.to_owned()));
            Some(Ok(()))
        }
        let sent = tawara_wallet_core::sample::sent_view();
        let saved = save(None, sheet, &sent);
        let (name, text) = SHEET
            .lock()
            .expect("not poisoned")
            .take()
            .expect("the sheet was asked for");
        assert_eq!(name, format!("{}.hex", artifact_stem(&sent)));
        assert_eq!(text, format!("{}\n", sent.artifact_hex));
        // The sheet's Ok is not a save, so the spend still counts as not
        // saved and its banner stays.
        assert_eq!(saved, Err(SHARED.to_owned()));
        let signed = Signed {
            sent: SentPage {
                sent,
                order: 0,
                resigned: false,
                saved: Some(saved),
                dismissing: false,
            },
            form: SpendForm::default(),
            open: BTreeMap::new(),
            resume: false,
            copied: false,
        };
        assert!(signed.unsaved());
    }

    #[test]
    fn save_artifact_says_why_the_share_sheet_did_not_open() {
        fn refused(_: &str, _: &str) -> Option<Result<(), String>> {
            Some(Err("no window".to_owned()))
        }
        let sent = tawara_wallet_core::sample::sent_view();
        let why = save(None, refused, &sent).expect_err("nothing is saved");
        assert!(why.contains("no window"), "{why}");
        assert_eq!(
            why,
            "The bytes could not be shared: no window. Copy the hex instead and keep \
             it somewhere safe."
        );
    }

    #[test]
    fn save_artifact_without_a_shell_says_there_is_no_downloads_folder() {
        fn no_shell(_: &str, _: &str) -> Option<Result<(), String>> {
            None
        }
        let sent = tawara_wallet_core::sample::sent_view();
        assert_eq!(
            save(None, no_shell, &sent),
            Err(
                "This system names no Downloads folder, so the bytes were not saved. \
                 Copy the hex instead and keep it somewhere safe."
                    .to_owned()
            )
        );
    }

    /// A spend's page as the sent page first shows it: nothing saved.
    fn sent_page() -> SentPage {
        SentPage {
            sent: tawara_wallet_core::sample::sent_view(),
            order: 0,
            resigned: false,
            saved: None,
            dismissing: false,
        }
    }

    #[test]
    fn save_artifact_on_a_phone_keeps_the_asked_for_sheet_as_shared_and_not_saved() {
        // When the sheet was asked for, the page keeps SHARED, shown as a
        // note, and the spend still counts as not saved.
        fn asked(_: &str, _: &str) -> Option<Result<(), String>> {
            Some(Ok(()))
        }
        let mut page = sent_page();
        keep_bytes(None, asked, &mut page);
        assert_eq!(page.saved, Some(Err(SHARED.to_owned())));
        assert!(page.shared(), "the sheet asked for is shown as a note");
    }

    #[test]
    fn save_artifact_on_a_phone_keeps_a_sheet_that_did_not_open_as_a_refusal() {
        fn refused(_: &str, _: &str) -> Option<Result<(), String>> {
            Some(Err("no window".to_owned()))
        }
        let mut page = sent_page();
        keep_bytes(None, refused, &mut page);
        assert_eq!(
            page.saved,
            Some(Err(
                "The bytes could not be shared: no window. Copy the hex instead \
                      and keep it somewhere safe."
                    .to_owned()
            ))
        );
        assert!(!page.shared(), "a sheet that did not open is a refusal");
    }

    #[test]
    fn save_artifact_on_the_desktop_keeps_the_file_written_and_never_asks_the_sheet() {
        let downloads = std::env::temp_dir().join(format!(
            "tawara-test-{}-keep-bytes-downloads",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&downloads);
        let mut page = sent_page();
        keep_bytes(Some(&downloads), sheet_not_wanted, &mut page);
        let name = format!("{}.hex", artifact_stem(&page.sent));
        assert_eq!(page.saved, Some(Ok(downloads.join(name))));
        assert!(!page.shared());
        let _ = std::fs::remove_dir_all(&downloads);
    }

    #[test]
    fn continue_checks_each_typed_address_and_reference_by_its_row_and_field() {
        let good = AccountId::from_tag([9; 20])
            .destination()
            .expect("a tag renders");
        let form = SpendForm {
            rows: vec![
                row(&good, "", "PAYROLL-11"),
                DestinationRow::default(),
                row("not-a-destination", "1", "abc"),
                row("", "2", ""),
            ],
            ..SpendForm::default()
        };
        let refused = destination_refusals(&form).expect("destinations are typed");
        assert_eq!(
            refused.keys().copied().collect::<Vec<_>>(),
            [
                (2, RefusedField::Address),
                (2, RefusedField::Reference),
                (3, RefusedField::Address),
            ]
        );
        // The row's number is its place in the form, as the page draws it.
        let address = &refused[&(2, RefusedField::Address)];
        assert!(
            address.starts_with("destination 3: `not-a-destination` is not a destination"),
            "{address}"
        );
        assert!(
            refused[&(2, RefusedField::Reference)]
                .starts_with("destination 3: `abc` is not a valid memo"),
            "{:?}",
            refused[&(2, RefusedField::Reference)]
        );
        assert_eq!(
            refused[&(3, RefusedField::Address)],
            "Give this destination's address."
        );
    }

    #[test]
    fn a_form_with_no_destination_typed_is_refused_as_a_whole() {
        assert_eq!(
            destination_refusals(&SpendForm::default()),
            Err("Give at least one destination.".to_owned())
        );
    }
}
