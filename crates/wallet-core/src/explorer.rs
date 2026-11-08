// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! What the node's explorer endpoints say, as the interface shows it: a
//! tag's transactions from the node's index (the activity list), the newest
//! blocks and the size of the node's queue (the wallet's network card), and
//! the explorer's pages: the chain and the queue, a block, a tag and a
//! transaction.
//!
//! The rows are the library's (`mesh::codec`), read with the command line's
//! own calls (`cli::cmd_recent_transactions_from`, `cli::cmd_blocks`,
//! `cli::cmd_mempool`, `cli::cmd_block`, `cli::cmd_transaction`) or the same
//! walks made in the worker, and every read carries the library's page for
//! it, word for word. What the interface derives from a row (direction and
//! net amount for one account) follows the command line's page for the same
//! rows (`cli::render`, `recent_transactions`), which is private there and so
//! repeated here.

use mochimo_crypto::Error;
use mochimo_crypto::mesh::codec::{
    MeshBlock, MeshTransaction, OP_DESTINATION, OP_FEE, OP_REWARD, OP_SOURCE, SearchPage,
};

use crate::view::AccountId;

/// The most rows the node's index answers for one tag in one request: one
/// page of an account's history. Older rows are read a page at a time, from
/// the offset the last page ended at.
pub const HISTORY_ROWS: u64 = 100;

/// How many of the newest blocks the wallet's network card shows.
pub const CARD_BLOCKS: u64 = 6;

/// What an operation in a transaction did, in the node's terms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationKind {
    /// A mining reward: newly minted, not moved.
    Reward,
    /// Value leaving its source. The index debits the source its gross
    /// amount and lists the change as a destination of its own.
    Source,
    /// Value arriving.
    Destination,
    /// The fee paid to the miner.
    Fee,
    /// A kind this version does not know, as the node spelled it.
    Other(String),
}

/// Who an operation's address names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Party {
    /// A twenty-byte tag: an account, which has a destination.
    Account(AccountId),
    /// A forty-byte ledger address (the tag and its key's hash), in hex. Not
    /// a destination: it must never be offered as somewhere to send funds.
    Ledger(String),
    /// Anything else, as the node sent it, made safe to show.
    Other(String),
}

/// One operation of a transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationView {
    pub kind: OperationKind,
    pub party: Party,
    /// In nanoMCM; negative for a source debited.
    pub amount: i128,
    /// The reference the node carries for it, made safe to show; empty for
    /// none. The node sends a destination's reference as its whole
    /// sixteen-byte field, a short one padded with NUL bytes, on `/block` and
    /// on the index's rows alike; the padding is not part of it and is left
    /// off.
    pub memo: String,
}

/// One transaction as the node's index holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionView {
    /// The transaction's id, in hex.
    pub id: String,
    /// The block it landed in, when the index says.
    pub block: Option<u64>,
    /// When that block was made, in milliseconds since the epoch, when the
    /// index says.
    pub time_ms: Option<i64>,
    pub operations: Vec<OperationView>,
}

/// Which way a transaction moved value for one account, as the command
/// line's page says it (`in`, `out`, `both`, `--`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// The account received.
    In,
    /// The account was a source, and nothing came back to it.
    Out,
    /// The account was a source and a destination: a spend whose change came
    /// back to it, as the index lists every spend that leaves change.
    Both,
    /// Nothing in it names the account.
    Neither,
}

impl TransactionView {
    fn touches(op: &OperationView, account: AccountId) -> bool {
        op.party == Party::Account(account)
    }

    /// What the transaction did to `account`'s balance, in nanoMCM: the sum
    /// of the operations that name it, as the command line's page sums
    /// them. For a spend that is what left net of the change that came back.
    #[must_use]
    pub fn net(&self, account: AccountId) -> i128 {
        self.operations
            .iter()
            .filter(|o| Self::touches(o, account))
            .map(|o| o.amount)
            .sum()
    }

    /// Which way it moved value for `account`, as the command line's page
    /// says it.
    #[must_use]
    pub fn direction(&self, account: AccountId) -> Direction {
        let out = self
            .operations
            .iter()
            .any(|o| o.kind == OperationKind::Source && Self::touches(o, account));
        let back = self
            .operations
            .iter()
            .any(|o| o.kind == OperationKind::Destination && Self::touches(o, account));
        match (out, back) {
            (true, true) => Direction::Both,
            (true, false) => Direction::Out,
            (false, true) => Direction::In,
            (false, false) => Direction::Neither,
        }
    }

    /// The destinations other than `account` itself: where a spend from it
    /// went, its change left out.
    pub fn paid_out(&self, account: AccountId) -> impl Iterator<Item = &OperationView> {
        self.operations
            .iter()
            .filter(move |o| o.kind == OperationKind::Destination && !Self::touches(o, account))
    }

    /// The sources other than `account`: who paid it.
    pub fn paid_by(&self, account: AccountId) -> impl Iterator<Item = &OperationView> {
        self.operations
            .iter()
            .filter(move |o| o.kind == OperationKind::Source && !Self::touches(o, account))
    }

    /// The fee, in nanoMCM, as the node lists it.
    #[must_use]
    pub fn fee(&self) -> u128 {
        self.operations
            .iter()
            .filter(|o| o.kind == OperationKind::Fee)
            .map(|o| o.amount.unsigned_abs())
            .sum()
    }

    /// Whether it is a mining reward paid to `account`.
    #[must_use]
    pub fn rewards(&self, account: AccountId) -> bool {
        self.operations
            .iter()
            .any(|o| o.kind == OperationKind::Reward && Self::touches(o, account))
    }

    /// Its destinations. As `/block` and `/mempool` list a transaction, the
    /// change is not an operation, so these are the payees; as the index
    /// lists one, the change back to the source is among them.
    pub fn destinations(&self) -> impl Iterator<Item = &OperationView> {
        self.operations
            .iter()
            .filter(|o| o.kind == OperationKind::Destination)
    }

    /// What it paid its destinations, in nanoMCM, as the command line's
    /// block and mempool pages total a transaction: for one as `/block` or
    /// `/mempool` lists it, what it sent.
    #[must_use]
    pub fn sent(&self) -> u128 {
        self.destinations().map(|o| o.amount.unsigned_abs()).sum()
    }

    /// Who it left: its first source's address.
    #[must_use]
    pub fn source(&self) -> Option<&Party> {
        self.operations
            .iter()
            .find(|o| o.kind == OperationKind::Source)
            .map(|o| &o.party)
    }

    pub(crate) fn of(tx: &MeshTransaction) -> TransactionView {
        TransactionView {
            id: hex(&tx.hash),
            block: tx.block.map(|b| b.index),
            time_ms: tx.timestamp_ms,
            operations: tx
                .operations
                .iter()
                .map(|o| OperationView {
                    kind: match o.kind.as_str() {
                        OP_REWARD => OperationKind::Reward,
                        OP_SOURCE => OperationKind::Source,
                        OP_DESTINATION => OperationKind::Destination,
                        OP_FEE => OperationKind::Fee,
                        other => OperationKind::Other(shown(other)),
                    },
                    party: party(&o.address),
                    amount: o.amount,
                    memo: shown(o.memo.trim_end_matches('\0')),
                })
                .collect(),
        }
    }
}

/// One account's transactions, newest first: the pages read so far, from
/// the newest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountHistory {
    pub account: AccountId,
    /// [`HISTORY_ROWS`] a page, each transaction once.
    pub transactions: Vec<TransactionView>,
    /// How many the index holds for it in all, as the last page read said.
    pub total: u64,
    /// The offset the next older page starts at: how many rows, newest
    /// first, the pages read so far cover. A row the index gained at the top
    /// since the first page pushes the rest down, so a later page can repeat
    /// rows already read; it is counted here and listed once.
    pub next: u64,
    /// The library's page for each page read, word for word, one after
    /// another.
    pub text: String,
}

impl AccountHistory {
    /// Whether the index holds rows older than the pages read so far.
    #[must_use]
    pub fn more(&self) -> bool {
        self.total > self.next
    }

    /// How many rows the index holds that the pages read so far do not.
    #[must_use]
    pub fn unread(&self) -> u64 {
        self.total.saturating_sub(self.next)
    }

    /// Add `older`, the page read from [`AccountHistory::next`], below the
    /// rows already read: each transaction once, the total and the offset as
    /// that page says them, and its page after the others.
    pub fn extend(&mut self, older: AccountHistory) {
        for tx in older.transactions {
            if !self.transactions.iter().any(|t| t.id == tx.id) {
                self.transactions.push(tx);
            }
        }
        self.total = older.total;
        self.next = older.next;
        if !older.text.is_empty() {
            if !self.text.is_empty() {
                self.text.push_str("\n\n");
            }
            self.text.push_str(&older.text);
        }
    }

    /// The page read at offset `from`.
    pub(crate) fn of(
        account: AccountId,
        page: &SearchPage,
        from: u64,
        text: String,
    ) -> AccountHistory {
        AccountHistory {
            account,
            transactions: page.transactions.iter().map(TransactionView::of).collect(),
            total: page.total_count,
            next: from.saturating_add(page.transactions.len() as u64),
            text,
        }
    }
}

/// What kind of block a block is (see [`kind`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// A real block: solved by proof of work before the v4 fork, made by a
    /// validator and voted on from it. Under proof of stake it can carry no
    /// transactions.
    Normal,
    /// Made when no real block came in time.
    Pseudo,
    /// The ledger, at a block number whose low byte is zero.
    Neogenesis,
}

/// The v4 fork, the node's `POSV4TRIGGER`: the neogenesis height where proof
/// of stake begins.
pub const POS_V4: u64 = 0x10_e000;

/// The difficulty a v4 pseudoblock is stamped with, `POS_DIFFICULTY_PSEUDO`.
/// A real v4 block is stamped `POS_DIFFICULTY_REAL` (42) less its attempt, so
/// never 0.
const POS_DIFFICULTY_PSEUDO: u32 = 0;

/// A block's kind. A neogenesis block is told by its number alone. Below
/// [`POS_V4`] a block whose trailer counts no transactions is a pseudoblock,
/// as the C reference (`bup.c`) and the library (`MeshBlock::kind`) test it.
/// From [`POS_V4`] a real block can carry none, and the trailer's difficulty
/// tells them apart: 0 for a pseudoblock. `None` when the node sent none of
/// the block's figures.
pub fn kind(block: &MeshBlock) -> Option<BlockKind> {
    let index = block.block.index;
    if index & 0xff == 0 {
        return Some(BlockKind::Neogenesis);
    }
    let figures = block.metadata.as_ref()?;
    let pseudo = if index >= POS_V4 {
        figures.difficulty == POS_DIFFICULTY_PSEUDO
    } else {
        figures.tx_count == 0
    };
    Some(if pseudo {
        BlockKind::Pseudo
    } else {
        BlockKind::Normal
    })
}

/// One block, as the newest blocks list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockSummary {
    pub index: u64,
    /// In hex.
    pub hash: String,
    /// When it was made, in milliseconds since the epoch.
    pub time_ms: i64,
    /// How many transactions it carries, its reward among them.
    pub transactions: usize,
    /// Its kind; `None` when the node sent none of its own figures and its
    /// number does not make it a neogenesis block.
    pub kind: Option<BlockKind>,
    /// The difficulty it was solved at, when the node sent its figures.
    pub difficulty: Option<u32>,
    /// Its reward, and who it was paid to: the miner. `None` for a block
    /// that carries none, as a pseudo-block and a neogenesis block do not.
    pub reward: Option<Reward>,
    /// How long after the block below it it was made, in milliseconds: how
    /// long it took to solve. `None` until the block below it is read.
    pub solve_ms: Option<i64>,
}

/// A block's reward: newly minted, paid to the miner who solved it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reward {
    pub to: Party,
    /// In nanoMCM.
    pub amount: u128,
}

impl BlockSummary {
    pub(crate) fn of(block: &MeshBlock) -> BlockSummary {
        // The reward transaction is the one carrying a REWARD operation, as
        // the command line's block page finds it (`cli::render`,
        // `block_page`).
        let reward = block
            .transactions
            .iter()
            .flat_map(|t| t.operations.iter())
            .find(|o| o.kind == OP_REWARD)
            .map(|o| Reward {
                to: party(&o.address),
                amount: o.amount.unsigned_abs(),
            });
        BlockSummary {
            index: block.block.index,
            hash: hex(&block.block.hash),
            time_ms: block.timestamp_ms,
            transactions: block.transactions.len(),
            kind: kind(block),
            difficulty: block.metadata.as_ref().map(|m| m.difficulty),
            reward,
            solve_ms: None,
        }
    }

    /// `blocks`, newest first, each with its solve time told from the block
    /// after it in the list when that is its parent: the block whose hash
    /// it names. Each block is asked for on its own, so the chain can
    /// reorganize between two of them, and two blocks of different branches
    /// have no solve time between them. The last has none: the block below
    /// it was not read.
    pub(crate) fn of_walk(blocks: &[MeshBlock]) -> Vec<BlockSummary> {
        let mut out: Vec<BlockSummary> = blocks.iter().map(BlockSummary::of).collect();
        for (i, pair) in blocks.windows(2).enumerate() {
            let (above, below) = (&pair[0], &pair[1]);
            if above.parent.hash == below.block.hash {
                out[i].solve_ms = between(below.timestamp_ms, above.timestamp_ms);
            }
        }
        out
    }
}

/// How long from `earlier` to `later`, both a block's time as the node sent
/// it. The node's times are any `i64` the Mesh parser accepts, so a
/// difference that overflows, or a block older than the one below it, is
/// no time at all rather than a panic or a negative one.
pub(crate) fn between(earlier: i64, later: i64) -> Option<i64> {
    later.checked_sub(earlier).filter(|ms| *ms >= 0)
}

/// How many of the newest blocks the explorer lists.
pub const LATEST_BLOCKS: u64 = 10;

/// How many blocks the explorer's average solve time is taken over.
pub const SOLVE_SPAN: u64 = 100;

/// How many transactions of the node's queue the explorer's overview reads
/// whole.
pub const PENDING_ROWS: u64 = 5;

/// The most of the node's queue one read takes whole: the command line's
/// most for `mempool --count` (`cli::args::MAX_COUNT`). The rest are counted.
pub const QUEUE_ROWS: u64 = mochimo_crypto::cli::args::MAX_COUNT;

/// A neogenesis block comes at every block number whose low byte is zero
/// (the library's `MeshBlock::kind`, the C reference's own test): every
/// 256th block.
pub const NEOGENESIS_EVERY: u64 = 256;

/// The next neogenesis block above `tip`.
#[must_use]
pub fn next_neogenesis(tip: u64) -> u64 {
    (tip / NEOGENESIS_EVERY)
        .saturating_add(1)
        .saturating_mul(NEOGENESIS_EVERY)
}

/// The chain as the explorer shows it: the tip, the newest blocks, and how
/// long a block has taken lately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainView {
    pub tip: u64,
    /// The [`LATEST_BLOCKS`] newest, newest first, each with its solve time.
    pub blocks: Vec<BlockSummary>,
    /// The time between blocks over the [`SOLVE_SPAN`] below the tip, on
    /// average, in milliseconds: the tip's time less the time of the block
    /// that many below it, over the span. `None` when the chain is not that
    /// long, or the node did not serve that block.
    pub average_ms: Option<i64>,
    /// The library's page for the blocks shown, word for word.
    pub text: String,
}

/// One transaction waiting in the node's queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRow {
    /// Its id, in hex.
    pub id: String,
    /// The transaction as `/mempool/transaction` renders it; `None` when the
    /// queue no longer held it by the time it was asked for: mined since the
    /// list was read, or dropped.
    pub transaction: Option<TransactionView>,
}

/// The node's queue, as the explorer shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingView {
    /// How many transactions wait.
    pub waiting: usize,
    /// The first of them read whole, in the queue's order: as many as were
    /// asked for, at most [`QUEUE_ROWS`].
    pub rows: Vec<PendingRow>,
    /// The library's page, word for word.
    pub text: String,
}

/// A block's own figures, as the node sent them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockFigures {
    /// In hex.
    pub nonce: String,
    /// The Merkle root, in hex.
    pub root: String,
    /// In bytes.
    pub size: u64,
    /// How many transactions it carries besides the reward.
    pub counted: u32,
    /// The least fee it took, in nanoMCM.
    pub minimum_fee: u64,
    /// For a normal block, the haiku the node sent, a line each, made safe
    /// to show; empty for any other, as the command line's block page shows
    /// it.
    pub haiku: Vec<String>,
}

/// One block, whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockDetail {
    pub summary: BlockSummary,
    /// The block below it, by number and hash.
    pub parent: u64,
    pub parent_hash: String,
    /// `None` when the node sent none of its figures.
    pub figures: Option<BlockFigures>,
    /// Its transactions other than the reward, in the block's order, as
    /// `/block` lists them: each source debited what left it net of its
    /// change, and the change not an operation.
    pub spends: Vec<TransactionView>,
    /// What every fee operation in it paid, in nanoMCM, as the command
    /// line's block page sums them.
    pub fees: u128,
    /// The chain's tip when the block was read, when the node said.
    pub tip: Option<u64>,
    /// The library's page for it, word for word.
    pub text: String,
}

impl BlockDetail {
    /// `block`, its solve time told from `parent_ms`, the time of the block
    /// below it when that was read.
    pub(crate) fn of(
        block: &MeshBlock,
        parent_ms: Option<i64>,
        tip: Option<u64>,
        text: String,
    ) -> BlockDetail {
        let mut summary = BlockSummary::of(block);
        summary.solve_ms = parent_ms.and_then(|p| between(p, block.timestamp_ms));
        let normal = summary.kind == Some(BlockKind::Normal);
        let figures = block.metadata.as_ref().map(|m| BlockFigures {
            nonce: hex(&m.nonce),
            root: hex(&m.root),
            size: m.block_size,
            counted: m.tx_count,
            minimum_fee: m.fee,
            haiku: if normal {
                m.haiku
                    .split('\n')
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(shown)
                    .collect()
            } else {
                Vec::new()
            },
        });
        let spends = block
            .transactions
            .iter()
            .filter(|t| !t.operations.iter().any(|o| o.kind == OP_REWARD))
            .map(|t| TransactionView {
                block: Some(block.block.index),
                time_ms: Some(block.timestamp_ms),
                ..TransactionView::of(t)
            })
            .collect();
        let fees = block
            .transactions
            .iter()
            .flat_map(|t| t.operations.iter())
            .filter(|o| o.kind == OP_FEE)
            .map(|o| o.amount.unsigned_abs())
            .sum();
        BlockDetail {
            summary,
            parent: block.parent.index,
            parent_hash: hex(&block.parent.hash),
            figures,
            spends,
            fees,
            tip,
            text,
        }
    }

    /// What its transactions paid their destinations, in nanoMCM: the
    /// command line's "moved". The reward is newly minted, not moved, and is
    /// not counted.
    #[must_use]
    pub fn moved(&self) -> u128 {
        self.spends.iter().map(TransactionView::sent).sum()
    }

    /// How many destinations its transactions paid.
    #[must_use]
    pub fn paid(&self) -> usize {
        self.spends.iter().map(|t| t.destinations().count()).sum()
    }

    /// How many blocks stand on it, itself among them, at the tip read with
    /// it.
    #[must_use]
    pub fn confirmations(&self) -> Option<u64> {
        self.tip?.checked_sub(self.summary.index)?.checked_add(1)
    }
}

/// What the ledger holds for a tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerRead {
    /// An entry: the ledger address it is held at now (the tag and the hash
    /// of its current key, `0x` and eighty hex digits) and its balance in
    /// nanoMCM.
    Held { address: String, balance: u64 },
    /// The node did not resolve it: the Mesh's code 4, *Account not
    /// found*, which it answers for a tag the ledger has no entry for, for
    /// one the ledger holds at zero, and for a lookup that failed (too few
    /// nodes answered, a node between blocks). It says nothing about whether
    /// the account exists (the library's `recon`, fact 3), and is shown as
    /// what the node said.
    Unresolved,
    /// The node did not say; the library's words for why.
    Refused(String),
}

/// One tag as the explorer shows it: what the ledger holds for it, and its
/// transactions from the node's index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagView {
    pub account: AccountId,
    pub ledger: LedgerRead,
    /// The newest page of its history, [`HISTORY_ROWS`] rows.
    pub history: Result<AccountHistory, ExplorerRefusal>,
}

/// What a hash names: a transaction the node's index holds, a block the
/// node serves, or neither.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    Transaction {
        transaction: TransactionView,
        /// The library's page for it, word for word.
        text: String,
    },
    Block(Box<BlockDetail>),
    /// Neither: the index's answer for a transaction, and the node's for a
    /// block, each in the library's words.
    Neither {
        /// The index answered, and holds no transaction with that id: which
        /// is not the same as there being none (the library's
        /// `Outcome::TransactionNotFound`). `false` when it was not read.
        searched: bool,
        transaction: ExplorerRefusal,
        block: ExplorerRefusal,
    },
}

/// A block to read: by its number, or by its hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockAt {
    Number(u64),
    Hash([u8; 32]),
}

/// What is typed in the explorer's search field, read as the command line
/// reads the same arguments (`cli::args`: `block_at`, `hash_arg` and
/// `tag_from_text`, which are private there and so repeated here).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Query {
    /// A block, by its number.
    Block(u64),
    /// Sixty-four hex digits: a transaction's id, or a block's hash. Nothing
    /// in the digits says which.
    Hash([u8; 32]),
    /// An account, by its Base58 address or its tag in hex.
    Tag(AccountId),
}

impl Query {
    /// `text` as one of the things the explorer finds, or why it is none of
    /// them. Spaces around it are left off, as the command line leaves them
    /// off a destination.
    ///
    /// # Errors
    /// A sentence saying what was typed and what the field takes.
    pub fn parse(text: &str) -> Result<Query, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err(
                "Type a block's number or hash, a transaction's id, an address or a tag.".into(),
            );
        }
        let body = t.strip_prefix("0x").unwrap_or(t);
        let hex_digits = !body.is_empty() && body.bytes().all(|b| b.is_ascii_hexdigit());
        // Sixty-four decimal digits are a hash whose digits are all decimal,
        // not a number.
        if t.bytes().all(|b| b.is_ascii_digit()) && t.len() != 64 {
            let n: u64 = t
                .parse()
                .map_err(|_| format!("{t} is larger than any block's number."))?;
            if n == 0 {
                // The endpoint serves index 0 as the current block, not as
                // genesis (`cli::args::block_at`).
                return Err(
                    "An API serves block 0 as its newest block, not as genesis, so it cannot be \
                     asked for by number. Blocks are numbered from 1."
                        .into(),
                );
            }
            return Ok(Query::Block(n));
        }
        if hex_digits && body.len() == 64 {
            return Ok(Query::Hash(bytes(body)));
        }
        if t.starts_with("0x") {
            if !hex_digits {
                return Err(format!("{t} starts with 0x but is not hexadecimal."));
            }
            if body.len() != 40 {
                return Err(format!(
                    "{t} has {} hex digits after 0x. A tag has 40, and a block's hash or a \
                     transaction's id 64. A ledger address, 80, is a tag and the hash of one key: \
                     its first 40 digits are the tag.",
                    body.len()
                ));
            }
            return Self::tag(bytes(body));
        }
        if hex_digits && body.len() == 40 {
            // As the command line refuses a bare hex tag: the second half of
            // a ledger address printed as eighty hex digits is forty hex
            // digits as well, a tag nobody holds.
            return Err(format!(
                "Write a tag in hex with 0x before it: 0x{t}. Hex carries no checksum, so it is \
                 read as a tag only when it says it is one: the second half of a ledger address \
                 is forty hex digits too."
            ));
        }
        match mochimo_crypto::addr::tag_from_base58(t) {
            Ok(tag) => Self::tag(tag),
            Err(e) => Err(format!(
                "{t} is not a block's number, a hash or an id (64 hex digits), or a tag (0x and \
                 40 hex digits); read as an address, {e}."
            )),
        }
    }

    /// The all-zero tag is refused, as the command line refuses it: its
    /// checksum is zero too, so the checksum cannot catch it, it is what an
    /// empty or truncated buffer holds, and nothing paid to it can be spent.
    fn tag(tag: [u8; 20]) -> Result<Query, String> {
        if tag.iter().all(|&b| b == 0) {
            return Err(
                "That is the all-zero tag, which names no one's account: nothing paid to \
                        it can ever be spent."
                    .into(),
            );
        }
        Ok(Query::Tag(AccountId::from_tag(tag)))
    }
}

/// `N` bytes from `2N` hex digits already checked as such.
fn bytes<const N: usize>(digits: &str) -> [u8; N] {
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = digits
            .get(i * 2..i * 2 + 2)
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
            .unwrap_or(0);
    }
    out
}

/// The chain's tip and the newest blocks below it, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlocksView {
    pub tip: u64,
    pub blocks: Vec<BlockSummary>,
    /// The library's page for them, word for word.
    pub text: String,
}

/// The node's queue of transactions waiting to be mined: how many wait,
/// with none of them read whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MempoolView {
    pub waiting: usize,
    /// The library's page, word for word.
    pub text: String,
}

/// Why an explorer read answered nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerRefusal {
    /// What a refused search of the node's index says about that index;
    /// `None` for any other read, and for a refusal that says nothing about
    /// it.
    pub index: Option<IndexState>,
    /// The library's page, word for word.
    pub text: String,
}

/// What a refused search of the node's transaction index says about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexState {
    /// The node runs none: a deployment indexes only when configured to,
    /// and another node may.
    Absent,
    /// The node runs one, and it did not answer: its database is not
    /// connected, or the search failed. Asked again it may answer.
    Unavailable,
}

/// What `cause`, a refused search of the node's index, says about that
/// index, by the Mesh middleware's answers. The middleware registers
/// `/search/transactions` only when its indexer is enabled at start, so a
/// node that runs none answers that route with a 404; one that runs an
/// indexer answers its internal error, code 2, while the indexer's database
/// is not connected and when a search fails. Code 1 is a request it could
/// not decode.
///
/// The library reads a refused search the same way for its own page
/// (`Outcome::SearchFailed`, `cli::search_refusal`); that reading is private
/// there and so repeated here.
pub(crate) fn index_state(cause: &Error) -> Option<IndexState> {
    match cause {
        Error::HttpStatus { status: 404 } => Some(IndexState::Absent),
        Error::Mesh { code: 2, .. } => Some(IndexState::Unavailable),
        _ => None,
    }
}

/// An operation's address, as the command line's page reads one
/// (`cli::explorer_address`): a twenty-byte tag is an account, a forty-byte
/// one a ledger address, anything else is shown as it came.
fn party(text: &str) -> Party {
    let body = text.strip_prefix("0x").unwrap_or(text);
    let bytes = || -> Option<Vec<u8>> {
        (0..body.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(body.get(i..i + 2)?, 16).ok())
            .collect()
    };
    match (body.len(), bytes()) {
        (40, Some(b)) => {
            let mut tag = [0u8; 20];
            tag.copy_from_slice(&b);
            Party::Account(AccountId::from_tag(tag))
        }
        (80, Some(_)) => Party::Ledger(format!("0x{}", body.to_ascii_lowercase())),
        _ => Party::Other(shown(text)),
    }
}

/// Text from the node, made safe to show: the command line's rule for
/// external text (`cli::terminal_text`). Controls, the bidirectional
/// formatting characters that reorder what follows them, the characters
/// that draw with no width, and the backslash are written as escapes, so a
/// node cannot make one reference look like another.
pub(crate) fn shown(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        let bidi = matches!(
            ch,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        );
        let zero_width = matches!(
            ch,
            '\u{00ad}' | '\u{180e}' | '\u{200b}'..='\u{200d}' | '\u{2060}'..='\u{2064}' | '\u{feff}'
        );
        if ch.is_control() || bidi || zero_width || ch == '\\' {
            out.extend(ch.escape_default());
        } else {
            out.push(ch);
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use mochimo_crypto::mesh::codec;

    use super::*;

    #[test]
    fn from_the_v4_fork_a_pseudoblock_is_told_by_its_difficulty_not_its_transactions() {
        let block = |index: u64, figures: Option<(u32, u32)>| MeshBlock {
            block: mochimo_crypto::mesh::ChainTip {
                index,
                hash: [0; 32],
            },
            parent: mochimo_crypto::mesh::ChainTip {
                index: index - 1,
                hash: [0; 32],
            },
            timestamp_ms: 0,
            transactions: Vec::new(),
            metadata: figures.map(|(tx_count, difficulty)| codec::BlockMetadata {
                block_size: 192,
                difficulty,
                fee: 0,
                haiku: "first line
second line"
                    .to_owned(),
                nonce: [0; 32],
                root: [0; 32],
                stime_ms: 0,
                tx_count,
            }),
        };
        // Mainnet real blocks with no transactions carry difficulty 42.
        assert_eq!(
            kind(&block(1_144_751, Some((0, 42)))),
            Some(BlockKind::Normal)
        );
        assert_eq!(
            kind(&block(POS_V4 + 1, Some((0, 0)))),
            Some(BlockKind::Pseudo)
        );
        assert_eq!(
            kind(&block(POS_V4 + 1, Some((3, 42)))),
            Some(BlockKind::Normal)
        );
        // Below the fork the v3 rule stands: a pseudoblock there inherited
        // its difficulty, so only its count of none tells it.
        assert_eq!(
            kind(&block(POS_V4 - 1, Some((0, 31)))),
            Some(BlockKind::Pseudo)
        );
        assert_eq!(
            kind(&block(POS_V4 - 1, Some((2, 31)))),
            Some(BlockKind::Normal)
        );
        assert_eq!(kind(&block(POS_V4, None)), Some(BlockKind::Neogenesis));
        assert_eq!(kind(&block(POS_V4 + 1, None)), None);
        // A real block with no transactions shows its haiku, as any normal
        // block does; a pseudoblock has none to show.
        let haiku = |figures| {
            BlockDetail::of(&block(1_144_751, Some(figures)), None, None, String::new())
                .figures
                .expect("figures sent")
                .haiku
        };
        assert_eq!(haiku((0, 42)), ["first line", "second line"]);
        assert!(haiku((0, 0)).is_empty());
    }

    #[test]
    fn a_time_or_a_tip_out_of_range_is_no_figure_rather_than_a_panic() {
        assert_eq!(between(1_000, 61_000), Some(60_000));
        assert_eq!(between(i64::MIN, 1), None, "overflows");
        assert_eq!(between(i64::MIN, i64::MAX), None, "overflows");
        assert_eq!(between(61_000, 1_000), None, "older than the block below");
        let block = |index: u64| MeshBlock {
            block: mochimo_crypto::mesh::ChainTip {
                index,
                hash: [0; 32],
            },
            parent: mochimo_crypto::mesh::ChainTip {
                index: index.saturating_sub(1),
                hash: [0; 32],
            },
            timestamp_ms: i64::MAX,
            transactions: Vec::new(),
            metadata: None,
        };
        let at = |index, tip| BlockDetail::of(&block(index), Some(i64::MIN), tip, String::new());
        assert_eq!(at(5, None).summary.solve_ms, None);
        assert_eq!(at(5, Some(7)).confirmations(), Some(3));
        assert_eq!(at(5, Some(4)).confirmations(), None, "above the tip");
        assert_eq!(at(0, Some(u64::MAX)).confirmations(), None, "overflows");
    }

    fn op(kind: &str, address: &str, amount: i128, memo: &str) -> codec::Operation {
        codec::Operation {
            index: 0,
            kind: kind.to_owned(),
            address: address.to_owned(),
            amount,
            memo: memo.to_owned(),
        }
    }

    fn tag_hex(n: u8) -> String {
        format!("0x{}", hex(&[n; 20]))
    }

    fn tx(ops: Vec<codec::Operation>) -> TransactionView {
        TransactionView::of(&MeshTransaction {
            hash: [7; 32],
            block: None,
            timestamp_ms: None,
            operations: ops,
            metadata: Vec::new(),
        })
    }

    #[test]
    fn a_spend_with_change_is_both_ways_and_nets_what_left() {
        let me = AccountId::from_tag([1; 20]);
        let t = tx(vec![
            op(OP_SOURCE, &tag_hex(1), -1_000_500, ""),
            op(OP_DESTINATION, &tag_hex(2), 400_000, "INV-1"),
            op(OP_DESTINATION, &tag_hex(1), 600_000, ""),
            op(OP_FEE, "", 500, ""),
        ]);
        assert_eq!(t.direction(me), Direction::Both);
        assert_eq!(t.net(me), -400_500);
        assert_eq!(t.fee(), 500);
        let out: Vec<_> = t.paid_out(me).collect();
        assert_eq!(out.len(), 1, "the change is not a payee");
        assert_eq!(out[0].memo, "INV-1");
        let them = AccountId::from_tag([2; 20]);
        assert_eq!(t.direction(them), Direction::In);
        assert_eq!(t.net(them), 400_000);
        assert_eq!(t.paid_by(them).count(), 1);
    }

    #[test]
    fn addresses_are_read_as_the_command_line_reads_them() {
        assert_eq!(
            party(&tag_hex(9)),
            Party::Account(AccountId::from_tag([9; 20]))
        );
        let ledger = format!("0x{}", "AB".repeat(40));
        assert_eq!(
            party(&ledger),
            Party::Ledger(format!("0x{}", "ab".repeat(40)))
        );
        assert_eq!(party("pool"), Party::Other("pool".to_owned()));
        assert_eq!(party(&"zz".repeat(20)), Party::Other("zz".repeat(20)));
    }

    #[test]
    fn a_reference_is_shown_without_its_field_padding() {
        let t = tx(vec![op(OP_DESTINATION, &tag_hex(2), 1, "INV-1\0\0\0\0\0")]);
        assert_eq!(t.operations[0].memo, "INV-1");
        // Only the padding: a NUL inside is the node's text, shown escaped.
        let t = tx(vec![op(OP_DESTINATION, &tag_hex(2), 1, "A\0B\0")]);
        assert_eq!(t.operations[0].memo, "A\\u{0}B");
    }

    #[test]
    fn text_from_the_node_cannot_disguise_itself() {
        assert_eq!(shown("INV-1"), "INV-1");
        assert_eq!(shown("A\u{202e}B"), "A\\u{202e}B");
        assert_eq!(shown("A\u{200b}B"), "A\\u{200b}B");
        assert_eq!(shown("A\nB\\"), "A\\nB\\\\");
    }

    #[test]
    fn a_refused_search_says_whether_the_node_runs_an_index() {
        let not_served = Error::HttpStatus { status: 404 };
        let down = Error::Mesh {
            code: 2,
            retriable: true,
        };
        let invalid = Error::Mesh {
            code: 1,
            retriable: false,
        };
        assert_eq!(index_state(&not_served), Some(IndexState::Absent));
        assert_eq!(index_state(&down), Some(IndexState::Unavailable));
        assert_eq!(
            index_state(&invalid),
            None,
            "an invalid request is not about the index"
        );
        assert_eq!(index_state(&Error::HttpStatus { status: 502 }), None);
    }
}
