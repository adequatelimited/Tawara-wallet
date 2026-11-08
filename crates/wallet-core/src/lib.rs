// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Everything in Tawara that touches the wallet library, `mochimo-crypto`.
//!
//! This crate has no interface dependency, so it is unchanged if a shell is
//! replaced. The `app` crate talks to it and never to the library.
//!
//! It provides:
//!
//! - **One worker thread** that owns the `Keystore` and the `Wallet`
//!   ([`worker`]). Nothing calls the library on the interface thread: the
//!   transport is synchronous, `Wallet::open` reconciles over the network,
//!   and unlocking at `Kdf::RECOMMENDED` costs 64 MiB over three passes.
//! - **Commands in, events out** ([`Command`], [`Event`]). Events carry view
//!   models ([`view`]): plain data that holds no secret. The one exception
//!   is the recovery phrase, shown once when a store is created
//!   ([`PhraseForDisplay`]).
//! - **Cancellation and progress** through the library's `recon::Cancel`
//!   and `recon::Progress`: opening the wallet, a refresh, a status read, a
//!   restore, an acknowledged advance and a discovery sweep can each be
//!   stopped, and the long ones report how far they have got.
//! - **Store location, entropy and secret input** ([`location`],
//!   [`entropy`], [`SecretText`]), and the store identifier that ties a
//!   password kept on a device to its store ([`store_identifier`]).
//! - **Lifecycle.** An idle timer, an explicit Lock, and a move to the
//!   background on mobile each drop the `Keystore`, which drops the secret
//!   and releases the store's lock together.
//! - **The library's wording** for every protective report and refusal: the
//!   worker builds the command line's outcomes and has the library's
//!   renderer write them.
//!
//! `tests/policy.rs` checks the repository's rules: the library pinned by
//! full commit hash and never patched, the release profile that keeps the
//! library's scrubs running, the test-only key-derivation cost kept out of
//! every shipped path, and the boundary between this crate and the interface.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod amount;
mod command;
pub mod entropy;
mod event;
pub mod explorer;
pub mod location;
pub mod node;
pub mod preferences;
pub mod retained;
pub mod sample;
mod secret;
pub mod spend;
pub mod staking;
pub mod store_identifier;
mod text;
pub mod view;
pub mod worker;

pub use command::{Command, PlanId, RequestId};
pub use event::{
    AccountReport, Activity, Discovered, Event, LockReason, PlanView, PlannedDestination, Progress,
    ReceiveView, Refusal, RefusalKind, Reply, SentView,
};
pub use node::{Connect, HttpsNode, NetworkName, NodeRefused, SyncState};
pub use secret::{PhraseForDisplay, SecretText};
pub use worker::{
    Config, DEFAULT_IDLE_LOCK, WorkerHandle, WorkerStopped, save_artifact, save_artifact_in, spawn,
};

/// Whether `dir` already holds a store, by the library's test
/// (`keystore::occupied`, which `create` refuses on). If it does, the
/// interface opens on unlocking rather than on creating a store.
#[must_use]
pub fn store_exists(dir: &std::path::Path) -> bool {
    mochimo_crypto::keystore::occupied(dir).is_some()
}

/// The shortest password a new store accepts, in characters: the library's
/// floor (`cli::create::MIN_PASSWORD_LEN`), shown before anything is typed.
/// The library enforces it regardless of the interface.
pub const MIN_PASSWORD_LEN: usize = mochimo_crypto::cli::create::MIN_PASSWORD_LEN;

/// The positions (one-based) of the words the confirmation asks for: the
/// library's (`cli::create::CONFIRM_POSITIONS`).
pub const CONFIRM_POSITIONS: [usize; 3] = mochimo_crypto::cli::create::CONFIRM_POSITIONS;

/// The furthest key index a scan or an acknowledged advance may name
/// (`Command::Status`'s and `Command::Restore`'s `scan_to`,
/// `Command::Reconcile`'s `advance_to`): the command line's bound for
/// `--scan-to` and `--advance-to`. `u32::MAX` is refused: the last position
/// cannot be advanced from, so an account placed there could never reserve a
/// spend, and a walk to it would end at a position it never derives.
///
/// The library walks every position up to the index named, about 1.6 ms
/// each in a release build, so a far index is a long wait. Each walk can be
/// stopped by a cancel or when the idle period passes, so none delays a lock.
pub const MAX_KEY_INDEX: u32 = u32::MAX - 1;

/// How far a discovery sweep goes by default: the library's default
/// (`cli::args::DISCOVER_DEFAULT_TO`).
pub const DISCOVER_DEFAULT_TO: u32 = mochimo_crypto::cli::args::DISCOVER_DEFAULT_TO;

/// The furthest a discovery sweep goes, one node request per account: the
/// library's ceiling (`cli::args::DISCOVER_MAX_TO`).
pub const DISCOVER_MAX_TO: u32 = mochimo_crypto::cli::args::DISCOVER_MAX_TO;

/// The most destinations one spend carries (`tx::MAX_DESTINATIONS`); the
/// interface stops adding rows at it.
pub const MAX_DESTINATIONS: u16 = mochimo_crypto::tx::MAX_DESTINATIONS;

/// The network fee per destination, in nanoMCM (`consts::MFEE`): what a
/// spend pays when it gives no fee of its own, as the Send page's spends do.
pub const FEE_PER_DESTINATION: u64 = mochimo_crypto::consts::MFEE;

/// The wallet library commit this build is pinned to, shown in Settings. The
/// library carries no version to read; the policy tests check this against
/// the workspace's `rev`.
pub const LIBRARY_REV: &str = "12b1fee9afde5fdf049a97c4f320515cd9655c38";

/// How a new store's key is derived from its password.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreKdf {
    /// Argon2id's memory, in KiB.
    pub memory_kib: u32,
    /// Its passes over that memory.
    pub passes: u32,
    /// Its lanes.
    pub lanes: u32,
}

/// The cost a new store's key is derived at: the library's
/// (`keystore::Kdf::RECOMMENDED`). A store made elsewhere opens at the cost
/// its own header names, which the library does not expose for an open store.
pub const NEW_STORE_KDF: StoreKdf = StoreKdf {
    memory_kib: mochimo_crypto::keystore::Kdf::RECOMMENDED.m_cost_kib,
    passes: mochimo_crypto::keystore::Kdf::RECOMMENDED.t_cost,
    lanes: mochimo_crypto::keystore::Kdf::RECOMMENDED.p_cost,
};

/// The node's rule for a destination's reference, in the library's wording
/// (`cli::args::REFERENCE_RULE`), shown beside the field.
pub const REFERENCE_RULE: &str = mochimo_crypto::cli::args::REFERENCE_RULE;
