// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The worker, end to end, against a scripted node.
//!
//! Every case drives the worker through its handle, as the interface will,
//! and reads only its events. The store is a real store in a scratch
//! directory and the key derivation is the library's at its recommended
//! cost, so each create or unlock costs about a second in a test build
//! (the workspace optimises the two derivation crates in the dev profile).

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mochimo_crypto::keystore::{self, Keystore, Unlock};
use mochimo_crypto::tx::wire::Transaction;
use support::*;
use tawara_wallet_core::explorer::IndexState;
use tawara_wallet_core::location::{Environment, Platform, default_store_dir};
use tawara_wallet_core::spend::{Amount, DestinationInput, SpendInputError, SpendRequest};
use tawara_wallet_core::view::{
    AccountId, AccountRow, AccountState, DivergenceKind, NoticeKind, Reads, ReservationState,
    SettledAccount, Total, WalletView, Watch,
};
use tawara_wallet_core::{
    Activity, CONFIRM_POSITIONS, Command, Config, DISCOVER_MAX_TO, Event, LockReason,
    MAX_KEY_INDEX, NetworkName, PlanView, Progress, Refusal, RefusalKind, Reply, RequestId,
    SentView, SyncState,
};

const FUNDS: u64 = 5_000_000;

fn refusal(reply: Reply) -> Refusal {
    match reply {
        Reply::Refused(r) => r,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn opened(reply: Reply) -> WalletView {
    match reply {
        Reply::Created {
            opened: Ok(view), ..
        }
        | Reply::Unlocked(view)
        | Reply::Wallet(view) => view,
        other => panic!("expected an open store, got {other:?}"),
    }
}

fn planned(reply: Reply) -> PlanView {
    match reply {
        Reply::Planned(p) => p,
        other => panic!("expected a plan, got {other:?}"),
    }
}

fn sent(reply: Reply) -> SentView {
    match reply {
        Reply::Sent(s) => s,
        other => panic!("expected a sent spend, got {other:?}"),
    }
}

fn account0() -> AccountId {
    AccountId::from_tag(tag(0))
}

fn pay(to: String, amount: Amount) -> SpendRequest {
    SpendRequest {
        from: account0(),
        destinations: vec![DestinationInput {
            to,
            amount,
            reference: String::new(),
        }],
        fee: None,
        blk_to_live: 0,
    }
}

/// A harness with a node, and the test phrase's store in `scratch`, funded
/// at account 0's first key and opened. Returns the key index it opened at.
fn funded(scratch: &Scratch) -> (Harness, u32) {
    let mut h = Harness::with_node();
    h.chain.hold(tag(0), address(0, 0), FUNDS);
    let view = opened(h.create_from_phrase(&scratch.store()));
    assert!(
        view.opened,
        "the wallet should open on a funded store: {view:?}"
    );
    let row = &view.accounts[0];
    assert_eq!(row.id, account0());
    assert_eq!(row.state, AccountState::InSync { balance: FUNDS });
    assert!(row.spendable);
    (h, row.index)
}

// ------------------------------------------------------------------ create

#[test]
fn a_new_store_is_written_only_once_its_phrase_is_confirmed() {
    let scratch = Scratch::new("create");
    let dir = scratch.store();
    let mut h = Harness::new();

    let (words, positions) = match h.call(Command::CreateBegin {
        dir: dir.clone(),
        password: secret(PASSWORD),
        password_again: secret(PASSWORD),
    }) {
        Reply::CreatePhrase {
            phrase,
            confirm_positions,
        } => (
            phrase.words().map(str::to_owned).collect::<Vec<_>>(),
            confirm_positions,
        ),
        other => panic!("expected a phrase, got {other:?}"),
    };
    assert_eq!(words.len(), 24);
    assert_eq!(positions, CONFIRM_POSITIONS);
    assert_eq!(
        keystore::occupied(&dir),
        None,
        "nothing is written before the confirmation"
    );

    // The wrong words: refused, nothing written, and the phrase still waits.
    let wrong = refusal(h.call(Command::CreateConfirm {
        answer: secret("zoo zoo zoo"),
    }));
    assert_eq!(wrong.kind, RefusalKind::ConfirmationWrong);
    assert!(wrong.text.contains("Nothing was created"), "{}", wrong.text);
    assert_eq!(keystore::occupied(&dir), None);

    let answer = positions
        .iter()
        .map(|&p| words[p - 1].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let reply = h.call(Command::CreateConfirm {
        answer: secret(&answer),
    });
    let (first, view) = match reply {
        Reply::Created {
            dir: written,
            first,
            opened: Ok(view),
        } => {
            assert_eq!(written, dir);
            (first, view)
        }
        other => panic!("expected a written store, got {other:?}"),
    };
    assert!(keystore::occupied(&dir).is_some(), "the store is written");
    assert_eq!(view.accounts.len(), 1);
    assert_eq!(view.accounts[0].id, first);
    // No node: the store is open and nothing was reconciled.
    assert!(!view.opened);
    assert_eq!(view.accounts[0].state, AccountState::NotReconciled);
    assert_eq!(
        view.notice.as_ref().map(|n| n.kind),
        Some(NoticeKind::NoNode)
    );
    assert!(
        view.notice
            .as_deref()
            .is_some_and(|n| n.contains("No API is chosen")),
        "{:?}",
        view.notice
    );
    assert!(
        h.seen.iter().any(|e| matches!(
            e,
            Event::Busy {
                activity: Activity::DerivingKey,
                ..
            }
        )),
        "the key derivation is announced"
    );

    // The phrase was used up.
    let again = refusal(h.call(Command::CreateConfirm {
        answer: secret(&answer),
    }));
    assert_eq!(again.kind, RefusalKind::NothingToConfirm);
}

#[test]
fn creating_refuses_before_a_phrase_is_shown() {
    let scratch = Scratch::new("create-refusals");
    let dir = scratch.store();
    let mut h = Harness::new();

    let short = refusal(h.call(Command::CreateBegin {
        dir: dir.clone(),
        password: secret("short"),
        password_again: secret("short"),
    }));
    assert_eq!(short.kind, RefusalKind::PasswordTooShort);
    assert_eq!(
        short.text,
        "That password is 5 character(s), and the minimum is 12. No wallet was created. Try again."
    );

    let differ = refusal(h.call(Command::CreateBegin {
        dir: dir.clone(),
        password: secret(PASSWORD),
        password_again: secret("correct horse battery!"),
    }));
    assert_eq!(differ.kind, RefusalKind::PasswordsDiffer);
    assert_eq!(keystore::occupied(&dir), None);

    let _ = opened(h.create_from_phrase(&dir));
    // A store is there now: refused first, before the password is looked at.
    let occupied = refusal(h.call(Command::CreateBegin {
        dir: dir.clone(),
        password: secret("short"),
        password_again: secret("other"),
    }));
    assert_eq!(occupied.kind, RefusalKind::Occupied);
    let occupied = refusal(h.create_from_phrase(&dir));
    assert_eq!(occupied.kind, RefusalKind::Occupied);
}

#[test]
fn abandoning_drops_the_pending_phrase() {
    let scratch = Scratch::new("abandon");
    let mut h = Harness::new();
    assert!(matches!(
        h.call(Command::CreateBegin {
            dir: scratch.store(),
            password: secret(PASSWORD),
            password_again: secret(PASSWORD),
        }),
        Reply::CreatePhrase { .. }
    ));
    assert!(matches!(
        h.call(Command::CreateAbandon),
        Reply::CreateAbandoned
    ));
    let none = refusal(h.call(Command::CreateConfirm {
        answer: secret("a b c"),
    }));
    assert_eq!(none.kind, RefusalKind::NothingToConfirm);
    assert_eq!(keystore::occupied(&scratch.store()), None);
}

#[cfg(unix)]
#[test]
fn a_store_the_library_will_not_write_leaves_its_phrase_waiting() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("create-unsafe");
    let dir = scratch.store();
    // An empty folder anyone may write to: the library will not put a
    // store in it.
    std::fs::create_dir_all(&dir).expect("store folder");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).expect("mode");
    let mut h = Harness::new();
    let (words, positions) = match h.call(Command::CreateBegin {
        dir: dir.clone(),
        password: secret(PASSWORD),
        password_again: secret(PASSWORD),
    }) {
        Reply::CreatePhrase {
            phrase,
            confirm_positions,
        } => (
            phrase.words().map(str::to_owned).collect::<Vec<_>>(),
            confirm_positions,
        ),
        other => panic!("expected a phrase, got {other:?}"),
    };
    let answer = positions
        .iter()
        .map(|&p| words[p - 1].as_str())
        .collect::<Vec<_>>()
        .join(" ");

    // The right words, and a folder the library refuses: nothing written,
    // and the phrase still waits.
    let unsafe_folder = refusal(h.call(Command::CreateConfirm {
        answer: secret(&answer),
    }));
    assert_eq!(
        unsafe_folder.kind,
        RefusalKind::UnsafeDirectory,
        "{}",
        unsafe_folder.text
    );
    assert!(
        unsafe_folder.text.contains("Nothing was created"),
        "{}",
        unsafe_folder.text
    );
    assert_eq!(keystore::occupied(&dir), None);

    // Put right, the same words write the store.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("mode");
    match h.call(Command::CreateConfirm {
        answer: secret(&answer),
    }) {
        Reply::Created { dir: written, .. } => assert_eq!(written, dir),
        other => panic!("expected a written store, got {other:?}"),
    }
    assert!(keystore::occupied(&dir).is_some());
}

// ------------------------------------------------------------------ unlock

#[test]
fn unlocking_refuses_a_wrong_password_a_missing_store_and_a_held_one() {
    let scratch = Scratch::new("unlock-refusals");
    let dir = scratch.store();
    let mut h = Harness::new();
    let _ = opened(h.create_from_phrase(&dir));
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    assert_eq!(h.wait_locked(Duration::from_secs(5)), LockReason::Asked);

    let wrong = refusal(h.unlock(&dir, "not the password at all"));
    assert_eq!(wrong.kind, RefusalKind::WrongPassword);
    // Tawara's wording, and nothing more.
    assert_eq!(wrong.text, "Password is likely wrong.");

    let missing = refusal(h.unlock(&scratch.0.join("nothing-here"), PASSWORD));
    assert_eq!(missing.kind, RefusalKind::NoStore);

    // Another process holding the store: its lock is `flock`, per open
    // file, so a second open in this process is refused the same way.
    let held = Keystore::open(
        &dir,
        &Unlock {
            password: PASSWORD.as_bytes(),
            nonce_seed: [7; 32],
        },
    )
    .expect("the test opens the store");
    let in_use = refusal(h.unlock(&dir, PASSWORD));
    assert_eq!(in_use.kind, RefusalKind::StoreInUse);
    drop(held);

    let view = opened(h.unlock(&dir, PASSWORD));
    assert_eq!(view.dir, dir);
}

#[test]
fn without_a_node_or_with_a_silent_one_the_store_stays_open() {
    let scratch = Scratch::new("no-node");
    let dir = scratch.store();
    let mut h = Harness::new();
    let view = opened(h.create_from_phrase(&dir));
    assert!(!view.opened);

    // The store's accounts can be shown, and nothing can be sent.
    assert!(matches!(
        h.call(Command::Receive {
            account: account0()
        }),
        Reply::Receive(_)
    ));
    let not_open = refusal(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(1_000)),
    }));
    assert_eq!(not_open.kind, RefusalKind::WalletNotOpen);

    // A node that does not answer.
    assert!(matches!(
        h.call(Command::SetNode { url: NODE.into() }),
        Reply::NodeSet { .. }
    ));
    h.chain.set_unreachable(true);
    let view = opened(h.call(Command::Refresh));
    assert!(!view.opened);
    assert!(
        view.notice
            .as_deref()
            .is_some_and(|n| n.contains("did not answer")),
        "{:?}",
        view.notice
    );

    // It answers, and the account is paid: the wallet opens.
    h.chain.set_unreachable(false);
    h.chain
        .hold(tag(0), address(0, view.accounts[0].index), FUNDS);
    let view = opened(h.call(Command::Refresh));
    assert!(view.opened, "{view:?}");
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync { balance: FUNDS }
    );
    assert!(view.accounts[0].spendable);
    assert_eq!(view.notice, None);
}

#[test]
fn a_store_nobody_has_paid_stays_a_store_until_it_is() {
    let scratch = Scratch::new("unpaid");
    let mut h = Harness::with_node();
    let view = opened(h.create_from_phrase(&scratch.store()));
    assert!(
        !view.opened,
        "nothing on the ledger: the wallet cannot open: {view:?}"
    );
    assert_eq!(view.accounts.len(), 1);
    // The library's reading of "account not found", word for word: it
    // cannot tell a new account from an emptied one or a failed lookup.
    assert!(
        matches!(&view.accounts[0].state, AccountState::Diverged {
            kind: DivergenceKind::NotFound,
            report,
            advance_to: None,
            ..
        } if report.contains("account not found")),
        "{:?}",
        view.accounts[0].state
    );
    let notice = view.notice.expect("a store that is not whole says so");
    assert_eq!(notice.kind, NoticeKind::NotWhole);
    assert!(notice.starts_with("THIS STORE IS NOT WHOLE"), "{notice}");
    assert!(!notice.ends_with("---"), "{notice:?}");

    h.chain
        .hold(tag(0), address(0, view.accounts[0].index), FUNDS);
    let view = opened(h.call(Command::Refresh));
    assert!(view.opened, "{view:?}");
    assert_eq!(view.total(), Total::Whole(u128::from(FUNDS)));
}

// ----------------------------------------------------------------- receive

#[test]
fn receiving_shows_the_destination_and_the_ledger_entry() {
    let scratch = Scratch::new("receive");
    let mut h = Harness::new();
    let _ = opened(h.create_from_phrase(&scratch.store()));
    match h.call(Command::Receive {
        account: account0(),
    }) {
        Reply::Receive(r) => {
            assert_eq!(r.destination.as_deref(), Some(destination(0).as_str()));
            assert_eq!(r.ledger_address, hex_of(&address(0, r.index)));
            assert!(!r.text.is_empty());
        }
        other => panic!("expected a destination, got {other:?}"),
    }
    // A tag the store does not hold: the command line's page, which echoes
    // it in hex so it cannot be taken for a destination.
    let other = refusal(h.call(Command::Receive {
        account: AccountId::from_tag(tag(5)),
    }));
    assert!(other.text.contains(&hex_of(&tag(5))), "{}", other.text);
}

// ------------------------------------------------------------------- spend

#[test]
fn a_spend_is_planned_signed_submitted_and_settled() {
    let scratch = Scratch::new("spend");
    let (mut h, start) = funded(&scratch);
    let calls = h.chain.calls();

    // A plan that is replaced cannot be confirmed.
    let stale = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    assert!(h.chain.calls() > calls, "planning reads the ledger");
    assert_ne!(stale.plan, plan.plan);
    assert_eq!(plan.send_total, 100_000);
    assert_eq!(plan.fee_total, 500);
    assert_eq!(plan.balance, FUNDS);
    assert_eq!(plan.change_total, FUNDS - 100_500);
    assert!(!plan.empties_account);
    // The library's page for a spend not yet signed, word for word.
    assert!(plan.text.starts_with("NOT SIGNED"), "{}", plan.text);
    assert!(plan.text.contains(&destination(1)), "{}", plan.text);
    assert!(
        plan.text.contains("No key has been reserved or used"),
        "{}",
        plan.text
    );
    assert!(
        !plan.text.contains("THIS EMPTIES THE ACCOUNT"),
        "{}",
        plan.text
    );
    assert_eq!(plan.destinations.len(), 1);
    assert_eq!(plan.destinations[0].destination, destination(1));
    assert_eq!(plan.destinations[0].amount, 100_000);
    assert!(
        h.chain.submits().is_empty(),
        "nothing is signed by planning"
    );

    let wrong = refusal(h.call(Command::ConfirmSend { plan: stale.plan }));
    assert_eq!(wrong.kind, RefusalKind::NoSuchPlan);
    assert!(h.chain.submits().is_empty());

    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    let s = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    let submits = h.chain.submits();
    assert_eq!(submits.len(), 1);
    assert!(s.submitted);
    assert!(s.tx_id.is_some());
    assert_eq!(
        s.artifact_hex,
        hex_of(&submits[0]),
        "the artifact is the bytes the API was sent"
    );
    assert!(!s.text.is_empty());
    let row = &s.view.accounts[0];
    assert!(!row.spendable, "a reserved account cannot spend again");
    assert!(
        matches!(
            row.state,
            AccountState::SpendOutstanding {
                spent_index,
                reservation: ReservationState::Live,
                ..
            } if spent_index == start
        ),
        "{:?}",
        row.state
    );

    // Nothing landed yet: settling changes nothing.
    match h.call(Command::Settle {
        account: account0(),
    }) {
        Reply::Settled { view, .. } => {
            assert!(matches!(
                view.accounts[0].state,
                AccountState::SpendOutstanding { .. }
            ));
        }
        other => panic!("expected a settlement report, got {other:?}"),
    }

    // The spend lands: the change is at the next key. The next read settles
    // it by itself, so the account is in sync and can spend again with no
    // Settle, and the settle moves no key index.
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    h.chain.hold(tag(1), address(1, 0), 100_000);
    let before = store_files(&scratch.store());
    let view = opened(h.call(Command::Refresh));
    let row = &view.accounts[0];
    assert_eq!(
        row.state,
        AccountState::InSync {
            balance: plan.change_total
        },
        "the refresh settles the landed spend"
    );
    assert!(row.spendable);
    assert_eq!(row.index, start + 1, "settling moves no key index");
    assert_eq!(settled_ids(&view), vec![account0()]);
    assert_ne!(
        store_files(&scratch.store()),
        before,
        "the settle is written"
    );

    // Settling again is a no-op: the account stays as it is, and nothing is
    // written.
    let settled = store_files(&scratch.store());
    match h.call(Command::Settle {
        account: account0(),
    }) {
        Reply::Settled { view, text } => {
            let row = &view.accounts[0];
            assert_eq!(
                row.state,
                AccountState::InSync {
                    balance: plan.change_total
                },
                "{text}"
            );
            assert_eq!(row.index, start + 1);
            assert!(row.spendable);
            assert!(view.settled.is_empty(), "Settle says what it did itself");
        }
        other => panic!("expected a settlement, got {other:?}"),
    }
    assert_eq!(
        store_files(&scratch.store()),
        settled,
        "a second settle writes nothing"
    );
}

/// Every file under a store's folder, with its bytes: a store writes nothing
/// unless one of them changes.
///
/// The lock file is not read. An open store holds it under an exclusive lock,
/// and Windows enforces a lock against every other handle, so a read of it
/// while the store is open fails with `ERROR_LOCK_VIOLATION` (33). The library
/// takes that lock and never writes the file, so it is recorded as empty, and
/// checked to be: its length is read from its metadata, which takes no handle
/// on its contents.
fn store_files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).expect("the store's folder") {
            let path = entry.expect("a folder entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.file_name() == Some(std::ffi::OsStr::new("keystore.lock")) {
                let len = std::fs::metadata(&path).expect("the lock file").len();
                assert_eq!(len, 0, "the library never writes the lock file");
                out.push((path, Vec::new()));
            } else {
                let bytes = std::fs::read(&path).expect("a store file");
                out.push((path, bytes));
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, &mut files);
    files.sort();
    files
}

/// Plan and send `nano` from account 0 to a payee, and return the plan. Its
/// change is the balance the account holds once the spend lands.
fn send_from_account0(h: &mut Harness, nano: u64) -> PlanView {
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(nano)),
    }));
    let s = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    assert!(s.submitted);
    plan
}

/// The accounts a view says the worker settled on its own, each with
/// the library's settled page, as Settle's reply carries it.
fn settled_ids(view: &WalletView) -> Vec<AccountId> {
    view.settled
        .iter()
        .map(|s| {
            assert!(s.text.contains("settled:"), "{}", s.text);
            s.account
        })
        .collect()
}

/// The account `id` in a view.
fn row_of(view: &WalletView, id: AccountId) -> AccountRow {
    view.accounts
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .expect("the account is in the store")
}

#[test]
fn a_landed_spend_settles_when_the_wallet_is_unlocked() {
    let scratch = Scratch::new("settle-unlock");
    let (mut h, start) = funded(&scratch);
    let plan = send_from_account0(&mut h, 100_000);
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    // The spend lands while the wallet is locked.
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let view = opened(h.unlock(&scratch.store(), PASSWORD));
    let row = &view.accounts[0];
    assert_eq!(
        row.state,
        AccountState::InSync {
            balance: plan.change_total
        },
        "{row:?}"
    );
    assert!(row.spendable);
    assert_eq!(row.index, start + 1);
    assert_eq!(settled_ids(&view), vec![account0()]);

    // The next refresh has nothing left to settle.
    let before = store_files(&scratch.store());
    let view = opened(h.call(Command::Refresh));
    assert!(view.settled.is_empty());
    assert_eq!(store_files(&scratch.store()), before);
}

#[test]
fn a_check_of_an_account_in_sync_reads_coins_paid_in_since() {
    // What the wallet's balance checks rely on: a check
    // of an account in sync reads its balance afresh, so a payment received
    // since the wallet opened shows at the next check.
    let scratch = Scratch::new("check-paid-in");
    let (mut h, start) = funded(&scratch);
    h.chain.hold(tag(0), address(0, start), FUNDS + 7);
    match h.call(Command::Status {
        account: account0(),
        scan_to: None,
    }) {
        Reply::Status { state, .. } => {
            assert_eq!(state, AccountState::InSync { balance: FUNDS + 7 });
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_check_settles_a_landed_spend_and_says_what_it_did() {
    let scratch = Scratch::new("settle-check");
    let (mut h, start) = funded(&scratch);
    let plan = send_from_account0(&mut h, 100_000);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    match h.call(Command::Status {
        account: account0(),
        scan_to: None,
    }) {
        Reply::Status {
            account,
            state,
            spendable,
            text,
            settled,
        } => {
            assert_eq!(
                state,
                AccountState::InSync {
                    balance: plan.change_total
                },
                "{text}"
            );
            assert!(spendable);
            // The library's own settled page, not the page that says it has
            // landed and waits for a settle.
            assert!(text.starts_with("settled:"), "{text}");
            assert_eq!(settled, Some(SettledAccount { account, text }));
        }
        other => panic!("expected a status, got {other:?}"),
    }
    let view = opened(h.call(Command::Refresh));
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert!(view.settled.is_empty());
}

#[test]
fn the_read_after_a_send_settles_a_spend_the_node_already_shows_landed() {
    let scratch = Scratch::new("settle-after-send");
    let (mut h, start) = funded(&scratch);
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    // Sending asks the node twice: the ledger is read once more before the
    // reservation, and then the bytes are submitted. The first request passes
    // its gate and reads the ledger as it stood. The submit waits at a second
    // gate, and while it waits the spend lands, so the read that follows the
    // submit finds the change key.
    let first = h.chain.close_gate();
    let confirm = h
        .handle
        .send(Command::ConfirmSend { plan: plan.plan })
        .expect("worker running");
    first.wait_reached(1);
    let submit = h.chain.close_gate();
    first.open();
    submit.wait_reached(1);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    submit.open();
    let s = sent(h.wait_for(confirm));
    assert!(s.submitted);
    let row = &s.view.accounts[0];
    assert_eq!(
        row.state,
        AccountState::InSync {
            balance: plan.change_total
        },
        "{:?}",
        row.state
    );
    assert!(row.spendable);
    assert_eq!(settled_ids(&s.view), vec![account0()]);
}

#[test]
fn a_stop_that_reaches_the_read_after_a_send_settles_nothing() {
    let scratch = Scratch::new("settle-after-send-stopped");
    let (mut h, start) = funded(&scratch);
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    // As in the test above, with a third gate on the read that follows the
    // submit. The spend lands and a stop is heard while that read waits, so
    // the read finds the change key but may not settle it.
    let first = h.chain.close_gate();
    let confirm = h
        .handle
        .send(Command::ConfirmSend { plan: plan.plan })
        .expect("worker running");
    first.wait_reached(1);
    let submit = h.chain.close_gate();
    first.open();
    submit.wait_reached(1);
    let read = h.chain.close_gate();
    submit.open();
    read.wait_reached(1);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    h.handle.cancel();
    read.open();
    let s = sent(h.wait_for(confirm));
    assert!(s.submitted, "the spend was submitted before the stop");
    assert!(
        matches!(
            s.view.accounts[0].state,
            AccountState::SpendLanded { spent_index, .. } if spent_index == start
        ),
        "{:?}",
        s.view.accounts[0].state
    );
    assert!(s.view.settled.is_empty());

    // A refresh that is not stopped settles it.
    let view = opened(h.call(Command::Refresh));
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert_eq!(settled_ids(&view), vec![account0()]);
}

#[test]
fn a_spend_still_outstanding_is_not_settled_and_writes_nothing() {
    let scratch = Scratch::new("settle-outstanding");
    let (mut h, start) = funded(&scratch);
    let _ = send_from_account0(&mut h, 100_000);
    let before = store_files(&scratch.store());

    // Live: the node still holds the spent key with its balance. One read of
    // the account is the whole of the refresh's work on it; a settle would
    // ask the node a second time.
    let reads = h.chain.resolves(tag(0));
    let view = opened(h.call(Command::Refresh));
    assert!(
        matches!(
            view.accounts[0].state,
            AccountState::SpendOutstanding {
                reservation: ReservationState::Live,
                ..
            }
        ),
        "{:?}",
        view.accounts[0].state
    );
    assert_eq!(
        h.chain.resolves(tag(0)) - reads,
        1,
        "one read of the account, and no settle"
    );
    assert!(view.settled.is_empty());
    assert_eq!(store_files(&scratch.store()), before, "nothing was written");

    // Dead: the balance the reservation was built on has moved.
    h.chain.hold(tag(0), address(0, start), FUNDS - 1);
    let view = opened(h.call(Command::Refresh));
    assert!(
        matches!(
            view.accounts[0].state,
            AccountState::SpendOutstanding {
                reservation: ReservationState::Dead {
                    balance_moved: true,
                    ..
                },
                ..
            }
        ),
        "{:?}",
        view.accounts[0].state
    );
    assert!(view.settled.is_empty());
    assert_eq!(store_files(&scratch.store()), before, "nothing was written");
}

#[test]
fn a_refresh_cancelled_while_it_reads_does_not_settle() {
    let scratch = Scratch::new("settle-cancelled");
    let (mut h, start) = funded(&scratch);
    let plan = send_from_account0(&mut h, 100_000);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let before = store_files(&scratch.store());
    let gate = h.chain.close_gate();
    let refresh = h.handle.send(Command::Refresh).expect("worker running");
    // The read of the account is held at the node; the stop lands in it.
    gate.wait_reached(1);
    h.handle.cancel();
    gate.open();
    assert_eq!(refusal(h.wait_for(refresh)).kind, RefusalKind::Cancelled);
    assert_eq!(
        store_files(&scratch.store()),
        before,
        "a cancelled read writes nothing"
    );
    // The next refresh, not cancelled, settles it.
    let view = opened(h.call(Command::Refresh));
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert_eq!(settled_ids(&view), vec![account0()]);
}

#[test]
fn a_stop_heard_during_one_automatic_settle_leaves_the_others_unsettled() {
    let scratch = Scratch::new("settle-stop");
    let (mut h, start) = funded(&scratch);
    let account1 = AccountId::from_tag(tag(1));
    h.chain.hold(tag(1), address(1, 0), 1_000_000);
    match h.call(Command::Restore {
        account_index: 1,
        scan_to: None,
    }) {
        Reply::Restored { ok: true, .. } => {}
        other => panic!("expected a restore, got {other:?}"),
    }
    let plan0 = send_from_account0(&mut h, 100_000);
    let plan1 = planned(h.call(Command::PlanSend {
        spend: SpendRequest {
            from: account1,
            ..pay(destination(2), Amount::Nano(100_000))
        },
    }));
    let _ = sent(h.call(Command::ConfirmSend { plan: plan1.plan }));
    // Both spends land.
    h.chain
        .hold(tag(0), address(0, start + 1), plan0.change_total);
    h.chain.hold(tag(1), address(1, 1), plan1.change_total);

    // Each node request takes a while. The refresh's two reads are its first
    // two requests, and its first settle's own read is the third. The stop is
    // heard while that settle waits on the node, so the second landed account
    // is left for a later read.
    h.chain.slow(Duration::from_millis(400));
    let base = h.chain.calls();
    let refresh = h.handle.send(Command::Refresh).expect("worker running");
    let until = Instant::now() + Duration::from_secs(60);
    while h.chain.calls() < base + 3 {
        assert!(Instant::now() < until, "the settle never asked the API");
        std::thread::sleep(Duration::from_millis(5));
    }
    h.handle.cancel();
    let view = opened(h.wait_for(refresh));
    h.chain.slow(Duration::ZERO);

    // Which account settles first is the store's order, not this test's to
    // choose: exactly one settles, and the other stays landed.
    assert_eq!(view.settled.len(), 1, "{:?}", view.settled);
    let balance_of = |account: AccountId| {
        if account == account0() {
            plan0.change_total
        } else {
            plan1.change_total
        }
    };
    let settled = view.settled[0].account;
    let other = if settled == account0() {
        account1
    } else {
        account0()
    };
    assert_eq!(
        row_of(&view, settled).state,
        AccountState::InSync {
            balance: balance_of(settled)
        }
    );
    assert!(
        matches!(row_of(&view, other).state, AccountState::SpendLanded { .. }),
        "{:?}",
        row_of(&view, other).state
    );
}

#[test]
fn a_stop_heard_while_the_wallet_opens_settles_nothing() {
    let scratch = Scratch::new("settle-open-stop");
    let dir = scratch.store();
    let (mut h, start) = funded(&scratch);
    let account1 = AccountId::from_tag(tag(1));
    h.chain.hold(tag(1), address(1, 0), 1_000_000);
    match h.call(Command::Restore {
        account_index: 1,
        scan_to: None,
    }) {
        Reply::Restored { ok: true, .. } => {}
        other => panic!("expected a restore, got {other:?}"),
    }
    let plan0 = send_from_account0(&mut h, 100_000);
    let plan1 = planned(h.call(Command::PlanSend {
        spend: SpendRequest {
            from: account1,
            ..pay(destination(2), Amount::Nano(100_000))
        },
    }));
    let _ = sent(h.call(Command::ConfirmSend { plan: plan1.plan }));
    h.chain
        .hold(tag(0), address(0, start + 1), plan0.change_total);
    h.chain.hold(tag(1), address(1, 1), plan1.change_total);
    assert!(matches!(h.call(Command::Lock), Reply::Locked));

    // The wallet opens against both landed accounts. The library asks before
    // each account whether to stop, so a stop heard during the second read
    // lets the open complete. The worker must then settle neither account.
    let first = h.chain.close_gate();
    let unlock = h
        .handle
        .send(Command::Unlock {
            dir: dir.clone(),
            password: secret(PASSWORD),
        })
        .expect("worker running");
    first.wait_reached(1);
    let second = h.chain.close_gate();
    first.open();
    second.wait_reached(1);
    h.handle.cancel();
    second.open();
    let view = opened(h.wait_for(unlock));
    assert!(view.opened, "{view:?}");
    assert!(view.settled.is_empty(), "{:?}", view.settled);
    for account in [account0(), account1] {
        assert!(
            matches!(
                row_of(&view, account).state,
                AccountState::SpendLanded { .. }
            ),
            "{:?}",
            row_of(&view, account).state
        );
    }

    // The next read, not stopped, settles both.
    let view = opened(h.call(Command::Refresh));
    assert_eq!(view.settled.len(), 2, "{:?}", view.settled);
}

#[test]
fn a_locked_store_settles_nothing_and_asks_the_node_nothing() {
    let scratch = Scratch::new("settle-locked");
    let (mut h, start) = funded(&scratch);
    let plan = send_from_account0(&mut h, 100_000);
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let before = store_files(&scratch.store());
    let calls = h.chain.calls();
    assert_eq!(
        refusal(h.call(Command::Refresh)).kind,
        RefusalKind::NotUnlocked
    );
    assert_eq!(
        refusal(h.call(Command::Status {
            account: account0(),
            scan_to: None,
        }))
        .kind,
        RefusalKind::NotUnlocked
    );
    assert_eq!(
        h.chain.calls(),
        calls,
        "a locked wallet asks the API nothing"
    );
    assert_eq!(store_files(&scratch.store()), before);
}

#[test]
fn an_idle_period_that_passes_during_a_refresh_settles_nothing_and_locks() {
    let scratch = Scratch::new("settle-idle-read");
    let mut h = Harness::with(Config {
        idle_lock: Duration::from_secs(5),
        ..Config::default()
    });
    assert!(matches!(
        h.call(Command::SetNode { url: NODE.into() }),
        Reply::NodeSet { .. }
    ));
    h.chain.hold(tag(0), address(0, 0), FUNDS);
    let start = opened(h.create_from_phrase(&scratch.store())).accounts[0].index;
    h.handle.touch();
    let plan = send_from_account0(&mut h, 100_000);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let before = store_files(&scratch.store());
    // The last input is marked here, and then the person does nothing for
    // longer than the idle period, while the read waits on the node.
    h.handle.touch();
    let gate = h.chain.close_gate();
    let refresh = h.handle.send(Command::Refresh).expect("worker running");
    gate.wait_reached(1);
    std::thread::sleep(Duration::from_millis(6_000));
    gate.open();
    assert_eq!(refusal(h.wait_for(refresh)).kind, RefusalKind::Cancelled);
    assert_eq!(h.wait_locked(Duration::from_secs(10)), LockReason::Idle);
    assert_eq!(
        store_files(&scratch.store()),
        before,
        "a stopped read writes nothing"
    );
}

#[test]
fn a_node_that_cannot_be_reached_settles_nothing_and_writes_nothing() {
    let scratch = Scratch::new("settle-unreachable");
    let (mut h, start) = funded(&scratch);
    let plan = send_from_account0(&mut h, 100_000);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let before = store_files(&scratch.store());
    h.chain.set_unreachable(true);
    let view = opened(h.call(Command::Refresh));
    assert!(
        matches!(view.accounts[0].state, AccountState::Diverged { .. }),
        "{:?}",
        view.accounts[0].state
    );
    assert!(view.settled.is_empty());
    assert_eq!(store_files(&scratch.store()), before, "nothing was written");

    h.chain.set_unreachable(false);
    let view = opened(h.call(Command::Refresh));
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert_eq!(settled_ids(&view), vec![account0()]);
}

#[test]
fn a_node_that_stops_answering_during_the_automatic_settle_writes_nothing() {
    let scratch = Scratch::new("settle-refused");
    let (mut h, start) = funded(&scratch);
    let plan = send_from_account0(&mut h, 100_000);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let before = store_files(&scratch.store());
    // The refresh's own read is the node's next answer; the settle's read
    // after it is not, so the library refuses the settle.
    let reads = h.chain.resolves(tag(0));
    h.chain.vanish_from(tag(0), reads + 2);
    let view = opened(h.call(Command::Refresh));
    assert!(
        matches!(
            view.accounts[0].state,
            AccountState::SpendLanded { spent_index, .. } if spent_index == start
        ),
        "{:?}",
        view.accounts[0].state
    );
    assert!(view.settled.is_empty());
    assert_eq!(store_files(&scratch.store()), before, "nothing was written");

    // Once the node answers again, the next read settles it.
    h.chain.vanish_from(tag(0), usize::MAX);
    let view = opened(h.call(Command::Refresh));
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert_eq!(settled_ids(&view), vec![account0()]);
}

#[test]
fn a_node_whose_answer_changes_before_the_settle_holds_the_lock_for_its_walk() {
    // The known case where a settle holds the lock past a move to the
    // background. The check's read shows the spend landed. The settle's own
    // read, which the library makes inside `settle_if_landed` with no
    // cancel, then finds the account at a key the
    // reservation does not explain, so the library walks key positions to
    // say where it is. A move to the background asked while that read waits
    // stops nothing: the walk runs to its end, the check answers, and only
    // then does the lock come.
    let scratch = Scratch::new("settle-walk");
    let (mut h, start) = funded(&scratch);
    let plan = send_from_account0(&mut h, 100_000);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let before = store_files(&scratch.store());
    // The check's own read passes the first gate, and the settle's waits at
    // the second.
    let first = h.chain.close_gate();
    let check = h
        .handle
        .send(Command::Status {
            account: account0(),
            scan_to: None,
        })
        .expect("worker running");
    first.wait_reached(1);
    let second = h.chain.close_gate();
    first.open();
    second.wait_reached(1);
    // Five keys past the change key: inside the window the library searches
    // around it, so the walk is short here. A key no position derives would
    // be searched for up to the library's ceiling of 10,000 positions, at
    // about 1.6 ms a position, and the lock would wait as long.
    h.chain
        .hold(tag(0), address(0, start + 6), plan.change_total);
    let asked = Instant::now();
    let _ = h.handle.background();
    second.open();
    match h.wait_for(check) {
        Reply::Status {
            state,
            spendable,
            settled,
            ..
        } => {
            // What the check's read found; the settle found the account
            // unexplained, and the library refused it.
            assert!(
                matches!(state, AccountState::SpendLanded { spent_index, .. } if spent_index == start),
                "{state:?}"
            );
            assert!(!spendable);
            assert_eq!(settled, None);
        }
        other => panic!("expected a status, not a cancel: {other:?}"),
    }
    assert!(
        !h.seen.iter().any(|e| matches!(e, Event::Locked { .. })),
        "the lock waited for the check, walk and all: {:?}",
        h.seen
    );
    assert_eq!(
        h.wait_locked(Duration::from_secs(30)),
        LockReason::Background
    );
    let waited = asked.elapsed();
    assert!(
        waited < Duration::from_secs(30),
        "the lock waited {waited:?} for a walk inside the window"
    );
    assert_eq!(store_files(&scratch.store()), before, "nothing was written");
}

/// A settle whose write fails leaves the library's handle poisoned:
/// the command answers as if nothing was settled, and then the store is
/// closed, as a reopen that cannot read it back closes it. Unlocking it reads
/// disk again, and that unlock settles the account.
#[cfg(unix)]
#[test]
fn a_settle_whose_write_fails_closes_the_store_after_the_answer() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("settle-poisoned");
    let dir = scratch.store();
    let (mut h, start) = funded(&scratch);
    let plan = send_from_account0(&mut h, 100_000);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let before = store_files(&dir);
    // A folder the store cannot write to: the library's commit fails before
    // anything is durable, and poisons the handle.
    let writable = std::fs::metadata(&dir).expect("the store").permissions();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500))
        .expect("the store's folder made read-only");
    let probe = dir.join("probe");
    if std::fs::write(&probe, b"").is_ok() {
        // The superuser writes whatever the mode says: nothing to test here.
        let _ = std::fs::remove_file(&probe);
        std::fs::set_permissions(&dir, writable).expect("the store's folder restored");
        return;
    }
    let reply = h.call(Command::Status {
        account: account0(),
        scan_to: None,
    });
    std::fs::set_permissions(&dir, writable).expect("the store's folder restored");
    match reply {
        Reply::Status {
            state,
            spendable,
            settled,
            ..
        } => {
            assert!(
                matches!(state, AccountState::SpendLanded { spent_index, .. } if spent_index == start),
                "{state:?}"
            );
            assert!(!spendable);
            assert_eq!(settled, None);
        }
        other => panic!("expected a status, got {other:?}"),
    }
    assert!(
        !h.seen.iter().any(|e| matches!(e, Event::Locked { .. })),
        "the store closes after the answer: {:?}",
        h.seen
    );
    assert_eq!(
        h.wait_locked(Duration::from_secs(10)),
        LockReason::ReopenFailed
    );
    assert_eq!(store_files(&dir), before, "nothing was written");
    assert_eq!(
        refusal(h.call(Command::Refresh)).kind,
        RefusalKind::NotUnlocked,
        "the poisoned handle is gone"
    );

    // Unlocking reads disk again, and that unlock settles the account.
    let view = opened(h.unlock(&dir, PASSWORD));
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert_eq!(settled_ids(&view), vec![account0()]);
}

#[test]
fn a_diverged_account_is_not_settled_by_a_refresh_or_a_check() {
    let scratch = Scratch::new("settle-diverged");
    let (mut h, start) = funded(&scratch);
    let plan = send_from_account0(&mut h, 100_000);
    // The node holds the account at neither the key that signed nor the
    // change key: the reservation is unexplained.
    h.chain
        .hold(tag(0), address(0, start + 5), plan.change_total);
    let before = store_files(&scratch.store());
    let view = opened(h.call(Command::Refresh));
    assert!(
        matches!(view.accounts[0].state, AccountState::Diverged { .. }),
        "{:?}",
        view.accounts[0].state
    );
    assert!(view.settled.is_empty());
    match h.call(Command::Status {
        account: account0(),
        scan_to: None,
    }) {
        Reply::Status { state, .. } => {
            assert!(matches!(state, AccountState::Diverged { .. }), "{state:?}");
        }
        other => panic!("expected a status, got {other:?}"),
    }
    assert_eq!(store_files(&scratch.store()), before, "nothing was written");
}

#[test]
fn resigning_reproduces_the_reserved_bytes_and_nothing_else() {
    let scratch = Scratch::new("resign");
    let (mut h, _) = funded(&scratch);
    let request = pay(destination(1), Amount::Nano(250_000));

    // The node refuses the write: the bytes are still handed back.
    h.chain.refuse_submits(true);
    let plan = planned(h.call(Command::PlanSend {
        spend: request.clone(),
    }));
    let first = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    assert!(!first.submitted);
    assert_eq!(first.tx_id, None);
    assert!(!first.artifact_hex.is_empty());

    h.chain.refuse_submits(false);
    let again = sent(h.call(Command::Resign {
        spend: request.clone(),
    }));
    assert!(again.submitted);
    assert_eq!(
        again.artifact_hex, first.artifact_hex,
        "the same spend, byte for byte"
    );
    let submits = h.chain.submits();
    assert_eq!(submits.len(), 2);
    assert_eq!(submits[0], submits[1]);

    // A different amount is another spend, and is refused.
    let other = refusal(h.call(Command::Resign {
        spend: pay(destination(1), Amount::Nano(250_001)),
    }));
    assert_eq!(other.kind, RefusalKind::Library);
    assert_eq!(h.chain.submits().len(), 2, "nothing more was sent");

    // The saved bytes go out again with no store open.
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    match h.call(Command::SubmitArtifact {
        artifact_hex: format!("{}\n", first.artifact_hex),
    }) {
        Reply::Submitted { accepted, text } => assert!(accepted, "{text}"),
        other => panic!("expected a submission, got {other:?}"),
    }
    assert_eq!(h.chain.submits().len(), 3);
    match h.call(Command::SubmitArtifact {
        artifact_hex: "not hex".into(),
    }) {
        Reply::Submitted { accepted, .. } => assert!(!accepted),
        other => panic!("expected a refused submission, got {other:?}"),
    }
}

#[test]
fn everything_empties_the_account_from_one_ledger_read() {
    let scratch = Scratch::new("everything");
    let (mut h, _) = funded(&scratch);
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Everything),
    }));
    assert_eq!(plan.send_total, FUNDS - 500);
    assert_eq!(plan.change_total, 0);
    assert!(plan.empties_account);
    // The library's own warning, before anything is signed.
    assert!(plan.text.starts_with("NOT SIGNED"), "{}", plan.text);
    assert!(
        plan.text.contains("THIS EMPTIES THE ACCOUNT"),
        "{}",
        plan.text
    );
    assert_eq!(plan.destinations[0].amount, FUNDS - 500);
}

#[test]
fn spend_input_is_refused_before_the_node_is_asked() {
    let scratch = Scratch::new("spend-input");
    let (mut h, _) = funded(&scratch);
    let calls = h.chain.calls();

    let mut twice = pay(destination(1), Amount::Nano(1_000));
    twice.destinations.push(twice.destinations[0].clone());
    let dup = refusal(h.call(Command::PlanSend { spend: twice }));
    assert_eq!(
        dup.kind,
        RefusalKind::Spend(SpendInputError::Duplicate {
            first: 1,
            second: 2
        })
    );

    let bare = refusal(h.call(Command::PlanSend {
        spend: pay(hex_of(&tag(1)), Amount::Nano(1_000)),
    }));
    assert!(matches!(
        bare.kind,
        RefusalKind::Spend(SpendInputError::BareHex { index: 1, .. })
    ));

    let none = refusal(h.call(Command::PlanSend {
        spend: SpendRequest {
            destinations: Vec::new(),
            ..pay(String::new(), Amount::Nano(0))
        },
    }));
    assert!(matches!(
        none.kind,
        RefusalKind::Spend(SpendInputError::Count { given: 0 })
    ));

    assert_eq!(h.chain.calls(), calls, "no refused input reached the API");
}

// ------------------------------------------------------ divergence, restore

#[test]
fn a_diverged_store_is_reopened_and_advanced_on_acknowledgement() {
    let scratch = Scratch::new("diverged");
    let dir = scratch.store();
    let mut h = Harness::new();
    let view = opened(h.create_from_phrase(&dir));
    let start = view.accounts[0].index;

    // The chain holds the account three keys ahead: another wallet spent
    // from it. Every account diverged, so the library would refuse the
    // wallet and drop the store; the store stays open with its page.
    h.chain.hold(tag(0), address(0, start + 3), FUNDS);
    assert!(matches!(
        h.call(Command::SetNode { url: NODE.into() }),
        Reply::NodeSet { .. }
    ));
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    assert_eq!(h.wait_locked(Duration::from_secs(5)), LockReason::Asked);
    let view = opened(h.unlock(&dir, PASSWORD));
    assert!(!view.opened, "{view:?}");
    let notice = view.notice.clone().expect("the refusal page");
    assert_eq!(notice.kind, NoticeKind::WillNotStart);
    assert!(notice.starts_with("WALLET WILL NOT START"), "{notice}");
    // Asking again keeps it open: nothing needs the password twice.
    let again = opened(h.call(Command::Refresh));
    assert!(!again.opened);
    assert_eq!(again.notice, view.notice);
    let advance_to = match &view.accounts[0].state {
        AccountState::Diverged {
            advance_to: Some(to),
            kind,
            ..
        } => {
            // The chain is ahead of the store, by the keys spent elsewhere.
            assert_eq!(*kind, DivergenceKind::Ahead { gap: 3 });
            *to
        }
        other => panic!("expected a divergence with an advance, got {other:?}"),
    };
    assert_eq!(advance_to, start + 3);

    let refused = refusal(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(1_000)),
    }));
    assert_eq!(refused.kind, RefusalKind::WalletNotOpen);

    // An index the report does not name: the library refuses it, and the
    // store is still open afterwards.
    match h.call(Command::Reconcile {
        account: account0(),
        advance_to: advance_to - 1,
    }) {
        Reply::Reconciled {
            ok,
            advanced_to,
            text,
            opened: Ok(view),
        } => {
            assert!(!ok, "{text}");
            assert_eq!(advanced_to, None, "{text}");
            assert!(!view.opened);
            assert_eq!(view.accounts[0].index, start);
        }
        other => panic!("expected the refused advance, got {other:?}"),
    }
    assert!(
        !h.seen.iter().any(|e| matches!(e, Event::Locked { .. })),
        "the store was never closed: {:?}",
        h.seen
    );

    let id = h
        .handle
        .send(Command::Reconcile {
            account: account0(),
            advance_to,
        })
        .expect("worker running");
    match h.wait_for(id) {
        Reply::Reconciled {
            ok,
            advanced_to,
            text,
            opened: Ok(view),
        } => {
            assert!(ok, "{text}");
            assert_eq!(advanced_to, Some(advance_to), "{text}");
            assert!(view.opened, "{view:?}");
            assert_eq!(view.accounts[0].index, advance_to);
            assert_eq!(
                view.accounts[0].state,
                AccountState::InSync { balance: FUNDS }
            );
        }
        other => panic!("expected the advance, got {other:?}"),
    }
    // The advance reported its account before reviewing it and again
    // before the re-check that comes before its write, and the wallet
    // opening after it reported it once more. None walked far enough to
    // report a position.
    let advancing = progress_of(&h, id);
    assert_eq!(advancing.len(), 3, "{advancing:?}");
    assert!(
        advancing
            .iter()
            .all(|p| p.account == 0 && p.accounts == 1 && p.position == 0),
        "{advancing:?}"
    );
}

#[test]
fn an_account_the_chain_holds_behind_is_rolled_back_only_to_the_index_the_chain_shows() {
    let scratch = Scratch::new("rollback");
    let dir = scratch.store();
    let mut h = Harness::new();
    let view = opened(h.create_from_phrase(&dir));
    let start = view.accounts[0].index;

    // The store is moved three keys on, to where the chain holds it.
    h.chain.hold(tag(0), address(0, start + 3), FUNDS);
    assert!(matches!(
        h.call(Command::SetNode { url: NODE.into() }),
        Reply::NodeSet { .. }
    ));
    assert!(matches!(
        h.call(Command::Reconcile {
            account: account0(),
            advance_to: start + 3,
        }),
        Reply::Reconciled { ok: true, .. }
    ));

    // The network rolls back: the chain holds it two keys behind the store.
    h.chain.hold(tag(0), address(0, start + 1), FUNDS);
    let view = opened(h.call(Command::Refresh));
    let rollback_to = match &view.accounts[0].state {
        AccountState::Diverged {
            kind,
            advance_to: None,
            rollback_to: Some(to),
            ..
        } => {
            assert_eq!(*kind, DivergenceKind::Behind { gap: 2 });
            *to
        }
        other => panic!("expected a divergence with a rollback, got {other:?}"),
    };
    assert_eq!(rollback_to, start + 1);

    // An index the chain does not show: refused, and nothing moved.
    match h.call(Command::Rollback {
        account: account0(),
        rollback_to: start,
    }) {
        Reply::Reconciled {
            ok,
            advanced_to,
            text,
            opened: Ok(view),
        } => {
            assert!(!ok, "{text}");
            assert_eq!(advanced_to, None, "{text}");
            assert_eq!(view.accounts[0].index, start + 3);
        }
        other => panic!("expected the refused rollback, got {other:?}"),
    }

    // The index the chain shows: moved back to it, and in sync.
    match h.call(Command::Rollback {
        account: account0(),
        rollback_to,
    }) {
        Reply::Reconciled {
            ok,
            advanced_to,
            text,
            opened: Ok(view),
        } => {
            assert!(ok, "{text}");
            assert_eq!(advanced_to, Some(rollback_to), "{text}");
            assert_eq!(view.accounts[0].index, rollback_to);
            assert_eq!(
                view.accounts[0].state,
                AccountState::InSync { balance: FUNDS }
            );
        }
        other => panic!("expected the rollback, got {other:?}"),
    }
}

#[test]
fn restoring_and_discovering_derived_accounts() {
    let scratch = Scratch::new("restore");
    let (mut h, _) = funded(&scratch);
    h.chain.hold(tag(2), address(2, 4), 7_000);

    match h.call(Command::Discover { to: 3 }) {
        Reply::Discovered { accounts, text } => {
            assert_eq!(accounts.len(), 4, "{text}");
            assert_eq!(accounts[0].ledger_balance, Some(FUNDS));
            assert!(accounts[0].held);
            assert_eq!(accounts[1].ledger_balance, None);
            assert_eq!(accounts[2].ledger_balance, Some(7_000));
            assert!(!accounts[2].held);
            assert_eq!(accounts[2].id, AccountId::from_tag(tag(2)));
        }
        other => panic!("expected a sweep, got {other:?}"),
    }

    match h.call(Command::Restore {
        account_index: 2,
        scan_to: None,
    }) {
        Reply::Restored {
            ok,
            text,
            opened: Ok(view),
        } => {
            assert!(ok, "{text}");
            assert!(view.opened, "{text}");
            assert_eq!(view.accounts.len(), 2, "{text}");
            let row = view
                .accounts
                .iter()
                .find(|r| r.id == AccountId::from_tag(tag(2)))
                .expect("the restored account");
            assert_eq!(row.index, 4, "{text}");
            assert_eq!(row.state, AccountState::InSync { balance: 7_000 });
            assert_eq!(view.total(), Total::Whole(u128::from(FUNDS) + 7_000));
        }
        other => panic!("expected a restore, got {other:?}"),
    }
}

// --------------------------------------------------------------- lifecycle

#[test]
fn an_idle_store_locks_itself() {
    let scratch = Scratch::new("idle");
    let mut h = Harness::with(Config {
        idle_lock: Duration::from_millis(300),
        ..Config::default()
    });
    let _ = opened(h.create_from_phrase(&scratch.store()));
    assert_eq!(h.wait_locked(Duration::from_secs(10)), LockReason::Idle);
    let locked = refusal(h.call(Command::Refresh));
    assert_eq!(locked.kind, RefusalKind::NotUnlocked);
}

#[test]
fn a_shorter_idle_period_takes_effect_at_once() {
    let scratch = Scratch::new("idle-period");
    let mut h = Harness::with(Config {
        idle_lock: Duration::from_secs(3_600),
        ..Config::default()
    });
    let _ = opened(h.create_from_phrase(&scratch.store()));
    // The worker is waiting out the hour it started with; the new period
    // wakes it, and counts from the person's last input.
    h.handle.set_idle_lock(Duration::from_millis(300));
    let started = Instant::now();
    assert_eq!(h.wait_locked(Duration::from_secs(10)), LockReason::Idle);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn an_idle_lock_drops_a_waiting_phrase_and_says_so() {
    let scratch = Scratch::new("idle-phrase");
    let mut h = Harness::with(Config {
        idle_lock: Duration::from_millis(300),
        ..Config::default()
    });
    assert!(matches!(
        h.call(Command::CreateBegin {
            dir: scratch.store(),
            password: secret(PASSWORD),
            password_again: secret(PASSWORD),
        }),
        Reply::CreatePhrase { .. }
    ));
    assert_eq!(h.wait_locked(Duration::from_secs(10)), LockReason::Idle);
    let none = refusal(h.call(Command::CreateConfirm {
        answer: secret("a b c"),
    }));
    assert_eq!(none.kind, RefusalKind::NothingToConfirm);
    assert_eq!(keystore::occupied(&scratch.store()), None);
}

#[test]
fn touching_keeps_the_store_open_and_polling_does_not() {
    let scratch = Scratch::new("polling");
    let mut h = Harness::with(Config {
        idle_lock: Duration::from_secs(1),
        ..Config::default()
    });
    assert!(matches!(
        h.call(Command::SetNode { url: NODE.into() }),
        Reply::NodeSet { .. }
    ));
    let _ = opened(h.create_from_phrase(&scratch.store()));

    let until = Instant::now() + Duration::from_millis(1_500);
    while Instant::now() < until {
        h.handle.touch();
        std::thread::sleep(Duration::from_millis(100));
    }
    while let Ok(event) = h.events.try_recv() {
        assert!(
            !matches!(event, Event::Locked { .. }),
            "locked while touched"
        );
    }

    // Asking the node over and over is not the person doing anything.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !h.seen.iter().any(|e| matches!(e, Event::Locked { .. })) {
        assert!(
            Instant::now() < deadline,
            "the store never locked while it was polled"
        );
        assert!(matches!(
            h.call(Command::NetworkStatus),
            Reply::Network { .. }
        ));
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(h.wait_locked(Duration::ZERO), LockReason::Idle);
}

#[test]
fn a_move_to_the_background_is_answered_in_turn_with_its_number() {
    // The worker locks after the command it is running, and what that
    // command answers (a recovery phrase, for one) comes before the lock. A
    // shell waits for the interface to have taken this move's
    // `Backgrounded`, which comes after both.
    let scratch = Scratch::new("backgrounded-in-turn");
    let (mut h, _) = funded(&scratch);
    let gate = h.chain.close_gate();
    let refresh = h.handle.send(Command::Refresh).expect("worker running");
    assert_eq!(h.wait_busy(refresh), Activity::AskingNode);
    // `Busy` comes before the request is made: wait until the worker is at
    // the gate, so the move cannot cancel the refresh before it asks (it
    // would then lock at once, as it should, and this test would not be
    // about a held command).
    gate.wait_reached(1);
    let first = h.handle.background();
    h.no_backgrounded(Duration::from_millis(300));
    gate.open();
    let (seq, before) = h.wait_backgrounded(Duration::from_secs(10));
    assert_eq!(seq, first, "the number background() gave");
    let answered = before
        .iter()
        .position(|e| matches!(e, Event::Done { id, .. } if *id == refresh))
        .expect("the refresh answered before the move was");
    let locked = before
        .iter()
        .position(|e| matches!(e, Event::Locked { .. }))
        .expect("the store locked before the move was answered");
    assert!(answered < locked, "{before:?}");

    let second = h.handle.background();
    assert_eq!(second, first + 1);
    assert_eq!(h.wait_backgrounded(Duration::from_secs(10)).0, second);
}

#[test]
fn once_stopped_is_said_no_command_is_left_in_the_queue() {
    // A command still queued when the worker ends (here an unlock, with its
    // password, sent behind the shutdown) is dropped before `Stopped`, so a
    // shell that lets the process be suspended on `Stopped` leaves no secret
    // behind. After it, the queue takes nothing more.
    let scratch = Scratch::new("stopped-queue");
    let dir = scratch.store();
    let h = Harness::new();
    h.handle.shutdown();
    let _ = h.handle.send(Command::Unlock {
        dir: dir.clone(),
        password: secret(PASSWORD),
    });
    loop {
        if let Event::Stopped { panicked } = h
            .events
            .recv_timeout(Duration::from_secs(10))
            .expect("Stopped")
        {
            assert!(!panicked);
            break;
        }
    }
    assert!(
        h.handle
            .send(Command::Unlock {
                dir,
                password: secret(PASSWORD),
            })
            .is_err(),
        "the queue still took a command after Stopped"
    );
}

#[test]
fn every_move_to_the_background_is_reported_after_any_lock() {
    // With nothing open there is nothing to lock and no `Locked`, but the
    // interface still hears of it: what is typed and unsubmitted is its own
    // to wipe.
    let scratch = Scratch::new("backgrounded");
    let dir = scratch.store();
    let mut h = Harness::new();
    let _ = h.handle.background();
    let (_, before) = h.wait_backgrounded(Duration::from_secs(10));
    assert!(
        !before.iter().any(|e| matches!(e, Event::Locked { .. })),
        "nothing was open: {before:?}"
    );

    // With a store open, `Locked` comes first.
    let _ = opened(h.create_from_phrase(&dir));
    let _ = h.handle.background();
    let (_, before) = h.wait_backgrounded(Duration::from_secs(10));
    assert!(
        matches!(
            before.last(),
            Some(Event::Locked {
                reason: LockReason::Background
            })
        ),
        "{before:?}"
    );
}

#[test]
fn the_background_and_an_explicit_lock_close_the_store() {
    let scratch = Scratch::new("background");
    let dir = scratch.store();
    let mut h = Harness::new();
    let _ = opened(h.create_from_phrase(&dir));

    let _ = h.handle.background();
    assert_eq!(
        h.wait_locked(Duration::from_secs(10)),
        LockReason::Background
    );
    assert_eq!(
        refusal(h.call(Command::Refresh)).kind,
        RefusalKind::NotUnlocked
    );

    let _ = opened(h.unlock(&dir, PASSWORD));
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    assert_eq!(h.wait_locked(Duration::from_secs(5)), LockReason::Asked);
    // The lock was released: the store opens from outside the worker.
    let reopened = Keystore::open(
        &dir,
        &Unlock {
            password: PASSWORD.as_bytes(),
            nonce_seed: [9; 32],
        },
    );
    assert!(reopened.is_ok(), "the store's lock is released on Lock");

    // Locking with nothing open answers, and reports no closure.
    drop(reopened);
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    assert!(!h.seen.iter().any(|e| matches!(e, Event::Locked { .. })));
}

#[test]
fn unlocking_another_store_replaces_the_open_one() {
    let a = Scratch::new("replace-a");
    let b = Scratch::new("replace-b");
    let mut h = Harness::new();
    let _ = opened(h.create_from_phrase(&a.store()));
    let _ = opened(h.create_from_phrase(&b.store()));
    assert_eq!(h.wait_locked(Duration::from_secs(5)), LockReason::Replaced);
}

#[test]
fn shutting_down_locks_and_stops() {
    let scratch = Scratch::new("shutdown");
    let mut h = Harness::new();
    let _ = opened(h.create_from_phrase(&scratch.store()));
    h.handle.shutdown();
    assert_eq!(h.wait_locked(Duration::from_secs(10)), LockReason::Shutdown);
    match h.events.recv_timeout(Duration::from_secs(10)) {
        Ok(Event::Stopped { panicked }) => assert!(!panicked),
        other => panic!("expected the worker to stop, got {other:?}"),
    }
}

#[test]
fn dropping_the_last_handle_locks_and_stops() {
    let scratch = Scratch::new("drop");
    let mut h = Harness::new();
    let _ = opened(h.create_from_phrase(&scratch.store()));
    let Harness { handle, events, .. } = h;
    let clone = handle.clone();
    drop(handle);
    assert!(
        events.recv_timeout(Duration::from_millis(300)).is_err(),
        "a clone keeps the worker running"
    );
    drop(clone);
    match events.recv_timeout(Duration::from_secs(10)) {
        Ok(Event::Locked { reason }) => assert_eq!(reason, LockReason::Shutdown),
        other => panic!("expected the store to lock, got {other:?}"),
    }
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(10)),
        Ok(Event::Stopped { panicked: false })
    ));
}

#[test]
fn a_cancel_stops_what_was_sent_before_it_and_nothing_after() {
    let scratch = Scratch::new("cancel");
    let (mut h, _) = funded(&scratch);
    // A second account, so a refresh has two to read.
    h.chain.hold(tag(1), address(1, 0), 1_000);
    match h.call(Command::Restore {
        account_index: 1,
        scan_to: None,
    }) {
        Reply::Restored {
            ok: true,
            opened: Ok(view),
            text,
        } => assert_eq!(view.accounts.len(), 2, "{text}"),
        other => panic!("expected a restore, got {other:?}"),
    }

    // Raised with nothing running, it stops nothing sent afterwards.
    h.handle.cancel();
    assert!(opened(h.call(Command::Refresh)).opened);

    // Raised while a refresh waits on the node, it stops that refresh, and
    // the rows are left as they were.
    let gate = h.chain.close_gate();
    let id = h.handle.send(Command::Refresh).expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::AskingNode);
    h.handle.cancel();
    gate.open();
    assert_eq!(refusal(h.wait_for(id)).kind, RefusalKind::Cancelled);

    let view = opened(h.call(Command::Refresh));
    assert!(view.opened);
    assert_eq!(view.total(), Total::Whole(u128::from(FUNDS) + 1_000));

    // A Lock sent before a cancel still locks.
    let gate = h.chain.close_gate();
    let id = h.handle.send(Command::Refresh).expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::AskingNode);
    let lock = h.handle.send(Command::Lock).expect("worker running");
    h.handle.cancel();
    gate.open();
    assert_eq!(refusal(h.wait_for(id)).kind, RefusalKind::Cancelled);
    assert!(matches!(h.wait_for(lock), Reply::Locked));
    assert_eq!(h.wait_locked(Duration::from_secs(5)), LockReason::Asked);
}

#[test]
fn moving_to_the_background_stops_a_queued_spend_before_it_signs() {
    let scratch = Scratch::new("background-spend");
    let dir = scratch.store();
    let (mut h, start) = funded(&scratch);
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));

    // A refresh waits on the node, and the confirmation is queued behind
    // it when the app goes to the background.
    let gate = h.chain.close_gate();
    let refresh = h.handle.send(Command::Refresh).expect("worker running");
    assert_eq!(h.wait_busy(refresh), Activity::AskingNode);
    let confirm = h
        .handle
        .send(Command::ConfirmSend { plan: plan.plan })
        .expect("worker running");
    let _ = h.handle.background();
    gate.open();

    assert_eq!(refusal(h.wait_for(refresh)).kind, RefusalKind::Cancelled);
    assert_eq!(refusal(h.wait_for(confirm)).kind, RefusalKind::Cancelled);
    assert_eq!(
        h.wait_locked(Duration::from_secs(10)),
        LockReason::Background
    );
    assert!(h.chain.submits().is_empty(), "nothing was sent");

    // Nothing was reserved either: the account is as it was.
    let view = opened(h.unlock(&dir, PASSWORD));
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync { balance: FUNDS }
    );
    assert_eq!(view.accounts[0].index, start);
    assert!(view.accounts[0].spendable);
}

#[test]
fn a_cancelled_status_is_not_a_report() {
    let scratch = Scratch::new("cancel-status");
    let (mut h, _) = funded(&scratch);
    h.chain.hold(tag(1), address(1, 0), 1_000);
    match h.call(Command::Restore {
        account_index: 1,
        scan_to: None,
    }) {
        Reply::Restored { ok: true, .. } => {}
        other => panic!("expected a restore, got {other:?}"),
    }
    // Another wallet spends from account 1: the chain is three keys ahead,
    // so reading it walks the key positions, which a cancel stops.
    h.chain.hold(tag(1), address(1, 3), 900);
    let account1 = AccountId::from_tag(tag(1));

    let gate = h.chain.close_gate();
    let id = h
        .handle
        .send(Command::Status {
            account: account1,
            scan_to: None,
        })
        .expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::AskingNode);
    h.handle.cancel();
    gate.open();
    assert_eq!(refusal(h.wait_for(id)).kind, RefusalKind::Cancelled);

    match h.call(Command::Status {
        account: account1,
        scan_to: None,
    }) {
        Reply::Status {
            state:
                AccountState::Diverged {
                    advance_to: Some(3),
                    ..
                },
            ..
        } => {}
        other => panic!("expected the divergence and its advance, got {other:?}"),
    }
}

#[test]
fn scans_advances_and_sweeps_are_bounded() {
    let scratch = Scratch::new("bounds");
    let mut h = Harness::with_node();
    let _ = opened(h.create_from_phrase(&scratch.store()));
    let calls = h.chain.calls();
    for command in [
        Command::Status {
            account: account0(),
            scan_to: Some(MAX_KEY_INDEX + 1),
        },
        Command::Restore {
            account_index: 1,
            scan_to: Some(MAX_KEY_INDEX + 1),
        },
        Command::Reconcile {
            account: account0(),
            advance_to: u32::MAX,
        },
        Command::Discover { to: 0 },
        Command::Discover {
            to: DISCOVER_MAX_TO + 1,
        },
    ] {
        let name = format!("{command:?}");
        let r = refusal(h.call(command));
        assert_eq!(r.kind, RefusalKind::OutOfRange, "{name}");
        assert!(r.text.contains("Nothing was done"), "{name}: {}", r.text);
    }
    assert_eq!(h.chain.calls(), calls, "nothing reached the API");
    assert!(
        matches!(
            h.call(Command::Receive {
                account: account0()
            }),
            Reply::Receive(_)
        ),
        "the store is still open"
    );
}

#[test]
fn commands_that_need_a_store_are_refused_while_locked() {
    let mut h = Harness::with_node();
    for command in [
        Command::Refresh,
        Command::Receive {
            account: account0(),
        },
        Command::PlanSend {
            spend: pay(destination(1), Amount::Nano(1)),
        },
        Command::Settle {
            account: account0(),
        },
        Command::Status {
            account: account0(),
            scan_to: None,
        },
        Command::Discover { to: 3 },
        Command::Reconcile {
            account: account0(),
            advance_to: 1,
        },
        Command::Restore {
            account_index: 1,
            scan_to: None,
        },
    ] {
        let name = format!("{command:?}");
        let r = refusal(h.call(command));
        assert_eq!(r.kind, RefusalKind::NotUnlocked, "{name}");
    }
    assert_eq!(h.chain.calls(), 0, "a locked worker asks the API nothing");
}

// --------------------------------------------------------------------- node

#[test]
fn status_reports_an_account_without_refusing() {
    let scratch = Scratch::new("status");
    let (mut h, _) = funded(&scratch);
    let status = |h: &mut Harness| match h.call(Command::Status {
        account: account0(),
        scan_to: None,
    }) {
        Reply::Status {
            account,
            state,
            spendable,
            text,
            settled,
        } => {
            assert_eq!(account, account0());
            assert!(!text.is_empty());
            assert_eq!(settled, None, "nothing here had landed");
            (state, spendable)
        }
        other => panic!("expected a status, got {other:?}"),
    };
    assert_eq!(
        status(&mut h),
        (AccountState::InSync { balance: FUNDS }, true)
    );

    // A check the node did not answer is a report, and the account does
    // not spend; once it answers, the account the wallet held spends again.
    h.chain.set_unreachable(true);
    let (state, spendable) = status(&mut h);
    assert!(
        matches!(
            state,
            AccountState::Diverged {
                kind: DivergenceKind::Unreachable,
                ..
            }
        ),
        "{state:?}"
    );
    assert!(!spendable);
    h.chain.set_unreachable(false);
    assert_eq!(
        status(&mut h),
        (AccountState::InSync { balance: FUNDS }, true)
    );
    let view = opened(h.call(Command::Refresh));
    assert!(view.accounts[0].spendable, "{view:?}");
}

/// A spend from account 0 that may not be included after `blk_to_live`,
/// planned and confirmed. Its reservation records that block, so a read of the
/// spend asks the node for the tip, and a tip below the block leaves the
/// reservation live.
fn send_expiring_from_account0(h: &mut Harness, nano: u64, blk_to_live: u64) -> PlanView {
    let plan = planned(h.call(Command::PlanSend {
        spend: SpendRequest {
            from: account0(),
            destinations: vec![DestinationInput {
                to: destination(1),
                amount: Amount::Nano(nano),
                reference: String::new(),
            }],
            fee: None,
            blk_to_live,
        },
    }));
    let s = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    assert!(s.submitted);
    plan
}

/// One poll of account 0, as the interface asks it: the worker's
/// `Reply::Status`, with its state noted in `watch`. Returns the state, whether
/// the account spends, and the settlement the read made, if it made one.
fn poll_account0(
    h: &mut Harness,
    watch: &mut Watch,
) -> (AccountState, bool, Option<SettledAccount>) {
    match h.call(Command::Status {
        account: account0(),
        scan_to: None,
    }) {
        Reply::Status {
            account,
            state,
            spendable,
            settled,
            ..
        } => {
            assert_eq!(account, account0());
            watch.read(account, &state);
            (state, spendable, settled)
        }
        other => panic!("expected a status, got {other:?}"),
    }
}

#[test]
fn a_poll_keeps_asking_through_transient_failures_and_settles_the_landed_spend() {
    // A node that does not answer, and an expiry that cannot be
    // read, leave an outstanding spend polled, so that the poll after the
    // outage finds it landed and settles it on its own.
    let scratch = Scratch::new("poll-transient");
    let (mut h, start) = funded(&scratch);
    let plan = send_expiring_from_account0(&mut h, 100_000, 1_100);
    let mut watch = Watch::default();

    let (state, spendable, settled) = poll_account0(&mut h, &mut watch);
    assert!(
        matches!(
            state,
            AccountState::SpendOutstanding {
                reservation: ReservationState::Live,
                ..
            }
        ),
        "{state:?}"
    );
    assert!(!spendable);
    assert_eq!(settled, None);
    assert!(watch.asks(account0(), &state));

    // The node does not answer for the account: diverged for this read, and
    // still asked about.
    h.chain.set_unreachable(true);
    let (state, spendable, settled) = poll_account0(&mut h, &mut watch);
    assert!(
        matches!(
            state,
            AccountState::Diverged {
                kind: DivergenceKind::Unreachable,
                ..
            }
        ),
        "{state:?}"
    );
    assert!(!spendable);
    assert_eq!(settled, None);
    assert!(
        watch.asks(account0(), &state),
        "an API that does not answer does not stop the polls"
    );

    // The node answers the ledger but not its status, so the expiry cannot be
    // read: the reservation is unclassified, and still asked about.
    h.chain.set_unreachable(false);
    h.chain.set_status_down(true);
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(
        matches!(
            state,
            AccountState::SpendOutstanding {
                reservation: ReservationState::Unclassified,
                ..
            }
        ),
        "{state:?}"
    );
    assert!(watch.asks(account0(), &state));

    // Recovery: the expiry reads as live again.
    h.chain.set_status_down(false);
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(
        matches!(
            state,
            AccountState::SpendOutstanding {
                reservation: ReservationState::Live,
                ..
            }
        ),
        "{state:?}"
    );
    assert!(watch.asks(account0(), &state));

    // The spend lands. The poll that finds the change key settles it on its
    // own, and nothing is asked about the account afterwards.
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let (state, spendable, settled) = poll_account0(&mut h, &mut watch);
    assert_eq!(
        state,
        AccountState::InSync {
            balance: plan.change_total
        },
        "{state:?}"
    );
    assert!(spendable);
    let settled = settled.expect("the landed spend settles on this poll");
    assert_eq!(settled.account, account0());
    assert!(settled.text.starts_with("settled:"), "{}", settled.text);
    h.chain.set_unreachable(true);
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(
        matches!(
            state,
            AccountState::Diverged {
                kind: DivergenceKind::Unreachable,
                ..
            }
        ),
        "{state:?}"
    );
    assert!(
        !watch.asks(account0(), &state),
        "a settled account is not asked about"
    );
}

#[test]
fn a_poll_stops_asking_once_a_read_shows_the_reservation_dead() {
    // A dead reservation never settles, so it is not polled, and a
    // node that then fails does not bring the polls back.
    let scratch = Scratch::new("poll-dead");
    let (mut h, start) = funded(&scratch);
    send_expiring_from_account0(&mut h, 100_000, 1_100);
    let mut watch = Watch::default();
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(watch.asks(account0(), &state));

    // The balance moved on the spent key: the reservation is dead.
    h.chain.hold(tag(0), address(0, start), FUNDS + 1);
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(
        matches!(
            state,
            AccountState::SpendOutstanding {
                reservation: ReservationState::Dead {
                    balance_moved: true,
                    ..
                },
                ..
            }
        ),
        "{state:?}"
    );
    assert!(!watch.asks(account0(), &state));

    h.chain.set_unreachable(true);
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(
        matches!(
            state,
            AccountState::Diverged {
                kind: DivergenceKind::Unreachable,
                ..
            }
        ),
        "{state:?}"
    );
    assert!(!watch.asks(account0(), &state));
}

#[test]
fn a_poll_stops_asking_once_a_read_shows_an_unexplained_reservation() {
    // A real divergence is for the person to look at, not for a
    // timer to walk the chain for.
    let scratch = Scratch::new("poll-diverged");
    let (mut h, start) = funded(&scratch);
    send_expiring_from_account0(&mut h, 100_000, 1_100);
    let mut watch = Watch::default();
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(watch.asks(account0(), &state));

    // The node holds the account at neither the key that signed nor the change
    // key.
    h.chain.hold(tag(0), address(0, start + 5), FUNDS);
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(
        matches!(
            state,
            AccountState::Diverged {
                kind: DivergenceKind::ReservationUnexplained,
                ..
            }
        ),
        "{state:?}"
    );
    assert!(!watch.asks(account0(), &state));
}

/// The next poll of account 0, as the interface sends it after a read that
/// showed `last`: only when the watch asks about it, and noted as asked.
fn poll_after(
    h: &mut Harness,
    watch: &mut Watch,
    last: &AccountState,
) -> (AccountState, bool, Option<SettledAccount>) {
    assert!(
        watch.asks(account0(), last),
        "no poll is owed after {last:?}"
    );
    watch.asked(account0(), last);
    poll_account0(h, watch)
}

fn is_not_found(state: &AccountState) -> bool {
    matches!(
        state,
        AccountState::Diverged {
            kind: DivergenceKind::NotFound,
            ..
        }
    )
}

fn is_live(state: &AccountState) -> bool {
    matches!(
        state,
        AccountState::SpendOutstanding {
            reservation: ReservationState::Live,
            ..
        }
    )
}

#[test]
fn a_poll_keeps_asking_through_a_mesh_not_found_for_a_bounded_run() {
    // The Mesh answers "account not found" for a lookup that failed as well
    // as for an empty account (see the library's recon module), so the
    // polls go on through such reads, but the third in a row ends them,
    // since an account spent to zero reads so every time.
    let scratch = Scratch::new("poll-not-found");
    let (mut h, start) = funded(&scratch);
    let plan = send_expiring_from_account0(&mut h, 100_000, 1_100);
    let mut watch = Watch::default();
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(is_live(&state), "{state:?}");

    // Two reads not found, then the node answers again: the polls went on
    // throughout, and the count starts over.
    h.chain.vanish_from(tag(0), h.chain.resolves(tag(0)) + 1);
    let (state, _, _) = poll_after(&mut h, &mut watch, &state);
    assert!(is_not_found(&state), "{state:?}");
    let (state, _, _) = poll_after(&mut h, &mut watch, &state);
    assert!(is_not_found(&state), "{state:?}");
    h.chain.vanish_from(tag(0), usize::MAX);
    let (state, _, _) = poll_after(&mut h, &mut watch, &state);
    assert!(is_live(&state), "{state:?}");

    // Three reads not found in a row end the polls.
    h.chain.vanish_from(tag(0), h.chain.resolves(tag(0)) + 1);
    let (state, _, _) = poll_after(&mut h, &mut watch, &state);
    let (state, _, _) = poll_after(&mut h, &mut watch, &state);
    let (state, _, _) = poll_after(&mut h, &mut watch, &state);
    assert!(is_not_found(&state), "{state:?}");
    assert!(
        !watch.asks(account0(), &state),
        "the third read not found in a row ends the polls"
    );

    // A live read starts them again, and the spend that lands is settled by
    // the poll that finds it.
    h.chain.vanish_from(tag(0), usize::MAX);
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(is_live(&state), "{state:?}");
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let (state, spendable, settled) = poll_after(&mut h, &mut watch, &state);
    assert_eq!(
        state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert!(spendable);
    assert!(settled.is_some(), "the landed spend settles on this poll");
    assert!(!watch.asks(account0(), &state));
}

#[test]
fn a_poll_tries_again_a_landed_spend_whose_automatic_settle_the_node_interrupted() {
    // A poll that finds the spend landed settles it. When the node stops
    // answering between the read and the settle, the account is left landed,
    // and the next poll tries the settle again.
    let scratch = Scratch::new("poll-landed");
    let (mut h, start) = funded(&scratch);
    let plan = send_expiring_from_account0(&mut h, 100_000, 1_100);
    let mut watch = Watch::default();
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    assert!(is_live(&state), "{state:?}");

    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    // The poll's own read is the node's next answer; the settle's read after
    // it is not, so the library refuses the settle.
    h.chain.vanish_from(tag(0), h.chain.resolves(tag(0)) + 2);
    let (state, _, settled) = poll_after(&mut h, &mut watch, &state);
    assert!(
        matches!(state, AccountState::SpendLanded { spent_index, .. } if spent_index == start),
        "{state:?}"
    );
    assert_eq!(settled, None);
    assert!(
        watch.asks(account0(), &state),
        "a settle the API interrupted is tried again"
    );

    h.chain.vanish_from(tag(0), usize::MAX);
    let (state, spendable, settled) = poll_after(&mut h, &mut watch, &state);
    assert_eq!(
        state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert!(spendable);
    assert_eq!(settled.map(|s| s.account), Some(account0()));
    assert!(!watch.asks(account0(), &state));
}

#[test]
fn a_wallet_opened_while_the_node_fails_for_the_spend_polls_it() {
    // The read that opens the wallet may
    // fail for the account whose spend is outstanding while another account
    // reconciles, so the wallet opens with that read alone. The row carries
    // the store's reservation, and the polls start from it.
    let scratch = Scratch::new("poll-open-outage");
    let dir = scratch.store();
    let mut h = two_accounts(&scratch);
    let plan = send_expiring_from_account0(&mut h, 100_000, 1_100);
    let row0 = |view: &WalletView| row_of(view, account0());

    // The node does not answer for the account, then answers "account not
    // found" for it.
    for silent in [true, false] {
        assert!(matches!(h.call(Command::Lock), Reply::Locked));
        assert_eq!(h.wait_locked(Duration::from_secs(5)), LockReason::Asked);
        if silent {
            h.chain.set_silent_for(tag(0), true);
        } else {
            h.chain.vanish_from(tag(0), h.chain.resolves(tag(0)) + 1);
        }
        let view = opened(h.unlock(&dir, PASSWORD));
        assert!(view.opened, "the other account reconciled: {view:?}");
        let row = row0(&view);
        let kind = if silent {
            DivergenceKind::Unreachable
        } else {
            DivergenceKind::NotFound
        };
        assert!(
            matches!(&row.state, AccountState::Diverged { kind: k, .. } if *k == kind),
            "{:?}",
            row.state
        );
        assert!(row.reserved, "the store holds the spend's reservation");
        assert!(
            !row_of(&view, AccountId::from_tag(tag(1))).reserved,
            "and none for the other account"
        );
        let mut watch = Watch::default();
        watch.read_answer(&view, Reads::Every);
        assert!(
            watch.asks(account0(), &row.state),
            "a wallet opened during an outage polls the outstanding spend"
        );

        // The node answers again: the poll reads the reservation live.
        h.chain.set_silent_for(tag(0), false);
        h.chain.vanish_from(tag(0), usize::MAX);
        let (state, _, _) = poll_after(&mut h, &mut watch, &row.state);
        assert!(is_live(&state), "{state:?}");
        assert!(watch.asks(account0(), &state));
    }

    // The spend lands, and the poll settles it.
    let mut watch = Watch::default();
    let (state, _, _) = poll_account0(&mut h, &mut watch);
    let start = match state {
        AccountState::SpendOutstanding { spent_index, .. } => spent_index,
        other => panic!("expected the spend outstanding, got {other:?}"),
    };
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let (state, _, settled) = poll_after(&mut h, &mut watch, &state);
    assert!(settled.is_some(), "{state:?}");
    assert!(!watch.asks(account0(), &state));
}

/// A payment of `nano` from account 0 that may not be included after
/// `blk_to_live`, the spend `send_expiring_from_account0` makes.
fn pay_expiring(nano: u64, blk_to_live: u64) -> SpendRequest {
    SpendRequest {
        from: account0(),
        destinations: vec![DestinationInput {
            to: destination(1),
            amount: Amount::Nano(nano),
            reference: String::new(),
        }],
        fee: None,
        blk_to_live,
    }
}

#[test]
fn a_spend_signed_after_an_unlock_in_sync_polls_through_a_failed_read_and_settles() {
    // An unlock finds the account in sync, which ends its polls. A
    // spend signed after that holds a reservation, and its polls
    // start afresh: the read the sign's reply carries fails, the polls go on
    // through the outage and the recovery, and the poll that finds the spend
    // landed settles it on its own.
    let scratch = Scratch::new("poll-after-sign");
    let (mut h, start) = funded(&scratch);
    let mut watch = Watch::default();

    // The unlock: the account reads in sync, and nothing is asked about it.
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    let view = opened(h.unlock(&scratch.store(), PASSWORD));
    watch.read_answer(&view, Reads::Every);
    let state = row_of(&view, account0()).state;
    assert_eq!(state, AccountState::InSync { balance: FUNDS });
    assert!(!watch.asks(account0(), &state));

    // The spend's expiry is read, so its signing reads the tip. The node
    // answers that read, and not the read the reply carries after it.
    let plan = planned(h.call(Command::PlanSend {
        spend: pay_expiring(100_000, 1_100),
    }));
    h.chain.set_status_down_after(1);
    let signed = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    assert!(signed.submitted);
    // The interface notes the sign, then the reads its reply carries.
    watch.signed(signed.from);
    watch.read_answer(&signed.view, Reads::One(signed.from));
    let state = row_of(&signed.view, account0()).state;
    assert!(
        matches!(
            &state,
            AccountState::SpendOutstanding {
                reservation: ReservationState::Unclassified,
                spent_index,
                ..
            } if *spent_index == start
        ),
        "{state:?}"
    );
    assert!(
        watch.asks(account0(), &state),
        "a spend whose first read fails is polled"
    );

    // The next poll fails too: the expiry still cannot be read.
    let (state, _, _) = poll_after(&mut h, &mut watch, &state);
    assert!(
        matches!(
            &state,
            AccountState::SpendOutstanding {
                reservation: ReservationState::Unclassified,
                ..
            }
        ),
        "{state:?}"
    );

    // The node answers again: the reservation reads live, and is still polled.
    h.chain.set_status_down(false);
    let (state, _, _) = poll_after(&mut h, &mut watch, &state);
    assert!(is_live(&state), "{state:?}");

    // The spend lands: the poll settles it, and nothing is asked about it after.
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    let (state, spendable, settled) = poll_after(&mut h, &mut watch, &state);
    assert_eq!(
        state,
        AccountState::InSync {
            balance: plan.change_total
        },
        "{state:?}"
    );
    assert!(spendable);
    assert_eq!(settled.map(|s| s.account), Some(account0()));
    assert!(!watch.asks(account0(), &state));
}

#[test]
fn a_re_sign_is_for_the_spend_whose_reservation_is_open_and_the_library_refuses_another() {
    // Spend A is signed and lands, and
    // its settle leaves the next key to spend B, with other parameters. The
    // reservation open is B's: the library re-signs B's parameters and refuses A's,
    // so the spend a re-sign is offered must be the one whose sign reply names the
    // reservation's key index.
    let scratch = Scratch::new("resign-holds");
    let (mut h, start) = funded(&scratch);
    let a = pay(destination(1), Amount::Nano(100_000));
    let plan_a = planned(h.call(Command::PlanSend { spend: a.clone() }));
    let sent_a = sent(h.call(Command::ConfirmSend { plan: plan_a.plan }));
    assert_eq!(sent_a.spent_index, Some(start));

    // A lands, and a check settles it: the account's next key is the one after.
    h.chain
        .hold(tag(0), address(0, start + 1), plan_a.change_total);
    match h.call(Command::Status {
        account: account0(),
        scan_to: None,
    }) {
        Reply::Status { state, settled, .. } => {
            assert_eq!(
                state,
                AccountState::InSync {
                    balance: plan_a.change_total
                },
                "{state:?}"
            );
            assert!(settled.is_some(), "the landed spend settles on the check");
        }
        other => panic!("expected a status, got {other:?}"),
    }

    // B, with other parameters, holds the reservation now.
    let b = pay(destination(1), Amount::Nano(200_000));
    let plan_b = planned(h.call(Command::PlanSend { spend: b.clone() }));
    let sent_b = sent(h.call(Command::ConfirmSend { plan: plan_b.plan }));
    assert_eq!(sent_b.spent_index, Some(start + 1));
    assert_eq!(
        row_of(&sent_b.view, account0()).state.reserved_index(),
        Some(start + 1)
    );

    // A's parameters are another spend, and the library refuses them.
    let other = refusal(h.call(Command::Resign { spend: a }));
    assert_eq!(other.kind, RefusalKind::Library);
    assert_eq!(h.chain.submits().len(), 2, "nothing more was sent");

    // B's parameters are the spend the reservation holds: the same bytes again.
    let again = sent(h.call(Command::Resign { spend: b }));
    assert!(again.submitted);
    assert_eq!(again.artifact_hex, sent_b.artifact_hex);
    assert_eq!(again.spent_index, Some(start + 1));
}

#[test]
fn a_sign_reports_its_reservation_though_the_read_after_it_settles_the_spend() {
    // The spend lands before the read that
    // follows its submit, and that read settles it, closing the reservation.
    // The index the reply reports was read before that, so it is still the key
    // the spend was signed with.
    let scratch = Scratch::new("sign-index-settled");
    let (mut h, start) = funded(&scratch);
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    h.chain
        .land_on_submit(tag(0), address(0, start + 1), plan.change_total);
    let s = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    assert!(s.submitted);
    assert_eq!(settled_ids(&s.view), vec![account0()]);
    assert_eq!(
        row_of(&s.view, account0()).state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert_eq!(s.spent_index, Some(start));
}

#[test]
fn a_re_sign_reports_its_reservation_though_the_read_after_it_settles_the_spend() {
    // As for a sign: the re-signed bytes land before the read that follows
    // their submit, and that read settles them. The reply still names the
    // reservation's index.
    let scratch = Scratch::new("resign-index-settled");
    let (mut h, start) = funded(&scratch);
    let spend = pay(destination(1), Amount::Nano(100_000));
    let plan = planned(h.call(Command::PlanSend {
        spend: spend.clone(),
    }));
    let first = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    assert_eq!(first.spent_index, Some(start));
    assert!(first.view.settled.is_empty());
    h.chain
        .land_on_submit(tag(0), address(0, start + 1), plan.change_total);
    let again = sent(h.call(Command::Resign { spend }));
    assert!(again.submitted);
    assert_eq!(again.artifact_hex, first.artifact_hex);
    assert_eq!(settled_ids(&again.view), vec![account0()]);
    assert_eq!(again.spent_index, Some(start));
}

#[test]
fn the_node_is_chosen_https_or_loopback_only() {
    let mut h = Harness::new();
    let none = refusal(h.call(Command::NetworkStatus));
    assert_eq!(none.kind, RefusalKind::NoNode);

    let plain = refusal(h.call(Command::SetNode {
        url: "http://node.example:8080".into(),
    }));
    assert_eq!(plain.kind, RefusalKind::NodeRefused);
    assert!(plain.text.contains("https"), "{}", plain.text);

    assert!(matches!(
        h.call(Command::SetNode {
            url: "http://127.0.0.1:8080".into()
        }),
        Reply::NodeSet { .. }
    ));
    assert!(matches!(
        h.call(Command::SetNode { url: format!(" {NODE} ") }),
        Reply::NodeSet { url, view: None } if url == NODE
    ));
    match h.call(Command::NetworkStatus) {
        Reply::Network {
            tip_index,
            tip_hash,
            tip_time_ms,
            sync,
        } => {
            assert_eq!(tip_index, 1_000);
            assert_eq!(tip_hash, "ab".repeat(32));
            assert_eq!(tip_time_ms, 1_000 * 60_000);
            assert_eq!(
                sync,
                Some(SyncState {
                    stage: "synchronized".into(),
                    synced: true
                })
            );
        }
        other => panic!("expected the tip, got {other:?}"),
    }
    // The middleware's own word for a refresh that failed, as it sent it.
    h.chain.sync("latest block error", false);
    match h.call(Command::NetworkStatus) {
        Reply::Network { sync, .. } => assert_eq!(
            sync,
            Some(SyncState {
                stage: "latest block error".into(),
                synced: false
            })
        ),
        other => panic!("expected the tip, got {other:?}"),
    }
    match h.call(Command::Networks) {
        Reply::Networks(names) => assert_eq!(
            names,
            vec![NetworkName {
                blockchain: "mochimo".into(),
                network: "mainnet".into()
            }]
        ),
        other => panic!("expected the networks, got {other:?}"),
    }
    assert!(matches!(
        h.call(Command::ClearNode),
        Reply::NodeCleared { view: None }
    ));
    assert_eq!(
        refusal(h.call(Command::NetworkStatus)).kind,
        RefusalKind::NoNode
    );
    assert_eq!(refusal(h.call(Command::Networks)).kind, RefusalKind::NoNode);
}

// ------------------------------------------------------- node and idle edges

#[test]
fn clearing_or_changing_the_node_stops_the_open_wallet_using_the_old_one() {
    let scratch = Scratch::new("node-change");
    let (mut h, start) = funded(&scratch);
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));

    // Forget the node: the spend laid out against it does not go to it,
    // and the store stays open without it.
    match h.call(Command::ClearNode) {
        Reply::NodeCleared { view: Some(view) } => {
            assert!(!view.opened, "{view:?}");
            assert_eq!(view.accounts[0].state, AccountState::NotReconciled);
            assert!(
                view.notice
                    .as_deref()
                    .is_some_and(|n| n.contains("No API is chosen")),
                "{:?}",
                view.notice
            );
        }
        other => panic!("expected the store detached, got {other:?}"),
    }
    let r = refusal(h.call(Command::ConfirmSend { plan: plan.plan }));
    assert_eq!(r.kind, RefusalKind::WalletNotOpen, "{}", r.text);
    assert!(
        h.chain.submits().is_empty(),
        "nothing went to the API that was cleared"
    );
    let calls = h.chain.calls();
    assert_eq!(
        refusal(h.call(Command::Status {
            account: account0(),
            scan_to: None,
        }))
        .kind,
        RefusalKind::NoNode
    );
    assert_eq!(
        refusal(h.call(Command::Discover { to: 3 })).kind,
        RefusalKind::NoNode
    );
    assert_eq!(h.chain.calls(), calls, "the cleared node is asked nothing");

    // Another node: every command that asks a node asks it from now on.
    h.chain_b.hold(tag(0), address(0, start), FUNDS);
    match h.call(Command::SetNode { url: NODE_B.into() }) {
        Reply::NodeSet {
            url,
            view: Some(view),
        } => {
            assert_eq!(url, NODE_B);
            assert!(!view.opened, "{view:?}");
        }
        other => panic!("expected the API set, got {other:?}"),
    }
    let r = refusal(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    assert_eq!(r.kind, RefusalKind::WalletNotOpen, "{}", r.text);
    match h.call(Command::Status {
        account: account0(),
        scan_to: None,
    }) {
        // The store is open on its own, not as a wallet: nothing spends
        // until a refresh opens it.
        Reply::Status {
            state: AccountState::InSync { balance },
            spendable: false,
            ..
        } => assert_eq!(balance, FUNDS),
        other => panic!("expected the new API's answer, got {other:?}"),
    }
    assert!(h.chain_b.calls() > 0, "the new API was asked");
    let view = opened(h.call(Command::Refresh));
    assert!(view.opened, "{view:?}");
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync { balance: FUNDS }
    );
    assert_eq!(
        h.chain.calls(),
        calls,
        "the first node is asked nothing after the change"
    );
    assert!(h.chain.submits().is_empty() && h.chain_b.submits().is_empty());

    // Choosing the node already chosen changes nothing.
    match h.call(Command::SetNode { url: NODE_B.into() }) {
        Reply::NodeSet {
            view: Some(view), ..
        } => assert!(view.opened, "{view:?}"),
        other => panic!("expected the API set, got {other:?}"),
    }
}

#[test]
fn queued_polls_do_not_keep_the_store_open() {
    let scratch = Scratch::new("poll-backlog");
    let mut h = Harness::with(Config {
        idle_lock: Duration::from_secs(1),
        ..Config::default()
    });
    assert!(matches!(
        h.call(Command::SetNode { url: NODE.into() }),
        Reply::NodeSet { .. }
    ));
    let _ = opened(h.create_from_phrase(&scratch.store()));

    // The interface polls faster than the node answers, so polls queue up:
    // a three-second backlog against a one-second idle period.
    h.chain.slow(Duration::from_millis(250));
    let ids: Vec<_> = (0..12)
        .map(|_| {
            h.handle
                .send(Command::NetworkStatus)
                .expect("worker running")
        })
        .collect();
    let last = ids[ids.len() - 1];
    let mut locked = None;
    loop {
        match h
            .events
            .recv_timeout(Duration::from_secs(60))
            .expect("an event")
        {
            Event::Locked { reason } => locked = Some(reason),
            Event::Done { id, reply } => {
                assert!(matches!(*reply, Reply::Network { .. }), "{reply:?}");
                if id == last {
                    break;
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        locked,
        Some(LockReason::Idle),
        "the store stayed open while polls were queued"
    );
}

// ------------------------------------------------- default folder, recovery

#[test]
fn creating_at_the_default_location_makes_its_application_folder() {
    let scratch = Scratch::new("default-dir");
    let root = scratch.0.to_str().expect("a UTF-8 scratch path");
    let data = scratch.0.join("data");
    let local = scratch.0.join("Local");
    // A fresh profile: none of the folders the default path runs through
    // exist yet below the home folder.
    let env = Environment::of(&[
        ("HOME", root),
        ("XDG_DATA_HOME", data.to_str().expect("UTF-8")),
        ("LOCALAPPDATA", local.to_str().expect("UTF-8")),
    ]);
    let Ok(dir) = default_store_dir(Platform::current(), &env) else {
        return; // Android and iOS: the shell supplies the folder.
    };
    let app = dir.parent().expect("the application folder").to_path_buf();
    assert!(!app.exists(), "{} should not exist yet", app.display());

    let mut h = Harness::new();
    let view = opened(h.create_from_phrase(&dir));
    assert_eq!(view.dir, dir);
    assert!(keystore::occupied(&dir).is_some());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&app)
            .expect("app folder")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "the application folder is private: {mode:o}"
        );
    }

    // The confirmed path makes it too, and only once the phrase is
    // confirmed.
    let other = scratch.0.join("elsewhere").join("Tawara").join("keystore");
    let (words, positions) = match h.call(Command::CreateBegin {
        dir: other.clone(),
        password: secret(PASSWORD),
        password_again: secret(PASSWORD),
    }) {
        Reply::CreatePhrase {
            phrase,
            confirm_positions,
        } => (
            phrase.words().map(str::to_owned).collect::<Vec<_>>(),
            confirm_positions,
        ),
        other => panic!("expected a phrase, got {other:?}"),
    };
    assert!(
        !scratch.0.join("elsewhere").exists(),
        "nothing is made before the confirmation"
    );
    let answer = positions
        .iter()
        .map(|&p| words[p - 1].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    match h.call(Command::CreateConfirm {
        answer: secret(&answer),
    }) {
        Reply::Created {
            opened: Ok(view), ..
        } => assert_eq!(view.dir, other),
        other => panic!("expected a written store, got {other:?}"),
    }
}

#[test]
fn an_account_set_aside_at_open_settles_and_rejoins_once_it_reconciles() {
    let scratch = Scratch::new("recover");
    let dir = scratch.store();
    let (mut h, _) = funded(&scratch);
    let account1 = AccountId::from_tag(tag(1));
    h.chain.hold(tag(1), address(1, 0), 1_000_000);
    match h.call(Command::Restore {
        account_index: 1,
        scan_to: None,
    }) {
        Reply::Restored { ok: true, .. } => {}
        other => panic!("expected a restore, got {other:?}"),
    }
    let from1 = |amount| SpendRequest {
        from: account1,
        ..pay(destination(2), amount)
    };
    let plan = planned(h.call(Command::PlanSend {
        spend: from1(Amount::Nano(100_000)),
    }));
    let _ = sent(h.call(Command::ConfirmSend { plan: plan.plan }));

    // Opened again while the node answers "account not found" for account
    // 1: the wallet opens with account 0 and sets account 1 aside.
    h.chain.forget(tag(1));
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    let view = opened(h.unlock(&dir, PASSWORD));
    assert!(view.opened, "{view:?}");
    let row = |v: &WalletView| {
        v.accounts
            .iter()
            .find(|r| r.id == account1)
            .cloned()
            .expect("account 1")
    };
    assert!(
        matches!(row(&view).state, AccountState::Diverged { .. }),
        "{:?}",
        row(&view)
    );

    // The spend landed: the node now holds account 1 at its change key.
    // Settling asks the node afresh, as the library's settle does, rather
    // than refusing on what it found when the wallet opened.
    h.chain.hold(tag(1), address(1, 1), plan.change_total);
    match h.call(Command::Settle { account: account1 }) {
        Reply::Settled { view, text } => {
            assert_eq!(
                row(&view).state,
                AccountState::InSync {
                    balance: plan.change_total
                },
                "{text}"
            );
            assert_eq!(row(&view).index, 1);
        }
        other => panic!("expected the settlement, got {other:?}"),
    }

    // A refresh opens the wallet again so the account can spend.
    let view = opened(h.call(Command::Refresh));
    assert!(view.opened);
    assert!(row(&view).spendable, "{:?}", row(&view));
    let _ = planned(h.call(Command::PlanSend {
        spend: from1(Amount::Nano(1_000)),
    }));
}

/// Store with account 1 restored and a spend from it sent, then opened again
/// while the node answers "account not found" for account 1, so that the
/// wallet sets it aside; then the spend lands. Returns the harness, the plan
/// sent, and the store's files from before anything settled.
fn set_aside_and_landed(scratch: &Scratch) -> (Harness, PlanView, Vec<(PathBuf, Vec<u8>)>) {
    let dir = scratch.store();
    let (mut h, _) = funded(scratch);
    let account1 = AccountId::from_tag(tag(1));
    h.chain.hold(tag(1), address(1, 0), 1_000_000);
    match h.call(Command::Restore {
        account_index: 1,
        scan_to: None,
    }) {
        Reply::Restored { ok: true, .. } => {}
        other => panic!("expected a restore, got {other:?}"),
    }
    let plan = planned(h.call(Command::PlanSend {
        spend: SpendRequest {
            from: account1,
            ..pay(destination(2), Amount::Nano(100_000))
        },
    }));
    let _ = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    h.chain.forget(tag(1));
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    let view = opened(h.unlock(&dir, PASSWORD));
    assert!(
        matches!(row_of(&view, account1).state, AccountState::Diverged { .. }),
        "{:?}",
        row_of(&view, account1)
    );
    assert!(view.settled.is_empty());
    h.chain.hold(tag(1), address(1, 1), plan.change_total);
    let before = store_files(&dir);
    (h, plan, before)
}

#[test]
fn an_account_set_aside_at_open_settles_on_a_check_and_rejoins_on_refresh() {
    // A set-aside account settles once a read shows it
    // landed, with no Settle pressed. A check that settles it leaves it
    // aside, and the refresh after it opens the wallet with it.
    let scratch = Scratch::new("settle-set-aside-check");
    let dir = scratch.store();
    let (mut h, plan, before) = set_aside_and_landed(&scratch);
    let account1 = AccountId::from_tag(tag(1));
    match h.call(Command::Status {
        account: account1,
        scan_to: None,
    }) {
        Reply::Status {
            account,
            state,
            spendable,
            text,
            settled,
        } => {
            assert_eq!(
                state,
                AccountState::InSync {
                    balance: plan.change_total
                },
                "{text}"
            );
            assert!(!spendable, "set aside until the wallet opens again");
            assert!(text.contains("settled:"), "{text}");
            assert_eq!(settled, Some(SettledAccount { account, text }));
        }
        other => panic!("expected a status, got {other:?}"),
    }
    assert_ne!(store_files(&dir), before, "the settle is written");

    // The refresh opens the wallet again with the account, and it spends.
    let view = opened(h.call(Command::Refresh));
    let row = row_of(&view, account1);
    assert_eq!(
        row.state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert!(row.spendable, "{row:?}");
    assert!(view.settled.is_empty(), "settled already, by the check");
    let _ = planned(h.call(Command::PlanSend {
        spend: SpendRequest {
            from: account1,
            ..pay(destination(2), Amount::Nano(1_000))
        },
    }));
}

#[test]
fn an_account_set_aside_at_open_settles_on_refresh_with_no_settle_command() {
    // The automatic twin of
    // an_account_set_aside_at_open_settles_and_rejoins_once_it_reconciles:
    // the refresh that finds the spend landed opens the
    // wallet again with the account, and that opening settles it.
    let scratch = Scratch::new("settle-set-aside-refresh");
    let dir = scratch.store();
    let (mut h, plan, before) = set_aside_and_landed(&scratch);
    let account1 = AccountId::from_tag(tag(1));
    let view = opened(h.call(Command::Refresh));
    let row = row_of(&view, account1);
    assert_eq!(
        row.state,
        AccountState::InSync {
            balance: plan.change_total
        }
    );
    assert!(row.spendable, "{row:?}");
    assert_eq!(settled_ids(&view), vec![account1]);
    assert_ne!(store_files(&dir), before, "the settle is written");
    assert_eq!(row.index, 1, "as Settle leaves it (PLAN 4.9)");
    let _ = planned(h.call(Command::PlanSend {
        spend: SpendRequest {
            from: account1,
            ..pay(destination(2), Amount::Nano(1_000))
        },
    }));
}

#[test]
fn a_check_that_settles_does_not_restart_the_idle_clock() {
    let scratch = Scratch::new("settle-polling");
    let mut h = Harness::with(Config {
        idle_lock: Duration::from_secs(8),
        ..Config::default()
    });
    assert!(matches!(
        h.call(Command::SetNode { url: NODE.into() }),
        Reply::NodeSet { .. }
    ));
    h.chain.hold(tag(0), address(0, 0), FUNDS);
    let start = opened(h.create_from_phrase(&scratch.store())).accounts[0].index;
    h.handle.touch();
    let plan = send_from_account0(&mut h, 100_000);
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);

    // Checks are not the person's input. The store locks eight seconds after
    // the last input however often the account is checked, and the first
    // check, which finds the spend landed, settles it. The input is marked
    // again here, so the period counts from it and not from the signing above.
    h.handle.touch();
    let until = Instant::now() + Duration::from_secs(30);
    let mut first = None;
    while !h.seen.iter().any(|e| matches!(e, Event::Locked { .. })) {
        assert!(
            Instant::now() < until,
            "the store never locked while it was checked"
        );
        let reply = h.call(Command::Status {
            account: account0(),
            scan_to: None,
        });
        first.get_or_insert(reply);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(h.wait_locked(Duration::ZERO), LockReason::Idle);
    match first {
        Some(Reply::Status { state, .. }) => assert_eq!(
            state,
            AccountState::InSync {
                balance: plan.change_total
            }
        ),
        other => panic!("expected the first check, got {other:?}"),
    }
}

/// A reorganisation that reverts a settle does not reopen the reservation.
/// The library keeps only the settled block, so every read that reconciles
/// the account reports it one key behind the store, with the block and no
/// index to advance to; Re-sign has no reservation to read, the
/// acknowledged advance writes nothing, and a store with no other account
/// will not start. The library's own test covers only its report.
#[test]
fn a_reverted_settle_leaves_the_account_behind_with_no_way_to_advance() {
    let scratch = Scratch::new("reverted");
    let dir = scratch.store();
    let (mut h, start) = funded(&scratch);
    let request = pay(destination(1), Amount::Nano(100_000));
    let plan = planned(h.call(Command::PlanSend {
        spend: request.clone(),
    }));
    let _ = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    match h.call(Command::Settle {
        account: account0(),
    }) {
        Reply::Settled { view, text } => assert_eq!(view.accounts[0].index, start + 1, "{text}"),
        other => panic!("expected the settlement, got {other:?}"),
    }

    // The block that showed the spend is reverted: the chain holds the
    // account at the key it spent from, with the balance it had.
    h.chain.hold(tag(0), address(0, start), FUNDS);
    let settle = format!("This store recorded a SETTLE at index {start}");
    let behind = |state: &AccountState| match state {
        AccountState::Diverged {
            kind: DivergenceKind::Behind { gap: 1 },
            report,
            advance_to: None,
            rollback_to: Some(_),
            reverted_settle,
        } => *reverted_settle && report.contains(&settle),
        _ => false,
    };

    let view = opened(h.call(Command::Refresh));
    assert!(view.opened, "Refresh leaves an open wallet open: {view:?}");
    let row = &view.accounts[0];
    assert!(behind(&row.state), "Refresh: {:?}", row.state);
    assert!(!row.spendable);
    assert_eq!(row.index, start + 1, "nothing moved the store back");

    match h.call(Command::Status {
        account: account0(),
        scan_to: None,
    }) {
        Reply::Status {
            state,
            spendable,
            text,
            ..
        } => {
            assert!(behind(&state), "Check now: {state:?}");
            assert!(!spendable);
            assert!(
                text.contains("No command rebuilds these bytes, and none may:")
                    && text.contains("`resubmit_reverted_settle` sends a saved copy only when"),
                "{text}"
            );
        }
        other => panic!("expected the report, got {other:?}"),
    }

    // No reservation is open, so there is nothing to re-sign.
    let submits = h.chain.submits().len();
    let _ = refusal(h.call(Command::Resign { spend: request }));
    assert_eq!(h.chain.submits().len(), submits, "nothing more was sent");

    // Its only account diverged, so the store will not start.
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    assert_eq!(h.wait_locked(Duration::from_secs(5)), LockReason::Asked);
    let view = opened(h.unlock(&dir, PASSWORD));
    assert!(!view.opened, "{view:?}");
    let notice = view.notice.clone().expect("the refusal page");
    assert_eq!(notice.kind, NoticeKind::WillNotStart);
    assert!(
        behind(&view.accounts[0].state),
        "{:?}",
        view.accounts[0].state
    );

    // The acknowledged advance names no index for a chain behind the store:
    // asked to go to the chain's key, it writes nothing.
    match h.call(Command::Reconcile {
        account: account0(),
        advance_to: start,
    }) {
        Reply::Reconciled {
            ok,
            advanced_to,
            text,
            opened: Ok(view),
        } => {
            assert!(!ok, "{text}");
            assert_eq!(advanced_to, None, "{text}");
            assert!(!view.opened);
            assert_eq!(view.accounts[0].index, start + 1);
        }
        other => panic!("expected the refused advance, got {other:?}"),
    }
}

/// The spend `pay` makes, with a block-to-live of its own (0 for none).
fn pay_by(to: String, amount: Amount, blk_to_live: u64) -> SpendRequest {
    SpendRequest {
        blk_to_live,
        ..pay(to, amount)
    }
}

/// Whether `state` is the one a reverted settle leaves: one key behind the
/// store, the settle retained, and no index to advance to.
fn one_behind_with_settle(state: &AccountState) -> bool {
    matches!(
        state,
        AccountState::Diverged {
            kind: DivergenceKind::Behind { gap: 1 },
            advance_to: None,
            rollback_to: Some(_),
            reverted_settle: true,
            ..
        }
    )
}

/// The setup of `a_reverted_settle_leaves_the_account_behind_with_no_way_to_advance`,
/// with the spend's own block-to-live: a funded store whose spend settled and
/// was then reverted, so account 0 is one key behind the store with the settle
/// retained. Returns the harness, the key index the store opened at, the spend
/// as the reply gave it, and the change the spend left, which the chain holds
/// at the key the settle moved to.
fn reverted_settle(scratch: &Scratch, blk_to_live: u64) -> (Harness, u32, SentView, u64) {
    let (mut h, start) = funded(scratch);
    let request = pay_by(destination(1), Amount::Nano(100_000), blk_to_live);
    let plan = planned(h.call(Command::PlanSend { spend: request }));
    let spent = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    match h.call(Command::Settle {
        account: account0(),
    }) {
        Reply::Settled { view, text } => assert_eq!(view.accounts[0].index, start + 1, "{text}"),
        other => panic!("expected the settlement, got {other:?}"),
    }
    // The block that showed the spend is reverted: the chain holds the
    // account at the key it spent from, with the balance it had.
    h.chain.hold(tag(0), address(0, start), FUNDS);
    let view = opened(h.call(Command::Refresh));
    assert!(view.opened, "{view:?}");
    assert!(
        one_behind_with_settle(&view.accounts[0].state),
        "{:?}",
        view.accounts[0].state
    );
    (h, start, spent, plan.change_total)
}

/// The saved copy of a reverted settle is sent again, and only that. The
/// node takes the identical bytes once, the account is read again and its
/// row moves when the copy lands, and no key index moves.
#[test]
fn a_saved_copy_of_a_reverted_settle_is_sent_again_and_the_account_is_read_again() {
    let scratch = Scratch::new("resubmit-sent");
    let (mut h, start, spent, change) = reverted_settle(&scratch, 0);
    let before = h.chain.submits().len();
    // The copy lands when the node takes it: the chain then holds the account
    // at the key the settle moved to, with the change the spend left.
    h.chain
        .land_on_submit(tag(0), address(0, start + 1), change);
    let (tx_id, text, view) = match h.call(Command::ResubmitSaved {
        account: account0(),
        artifact_hex: spent.artifact_hex.clone(),
    }) {
        Reply::Resubmitted { tx_id, text, view } => (tx_id, text, view),
        other => panic!("expected the copy sent again, got {other:?}"),
    };

    // The bytes given went to the node unchanged, once, and the node echoed
    // the spend's own transaction id.
    let submits = h.chain.submits();
    assert_eq!(submits.len(), before + 1, "{text}");
    assert_eq!(hex_of(&submits[before]), spent.artifact_hex);
    assert_eq!(Some(tx_id.clone()), spent.tx_id);
    assert!(text.contains(&tx_id), "{text}");
    assert!(
        text.contains("it does not establish that the network accepted"),
        "{text}"
    );

    // No key index moved, and the account was read again: the copy has landed.
    assert_eq!(view.accounts[0].index, start + 1, "{text}");
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync { balance: change }
    );

    // Nothing else is signed or sent. Re-sign has no open reservation to
    // rebuild, and an advance names no index to move to.
    let _ = refusal(h.call(Command::Resign {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    match h.call(Command::Reconcile {
        account: account0(),
        advance_to: start + 1,
    }) {
        Reply::Reconciled {
            ok,
            advanced_to,
            text,
            opened: Ok(view),
        } => {
            assert!(!ok, "{text}");
            assert_eq!(advanced_to, None, "{text}");
            assert_eq!(view.accounts[0].index, start + 1);
        }
        other => panic!("expected the refused advance, got {other:?}"),
    }
    assert_eq!(h.chain.submits().len(), before + 1, "nothing else was sent");
}

/// Every spend's bytes are kept beside the keystore
/// when it is signed, so that a reverted settle is sent again from that copy
/// with nothing signed, and no copy needs saving by hand. The resend takes the
/// copy of the spend the store holds the digest of, not a stray file at
/// another index. The copy of the account's last spend stays however far
/// ahead the node's tip is; once the next spend moves the index, it goes at
/// the first open with the account in sync and the spend more than 256 blocks
/// deep.
#[test]
fn a_kept_copy_sends_a_reverted_settle_again_and_goes_once_final() {
    let scratch = Scratch::new("resend-retained");
    let (mut h, start, spent, change) = reverted_settle(&scratch, 0);
    let dir = scratch.store();
    let kept = tawara_wallet_core::retained::list(&dir);
    assert_eq!(kept.len(), 1, "the sign kept a copy");
    assert_eq!(kept[0].tag, tag(0));
    assert_eq!(kept[0].spent_index, start);
    assert_eq!(
        tawara_wallet_core::retained::bytes_of(&dir, &tag(0), start).as_deref(),
        Some(spent.artifact_hex.as_str()),
        "the copy is the bytes the sent page shows"
    );
    let height = kept[0].height.expect("the tip was read at the sign");

    // A stray file at a higher index, such as an earlier keystore in the same
    // folder left, is not the settled spend and is not sent.
    tawara_wallet_core::retained::keep(&dir, &tag(0), start + 5, Some(height), &[0x01, 0x02])
        .expect("a stray file");

    // The reverted settle sent again from the copy: the identical bytes, once,
    // and the account read again, in sync at the key the settle moved to.
    let before = h.chain.submits().len();
    h.chain
        .land_on_submit(tag(0), address(0, start + 1), change);
    let view = match h.call(Command::ResendRetained {
        account: account0(),
    }) {
        Reply::Resubmitted { view, .. } => view,
        other => panic!("expected the copy sent again, got {other:?}"),
    };
    let submits = h.chain.submits();
    assert_eq!(submits.len(), before + 1);
    assert_eq!(hex_of(&submits[before]), spent.artifact_hex);
    assert_eq!(view.accounts[0].index, start + 1, "no key index moved");
    assert_eq!(
        view.accounts[0].state,
        AccountState::InSync { balance: change }
    );

    // The account in sync and the node's tip far past the copy's height: the
    // stray file goes, and the copy of the last spend, whose digest the store
    // still holds, stays.
    h.chain.set_tip(height + 10_000);
    let _ = opened(h.call(Command::Refresh));
    let kept = tawara_wallet_core::retained::list(&dir);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].spent_index, start, "the last spend's copy stays");

    // The next spend moves the index; it lands and settles. Its copy is the
    // one held now, and the earlier copy is released by depth.
    h.chain.set_tip(height + 256);
    let request = pay(destination(1), Amount::Nano(100_000));
    let plan = planned(h.call(Command::PlanSend { spend: request }));
    let _ = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    h.chain
        .hold(tag(0), address(0, start + 2), plan.change_total);
    match h.call(Command::Settle {
        account: account0(),
    }) {
        Reply::Settled { view, text } => assert_eq!(view.accounts[0].index, start + 2, "{text}"),
        other => panic!("expected the settlement, got {other:?}"),
    }
    let _ = opened(h.call(Command::Refresh));
    let indexes = |dir: &std::path::Path| {
        tawara_wallet_core::retained::list(dir)
            .iter()
            .map(|c| c.spent_index)
            .collect::<Vec<_>>()
    };
    assert_eq!(indexes(&dir), vec![start, start + 1], "not yet final");
    h.chain.set_tip(height + 257);
    let _ = opened(h.call(Command::Refresh));
    assert_eq!(indexes(&dir), vec![start + 1], "final: let go");
    h.chain.set_tip(height + 100_000);
    let _ = opened(h.call(Command::Refresh));
    assert_eq!(indexes(&dir), vec![start + 1], "the held copy stays");

    // With no copy kept, nothing is sent.
    for copy in tawara_wallet_core::retained::list(&dir) {
        tawara_wallet_core::retained::release(&copy).expect("released");
    }
    let before = h.chain.submits().len();
    let _ = refusal(h.call(Command::ResendRetained {
        account: account0(),
    }));
    assert_eq!(h.chain.submits().len(), before);
}

/// The library's digest check: a copy whose signed part is not the settled
/// spend is refused before the chain is asked, and nothing is sent.
#[test]
fn a_saved_copy_whose_signed_part_is_not_the_settled_spend_is_refused() {
    let scratch = Scratch::new("resubmit-digest");
    let (mut h, start, spent, _) = reverted_settle(&scratch, 0);
    let wire = mochimo_crypto::mesh::hex::decode(&spent.artifact_hex, "artifact")
        .expect("the saved copy is hex");
    // The same layout with one field of the signed part changed, so the
    // digest of that part is not the one the store retained.
    let mut tx = Transaction::from_wire(&wire).expect("a transaction");
    tx.change_total = tx.change_total.wrapping_add(1);
    let altered = tx.to_wire();
    assert_ne!(altered, wire);

    let before = h.chain.submits().len();
    let r = refusal(h.call(Command::ResubmitSaved {
        account: account0(),
        artifact_hex: hex_of(&altered),
    }));
    assert_eq!(r.kind, RefusalKind::Library);
    assert!(
        r.text.contains("is not the spend this account settled"),
        "{}",
        r.text
    );
    assert!(r.text.contains("Nothing was submitted"), "{}", r.text);
    assert_eq!(h.chain.submits().len(), before, "nothing was sent");

    // The refusal leaves the account as it was, and Re-sign and Reconcile are
    // still refused.
    match h.call(Command::Status {
        account: account0(),
        scan_to: None,
    }) {
        Reply::Status { state, .. } => {
            assert!(one_behind_with_settle(&state), "{state:?}");
        }
        other => panic!("expected the check, got {other:?}"),
    }
    let _ = refusal(h.call(Command::Resign {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    match h.call(Command::Reconcile {
        account: account0(),
        advance_to: start,
    }) {
        Reply::Reconciled {
            ok,
            advanced_to,
            text,
            opened: Ok(view),
        } => {
            assert!(!ok, "{text}");
            assert_eq!(advanced_to, None, "{text}");
            assert_eq!(view.accounts[0].index, start + 1);
        }
        other => panic!("expected the refused advance, got {other:?}"),
    }
    assert_eq!(h.chain.submits().len(), before, "nothing was sent");
}

/// The library's balance check: once the source holds other than the balance
/// the settled spend was built against, the copy can no longer be accepted,
/// and it is refused.
#[test]
fn a_saved_copy_is_refused_once_the_balance_at_the_source_has_moved() {
    let scratch = Scratch::new("resubmit-moved");
    let (mut h, start, spent, _) = reverted_settle(&scratch, 0);
    // A credit reached the source after the settled spend was built.
    h.chain.hold(tag(0), address(0, start), FUNDS + 1);
    let before = h.chain.submits().len();
    let r = refusal(h.call(Command::ResubmitSaved {
        account: account0(),
        artifact_hex: spent.artifact_hex.clone(),
    }));
    assert_eq!(r.kind, RefusalKind::Library);
    assert!(
        r.text.contains(&format!(
            "the balance at the source is {} nanoMCM, not the {FUNDS} the settled spend",
            FUNDS + 1
        )),
        "{}",
        r.text
    );
    assert_eq!(h.chain.submits().len(), before, "nothing was sent");

    // Re-sign and Reconcile are still refused.
    let _ = refusal(h.call(Command::Resign {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    assert_eq!(h.chain.submits().len(), before, "nothing was sent");
}

/// The library's expiry check: once the tip has reached the settled spend's
/// block-to-live, the copy can no longer be included, and it is refused.
#[test]
fn a_saved_copy_is_refused_once_the_block_to_live_of_the_settled_spend_is_reached() {
    let scratch = Scratch::new("resubmit-expired");
    let (mut h, _, spent, _) = reverted_settle(&scratch, 1_010);
    h.chain.set_tip(1_010);
    let before = h.chain.submits().len();
    let r = refusal(h.call(Command::ResubmitSaved {
        account: account0(),
        artifact_hex: spent.artifact_hex.clone(),
    }));
    assert_eq!(r.kind, RefusalKind::Library);
    assert!(
        r.text
            .contains("block-to-live 1010 has been reached at tip 1010"),
        "{}",
        r.text
    );
    assert_eq!(h.chain.submits().len(), before, "nothing was sent");
}

/// The library's own check that the chain shows the settle reverted: an
/// account the chain holds in sync, with its settle retained, is refused.
#[test]
fn a_saved_copy_is_refused_for_an_account_the_chain_holds_in_sync() {
    let scratch = Scratch::new("resubmit-in-sync");
    let (mut h, start) = funded(&scratch);
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));
    let spent = sent(h.call(Command::ConfirmSend { plan: plan.plan }));
    h.chain
        .hold(tag(0), address(0, start + 1), plan.change_total);
    match h.call(Command::Settle {
        account: account0(),
    }) {
        Reply::Settled { view, text } => assert_eq!(view.accounts[0].index, start + 1, "{text}"),
        other => panic!("expected the settlement, got {other:?}"),
    }

    // Nothing was reverted: the chain holds the account at the key the settle
    // moved to.
    let before = h.chain.submits().len();
    let r = refusal(h.call(Command::ResubmitSaved {
        account: account0(),
        artifact_hex: spent.artifact_hex,
    }));
    assert_eq!(r.kind, RefusalKind::Library);
    assert!(r.text.contains("the chain is in sync"), "{}", r.text);
    assert_eq!(h.chain.submits().len(), before, "nothing was sent");
}

/// The copy is sent only with the wallet open.
#[test]
fn a_saved_copy_is_refused_while_the_wallet_is_locked() {
    let scratch = Scratch::new("resubmit-locked");
    let (mut h, _) = funded(&scratch);
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    assert_eq!(h.wait_locked(Duration::from_secs(5)), LockReason::Asked);
    let r = refusal(h.call(Command::ResubmitSaved {
        account: account0(),
        artifact_hex: "00".to_owned(),
    }));
    assert_eq!(r.kind, RefusalKind::NotUnlocked);
    assert_eq!(h.chain.submits().len(), 0, "nothing was sent");
}

/// Text that is not hex is refused before the node is asked.
#[test]
fn a_saved_copy_that_is_not_hex_is_refused_before_the_node_is_asked() {
    let scratch = Scratch::new("resubmit-not-hex");
    let (mut h, _) = funded(&scratch);
    let calls = h.chain.calls();
    let r = refusal(h.call(Command::ResubmitSaved {
        account: account0(),
        artifact_hex: "xyz".to_owned(),
    }));
    assert_eq!(r.kind, RefusalKind::Library);
    assert!(r.text.contains("not hex"), "{}", r.text);
    assert_eq!(h.chain.calls(), calls, "the API was not asked");
}

/// A write the node does not take is reported in the library's words, and
/// the write was attempted.
#[test]
fn a_saved_copy_the_node_does_not_take_is_refused_in_the_library_words() {
    let scratch = Scratch::new("resubmit-write-refused");
    let (mut h, _, spent, _) = reverted_settle(&scratch, 0);
    h.chain.refuse_submits(true);
    let before = h.chain.submits().len();
    let r = refusal(h.call(Command::ResubmitSaved {
        account: account0(),
        artifact_hex: spent.artifact_hex.clone(),
    }));
    assert_eq!(r.kind, RefusalKind::Library);
    assert!(!r.text.is_empty(), "the library's words are shown");
    assert_eq!(
        h.chain.submits().len(),
        before + 1,
        "the write was attempted"
    );
}

// ------------------------------------------------- teardown, failed reopen

#[test]
fn dropping_the_last_handle_stops_a_queued_spend_before_it_signs() {
    let scratch = Scratch::new("drop-queued");
    let (mut h, _) = funded(&scratch);
    let plan = planned(h.call(Command::PlanSend {
        spend: pay(destination(1), Amount::Nano(100_000)),
    }));

    // A refresh waits on the node, the confirmation is queued behind it,
    // and the interface goes away.
    let gate = h.chain.close_gate();
    let refresh = h.handle.send(Command::Refresh).expect("worker running");
    assert_eq!(h.wait_busy(refresh), Activity::AskingNode);
    let confirm = h
        .handle
        .send(Command::ConfirmSend { plan: plan.plan })
        .expect("worker running");
    let Harness {
        handle,
        events,
        chain,
        ..
    } = h;
    drop(handle);
    gate.open();

    let mut answers = Vec::new();
    let mut locked = None;
    loop {
        match events
            .recv_timeout(Duration::from_secs(60))
            .expect("an event")
        {
            Event::Done { id, reply } => answers.push((id, *reply)),
            Event::Locked { reason } => locked = Some(reason),
            Event::Stopped { panicked } => {
                assert!(!panicked);
                break;
            }
            Event::Busy { .. } | Event::Progress { .. } | Event::Backgrounded { .. } => {}
        }
    }
    assert!(chain.submits().is_empty(), "nothing was signed or sent");
    assert_eq!(locked, Some(LockReason::Shutdown));
    for (id, reply) in &answers {
        if *id == refresh || *id == confirm {
            assert!(
                matches!(reply, Reply::Refused(r) if r.kind == RefusalKind::Cancelled),
                "{reply:?}"
            );
        }
    }
}

/// A single-account store whose account diverged three keys ahead, open in
/// the Store session against [`NODE`], and the index the advance goes to.
fn diverged_store(scratch: &Scratch) -> (Harness, u32) {
    let mut h = Harness::new();
    let view = opened(h.create_from_phrase(&scratch.store()));
    let advance_to = view.accounts[0].index + 3;
    h.chain.hold(tag(0), address(0, advance_to), FUNDS);
    let _ = h.call(Command::SetNode { url: NODE.into() });
    let view = opened(h.call(Command::Refresh));
    assert!(!view.opened, "{view:?}");
    (h, advance_to)
}

#[test]
fn a_refused_reopen_after_an_advance_keeps_the_store_open() {
    // How many times the advance and the reopen after it resolve the tag.
    let dry = Scratch::new("reopen-dry");
    let (mut h, advance_to) = diverged_store(&dry);
    let before = h.chain.resolves(tag(0));
    match h.call(Command::Reconcile {
        account: account0(),
        advance_to,
    }) {
        Reply::Reconciled { ok: true, .. } => {}
        other => panic!("expected the advance, got {other:?}"),
    }
    let per_command = h.chain.resolves(tag(0)) - before;
    drop(h);

    // The same again, with the node answering "account not found" from the
    // last of those on: when the wallet is opened again after the advance.
    // The library refuses it, and hands the store back: it stays open on
    // its own, with the library's report, and nothing asks for the
    // password again.
    let scratch = Scratch::new("reopen");
    let (mut h, advance_to) = diverged_store(&scratch);
    let from = h.chain.resolves(tag(0)) + per_command;
    h.chain.vanish_from(tag(0), from);
    match h.call(Command::Reconcile {
        account: account0(),
        advance_to,
    }) {
        Reply::Reconciled {
            ok,
            advanced_to: Some(index),
            text,
            opened: Ok(view),
        } => {
            assert!(ok, "{text}");
            assert_eq!(index, advance_to, "the advance stands");
            assert!(!view.opened, "{view:?}");
            assert_eq!(view.accounts[0].index, advance_to);
            let notice = view.notice.expect("the library's report");
            assert!(notice.starts_with("THIS STORE IS NOT WHOLE"), "{notice}");
            assert!(
                notice.contains("account not found"),
                "with the account's own report: {notice}"
            );
        }
        other => panic!("expected the advance and the refused reopen, got {other:?}"),
    }
    assert!(
        !h.seen.iter().any(|e| matches!(e, Event::Locked { .. })),
        "the store was never closed: {:?}",
        h.seen
    );

    // The node answers again: the same store opens, as it is.
    h.chain.vanish_from(tag(0), usize::MAX);
    let view = opened(h.call(Command::Refresh));
    assert!(view.opened, "{view:?}");
    assert_eq!(view.accounts[0].index, advance_to);
}

// ------------------------------------------- the long operations: cancel

/// A funded store with a second account, restored, both in sync.
fn two_accounts(scratch: &Scratch) -> Harness {
    let (mut h, _) = funded(scratch);
    h.chain.hold(tag(1), address(1, 0), 1_000);
    match h.call(Command::Restore {
        account_index: 1,
        scan_to: None,
    }) {
        Reply::Restored {
            ok: true,
            opened: Ok(view),
            text,
        } => assert_eq!(view.accounts.len(), 2, "{text}"),
        other => panic!("expected a restore, got {other:?}"),
    }
    h
}

#[test]
fn a_cancelled_unlock_leaves_the_store_open_and_unreconciled() {
    let scratch = Scratch::new("cancel-unlock");
    let dir = scratch.store();
    let mut h = two_accounts(&scratch);
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    assert_eq!(h.wait_locked(Duration::from_secs(5)), LockReason::Asked);

    // The node holds the first account's lookup while the person cancels:
    // the wallet stops opening before it asks about the second.
    let gate = h.chain.close_gate();
    let calls = h.chain.calls();
    let id = h
        .handle
        .send(Command::Unlock {
            dir: dir.clone(),
            password: secret(PASSWORD),
        })
        .expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::DerivingKey);
    assert_eq!(h.wait_busy(id), Activity::AskingNode);
    h.handle.cancel();
    gate.open();
    let view = match h.wait_for(id) {
        Reply::Unlocked(view) => view,
        other => panic!("expected the store open on its own, got {other:?}"),
    };
    assert!(!view.opened, "{view:?}");
    assert!(
        view.notice
            .as_deref()
            .is_some_and(|n| n.contains("cancelled")),
        "{:?}",
        view.notice
    );
    assert_eq!(view.accounts.len(), 2);
    assert!(
        view.accounts
            .iter()
            .all(|r| r.state == AccountState::NotReconciled && !r.spendable),
        "{view:?}"
    );
    assert!(
        h.chain.calls() - calls <= 1,
        "nothing was asked after the cancel"
    );
    assert!(
        !h.seen.iter().any(|e| matches!(e, Event::Locked { .. })),
        "the store stayed open"
    );

    // Nothing needs the password again: a refresh opens the wallet.
    let view = opened(h.call(Command::Refresh));
    assert!(view.opened, "{view:?}");
    assert_eq!(view.total(), Total::Whole(u128::from(FUNDS) + 1_000));
}

#[test]
fn a_cancelled_restore_writes_nothing() {
    let scratch = Scratch::new("cancel-restore");
    let (mut h, _) = funded(&scratch);
    h.chain.hold(tag(1), address(1, 0), 1_000);

    let gate = h.chain.close_gate();
    let calls = h.chain.calls();
    let id = h
        .handle
        .send(Command::Restore {
            account_index: 1,
            scan_to: None,
        })
        .expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::AskingNode);
    h.handle.cancel();
    gate.open();
    match h.wait_for(id) {
        Reply::Restored {
            ok,
            text,
            opened: Ok(view),
        } => {
            assert!(!ok, "{text}");
            assert!(text.contains("cancelled"), "{text}");
            assert_eq!(view.accounts.len(), 1, "nothing was added: {view:?}");
            assert!(!view.opened, "{view:?}");
            assert!(
                view.notice
                    .as_deref()
                    .is_some_and(|n| n.contains("cancelled")),
                "{:?}",
                view.notice
            );
        }
        other => panic!("expected the cancelled restore, got {other:?}"),
    }
    // At most the restore's own lookup: the wallet opening after it heard
    // the same cancel before it asked anything.
    assert!(h.chain.calls() - calls <= 1);
    assert!(
        !h.seen.iter().any(|e| matches!(e, Event::Locked { .. })),
        "the store stayed open"
    );

    let view = opened(h.call(Command::Refresh));
    assert!(view.opened, "{view:?}");
    assert_eq!(view.accounts.len(), 1, "{view:?}");
}

#[test]
fn a_cancelled_advance_moves_no_key_index() {
    let scratch = Scratch::new("cancel-advance");
    let (mut h, advance_to) = diverged_store(&scratch);

    let gate = h.chain.close_gate();
    let id = h
        .handle
        .send(Command::Reconcile {
            account: account0(),
            advance_to,
        })
        .expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::AskingNode);
    h.handle.cancel();
    gate.open();
    match h.wait_for(id) {
        Reply::Reconciled {
            ok,
            advanced_to,
            text,
            opened: Ok(view),
        } => {
            assert!(!ok, "{text}");
            assert_eq!(advanced_to, None, "{text}");
            assert!(text.contains("cancelled"), "{text}");
            assert!(!view.opened, "{view:?}");
            assert_eq!(view.accounts[0].index, advance_to - 3, "{view:?}");
        }
        other => panic!("expected the cancelled advance, got {other:?}"),
    }

    // Still diverged where it was: the library names the same advance.
    let view = opened(h.call(Command::Refresh));
    assert!(!view.opened, "{view:?}");
    assert!(
        matches!(
            &view.accounts[0].state,
            AccountState::Diverged { advance_to: Some(to), .. } if *to == advance_to
        ),
        "{:?}",
        view.accounts[0].state
    );
}

#[test]
fn a_cancelled_sweep_reports_nothing_found() {
    let scratch = Scratch::new("cancel-sweep");
    let (mut h, _) = funded(&scratch);

    let gate = h.chain.close_gate();
    let calls = h.chain.calls();
    let id = h
        .handle
        .send(Command::Discover { to: 50 })
        .expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::AskingNode);
    h.handle.cancel();
    gate.open();
    assert_eq!(refusal(h.wait_for(id)).kind, RefusalKind::Cancelled);
    assert!(
        h.chain.calls() - calls <= 1,
        "it stopped before the next request"
    );
    assert!(opened(h.call(Command::Refresh)).opened);
}

#[test]
fn the_idle_period_stops_a_walk_to_a_far_index_and_locks() {
    let scratch = Scratch::new("idle-walk");
    let mut h = Harness::with(Config {
        idle_lock: Duration::from_millis(1_500),
        ..Config::default()
    });
    assert!(matches!(
        h.call(Command::SetNode { url: NODE.into() }),
        Reply::NodeSet { .. }
    ));
    let view = opened(h.create_from_phrase(&scratch.store()));
    assert!(!view.opened, "nothing is on the ledger yet: {view:?}");

    // The node holds the account under its own tag at a key none of its
    // positions derives (another account's), so the search runs to the
    // index named, the last there is: far longer than the idle period,
    // which stops it and locks.
    let mut elsewhere = address(0, 0);
    elsewhere[20..].copy_from_slice(&address(5, 0)[20..]);
    h.chain.hold(tag(0), elsewhere, FUNDS);
    let id = h
        .handle
        .send(Command::Status {
            account: account0(),
            scan_to: Some(MAX_KEY_INDEX),
        })
        .expect("worker running");
    assert_eq!(refusal(h.wait_for(id)).kind, RefusalKind::Cancelled);
    assert_eq!(h.wait_locked(Duration::from_secs(10)), LockReason::Idle);
}

// ----------------------------------------- the long operations: progress

/// The progress reports seen for request `id`, in the order they came.
fn progress_of(h: &Harness, id: RequestId) -> Vec<Progress> {
    h.seen
        .iter()
        .filter_map(|e| match e {
            Event::Progress { id: got, progress } if *got == id => Some(*progress),
            _ => None,
        })
        .collect()
}

#[test]
fn long_operations_report_how_far_they_have_got() {
    let scratch = Scratch::new("progress");
    let dir = scratch.store();
    let (mut h, _) = funded(&scratch);

    // A sweep reports each account it is about to ask about, and walks
    // nothing.
    let id = h
        .handle
        .send(Command::Discover { to: 3 })
        .expect("worker running");
    assert!(matches!(h.wait_for(id), Reply::Discovered { .. }));
    let swept = progress_of(&h, id);
    assert_eq!(
        swept.iter().map(|p| p.account).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert!(
        swept
            .iter()
            .all(|p| p.accounts == 4 && p.position == 0 && p.ceiling == 0),
        "{swept:?}"
    );

    // A restore that walks three hundred key positions reports along the
    // way, and the wallet opening after it reports each account.
    h.chain.hold(tag(2), address(2, 300), 7_000);
    let id = h
        .handle
        .send(Command::Restore {
            account_index: 2,
            scan_to: None,
        })
        .expect("worker running");
    match h.wait_for(id) {
        Reply::Restored {
            ok: true,
            opened: Ok(view),
            text,
        } => assert!(view.opened, "{text}"),
        other => panic!("expected a restore, got {other:?}"),
    }
    let restored = progress_of(&h, id);
    assert!(
        restored
            .iter()
            .any(|p| p.accounts == 1 && p.position == 256 && p.ceiling >= 300),
        "{restored:?}"
    );
    assert_eq!(
        restored
            .iter()
            .filter(|p| p.accounts == 2)
            .map(|p| p.account)
            .collect::<Vec<_>>(),
        [0, 1],
        "{restored:?}"
    );

    // Unlocking reports each account as the wallet opens.
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    let id = h
        .handle
        .send(Command::Unlock {
            dir,
            password: secret(PASSWORD),
        })
        .expect("worker running");
    assert!(opened(h.wait_for(id)).opened);
    let opening = progress_of(&h, id);
    assert_eq!(
        opening.iter().map(|p| p.account).collect::<Vec<_>>(),
        [0, 1],
        "{opening:?}"
    );
    assert!(opening.iter().all(|p| p.accounts == 2), "{opening:?}");
}

// ---------------------------------------------------------------- explorer

#[test]
fn activity_reads_every_account_from_the_index_and_says_when_there_is_none() {
    use tawara_wallet_core::explorer::Direction;
    let scratch = Scratch::new("activity");
    let (mut h, _) = funded(&scratch);
    let payee = AccountId::from_tag(tag(5));
    h.chain
        .index_transfer(tag(0), tag(5), 400_000, 1_000, 990, "INV-7");

    let id = h.handle.send(Command::Activity).expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::ReadingIndex);
    let histories = match h.wait_for(id) {
        Reply::Activity(Ok(histories)) => histories,
        other => panic!("expected the activity, got {other:?}"),
    };
    assert_eq!(histories.len(), 1, "one per account in the store");
    let mine = &histories[0];
    assert_eq!(mine.account, account0());
    assert_eq!(mine.total, 1);
    assert!(!mine.more());
    assert!(mine.text.contains("recent transactions"), "{}", mine.text);
    let tx = &mine.transactions[0];
    assert_eq!(tx.block, Some(990));
    assert_eq!(tx.time_ms, Some(990 * 60_000));
    assert_eq!(tx.direction(account0()), Direction::Both);
    assert_eq!(tx.net(account0()), -400_500, "what left, net of the change");
    assert_eq!(tx.paid_out(account0()).count(), 1);
    assert_eq!(tx.fee(), 500);
    assert_eq!(tx.net(payee), 400_000);
    let paid: Vec<_> = tx.paid_out(account0()).collect();
    assert_eq!(
        paid[0].memo, "INV-7",
        "the index's row carries the reference, its padding left off"
    );

    // A node that runs no index does not serve the search, for any account.
    h.chain.set_no_index(true);
    match h.call(Command::Activity) {
        Reply::Activity(Err(refused)) => {
            assert_eq!(refused.index, Some(IndexState::Absent));
            assert!(refused.text.contains("HTTP 404"), "{}", refused.text);
            assert!(
                refused.text.contains("this node does not index"),
                "the library's page says which: {}",
                refused.text
            );
        }
        other => panic!("expected the index refused, got {other:?}"),
    }
    // One that runs an index that does not answer answers code 2.
    h.chain.set_no_index(false);
    h.chain.set_index_down(true);
    match h.call(Command::OlderActivity(vec![(account0(), 0)])) {
        Reply::Activity(Err(refused)) => {
            assert_eq!(refused.index, Some(IndexState::Unavailable));
            assert!(refused.text.contains("code 2"), "{}", refused.text);
            assert!(
                refused.text.contains("it did not answer"),
                "the library's page says which: {}",
                refused.text
            );
        }
        other => panic!("expected the index refused, got {other:?}"),
    }

    // A cancel stops it before it starts, and nothing is reported.
    h.chain.set_index_down(false);
    let gate = h.chain.close_gate();
    let id = h.handle.send(Command::Activity).expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::ReadingIndex);
    h.handle.cancel();
    gate.open();
    assert_eq!(refusal(h.wait_for(id)).kind, RefusalKind::Cancelled);
}

#[test]
fn older_activity_reads_on_from_where_the_last_page_ended() {
    let scratch = Scratch::new("older-activity");
    let (mut h, _) = funded(&scratch);
    for n in 0..105 {
        h.chain
            .index_transfer(tag(0), tag(5), 1_000 + n, 1_000, 900 + n, "");
    }
    let mut mine = match h.call(Command::Activity) {
        Reply::Activity(Ok(mut histories)) => histories.remove(0),
        other => panic!("expected the activity, got {other:?}"),
    };
    assert_eq!(mine.transactions.len(), 100);
    assert_eq!((mine.total, mine.next, mine.unread()), (105, 100, 5));
    assert!(mine.more());

    // A row the index gains at the top pushes the rest down a place: the
    // older page repeats one row, which is listed once.
    h.chain
        .index_transfer(tag(0), tag(5), 9_999, 1_000, 1_100, "");
    let older = match h.call(Command::OlderActivity(vec![
        (account0(), mine.next),
        (AccountId::from_tag(tag(9)), 0),
    ])) {
        Reply::Activity(Ok(histories)) => histories,
        other => panic!("expected the older page, got {other:?}"),
    };
    assert_eq!(
        older.len(),
        1,
        "an account the store does not hold is not read"
    );
    assert_eq!(older[0].transactions.len(), 6);
    let first_text = mine.text.clone();
    mine.extend(older.into_iter().next().expect("one page"));
    assert_eq!(mine.transactions.len(), 105, "the repeated row once");
    assert_eq!((mine.total, mine.next), (106, 106));
    assert!(!mine.more());
    assert!(mine.text.starts_with(&first_text));
    assert!(
        mine.text.len() > first_text.len(),
        "both pages, one after the other"
    );
    let blocks: Vec<_> = mine.transactions.iter().map(|t| t.block).collect();
    assert!(
        blocks.windows(2).all(|w| w[0] > w[1]),
        "newest first, each once: {blocks:?}"
    );

    // Locked, nothing is read.
    assert!(matches!(h.call(Command::Lock), Reply::Locked));
    assert_eq!(
        refusal(h.call(Command::OlderActivity(vec![(account0(), 100)]))).kind,
        RefusalKind::NotUnlocked
    );
}

#[test]
fn the_newest_blocks_are_read_down_from_the_tip_without_a_store() {
    use tawara_wallet_core::explorer::BlockKind;
    let mut h = Harness::with_node();
    h.chain.pseudo(997);
    match h.call(Command::Blocks) {
        Reply::Blocks(Ok(view)) => {
            assert_eq!(view.tip, 1_000);
            let indexes: Vec<u64> = view.blocks.iter().map(|b| b.index).collect();
            assert_eq!(indexes, vec![1_000, 999, 998, 997, 996, 995]);
            assert_eq!(view.blocks[0].time_ms, 1_000 * 60_000);
            assert_eq!(view.blocks[0].difficulty, Some(30));
            assert_eq!(view.blocks[1].difficulty, Some(34));
            let kinds: Vec<_> = view.blocks.iter().map(|b| b.kind).collect();
            let normal = Some(BlockKind::Normal);
            assert_eq!(
                kinds,
                vec![
                    normal,
                    normal,
                    normal,
                    Some(BlockKind::Pseudo),
                    normal,
                    normal
                ],
                "a block whose figures count no transactions is a pseudo-block"
            );
            assert!(view.text.contains("pseudo"), "{}", view.text);
        }
        other => panic!("expected the blocks, got {other:?}"),
    }
    // A block numbered at a multiple of 256 is a neogenesis block, whatever
    // its figures count.
    h.chain.set_tip(1_026);
    h.chain.pseudo(1_024);
    match h.call(Command::Blocks) {
        Reply::Blocks(Ok(view)) => {
            assert_eq!(view.blocks[2].index, 1_024);
            assert_eq!(view.blocks[2].kind, Some(BlockKind::Neogenesis));
        }
        other => panic!("expected the blocks, got {other:?}"),
    }
    assert_eq!(
        refusal(Harness::new().call(Command::Blocks)).kind,
        RefusalKind::NoNode
    );
}

/// The network card's read holds the worker, which holds the open store,
/// for as long as it asks the node: a lock, a cancel or the idle period
/// stops it between two requests, and the store is then locked at once.
#[test]
fn a_lock_or_the_idle_period_stops_the_newest_blocks_between_requests() {
    // Lock pressed while the node is slow to answer the first request: the
    // application cancels what is running, then sends the lock.
    let scratch = Scratch::new("blocks-lock");
    let (mut h, _) = funded(&scratch);
    let gate = h.chain.close_gate();
    let calls = h.chain.calls();
    let id = h.handle.send(Command::Blocks).expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::ReadingIndex);
    gate.wait_reached(1);
    h.handle.cancel();
    let lock = h.handle.send(Command::Lock).expect("worker running");
    gate.open();
    assert_eq!(refusal(h.wait_for(id)).kind, RefusalKind::Cancelled);
    assert!(matches!(h.wait_for(lock), Reply::Locked));
    assert_eq!(
        h.chain.calls() - calls,
        1,
        "the tip was asked for, and no block after the cancel"
    );

    // The idle period runs out while a slow node answers one request at a
    // time: the walk of seven requests stops partway, and the store locks.
    let scratch = Scratch::new("blocks-idle");
    let mut h = Harness::with(Config {
        idle_lock: Duration::from_millis(1_500),
        ..Config::default()
    });
    assert!(matches!(
        h.call(Command::SetNode { url: NODE.into() }),
        Reply::NodeSet { .. }
    ));
    h.chain.hold(tag(0), address(0, 0), FUNDS);
    assert!(opened(h.create_from_phrase(&scratch.store())).opened);
    h.chain.slow(Duration::from_millis(600));
    let calls = h.chain.calls();
    h.handle.touch();
    let id = h.handle.send(Command::Blocks).expect("worker running");
    assert_eq!(refusal(h.wait_for(id)).kind, RefusalKind::Cancelled);
    assert_eq!(h.wait_locked(Duration::from_secs(10)), LockReason::Idle);
    let made = h.chain.calls() - calls;
    assert!(made < 7, "the whole walk ran: {made} requests");
}

#[test]
fn the_queue_is_counted_without_a_store() {
    let mut h = Harness::with_node();
    match h.call(Command::Mempool) {
        Reply::Mempool(Ok(view)) => {
            assert_eq!(view.waiting, 0, "Go's null is an empty queue");
            assert!(!view.text.is_empty());
        }
        other => panic!("expected the queue, got {other:?}"),
    }
    h.chain.queue(&[[1; 32], [2; 32], [3; 32]]);
    let calls = h.chain.calls();
    match h.call(Command::Mempool) {
        Reply::Mempool(Ok(view)) => assert_eq!(view.waiting, 3),
        other => panic!("expected the queue, got {other:?}"),
    }
    assert_eq!(
        h.chain.calls() - calls,
        1,
        "the ids are counted, and none is read whole"
    );
    h.chain.set_unreachable(true);
    match h.call(Command::Mempool) {
        Reply::Mempool(Err(refused)) => {
            assert_eq!(refused.index, None, "the queue is not the index")
        }
        other => panic!("expected the queue refused, got {other:?}"),
    }
    assert_eq!(
        refusal(Harness::new().call(Command::Mempool)).kind,
        RefusalKind::NoNode
    );
}

#[test]
fn a_review_reports_every_account_and_applies_it() {
    let scratch = Scratch::new("review");
    let (mut h, start) = funded(&scratch);
    h.chain.hold(tag(1), address(1, 0), 900);
    assert!(matches!(
        h.call(Command::Restore {
            account_index: 1,
            scan_to: None,
        }),
        Reply::Restored { ok: true, .. }
    ));
    let order: Vec<AccountId> = opened(h.call(Command::Refresh))
        .accounts
        .iter()
        .map(|a| a.id)
        .collect();
    assert_eq!(order.len(), 2);

    // The chain moves account 0 three keys ahead: another wallet spent.
    h.chain.hold(tag(0), address(0, start + 3), FUNDS);
    let id = h.handle.send(Command::Review).expect("worker running");
    assert_eq!(h.wait_busy(id), Activity::AskingNode);
    let reports = match h.wait_for(id) {
        Reply::Reviewed(reports) => reports,
        other => panic!("expected the review, got {other:?}"),
    };
    let reported: Vec<AccountId> = reports.iter().map(|r| r.account).collect();
    assert_eq!(reported, order, "every account, in the store's order");
    let report = |id: AccountId| reports.iter().find(|r| r.account == id).expect("reported");
    let first = report(account0());
    assert!(
        matches!(
            first.state,
            AccountState::Diverged {
                kind: DivergenceKind::Ahead { gap: 3 },
                ..
            }
        ),
        "{:?}",
        first.state
    );
    assert!(!first.spendable);
    assert!(!first.text.is_empty());
    assert_eq!(
        report(AccountId::from_tag(tag(1))).state,
        AccountState::InSync { balance: 900 }
    );
    let view = opened(h.call(Command::Refresh));
    let row = view
        .accounts
        .iter()
        .find(|a| a.id == account0())
        .expect("held");
    assert!(matches!(row.state, AccountState::Diverged { .. }));

    let locked = refusal(Harness::new().call(Command::Review));
    assert_eq!(locked.kind, RefusalKind::NotUnlocked);
}

#[test]
fn derived_accounts_carry_the_number_the_command_line_names_them_by() {
    let scratch = Scratch::new("numbers");
    let (mut h, _) = funded(&scratch);
    h.chain.hold(tag(3), address(3, 0), 900);
    assert!(matches!(
        h.call(Command::Restore {
            account_index: 3,
            scan_to: None,
        }),
        Reply::Restored { ok: true, .. }
    ));
    let view = opened(h.call(Command::Refresh));
    let number = |id: AccountId| {
        view.accounts
            .iter()
            .find(|a| a.id == id)
            .expect("held")
            .number
    };
    assert_eq!(number(account0()), Some(0));
    assert_eq!(number(AccountId::from_tag(tag(3))), Some(3));
}
