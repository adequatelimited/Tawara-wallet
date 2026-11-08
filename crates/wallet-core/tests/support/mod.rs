// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! A scripted node and a harness for driving the worker.
//!
//! # The node is a `Transport`, not a mock of the wallet
//!
//! The worker is handed a [`FakeNode`] in place of the HTTPS one, and the
//! real `MeshClient` and the real codec run on top of it, so every answer the
//! worker sees was parsed by the library's own parser. Only the bytes on the
//! wire are scripted here, in the shapes the library's own test chain uses
//! (its `tests/support/chain.rs`, after the live captures in
//! `fixtures/group_n_mesh_live.json`): `tag_resolve` answers with
//! `result.{address,amount}` or the middleware's code 4 "Account not found",
//! `/network/status` with `current_block_identifier`, the tip's timestamp,
//! the genesis block and `sync_status`, `/network/list` with the one
//! network the middleware serves, `/block` by number or by hash with the
//! eight keys of `block.metadata` and, for a normal block, its reward and
//! the spends scripted for it, `/mempool` with its ids (Go's `null` for
//! none), `/mempool/transaction` with a scripted spend or code 3 for one that
//! left the queue, `/search/transactions` by account or by a transaction's
//! id, and `/construction/submit` with `transaction_identifier.hash` in bare
//! hex.
//!
//! The submit echo is computed the way the library's own `submit` verb
//! computes the id it expects, `tx::wire::Transaction::from_wire(..).id_digest()`:
//! a node computes it from the bytes it was sent, and so does this one.
//!
//! # Where the addresses come from
//!
//! The library: an account's tag is `derive::derive_account_tag(master, n)`
//! and its address at a position is `recon::derived_address_at`, the restore
//! path's derivation. The master seed is the one the BIP39 test phrase below
//! derives through `mnemonic::master_seed_from_phrase`, which is what
//! `cli::create::create` does with a phrase. That phrase is a public test
//! vector and must never be funded.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use mochimo_crypto::account::WotsIndex;
use mochimo_crypto::addr::{Address, Tag};
use mochimo_crypto::consts::SEED_LEN;
use mochimo_crypto::mesh::Transport;
use mochimo_crypto::{Error, Secret, TransportKind, derive, mnemonic, recon};
use tawara_wallet_core::{
    Activity, Command, Config, Connect, Event, LockReason, Reply, RequestId, SecretText,
    WorkerHandle, spawn,
};

/// The BIP39 all-`abandon` 24-word test vector. Public; never fund it.
pub const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon \
                          abandon abandon abandon abandon abandon abandon abandon abandon abandon \
                          abandon abandon abandon abandon abandon art";

/// A password over the library's floor.
pub const PASSWORD: &str = "correct horse battery";

pub const NODE: &str = "https://node.test";

/// A second node, answered by its own chain ([`Harness::chain_b`]), so a
/// test can tell which node the worker asked.
pub const NODE_B: &str = "https://other-node.test";

pub fn master() -> Secret<SEED_LEN> {
    mnemonic::master_seed_from_phrase(PHRASE, "").expect("the test phrase parses")
}

/// Derived account `n`'s tag.
pub fn tag(n: u32) -> Tag {
    derive::derive_account_tag(&master(), n)
}

/// Derived account `n`'s address at position `i`.
pub fn address(n: u32, i: u32) -> Address {
    recon::derived_address_at(&master(), n, position(i))
}

pub fn position(i: u32) -> WotsIndex {
    let mut p = WotsIndex::ZERO;
    for _ in 0..i {
        p = p.advanced().expect("position");
    }
    p
}

pub fn destination(n: u32) -> String {
    mochimo_crypto::addr::tag_to_base58(&tag(n)).expect("renders")
}

pub fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[derive(Default)]
struct State {
    ledger: BTreeMap<Tag, (Address, u64)>,
    tip: u64,
    unreachable: bool,
    /// `/network/status` is not answered, so the tip cannot be read and the
    /// expiry of an outstanding spend cannot be checked (the library's
    /// `Expiry::Unreadable`).
    status_down: bool,
    /// How many more status reads the node answers before it stops answering
    /// them, from [`Chain::set_status_down_after`]; `None` when no countdown
    /// is set.
    status_answers: Option<usize>,
    /// The tags whose lookups the node does not answer, while it answers the
    /// rest: one account reads `ChainUnreachable` and the others reconcile.
    silent_for: BTreeSet<Tag>,
    refuse_submits: bool,
    /// What the ledger holds once the next submit is taken, from
    /// [`Chain::land_on_submit`]: the spend lands before any read after it.
    land_on_submit: Option<(Tag, Address, u64)>,
    submits: Vec<Vec<u8>>,
    calls: usize,
    gate: Option<Gate>,
    delay: Duration,
    /// How many times each tag was resolved.
    resolved: BTreeMap<Tag, usize>,
    /// From this resolve of this tag on, answer "account not found".
    vanish: Option<(Tag, usize)>,
    /// The index's rows for each tag, newest first, as the endpoint spells
    /// them.
    history: BTreeMap<Tag, Vec<serde_json::Value>>,
    /// The deployment runs no index: the search is not served, and answers
    /// the router's 404.
    no_index: bool,
    /// The deployment runs an index that does not answer: a search answers
    /// the middleware's internal error, code 2.
    index_down: bool,
    /// How many transactions the index has recorded, for their ids.
    indexed: u64,
    /// The blocks whose figures count no transactions: pseudo-blocks.
    pseudo: Vec<u64>,
    /// The ids in the node's queue, in its order.
    mempool: Vec<[u8; 32]>,
    /// The middleware's sync state: its stage and whether it finished.
    sync: (String, bool),
    /// The spends each block carries besides its reward, as `/block` spells
    /// them.
    spends: BTreeMap<u64, Vec<serde_json::Value>>,
    /// The transactions `/mempool/transaction` answers for, by id; an id in
    /// the queue that is not here has left it.
    pending: BTreeMap<[u8; 32], serde_json::Value>,
    /// The blocks `/block` does not serve, by number or by hash.
    unserved: Vec<u64>,
    /// The times `/block` sends for blocks, in milliseconds, where not a
    /// block a minute.
    times: BTreeMap<u64, i64>,
    /// The blocks `/block` serves from another branch: each names a parent
    /// that is not the block below it as served.
    forked: Vec<u64>,
    /// The v4.1 trigger `/stats/staking` reports; with none the staking
    /// routes are not served, as on a middleware built before v4.1.
    trigger: Option<u64>,
    /// Each staking tag's stake: (staked, commit, expiry, reward).
    stakes: BTreeMap<Tag, (u64, u64, u64, u64)>,
    /// `/account/entitlement`'s figure for a tag that does not stake.
    entitlements: BTreeMap<Tag, u64>,
    /// How many times `/account/entitlement` was asked.
    entitlement_calls: usize,
}

/// The tag every reward of the scripted chain is paid to.
pub const MINER: u32 = 77;

/// The reward the scripted chain pays for a normal block, in nanoMCM.
pub const REWARD: u64 = 5_000_000_000;

/// The fee the scripted chain's transactions pay, in nanoMCM.
pub const FEE: u64 = 500;

/// One operation, as the endpoints spell it.
fn operation(i: u64, kind: &str, tag: Option<&Tag>, value: i128) -> serde_json::Value {
    serde_json::json!({
        "operation_identifier": { "index": i },
        "type": kind,
        "account": { "address": tag.map(|t| format!("0x{}", hex_of(t))).unwrap_or_default() },
        "amount": { "value": value.to_string() },
    })
}

/// A spend as `/block` and `/mempool/transaction` spell one: the source
/// debited what left it net of its change, each payee, and the fee; the
/// change is not an operation.
pub fn spend_json(id: &[u8; 32], from: Tag, to: &[(Tag, u64)]) -> serde_json::Value {
    let sent: u64 = to.iter().map(|(_, a)| a).sum();
    let mut ops = vec![operation(
        0,
        "SOURCE_TRANSFER",
        Some(&from),
        -i128::from(sent + FEE),
    )];
    for (i, (tag, amount)) in to.iter().enumerate() {
        ops.push(operation(
            i as u64 + 1,
            "DESTINATION_TRANSFER",
            Some(tag),
            i128::from(*amount),
        ));
    }
    ops.push(operation(to.len() as u64 + 1, "FEE", None, i128::from(FEE)));
    serde_json::json!({
        "transaction_identifier": { "hash": format!("0x{}", hex_of(id)) },
        "operations": ops,
    })
}

/// Each wait in this module that has no bound of its own gives up after this
/// long and panics: an answer or event from the worker, a request reaching a
/// gate, and the release of a request held at one. A test that works never
/// comes near it; without the bound, a stuck wait would hang the test run.
const WAIT: Duration = Duration::from_secs(120);

/// Holds every request at the chain until it is opened, so a test can act
/// while the worker is waiting on the node. Neither side waits on it for
/// longer than its bound: [`WAIT`], unless the module's own tests shorten it.
#[derive(Clone)]
pub struct Gate {
    shared: Arc<(Mutex<GateState>, Condvar)>,
    bound: Duration,
}

#[derive(Default)]
struct GateState {
    open: bool,
    /// How many requests have reached the gate, held or let through.
    reached: usize,
}

impl Default for Gate {
    fn default() -> Gate {
        Gate::with_bound(WAIT)
    }
}

impl Gate {
    fn with_bound(bound: Duration) -> Gate {
        Gate {
            shared: Arc::default(),
            bound,
        }
    }

    pub fn open(&self) {
        let (state, changed) = &*self.shared;
        state.lock().expect("gate").open = true;
        changed.notify_all();
    }

    /// Wait until `n` requests have reached the gate: the worker is then
    /// waiting on the node, not about to ask it. Panics after the gate's bound
    /// if they have not.
    pub fn wait_reached(&self, n: usize) {
        let (state, changed) = &*self.shared;
        let bound = self.bound;
        let (s, _) = changed
            .wait_timeout_while(state.lock().expect("gate"), bound, |s| s.reached < n)
            .expect("gate");
        if s.reached < n {
            let reached = s.reached;
            // Released before the panic: a panic with the lock held would
            // poison the gate for the next thread to take it.
            drop(s);
            panic!(
                "Gate::wait_reached: only {reached} of the {n} requests the test awaited \
                 reached the gate within {bound:?}"
            );
        }
    }

    /// Holds the calling request until the gate is opened. It runs on the
    /// worker's thread, so a gate the test never opens
    /// must end it after the bound, not leave that thread blocked for good.
    fn pass(&self) {
        let (state, changed) = &*self.shared;
        let bound = self.bound;
        let mut s = state.lock().expect("gate");
        s.reached += 1;
        changed.notify_all();
        let (s, _) = changed
            .wait_timeout_while(s, bound, |s| !s.open)
            .expect("gate");
        if !s.open {
            drop(s);
            panic!(
                "Gate::pass: a request held at the gate was not released within {bound:?}; \
                 the test never opened the gate"
            );
        }
    }
}

/// The scripted chain, shared between the test and every transport the
/// worker builds from it.
#[derive(Clone, Default)]
pub struct Chain(Arc<Mutex<State>>);

impl Chain {
    pub fn new() -> Chain {
        let chain = Chain::default();
        {
            let mut s = chain.0.lock().expect("chain");
            s.tip = 1_000;
            s.sync = ("synchronized".to_owned(), true);
        }
        chain
    }

    /// The chain's tip is block `tip`.
    pub fn set_tip(&self, tip: u64) {
        self.0.lock().expect("chain").tip = tip;
    }

    /// Block `index` counts no transactions in its figures.
    pub fn pseudo(&self, index: u64) {
        self.0.lock().expect("chain").pseudo.push(index);
    }

    /// The node's queue holds `ids`, in that order.
    pub fn queue(&self, ids: &[[u8; 32]]) {
        self.0.lock().expect("chain").mempool = ids.to_vec();
    }

    /// The middleware says its last refresh reached `stage`, finished or not.
    pub fn sync(&self, stage: &str, synced: bool) {
        self.0.lock().expect("chain").sync = (stage.to_owned(), synced);
    }

    /// The ledger holds `tag` at `address` with `balance`.
    pub fn hold(&self, tag: Tag, address: Address, balance: u64) {
        self.0
            .lock()
            .expect("chain")
            .ledger
            .insert(tag, (address, balance));
    }

    /// The ledger has no entry for `tag`.
    pub fn forget(&self, tag: Tag) {
        self.0.lock().expect("chain").ledger.remove(&tag);
    }

    pub fn set_unreachable(&self, unreachable: bool) {
        self.0.lock().expect("chain").unreachable = unreachable;
    }

    /// The node answers the ledger but not its status: a read of the tip fails.
    /// Setting it clears any countdown from [`Chain::set_status_down_after`].
    pub fn set_status_down(&self, down: bool) {
        let mut s = self.0.lock().expect("chain");
        s.status_down = down;
        s.status_answers = None;
    }

    /// The node answers the next `answers` status reads, and then none of them
    /// until [`Chain::set_status_down`] is called: a spend's own read of the
    /// tip, which its signing makes first, is answered, and the read its reply
    /// carries after that is not.
    pub fn set_status_down_after(&self, answers: usize) {
        self.0.lock().expect("chain").status_answers = Some(answers);
    }

    /// The node does not answer a lookup of `tag`, or answers again.
    pub fn set_silent_for(&self, tag: Tag, silent: bool) {
        let mut s = self.0.lock().expect("chain");
        if silent {
            s.silent_for.insert(tag);
        } else {
            s.silent_for.remove(&tag);
        }
    }

    pub fn refuse_submits(&self, refuse: bool) {
        self.0.lock().expect("chain").refuse_submits = refuse;
    }

    /// The next submit the node takes lands at once: the ledger then holds
    /// `tag` at `address` with `balance`, so the read that follows the submit
    /// finds the spend landed. A refused submit lands nothing.
    pub fn land_on_submit(&self, tag: Tag, address: Address, balance: u64) {
        self.0.lock().expect("chain").land_on_submit = Some((tag, address, balance));
    }

    /// Every wire image posted to `/construction/submit`.
    pub fn submits(&self) -> Vec<Vec<u8>> {
        self.0.lock().expect("chain").submits.clone()
    }

    pub fn calls(&self) -> usize {
        self.0.lock().expect("chain").calls
    }

    /// How many times `tag` has been resolved.
    pub fn resolves(&self, tag: Tag) -> usize {
        self.0
            .lock()
            .expect("chain")
            .resolved
            .get(&tag)
            .copied()
            .unwrap_or(0)
    }

    /// Answer "account not found" for `tag` from its `nth` resolve on
    /// (counted as [`Chain::resolves`] counts), whatever the ledger holds:
    /// the node changing its answer partway through a command.
    pub fn vanish_from(&self, tag: Tag, nth: usize) {
        self.0.lock().expect("chain").vanish = Some((tag, nth));
    }

    /// Serve the v4.1 staking routes, `/stats/staking` reporting `trigger`.
    pub fn serve_staking(&self, trigger: u64) {
        self.0.lock().expect("chain").trigger = Some(trigger);
    }

    /// `tag` stakes `staked`, committed at `commit` and locked until `expiry`,
    /// with `reward` of entitlement earned.
    pub fn stake(&self, tag: Tag, staked: u64, commit: u64, expiry: u64, reward: u64) {
        self.0
            .lock()
            .expect("chain")
            .stakes
            .insert(tag, (staked, commit, expiry, reward));
    }

    /// `/account/entitlement` reports `amount` for `tag`.
    pub fn entitle(&self, tag: Tag, amount: u64) {
        self.0
            .lock()
            .expect("chain")
            .entitlements
            .insert(tag, amount);
    }

    /// How many times `/account/entitlement` has been asked.
    pub fn entitlement_calls(&self) -> usize {
        self.0.lock().expect("chain").entitlement_calls
    }

    /// Take `delay` to answer every request from now on, as a slow node does.
    pub fn slow(&self, delay: Duration) {
        self.0.lock().expect("chain").delay = delay;
    }

    /// The index records a transfer of `amount` from `from` to `to` in
    /// `block`, as the index spells one: the source debited gross, the
    /// change back to it as a destination of its own, and the fee. A
    /// `reference` that is not empty rides on the payee's operation as the
    /// indexer stores it: the whole sixteen-byte field, padded with NULs.
    pub fn index_transfer(
        &self,
        from: Tag,
        to: Tag,
        amount: u64,
        change: u64,
        block: u64,
        reference: &str,
    ) {
        let mut s = self.0.lock().expect("chain");
        s.indexed += 1;
        let fee = 500;
        let op = |i: u64, kind: &str, tag: &Tag, value: i128| {
            serde_json::json!({
                "operation_identifier": { "index": i },
                "type": kind,
                "account": { "address": format!("0x{}", hex_of(tag)) },
                "amount": { "value": value.to_string() },
            })
        };
        let gross = i128::from(amount + change + fee);
        let mut paid = op(1, "DESTINATION_TRANSFER", &to, i128::from(amount));
        if !reference.is_empty() {
            let field = format!("{reference:\0<16}");
            paid["metadata"] = serde_json::json!({ "memo": field });
        }
        let row = serde_json::json!({
            "transaction_identifier": { "hash": format!("0x{:064x}", s.indexed) },
            "block_identifier": { "index": block, "hash": format!("0x{:064x}", block) },
            "timestamp": block * 60_000,
            "operations": [
                op(0, "SOURCE_TRANSFER", &from, -gross),
                paid,
                op(2, "DESTINATION_TRANSFER", &from, i128::from(change)),
                {
                    "operation_identifier": { "index": 3 },
                    "type": "FEE",
                    "account": { "address": "" },
                    "amount": { "value": fee.to_string() },
                },
            ],
        });
        for tag in [from, to] {
            s.history.entry(tag).or_default().insert(0, row.clone());
        }
    }

    /// Block `block` carries a spend `id` from `from` to `to`, besides its
    /// reward.
    pub fn block_spend(&self, block: u64, id: [u8; 32], from: Tag, to: &[(Tag, u64)]) {
        self.0
            .lock()
            .expect("chain")
            .spends
            .entry(block)
            .or_default()
            .push(spend_json(&id, from, to));
    }

    /// `/mempool/transaction` answers for `id` with a spend from `from` to
    /// `to`. The queue's order is [`Chain::queue`]'s.
    pub fn pending(&self, id: [u8; 32], from: Tag, to: &[(Tag, u64)]) {
        self.0
            .lock()
            .expect("chain")
            .pending
            .insert(id, spend_json(&id, from, to));
    }

    /// `/block` does not serve block `index`, by number or by hash.
    pub fn unserve(&self, index: u64) {
        self.0.lock().expect("chain").unserved.push(index);
    }

    /// `/block` sends `ms` as block `index`'s time: anything the Mesh
    /// parser takes, however far from the blocks either side.
    pub fn set_time(&self, index: u64, ms: i64) {
        self.0.lock().expect("chain").times.insert(index, ms);
    }

    /// `/block` serves block `index` from another branch, as a node does
    /// when the chain reorganizes between two reads: its parent's hash is
    /// not the hash of the block below it as served.
    pub fn fork(&self, index: u64) {
        self.0.lock().expect("chain").forked.push(index);
    }

    pub fn set_no_index(&self, no_index: bool) {
        self.0.lock().expect("chain").no_index = no_index;
    }

    pub fn set_index_down(&self, down: bool) {
        self.0.lock().expect("chain").index_down = down;
    }

    /// Hold every request from now on until the returned gate is opened.
    pub fn close_gate(&self) -> Gate {
        let gate = Gate::default();
        self.0.lock().expect("chain").gate = Some(gate.clone());
        gate
    }
}

fn mesh(what: &'static str) -> Error {
    Error::MeshResponse { what }
}

impl Transport for Chain {
    fn post(&self, path: &str, body: &[u8]) -> mochimo_crypto::Result<Vec<u8>> {
        // Waited at with the chain's lock released, so the test can script
        // the chain meanwhile.
        let (gate, delay) = {
            let s = self.0.lock().map_err(|_| mesh("test: chain lock"))?;
            (s.gate.clone(), s.delay)
        };
        if let Some(gate) = gate {
            gate.pass();
        }
        std::thread::sleep(delay);
        let mut s = self.0.lock().map_err(|_| mesh("test: chain lock"))?;
        s.calls += 1;
        if s.unreachable {
            return Err(Error::Transport {
                op: "connect",
                kind: TransportKind::Io(std::io::ErrorKind::ConnectionRefused),
            });
        }
        let req: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| mesh("test: request json"))?;
        // A status read the countdown answers uses one of its answers up.
        let status_refused = s.status_down || s.status_answers == Some(0);
        if path == "/network/status"
            && !status_refused
            && let Some(answers) = s.status_answers.as_mut()
        {
            *answers -= 1;
        }
        match path {
            "/network/status" if status_refused => Err(Error::Transport {
                op: "status",
                kind: TransportKind::Io(std::io::ErrorKind::TimedOut),
            }),
            "/network/status" => Ok(serde_json::json!({
                "current_block_identifier": { "index": s.tip, "hash": format!("0x{}", "ab".repeat(32)) },
                "current_block_timestamp": s.tip * 60_000,
                "genesis_block_identifier": { "index": 0, "hash": format!("0x{}", "00".repeat(32)) },
                "sync_status": { "stage": s.sync.0, "synced": s.sync.1 },
            })
            .to_string()
            .into_bytes()),
            "/network/list" => Ok(
                br#"{"network_identifiers":[{"blockchain":"mochimo","network":"mainnet"}]}"#.to_vec(),
            ),
            "/mempool" => {
                let ids: Vec<_> = s
                    .mempool
                    .iter()
                    .map(|id| serde_json::json!({ "hash": format!("0x{}", hex_of(id)) }))
                    .collect();
                let list = if ids.is_empty() { serde_json::Value::Null } else { ids.into() };
                Ok(serde_json::json!({ "transaction_identifiers": list }).to_string().into_bytes())
            }
            "/call" => {
                let asked = req["parameters"]["tag"].as_str().ok_or(mesh("test: parameters.tag"))?;
                let raw = asked
                    .strip_prefix("0x")
                    .and_then(unhex)
                    .ok_or(mesh("test: tag hex"))?;
                let tag: Tag = raw.try_into().map_err(|_| mesh("test: tag length"))?;
                if s.silent_for.contains(&tag) {
                    return Err(Error::Transport {
                        op: "call",
                        kind: TransportKind::Io(std::io::ErrorKind::TimedOut),
                    });
                }
                let nth = {
                    let count = s.resolved.entry(tag).or_default();
                    *count += 1;
                    *count
                };
                let vanished = matches!(s.vanish, Some((t, from)) if t == tag && nth >= from);
                match s.ledger.get(&tag).filter(|_| !vanished) {
                    Some((address, balance)) => Ok(format!(
                        r#"{{"result":{{"address":"0x{}","amount":{balance}}},"idempotent":true}}"#,
                        hex_of(address)
                    )
                    .into_bytes()),
                    None => Ok(br#"{"code":4,"message":"Account not found","retriable":false}"#.to_vec()),
                }
            }
            "/search/transactions" => {
                // `mochimo-mesh` registers the route only with its indexer
                // enabled; the library's transport reports the router's 404 as
                // a status other than 200.
                if s.no_index {
                    return Err(Error::HttpStatus { status: 404 });
                }
                if s.index_down {
                    return Ok(br#"{"code":2,"message":"Internal general error","retriable":true}"#.to_vec());
                }
                // One transaction, by its id.
                if let Some(id) = req["transaction_identifier"]["hash"].as_str() {
                    let found: Vec<_> = s
                        .history
                        .values()
                        .flatten()
                        .find(|row| row["transaction_identifier"]["hash"].as_str() == Some(id))
                        .cloned()
                        .into_iter()
                        .collect();
                    return Ok(serde_json::json!({ "transactions": found, "total_count": found.len() })
                        .to_string()
                        .into_bytes());
                }
                let asked = req["account_identifier"]["address"].as_str().ok_or(mesh("test: account_identifier"))?;
                let limit = req["limit"].as_u64().filter(|l| (1..=100).contains(l)).unwrap_or(10);
                let raw = asked.strip_prefix("0x").and_then(unhex).ok_or(mesh("test: tag hex"))?;
                let tag: Tag = raw.try_into().map_err(|_| mesh("test: tag length"))?;
                let offset = req["offset"].as_u64().unwrap_or(0);
                let rows = s.history.get(&tag).cloned().unwrap_or_default();
                let total = rows.len();
                let page: Vec<_> = rows
                    .into_iter()
                    .skip(usize::try_from(offset).unwrap_or(usize::MAX))
                    .take(usize::try_from(limit).unwrap_or(10))
                    .collect();
                Ok(serde_json::json!({ "transactions": page, "total_count": total }).to_string().into_bytes())
            }
            "/block" => {
                // By number, or by hash: the scripted chain's hashes are their
                // numbers, in hex.
                let index = match req["block_identifier"]["hash"].as_str() {
                    Some(hash) => {
                        let digits = hash.strip_prefix("0x").ok_or(mesh("test: block hash"))?;
                        u64::from_str_radix(digits, 16).unwrap_or(u64::MAX)
                    }
                    None => req["block_identifier"]["index"].as_u64().ok_or(mesh("test: block index"))?,
                };
                // Index 0 is the tip to this endpoint, as the library notes.
                let index = if index == 0 { s.tip } else { index };
                if index > s.tip || s.unserved.contains(&index) {
                    return Ok(br#"{"code":2,"message":"Block not found","retriable":false}"#.to_vec());
                }
                let pseudo = s.pseudo.contains(&index);
                let neogenesis = index & 0xff == 0;
                let spends = s.spends.get(&index).cloned().unwrap_or_default();
                let tx_count = if pseudo { 0 } else { u32::try_from(spends.len()).unwrap_or(0).max(1) };
                // A normal block pays its miner; a pseudo-block and a
                // neogenesis block carry no reward.
                let mut transactions = Vec::new();
                if !pseudo && !neogenesis {
                    transactions.push(serde_json::json!({
                        "transaction_identifier": { "hash": format!("0x{:064x}", u128::from(index) << 64) },
                        "operations": [operation(0, "REWARD", Some(&tag(MINER)), i128::from(REWARD))],
                    }));
                    transactions.extend(spends);
                }
                Ok(serde_json::json!({ "block": {
                    "block_identifier": { "index": index, "hash": format!("0x{index:064x}") },
                    "parent_block_identifier": {
                        "index": index - 1,
                        "hash": format!("0x{:064x}", u128::from(index - 1) | u128::from(s.forked.contains(&index)) << 120),
                    },
                    "timestamp": s.times.get(&index).copied().unwrap_or_else(|| i64::try_from(index * 60_000).unwrap_or(i64::MAX)),
                    "transactions": transactions,
                    "metadata": {
                        "block_size": 4_096,
                        "difficulty": 30 + index % 5,
                        "fee": 500,
                        "haiku": "winter frost settles\nthe API keeps its quiet count\nblock \
                                  after block",
                        "nonce": format!("0x{:064x}", index + 1),
                        "root": format!("0x{:064x}", index + 2),
                        "stime": index * 60_000,
                        "tx_count": tx_count,
                    },
                }})
                .to_string()
                .into_bytes())
            }
            "/mempool/transaction" => {
                let asked = req["transaction_identifier"]["hash"].as_str().ok_or(mesh("test: transaction id"))?;
                let found = s
                    .pending
                    .iter()
                    .find(|(id, _)| format!("0x{}", hex_of(*id)) == asked)
                    .map(|(_, t)| t.clone());
                match found {
                    Some(t) => Ok(serde_json::json!({ "transaction": t }).to_string().into_bytes()),
                    None => Ok(br#"{"code":3,"message":"Transaction not found","retriable":false}"#.to_vec()),
                }
            }
            "/construction/submit" => {
                let text = req["signed_transaction"].as_str().ok_or(mesh("test: signed_transaction"))?;
                let wire = unhex(text).ok_or(mesh("test: wire hex"))?;
                s.submits.push(wire.clone());
                if s.refuse_submits {
                    return Err(Error::Transport {
                        op: "write",
                        kind: TransportKind::Io(std::io::ErrorKind::BrokenPipe),
                    });
                }
                if let Some((tag, address, balance)) = s.land_on_submit.take() {
                    s.ledger.insert(tag, (address, balance));
                }
                let tx = mochimo_crypto::tx::wire::Transaction::from_wire(&wire)?;
                Ok(format!(r#"{{"transaction_identifier":{{"hash":"{}"}}}}"#, hex_of(&tx.id_digest())).into_bytes())
            }
            "/stats/staking" | "/account/balance" | "/account/entitlement" if s.trigger.is_none() => {
                Err(Error::HttpStatus { status: 404 })
            }
            "/stats/staking" => {
                let tip = s.tip;
                let trigger = s.trigger.unwrap_or(0);
                let staked: u64 = s.stakes.values().map(|k| k.0).sum();
                let mcm = r#"{"symbol":"MCM","decimals":9}"#;
                let h = "aa".repeat(32);
                Ok(format!(
                    r#"{{"block_identifier":{{"index":{tip},"hash":"0x{h}"}},"v41_trigger":{trigger},"v41_active":{},"min_stake":{{"value":"500000000000","currency":{mcm}}},"stake_set":{{"block_identifier":{{"index":{tip},"hash":"0x{h}"}},"count":{},"pool":{{"value":"2500000000","currency":{mcm}}},"forfeit":{{"value":"0","currency":{mcm}}},"dust":{{"value":"1","currency":{mcm}}},"weight":"3600000000000"}},"ledger":{{"block_index":{tip},"last_updated":"2026-11-01T12:00:00Z","stakers":{},"staked":{{"value":"{staked}","currency":{mcm}}},"weight":"3600000000000"}}}}"#,
                    tip > trigger,
                    s.stakes.len(),
                    s.stakes.len(),
                )
                .into_bytes())
            }
            "/account/balance" => {
                let asked = req["account_identifier"]["address"]
                    .as_str()
                    .ok_or(mesh("test: account_identifier.address"))?;
                let raw = asked.strip_prefix("0x").and_then(unhex).ok_or(mesh("test: tag hex"))?;
                let tag: Tag = raw.try_into().map_err(|_| mesh("test: tag length"))?;
                let spendable = s.ledger.get(&tag).map_or(0, |(_, b)| *b);
                let stake = s.stakes.get(&tag).copied();
                if spendable == 0 && stake.is_none() {
                    return Ok(br#"{"code":4,"message":"Account not found","retriable":false}"#.to_vec());
                }
                let tip = s.tip;
                let mcm = r#"{"symbol":"MCM","decimals":9}"#;
                let head = format!(
                    r#""block_identifier":{{"index":{tip},"hash":"0x{}"}},"balances":[{{"value":"{spendable}","currency":{mcm}}}]"#,
                    "ef".repeat(32)
                );
                let meta = match stake {
                    None => format!(
                        r#","metadata":{{"spendable":"{spendable}","staked":"0","stake_source":"node","stake_block_index":{tip}}}"#
                    ),
                    Some((staked, commit, expiry, reward)) => {
                        let tier = mochimo_crypto::staking::tier_of(commit, expiry);
                        let tier_fields = match tier {
                            Some(k) => format!(
                                r#","tier":{k},"coefficient":{},"weight":"{}""#,
                                u64::from(k) + 1,
                                u128::from(staked + reward) * (u128::from(k) + 1)
                            ),
                            None => String::new(),
                        };
                        format!(
                            r#","metadata":{{"spendable":"{spendable}","staked":"{staked}","stake_source":"node","stake_block_index":{tip},"entitlement":"{reward}","total":"{}","stake":{{"commit":{commit},"expiry":{expiry},"locked":{}{tier_fields}}}}}"#,
                            spendable + staked + reward,
                            tip < expiry
                        )
                    }
                };
                Ok(format!("{{{head}{meta}}}").into_bytes())
            }
            "/account/entitlement" => {
                s.entitlement_calls += 1;
                let asked = req["account_identifier"]["address"]
                    .as_str()
                    .ok_or(mesh("test: account_identifier.address"))?
                    .to_owned();
                let raw = asked.strip_prefix("0x").and_then(unhex).ok_or(mesh("test: tag hex"))?;
                let tag: Tag = raw.try_into().map_err(|_| mesh("test: tag length"))?;
                let amount = s.entitlements.get(&tag).copied().unwrap_or(0);
                Ok(format!(
                    r#"{{"block_identifier":{{"index":{},"hash":"0x{}"}},"account_identifier":{{"address":"{asked}"}},"entitlement":{{"value":"{amount}","currency":{{"symbol":"MCM","decimals":9}}}}}}"#,
                    s.tip,
                    "cd".repeat(32)
                )
                .into_bytes())
            }
            _ => Err(mesh("test: a path this API does not script")),
        }
    }
}

/// The worker's [`Connect`] for the scripted chains: [`NODE_B`] is answered
/// by the second chain, any other URL by the first.
pub struct FakeNode {
    pub chain: Chain,
    pub chain_b: Chain,
}

impl Connect for FakeNode {
    type Transport = Chain;

    fn connect(&self, url: &str) -> Result<Chain, Error> {
        Ok(if url == NODE_B {
            self.chain_b.clone()
        } else {
            self.chain.clone()
        })
    }
}

/// A directory to put stores under, removed on drop. It is under cargo's
/// per-target scratch directory, as the library's own tests keep theirs, and
/// the store's folder inside it is left for the library to make, with the
/// owner and access it requires.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        // Unique per run as well as per test: on Windows a store the worker
        // has not yet released cannot be removed, and must not be found by
        // the next run.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("wallet-core-{}-{name}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }

    pub fn store(&self) -> PathBuf {
        self.0.join("keystore")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn secret(text: &str) -> SecretText {
    let mut field = text.to_owned();
    SecretText::take(&mut field)
}

/// The worker, its events, and a scripted chain.
pub struct Harness {
    pub handle: WorkerHandle,
    pub events: Receiver<Event>,
    pub chain: Chain,
    /// The chain behind [`NODE_B`].
    pub chain_b: Chain,
    /// Every event that was not the answer being waited for.
    pub seen: Vec<Event>,
}

impl Harness {
    pub fn new() -> Harness {
        Harness::with(Config::default())
    }

    pub fn with(config: Config) -> Harness {
        let chain = Chain::new();
        let chain_b = Chain::new();
        let node = FakeNode {
            chain: chain.clone(),
            chain_b: chain_b.clone(),
        };
        let (handle, events) = spawn(config, node).expect("worker starts");
        Harness {
            handle,
            events,
            chain,
            chain_b,
            seen: Vec::new(),
        }
    }

    /// A harness with the node chosen.
    pub fn with_node() -> Harness {
        let mut h = Harness::new();
        match h.call(Command::SetNode { url: NODE.into() }) {
            Reply::NodeSet { .. } => {}
            other => panic!("node not set: {other:?}"),
        }
        h
    }

    /// Send a command and wait for its answer.
    pub fn call(&mut self, command: Command) -> Reply {
        let id = self.handle.send(command).expect("worker running");
        self.wait_for(id)
    }

    /// Wait for the answer to a command sent with `handle.send`.
    pub fn wait_for(&mut self, id: RequestId) -> Reply {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(Event::Done { id: got, reply }) if got == id => return *reply,
                Ok(other) => self.seen.push(other),
                Err(e) => panic!("no answer to {id:?} within {WAIT:?}: {e}"),
            }
        }
    }

    /// Wait until command `id` reports what it is doing.
    pub fn wait_busy(&mut self, id: RequestId) -> Activity {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(Event::Busy { id: got, activity }) if got == id => return activity,
                Ok(other) => self.seen.push(other),
                Err(e) => panic!("{id:?} never reported busy within {WAIT:?}: {e}"),
            }
        }
    }

    /// Wait for the next `Locked` event, keeping the rest.
    pub fn wait_locked(&mut self, within: Duration) -> LockReason {
        if let Some(i) = self
            .seen
            .iter()
            .position(|e| matches!(e, Event::Locked { .. }))
            && let Event::Locked { reason } = self.seen.remove(i)
        {
            return reason;
        }
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(Event::Locked { reason }) => return reason,
                Ok(other) => self.seen.push(other),
                Err(e) => panic!("no Locked event within {within:?}: {e}"),
            }
        }
    }

    /// Wait for `Backgrounded`, and return its number and every event that
    /// came before it, in order, those already set aside included.
    pub fn wait_backgrounded(&mut self, within: Duration) -> (u64, Vec<Event>) {
        let mut before = std::mem::take(&mut self.seen);
        if let Some(i) = before
            .iter()
            .position(|e| matches!(e, Event::Backgrounded { .. }))
        {
            self.seen = before.split_off(i + 1);
            let Some(Event::Backgrounded { seq }) = before.pop() else {
                unreachable!("found just above")
            };
            return (seq, before);
        }
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(Event::Backgrounded { seq }) => return (seq, before),
                Ok(other) => before.push(other),
                Err(e) => panic!("no Backgrounded event within {within:?}: {e}"),
            }
        }
    }

    /// No `Backgrounded` comes within `within`; anything else that does is
    /// set aside.
    pub fn no_backgrounded(&mut self, within: Duration) {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(Event::Backgrounded { seq }) => panic!("Backgrounded {seq} came early"),
                Ok(other) => self.seen.push(other),
                Err(_) => return,
            }
        }
    }

    /// Make a store from the test phrase in `dir` and open it.
    pub fn create_from_phrase(&mut self, dir: &Path) -> Reply {
        self.call(Command::CreateFromPhrase {
            dir: dir.to_path_buf(),
            password: secret(PASSWORD),
            password_again: secret(PASSWORD),
            phrase: secret(PHRASE),
        })
    }

    pub fn unlock(&mut self, dir: &Path, password: &str) -> Reply {
        self.call(Command::Unlock {
            dir: dir.to_path_buf(),
            password: secret(password),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::mpsc;

    use super::*;

    /// The bound the panic tests use, short enough that they run in a moment.
    const SHORT: Duration = Duration::from_millis(200);

    /// What a loaded machine may add to [`SHORT`] before a wait counts as hung.
    const SLACK: Duration = Duration::from_secs(10);

    fn panic_text(payload: Box<dyn Any + Send>) -> String {
        if let Some(text) = payload.downcast_ref::<String>() {
            return text.clone();
        }
        payload.downcast_ref::<&str>().map_or_else(
            || "a panic with no text".to_owned(),
            |text| (*text).to_owned(),
        )
    }

    /// Runs `wait` on a thread of its own, and returns the text of its panic,
    /// or `None` if it returned. The test waits no longer than [`SHORT`] plus
    /// [`SLACK`] for that thread, so a wait that never ends fails the test here
    /// rather than hanging the run.
    fn run_bounded(wait: impl FnOnce() + Send + 'static) -> Option<String> {
        let (done, answer) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(wait));
            let _ = done.send(outcome.err().map(panic_text));
        });
        answer
            .recv_timeout(SHORT + SLACK)
            .expect("the wait neither returned nor panicked within its bound")
    }

    #[test]
    fn a_gate_no_request_reaches_panics_within_its_bound() {
        let gate = Gate::with_bound(SHORT);
        let started = Instant::now();
        let panicked = run_bounded(move || gate.wait_reached(1));
        assert!(
            started.elapsed() >= SHORT,
            "the wait gave up before its bound"
        );
        let message = panicked.expect("a gate nothing reaches must panic, not return");
        assert!(message.contains("Gate::wait_reached"), "{message}");
        assert!(message.contains("only 0 of the 1 requests"), "{message}");
    }

    #[test]
    fn a_request_the_gate_never_releases_panics_within_its_bound() {
        let held = Gate::with_bound(SHORT);
        let started = Instant::now();
        let panicked = run_bounded(move || held.pass());
        assert!(
            started.elapsed() >= SHORT,
            "the request gave up before its bound"
        );
        let message = panicked.expect("a request the gate never releases must panic");
        assert!(
            message.contains("the test never opened the gate"),
            "{message}"
        );
    }

    #[test]
    fn a_gate_reached_in_time_lets_the_wait_return() {
        let gate = Gate::default();
        let held = gate.clone();
        let request = std::thread::spawn(move || held.pass());
        gate.wait_reached(1);
        gate.open();
        request
            .join()
            .expect("the held request is released once the gate opens");
    }
}
