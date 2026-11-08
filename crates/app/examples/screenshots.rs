// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Draws every built screen headlessly into `target/screenshots/`, or checks
//! that the images there are what the screens draw now. The phone's screens
//! are drawn the same way, in their own images.
//!
//! ```text
//! cargo run -p tawara-app --example screenshots                 # write them all
//! cargo run -p tawara-app --example screenshots -- --check      # compare them
//! cargo run -p tawara-app --example screenshots -- --fit        # the phone at 360
//! ```
//!
//! Each screen is drawn from sample data (`tawara_wallet_core::sample` and
//! the forms below) with iced's software renderer, tiny-skia, at a scale of one.
//! The desktop's images are `<name>.png`. The phone's are
//! `phone-<name>.png` at 390 by 844 logical pixels with a safe area of 47 at
//! the top and 34 at the bottom. The phone set also includes the long pages
//! scrolled to their end (`phone-<name>-end.png`), screens with the keyboard
//! up (`phone-<name>-keyboard.png`), the recovery phrase on a short phone
//! (640 tall), after a screenshot and while the screen is captured, the
//! phone-only screens (welcome, an opened account card, the paged Send flow,
//! the scanner, sideways layouts) and the device unlock, which the desktop
//! does not offer. Every piece of text names a bundled face
//! (`tawara_app::fonts`), so the pixels do not depend on the fonts a machine
//! has installed.
//!
//! Flags:
//!
//! - `DIR`: the folder to write into, or to check. The default is
//!   `target/screenshots`.
//! - `--check`: compares every selected image with the one in the folder, and
//!   reports `same`, `DIFFERS` (with the pixels that differ), `MISSING`, and
//!   in an unfiltered run `STALE` for a PNG that no sample draws. It exits
//!   non-zero on anything but `same`. What it drew is written into the
//!   system's temporary folder, as `<name>.drawn.png`, when an image differs.
//!   The selected phone screens are also laid out at 360 logical pixels, as
//!   `--fit` lays them out.
//! - `--only PREFIX`, repeatable: only the samples whose name starts with a
//!   prefix, for example `--only phone-`.
//! - `--except PREFIX`, repeatable: leaves out the samples whose name starts
//!   with a prefix, for example `--except phone-` for the desktop alone.
//! - `--phone-width N`: draws the phone's images N logical pixels wide rather
//!   than 390, for looking at a width, and with `--fit` lays them out at N
//!   rather than 360. It is refused with `--check`.
//! - `--fit`: lays every selected phone sample out at its own height (844,
//!   or 640 for the two short frames), at the width `--phone-width` gives or
//!   else at 360, and prints `OVERFLOW name: left..right` for each node
//!   whose right edge passes that width, and for each text whose words do,
//!   with its content (ten a sample at most). The screens that show all of
//!   their words at once, the welcome screen and each frame of the recovery
//!   phrase, are held further, as their tests hold them
//!   (`tawara_app::overflow::viewport_problems`): `OVERFLOW name:` is also
//!   printed, with the problem, for a word above the top inset or below the
//!   bottom one, a page
//!   that scrolls, and a word wider than its box. The phone's wallet page is
//!   also laid out in nanoMCM. It exits non-zero if there is any, and writes
//!   nothing. `--fit --phone-width 390` holds the phone set to the width it is
//!   drawn at.
//! - The environment variable `TAWARA_SCREENSHOTS_PRINT`: with `--check`,
//!   prints each image that DIFFERS or is MISSING as base64, between
//!   `----- BEGIN <name>.png -----` and `----- END <name>.png -----`, so that
//!   a CI log carries the drawn image.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::operation::scrollable::{self, AbsoluteOffset, RelativeOffset};
use iced::advanced::widget::operation::{Outcome, focusable};
use iced::advanced::widget::{Id, Operation};
use iced::advanced::{clipboard, graphics};
use iced::mouse::Cursor;
use iced::theme::Base;
use iced::{Event, Size, window};
use iced_runtime::user_interface::{Cache, UserInterface};
use tawara_app::app::{
    AccountPage, ActivityPage, AddAccountPage, Back, BlockPage, Busy, DestinationRow, Explored,
    ExplorerPage, FormOf, Level, Missed, Model, Movement, NodeForm, NodeState, Page, PasswordForm,
    PhraseState, QueuePage, ReceivePage, RecoveryPage, ReportKey, ResignPage, RestoreForm, SHARED,
    Scanning, Screen, SendPage, SendStage, SentPage, SettingsPage, Signed, SpendForm, StakePage,
    StartChoice, SubmitMode, SubmitPage, TagPage, TransactionPage, UnlockForm, WalletPage,
};
use tawara_app::history::Filter;
use tawara_app::host::{Device, DeviceCheck, Host, Released, ScanDone, ScanPreview, ScanStart};
use tawara_app::scan::Scanner;
use tawara_app::{fonts, overflow, screens, theme};
use tawara_wallet_core::explorer::{BlockAt, IndexState, LedgerRead, Party, TagView};
use tawara_wallet_core::location::SyncWarning;
use tawara_wallet_core::preferences::{AmountUnit, DEFAULT_NODE, DeviceUnlock, Preferences};
use tawara_wallet_core::sample;
use tawara_wallet_core::view::{AccountState, DivergenceKind};
use tawara_wallet_core::{
    Activity, CONFIRM_POSITIONS, NetworkName, PhraseForDisplay, Progress, SyncState,
};

/// The window size for each screen, and the smallest window
/// (`tawara_app::WINDOW_MIN`).
const FIRST_RUN: (u32, u32) = (1440, 900);
const DASHBOARD: (u32, u32) = (1440, 1160);
const SMALLEST: (u32, u32) = (1024, 700);
const DASHBOARD_NARROW: (u32, u32) = (1024, 1160);
const EXPLORER: (u32, u32) = (1440, 980);
const BLOCK: (u32, u32) = (1440, 1200);
const TAG: (u32, u32) = (1440, 1000);
const TRANSACTION: (u32, u32) = (1440, 860);

const NODE: &str = "https://node.example";
const DIR: &str = "/home/you/.local/share/tawara/keystore";

fn base() -> Model {
    let prefs = Preferences {
        node: Some(NODE.to_owned()),
        ..Preferences::default()
    };
    let mut model = Model::new(prefs, Some(PathBuf::from(DIR)));
    model.node = NodeState {
        url: Some(NODE.to_owned()),
        tip: Some((871_173, Duration::from_millis(42))),
        solved_ms: Some(sample::TIP_MS),
        sync: Some(SyncState {
            stage: "synchronized".to_owned(),
            synced: true,
        }),
        network: Some(NetworkName {
            blockchain: "mochimo".to_owned(),
            network: "mainnet".to_owned(),
        }),
        error: None,
    };
    // The clock stands 48 seconds after the sample chain's newest block,
    // so the drawn age is the same on every run.
    model.clock_ms = sample::TIP_MS + 48_000;
    // Dates in UTC whatever the machine's zone, for the same reason.
    model.zone = tawara_app::ui::Zone::Fixed(0);
    model.screen = Screen::Start {
        choice: StartChoice::Create,
    };
    model
}

/// `model` with its page's report `key` open to `level`.
fn opened(mut model: Model, key: ReportKey, level: Level) -> Model {
    if let Screen::Wallet(p) = &mut model.screen {
        p.open.insert(key, level);
    }
    model
}

/// The sent page opens its summary, as the application does.
fn opened_sent(model: Model) -> Model {
    opened(model, ReportKey::Sent, Level::Summary)
}

/// The sample store open on a wallet page.
fn on(page: Page) -> Model {
    let mut m = with(Screen::Wallet(WalletPage {
        page,
        ..WalletPage::default()
    }));
    let wallet = sample::wallet_view();
    // The dashboard's Recent activity: the balance changes seen since the
    // unlock.
    m.movements = Movement::samples(&wallet, m.clock_ms);
    m.wallet = Some(wallet);
    m.activity = read(sample::activity());
    m.blocks = read(sample::blocks());
    m.mempool = read(sample::mempool());
    m.chain = read(sample::chain());
    m.pending = read(sample::pending());
    // Read three seconds before the clock stands.
    m.chain.read_ms = Some(m.clock_ms - 3_000);
    m
}

fn read<T>(value: T) -> Explored<T> {
    Explored {
        last: Some(Ok(value)),
        ..Explored::default()
    }
}

fn with(screen: Screen) -> Model {
    let mut m = base();
    m.screen = screen;
    m
}

/// `screen` at start-up as most people meet Get started and Unlock, before a
/// wallet is opened: no node was ever chosen, so the default is set, and
/// nothing has asked it yet.
fn first_start(screen: Screen) -> Model {
    let mut m = with(screen);
    m.prefs = Preferences::default();
    m.node = NodeState {
        url: Some(DEFAULT_NODE.to_owned()),
        ..NodeState::default()
    };
    m
}

fn form(dir: &str, password: &str) -> PasswordForm {
    PasswordForm {
        dir: dir.to_owned(),
        password: password.to_owned(),
        again: password.to_owned(),
        error: None,
        synced: None,
        note: None,
    }
}

/// Every built screen: its file name, its size, and its model.
fn samples() -> Vec<(&'static str, (u32, u32), Model)> {
    let phrase = || PhraseState {
        phrase: PhraseForDisplay::example(),
        dir: DIR.to_owned(),
        positions: CONFIRM_POSITIONS,
        written: true,
        words: Default::default(),
        error: None,
        shown: false,
        photographed: false,
    };
    let synced = SyncWarning {
        service: "Dropbox",
        folder: PathBuf::from("/home/you/Dropbox"),
        only_if_enabled: false,
    }
    .to_string();
    let busy = |activity, progress| Busy {
        id: tawara_wallet_core::RequestId::example(),
        activity,
        progress,
    };
    let mut waiting = with(Screen::Unlock(UnlockForm::default()));
    waiting.busy = Some(busy(None, None));
    let mut opening = with(Screen::Unlock(UnlockForm::default()));
    opening.busy = Some(busy(Some(Activity::DerivingKey), None));
    let mut reconciling = with(Screen::Unlock(UnlockForm::default()));
    reconciling.busy = Some(busy(
        Some(Activity::AskingNode),
        Some(Progress {
            account: 2,
            accounts: 3,
            position: 3_840,
            ceiling: 10_017,
        }),
    ));
    let wallet = || on(Page::Dashboard);
    let mut new_wallet = on(Page::Dashboard);
    new_wallet.wallet = Some(sample::new_view());
    let accounts = sample::wallet_view().accounts;
    let plan = sample::plan_view();
    let typed = SpendForm {
        rows: plan
            .destinations
            .iter()
            .map(|d| DestinationRow {
                to: d.destination.clone(),
                // As a person types it.
                amount: tawara_wallet_core::amount::format_mcm(d.amount)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_owned(),
                reference: d.reference.clone(),
                memo_checked: false,
            })
            .collect(),
        ..SpendForm::default()
    };
    let compose = || {
        on(Page::Send(SendPage {
            from: Some(accounts[0].id),
            form: typed.clone(),
            stage: SendStage::Compose,
        }))
    };
    let mut sent = opened_sent(on(Page::Send(SendPage {
        from: Some(accounts[0].id),
        form: typed.clone(),
        stage: SendStage::Sent(Box::new(SentPage {
            sent: sample::sent_view(),
            order: 0,
            resigned: false,
            saved: Some(Ok(PathBuf::from(
                "/home/you/Downloads/tawara-spend-5e5e5e5e5e5e5e5e.hex",
            ))),
            dismissing: false,
        })),
    })));
    sent.wallet = Some(sample::sent_view().view);
    // The same spend, its page left before its bytes were saved.
    let kept = || Signed {
        sent: SentPage {
            sent: sample::sent_view(),
            order: 0,
            resigned: false,
            saved: None,
            dismissing: false,
        },
        form: typed.clone(),
        open: Default::default(),
        resume: false,
        copied: false,
    };
    let mut unsaved = wallet();
    unsaved.wallet = Some(sample::sent_view().view);
    unsaved.signed = vec![kept()];
    // The second account in sync now, and set aside when the wallet opened.
    let mut aside = sample::wallet_view();
    aside.accounts[1].state = AccountState::InSync {
        balance: 3_180_000_000_000,
    };
    aside.accounts[1].spendable = false;
    let set_aside = |page| {
        let mut m = on(page);
        m.wallet = Some(aside.clone());
        m
    };
    // The second account's spend has landed: the chain shows the change key,
    // and the automatic settle has not yet been made, so the account page
    // offers Settle as its fallback.
    let landed_wallet = sample::landed_view();
    let landed = |page| {
        let mut m = on(page);
        m.wallet = Some(landed_wallet.clone());
        m
    };
    // The third account's settle was reverted by a reorganisation: the chain
    // holds it one key behind the store, with the settle retained, so the
    // account page offers Submit saved artifact. Its report is the sample's
    // words, not the library's.
    let mut reverted_wallet = sample::wallet_view();
    reverted_wallet.accounts[2].state = AccountState::Diverged {
        kind: DivergenceKind::Behind { gap: 1 },
        report: "This store recorded a SETTLE at index 30, and this API's answer does not show it."
            .to_owned(),
        advance_to: None,
        rollback_to: None,
        reverted_settle: true,
    };
    let reverted = |page| {
        let mut m = on(page);
        m.wallet = Some(reverted_wallet.clone());
        m
    };
    // The first account's spend once it has settled: the account is in sync
    // again, so the sent page's Settle step is done.
    let settled_sent = {
        let mut settled_wallet = sample::sent_view().view;
        let balance = settled_wallet.accounts[0]
            .state
            .balance()
            .unwrap_or_default();
        settled_wallet.accounts[0].state = AccountState::InSync { balance };
        settled_wallet.accounts[0].spendable = true;
        let mut m = opened_sent(on(Page::Send(SendPage {
            from: Some(accounts[0].id),
            form: typed.clone(),
            stage: SendStage::Sent(Box::new(SentPage {
                sent: sample::sent_view(),
                order: 0,
                resigned: false,
                saved: Some(Ok(PathBuf::from(
                    "/home/you/Downloads/tawara-spend-5e5e5e5e5e5e5e5e.hex",
                ))),
                dismissing: false,
            })),
        })));
        m.wallet = Some(settled_wallet);
        m
    };
    let refused = |state| {
        let mut m = on(Page::Activity(ActivityPage::default()));
        m.activity = Explored {
            last: Some(Err(sample::index_refusal(state))),
            ..Explored::default()
        };
        m
    };
    // The payment received, as Activity lists it.
    let histories = sample::activity();
    let received = tawara_app::history::rows(&histories)
        .into_iter()
        .find(|r| r.tx.id == histories[0].transactions[1].id)
        .map(|r| r.id())
        .expect("the sample's payment received is a row");
    let no_index = refused(IndexState::Absent);
    // The walk under way: the first account read, the second being checked.
    let checking = {
        let mut m = on(Page::Activity(ActivityPage::default()));
        let first = histories[0].clone();
        let next = m
            .wallet
            .as_ref()
            .and_then(|w| w.accounts.iter().find(|a| a.id != first.account))
            .map(|a| a.id);
        m.activity = Explored {
            last: Some(Ok(vec![first])),
            reading: true,
            ..Explored::default()
        };
        m.activity_checking = next;
        m
    };
    // Opened to its summary, so its words are drawn.
    let index_down = opened(
        refused(IndexState::Unavailable),
        ReportKey::Explorer,
        Level::Summary,
    );
    // Every account's report: the two that need nothing at `others`, the
    // paused one at `paused`.
    let recovery = |others: Level, paused: Level| {
        let mut m = on(Page::Recovery(RecoveryPage {
            reports: Some(sample::review()),
            rollback: None,
            result: None,
        }));
        if let Screen::Wallet(p) = &mut m.screen {
            for n in 0..2 {
                p.open.insert(ReportKey::Review(n), others);
            }
            p.open.insert(ReportKey::Review(2), paused);
        }
        m
    };
    let stake_new = sample::stake_view();
    let stake_locked = sample::stake_view_locked();
    let stake_review = sample::stake_plan_view();
    // A stake of 500 MCM at tier 2 laid out, for its review: the
    // unstake's sample with the stake's figures, its lock 65,536 x 4 heights.
    let mut staking_review = stake_review.clone();
    staking_review.action = tawara_wallet_core::staking::StakeAction::Stake {
        amount: 500_000_000_000,
        tier: 2,
    };
    staking_review.staked = 0;
    staking_review.forfeits = None;
    staking_review.expiry = Some(staking_review.height + 262_144);
    let mut stopped = base();
    stopped.stopped = Some(true);
    stopped.signed = vec![kept()];
    let mut unreconciled = with(Screen::Wallet(WalletPage::default()));
    unreconciled.wallet = Some(tawara_wallet_core::sample::unreconciled_view());
    unreconciled.node.tip = None;
    unreconciled.node.error = Some("the API did not answer".to_owned());
    let mut refreshing = wallet();
    refreshing.busy = Some(busy(
        Some(Activity::AskingNode),
        Some(Progress {
            account: 1,
            accounts: 3,
            position: 0,
            ceiling: 0,
        }),
    ));
    let explorer = || on(Page::Explorer(ExplorerPage::default()));
    // A hash typed that names nothing: the index's answer, and the node's.
    let hash_typed = "7c3e9a1f05b84d62e1a9c7f30d58b2e46a1f9c03e7d5b28a4f6e1c90d02b5e84";
    let mut hash = [0u8; 32];
    for (i, b) in hash.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hash_typed[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    let (transaction, block) = sample::not_found(hash);
    let missed = on(Page::Explorer(ExplorerPage {
        search: hash_typed.to_owned(),
        missed: Some(Box::new(Missed {
            searched: true,
            transaction,
            block,
        })),
        ..ExplorerPage::default()
    }));
    let tag = |view: TagView, typed| {
        on(Page::Tag(TagPage {
            account: view.account,
            read: Some(Box::new(view)),
            reading_older: false,
            typed,
        }))
    };
    // The sample block's first payee: an account not the store's.
    let payee = sample::block().spends[0]
        .destinations()
        .find_map(|o| match o.party {
            Party::Account(id) => Some(id),
            _ => None,
        })
        .expect("the sample block's spend has a payee");
    let found = sample::transaction();
    vec![
        (
            "s1-get-started",
            FIRST_RUN,
            first_start(Screen::Start {
                choice: StartChoice::Create,
            }),
        ),
        (
            "s1-get-started-restore",
            FIRST_RUN,
            with(Screen::Start {
                choice: StartChoice::Restore,
            }),
        ),
        (
            "s2-node",
            FIRST_RUN,
            with(Screen::Node(NodeForm {
                url: NODE.to_owned(),
                error: None,
                back: Back::Start,
            })),
        ),
        (
            "s3-new-wallet",
            FIRST_RUN,
            with(Screen::NewWallet(form(DIR, "correct horse battery staple"))),
        ),
        (
            "s3-new-wallet-synced",
            FIRST_RUN,
            with(Screen::NewWallet(PasswordForm {
                synced: Some(synced),
                ..form("/home/you/Dropbox/tawara", "")
            })),
        ),
        (
            "s3-new-wallet-discarded",
            FIRST_RUN,
            with(Screen::NewWallet(PasswordForm {
                note: Some(
                    "The recovery phrase was discarded after 5 minutes with nothing done, \
                     before it was confirmed, and no store was written. Choose the password \
                     again and continue to get a new phrase."
                        .to_owned(),
                ),
                ..form(DIR, "")
            })),
        ),
        ("s4-phrase", FIRST_RUN, with(Screen::Phrase(phrase()))),
        (
            "s5-confirm",
            FIRST_RUN,
            with(Screen::Confirm(PhraseState {
                words: ["abandon".into(), "abandon".into(), String::new()],
                ..phrase()
            })),
        ),
        (
            "s6-restore",
            FIRST_RUN,
            with(Screen::Restore(RestoreForm {
                form: form(DIR, "correct horse battery staple"),
                phrase: "abandon abandon abandon".to_owned(),
            })),
        ),
        (
            "s7-unlock",
            FIRST_RUN,
            first_start(Screen::Unlock(UnlockForm {
                dir: DIR.to_owned(),
                ..UnlockForm::default()
            })),
        ),
        (
            "s7-unlock-idle",
            FIRST_RUN,
            with(Screen::Unlock(UnlockForm {
                dir: DIR.to_owned(),
                note: Some(
                    "Locked after 5 minutes with nothing done. The store is closed and its \
                     key is gone from memory; unlock it to go on."
                        .to_owned(),
                ),
                ..UnlockForm::default()
            })),
        ),
        (
            "s7-unlock-no-folder",
            FIRST_RUN,
            with(Screen::Unlock(UnlockForm {
                dir: "/home/you/wallet".to_owned(),
                error: Some(
                    "there is no keystore at /home/you/wallet: the folder does not exist"
                        .to_owned(),
                ),
                ..UnlockForm::default()
            })),
        ),
        (
            "s7-unlock-spend-kept",
            FIRST_RUN,
            with(Screen::Unlock(UnlockForm {
                dir: DIR.to_owned(),
                note: Some(
                    "A signed spend's page was open when the wallet locked. It is kept, with \
                     the signed bytes, and shown again when this store is unlocked."
                        .to_owned(),
                ),
                ..UnlockForm::default()
            })),
        ),
        ("s8-waiting", FIRST_RUN, waiting),
        ("s8-opening", FIRST_RUN, opening),
        ("s8-reconciling", FIRST_RUN, reconciling),
        ("w1-wallet", DASHBOARD, wallet()),
        // A wallet just made, its one account not yet paid: "New Account",
        // and no notice to open.
        ("w1-wallet-new", DASHBOARD, new_wallet),
        (
            "w1-wallet-notice-open",
            DASHBOARD,
            opened(wallet(), ReportKey::Notice, Level::Summary),
        ),
        ("w1-wallet-refreshing", DASHBOARD, refreshing),
        ("w1-wallet-unsaved-spend", DASHBOARD, unsaved),
        (
            "w2-receive",
            DASHBOARD,
            on(Page::Receive(ReceivePage::answered(
                accounts[0].id,
                sample::receive_view(),
            ))),
        ),
        (
            "w3-add-account",
            DASHBOARD,
            on(Page::AddAccount(AddAccountPage {
                found: Some(sample::discovered()),
                ..AddAccountPage::default()
            })),
        ),
        ("w4-send", DASHBOARD, compose()),
        (
            "w4-send-cannot-spend",
            DASHBOARD,
            set_aside(Page::Send(SendPage {
                from: Some(accounts[1].id),
                form: typed.clone(),
                stage: SendStage::Compose,
            })),
        ),
        (
            "w5-review",
            DASHBOARD,
            on(Page::Send(SendPage {
                from: Some(accounts[0].id),
                form: typed.clone(),
                stage: SendStage::Review(Box::new(plan.clone())),
            })),
        ),
        ("w6-sent", DASHBOARD, sent),
        ("w6-sent-settled", DASHBOARD, settled_sent),
        (
            "w7-account-outstanding",
            DASHBOARD,
            on(Page::Account(AccountPage {
                account: accounts[1].id,
                report: None,
            })),
        ),
        (
            "w7-account-landed",
            DASHBOARD,
            landed(Page::Account(AccountPage {
                account: accounts[1].id,
                report: None,
            })),
        ),
        (
            "w7-account-set-aside",
            DASHBOARD,
            set_aside(Page::Account(AccountPage {
                account: accounts[1].id,
                report: None,
            })),
        ),
        (
            "w7-account-diverged",
            DASHBOARD,
            on(Page::Account(AccountPage {
                account: accounts[2].id,
                report: None,
            })),
        ),
        (
            "w7-account-diverged-open",
            DASHBOARD,
            opened(
                on(Page::Account(AccountPage {
                    account: accounts[2].id,
                    report: None,
                })),
                ReportKey::Diverged,
                Level::Summary,
            ),
        ),
        (
            "w7-account-diverged-full",
            DASHBOARD,
            opened(
                on(Page::Account(AccountPage {
                    account: accounts[2].id,
                    report: None,
                })),
                ReportKey::Diverged,
                Level::Full,
            ),
        ),
        (
            "w7-account-reverted-settle",
            DASHBOARD,
            reverted(Page::Account(AccountPage {
                account: accounts[2].id,
                report: None,
            })),
        ),
        (
            "w8-resign",
            DASHBOARD,
            on(Page::Resign(ResignPage {
                account: accounts[1].id,
                form: typed.clone(),
            })),
        ),
        (
            "w9-submit",
            DASHBOARD,
            on(Page::Submit(SubmitPage {
                hex: sample::sent_view().artifact_hex,
                result: Some((true, sample::submitted_text())),
                ..SubmitPage::default()
            })),
        ),
        // The same page for the saved copy of a settle the chain reverted, from
        // the account page's Submit saved artifact.
        (
            "w9-submit-resubmit",
            DASHBOARD,
            on(Page::Submit(SubmitPage {
                hex: sample::sent_view().artifact_hex,
                mode: SubmitMode::Resubmit(accounts[2].id),
                ..SubmitPage::default()
            })),
        ),
        // Saved bytes the API did not take: the banner's reason line.
        (
            "w9-submit-refused",
            DASHBOARD,
            on(Page::Submit(SubmitPage {
                hex: sample::sent_view().artifact_hex,
                result: Some((
                    false,
                    "submission FAILED: the API did not answer within 30 seconds\nNothing was sent."
                        .to_owned(),
                )),
                ..SubmitPage::default()
            })),
        ),
        ("w1-wallet-unreconciled", DASHBOARD, unreconciled),
        (
            "w10-activity",
            DASHBOARD,
            on(Page::Activity(ActivityPage::default())),
        ),
        (
            "w10-activity-pending",
            DASHBOARD,
            on(Page::Activity(ActivityPage {
                filter: Filter::Pending,
                ..ActivityPage::default()
            })),
        ),
        // A payment received chosen: its reference is on the destination
        // that reached the account.
        (
            "w10-activity-received",
            DASHBOARD,
            on(Page::Activity(ActivityPage {
                selected: Some(received.clone()),
                ..ActivityPage::default()
            })),
        ),
        ("w10-activity-checking", DASHBOARD, checking),
        ("w10-activity-no-index", DASHBOARD, no_index),
        ("w10-activity-index-down", DASHBOARD, index_down),
        (
            "w11-settings",
            DASHBOARD,
            on(Page::Settings(SettingsPage {
                node: NODE.to_owned(),
            })),
        ),
        // The licences, from Settings' About card: the
        // wallet's licence, then the third-party notices, each in a card with
        // its copy button, under Back. The phone's image is drawn from it too.
        ("w11-licences", DASHBOARD, on(Page::Licences)),
        // As the reports come: every one at its summary, the paused one
        // with its Resync.
        (
            "w12-recovery",
            DASHBOARD,
            recovery(Level::Summary, Level::Summary),
        ),
        // The reports that need nothing closed, the paused one open.
        (
            "w12-recovery-resync",
            DASHBOARD,
            recovery(Level::Closed, Level::Summary),
        ),
        // The stake page for an account that does not
        // stake: 500 MCM typed and tier 3 chosen, the library's page under the
        // cards opened to its summary.
        (
            "w13-stake",
            DASHBOARD,
            opened(
                on(Page::Stake(StakePage {
                    account: Some(stake_new.account),
                    read: Some(Box::new(stake_new)),
                    reading: false,
                    amount: "500".to_owned(),
                    tier: 3,
                    review: None,
                })),
                ReportKey::Read,
                Level::Summary,
            ),
        ),
        // An account whose stake is still locked: unstaking forfeits.
        (
            "w13-stake-locked",
            DASHBOARD,
            on(Page::Stake(StakePage {
                account: Some(stake_locked.account),
                read: Some(Box::new(stake_locked)),
                reading: false,
                amount: String::new(),
                tier: 0,
                review: None,
            })),
        ),
        // The review of that unstake, before anything is signed.
        (
            "w13-stake-review",
            DASHBOARD,
            on(Page::Stake(StakePage {
                account: Some(stake_review.account),
                read: Some(Box::new(sample::stake_view_locked())),
                reading: false,
                amount: String::new(),
                tier: 0,
                review: Some(Box::new(stake_review)),
            })),
        ),
        // The review of a stake.
        (
            "w13-stake-review-stake",
            DASHBOARD,
            on(Page::Stake(StakePage {
                account: Some(staking_review.account),
                read: Some(Box::new(sample::stake_view())),
                reading: false,
                amount: "500".to_owned(),
                tier: 2,
                review: Some(Box::new(staking_review)),
            })),
        ),
        ("e1-explorer", EXPLORER, explorer()),
        ("e1-explorer-missed", EXPLORER, missed),
        // "View all pending": the whole queue, read whole.
        (
            "e1-explorer-queue",
            BLOCK,
            on(Page::Queue(QueuePage {
                read: Some(Ok(sample::queue())),
            })),
        ),
        (
            "e2-block",
            BLOCK,
            on(Page::Block(BlockPage {
                at: BlockAt::Number(871_172),
                read: Some(Ok(Box::new(sample::block()))),
                rows_from: 0,
            })),
        ),
        // Typed into the search field, so it can be sent to.
        ("e3-tag", TAG, tag(sample::tag_view(), true)),
        // An account not the store's, reached through a link, which the
        // node did not resolve, on a node that keeps no index: why it can
        // be, opened.
        (
            "e3-tag-unresolved",
            TAG,
            opened(
                tag(
                    TagView {
                        account: payee,
                        ledger: LedgerRead::Unresolved,
                        history: Err(sample::index_refusal(IndexState::Absent)),
                    },
                    false,
                ),
                ReportKey::Ledger,
                Level::Summary,
            ),
        ),
        (
            "e4-transaction",
            TRANSACTION,
            on(Page::Transaction(TransactionPage {
                transaction: found.0,
                from_index: true,
                text: Some(found.1),
                tip: None,
            })),
        ),
        ("stopped-spends", FIRST_RUN, stopped),
        ("narrow-s1-get-started", SMALLEST, base()),
        ("narrow-s4-phrase", SMALLEST, with(Screen::Phrase(phrase()))),
        ("narrow-w1-wallet", DASHBOARD_NARROW, wallet()),
        ("narrow-w4-send", DASHBOARD_NARROW, compose()),
        ("narrow-e1-explorer", DASHBOARD_NARROW, explorer()),
    ]
}

/// The phone's window, and the folder a
/// phone keeps its stores in: the one the system protects, as the shells
/// report it.
const PHONE: (u32, u32) = (390, 844);
const PHONE_DIR: &str =
    "/private/var/mobile/Containers/Data/Application/0A1B2C3D/Library/Application Support/Tawara";
/// A phone held sideways: 844 by 390 logical
/// pixels, as an iPhone 14 is held.
const SIDEWAYS: (u32, u32) = (844, 390);
/// How far the sideways To page is scrolled down, in logical pixels, so that its
/// first destination's row, with its Paste and its Scan, is in the window.
const SIDEWAYS_SCROLL: f32 = 90.0;
/// How far the sideways amounts page is scrolled down, in logical pixels, so that
/// its table's header and rows, and the fee and the block-to-live side by side,
/// are in the window.
const SIDEWAYS_AMOUNTS_SCROLL: f32 = 100.0;
/// A short phone's height: under 733 pixels between its insets
/// (`PHRASE_ONE_FRAME` in crates/app/src/screens/first_run.rs), so the
/// recovery phrase is drawn in two frames on it.
const SHORT_PHONE: u32 = 640;

/// The long pages drawn a second time scrolled to their end. Receive is one,
/// so that its QR code, below the fold, is in an image. Five of the Send
/// pages are among them, To, Amount, Review, Sign and Sent, so that the foot
/// of each, with its button, is in an image. The recovery phrase is not: its
/// frame fits 390 by 844 without scrolling.
const PHONE_ENDS: [&str; 14] = [
    "w1-wallet",
    "w2-receive",
    "w4-send-to",
    "w4-send-amount",
    "w5-review",
    "w5-sign",
    "w6-sent",
    "w10-activity",
    "w11-settings",
    "w12-recovery",
    "w13-stake",
    "e1-explorer",
    "e2-block",
    "e3-tag",
];

/// The phone's keyboard, in logical pixels, when it shows for a screen drawn
/// with one.
const KEYBOARD: f32 = 336.0;

/// How far the phone's wallet page is scrolled down for the image of an open
/// account's card, in logical pixels: the accounts start under the bar, and
/// the card under the open row is drawn whole.
const ACCOUNTS_SCROLL: f32 = 555.0;

/// How far the phone's Settings page is scrolled down for the image of the
/// device unlock's row, in logical pixels: the keystore card's last row is in
/// the window.
const DEVICE_ROW_SCROLL: f32 = 650.0;

/// `model` as a phone draws it: the phone's safe area, with the keyboard up
/// when `keyboard` is above zero, every store in the phone's own folder, and
/// Save artifact's share sheet where the desktop wrote a file.
fn phone(mut model: Model, keyboard: f32) -> Model {
    model.safe_area = iced::mobile::SafeArea::new(iced::Padding {
        top: 47.0,
        right: 0.0,
        bottom: 34.0,
        left: 0.0,
    })
    .with_keyboard(keyboard);
    model.default_dir = Some(Path::new(PHONE_DIR).join("keystore"));
    model.confine(PathBuf::from(PHONE_DIR));
    // An open store on a phone is the one in its own folder: Settings shows it.
    let keystore = Path::new(PHONE_DIR).join("keystore");
    if let Some(wallet) = &mut model.wallet {
        wallet.dir = keystore.clone();
    }
    match &mut model.screen {
        // The worker names the folder it found no store in, and on a phone that
        // is the phone's own.
        Screen::Unlock(UnlockForm {
            error: Some(error), ..
        }) if error.starts_with("there is no keystore at ") => {
            *error = format!(
                "there is no keystore at {}: the folder does not exist",
                keystore.display()
            );
        }
        // A phone has no Downloads folder: Save artifact asked for the share sheet.
        Screen::Wallet(WalletPage {
            page:
                Page::Send(SendPage {
                    stage: SendStage::Sent(sent),
                    ..
                }),
            ..
        }) if sent.saved.is_some() => {
            sent.saved = Some(Err(SHARED.to_owned()));
        }
        _ => {}
    }
    model
}

/// One image to draw: its name (its file's), the window it is drawn in, the
/// model it draws, whether it is drawn scrolled to the end of its page, the
/// field given the focus, if any, and the offset, in logical pixels from the
/// top, the page is scrolled to, if any.
struct Image {
    name: String,
    size: (u32, u32),
    model: Model,
    to_end: bool,
    focus: Option<Id>,
    scroll: Option<f32>,
}

/// Every desktop image, as the screens are drawn in their windows.
fn desktop() -> Vec<Image> {
    samples()
        .into_iter()
        .map(|(name, size, model)| Image {
            name: name.to_owned(),
            size,
            model,
            to_end: false,
            focus: None,
            scroll: None,
        })
        .collect()
}

/// The sample named `name`, built again: a model is not cloned.
fn sample_named(name: &str) -> Model {
    samples()
        .into_iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, model)| model)
        .unwrap_or_else(|| panic!("no sample named {name}"))
}

/// The state the phone's image named `name` is drawn from, without its `phone-`
/// prefix. That is one of the phone's Send pages, or else the sample of that
/// name. A Send page is the desktop's Send sample, with its account and form,
/// and its Send tab on that page.
fn phone_named(name: &str) -> Model {
    let stage = match name {
        "w4-send-to" => SendStage::To(BTreeMap::new()),
        "w4-send-amount" => SendStage::Amount,
        "w5-sign" => SendStage::Sign(Box::new(sample::plan_view())),
        _ => return sample_named(name),
    };
    let mut model = sample_named("w4-send");
    if let Screen::Wallet(WalletPage {
        page: Page::Send(send),
        ..
    }) = &mut model.screen
    {
        send.stage = stage;
    }
    model
}

/// The phone images at `width` logical pixels across: every desktop sample
/// but the narrow ones, the phone-only screens, the long pages scrolled to
/// their end, the screens drawn with the keyboard up, the recovery phrase
/// after a screenshot and while captured, and the device unlock's images.
fn phones(width: u32) -> Vec<Image> {
    let size = (width, PHONE.1);
    // A phone's Send starts on page 1, so the desktop's Send sample is not drawn
    // as an image of its own: its page 1 is "phone-w4-send-to".
    let mut out: Vec<Image> = samples()
        .into_iter()
        .filter(|(name, _, _)| !name.starts_with("narrow-") && *name != "w4-send")
        .map(|(name, _, model)| Image {
            name: format!("phone-{name}"),
            size,
            model: phone(model, 0.0),
            to_end: false,
            focus: None,
            scroll: None,
        })
        .collect();
    // The welcome screen is a phone's alone, so it has no desktop image. The
    // sample's phone holds no store, so welcome is its first screen.
    out.push(Image {
        name: "phone-s0-welcome".to_owned(),
        size,
        model: phone(with(Screen::Welcome), 0.0),
        to_end: false,
        focus: None,
        scroll: None,
    });
    // The dashboard with its first account's row opened into its card. The
    // desktop lists its accounts in a table, which opens
    // nothing, so this image is the phone's alone. The page is scrolled so that
    // the accounts, and the card under the row, are in the frame.
    let mut open = sample_named("w1-wallet");
    if let (Screen::Wallet(page), Some(wallet)) = (&mut open.screen, &open.wallet) {
        page.expanded = Some(wallet.accounts[0].id);
    }
    out.push(Image {
        name: "phone-w1-wallet-account-open".to_owned(),
        size,
        model: phone(open, 0.0),
        to_end: false,
        focus: None,
        scroll: Some(ACCOUNTS_SCROLL),
    });
    // The phone's Send pages 1, 2 and 4. The desktop's samples draw the form,
    // the review (page 3, "phone-w5-review") and the sent page, not these.
    for name in ["w4-send-to", "w4-send-amount", "w5-sign"] {
        out.push(Image {
            name: format!("phone-{name}"),
            size,
            model: phone(phone_named(name), 0.0),
            to_end: false,
            focus: None,
            scroll: None,
        });
    }
    // The scanner's scenes: page 1 with a shell's scanner,
    // held sideways as well, and the application's own scanner page, with a
    // stand-in frame for the camera's.
    out.push(Image {
        name: "phone-w4-send-to-scan".to_owned(),
        size,
        model: phone(
            with_scanner(phone_named("w4-send-to"), ScanPreview::Shell),
            0.0,
        ),
        to_end: false,
        focus: None,
        scroll: None,
    });
    out.push(Image {
        name: "phone-w4-send-to-sideways".to_owned(),
        size: SIDEWAYS,
        model: phone_sideways(with_scanner(phone_named("w4-send-to"), ScanPreview::Shell)),
        to_end: false,
        focus: None,
        scroll: Some(SIDEWAYS_SCROLL),
    });
    // Page 2 held sideways: the amounts as the table's rows, and the fee and the
    // block-to-live side by side, in the width a sideways phone has.
    out.push(Image {
        name: "phone-w4-send-amount-sideways".to_owned(),
        size: SIDEWAYS,
        model: phone_sideways(phone_named("w4-send-amount")),
        to_end: false,
        focus: None,
        scroll: Some(SIDEWAYS_AMOUNTS_SCROLL),
    });
    out.push(Image {
        name: "phone-w4-send-scan-preview".to_owned(),
        size,
        model: phone(
            scanning_first(with_scanner(
                phone_named("w4-send-to"),
                ScanPreview::Application,
            )),
            0.0,
        ),
        to_end: false,
        focus: None,
        scroll: None,
    });
    // The re-sign form with a shell's scanner: its Paste stands beside each
    // field, and its Scan beneath it.
    out.push(Image {
        name: "phone-w8-resign-scan".to_owned(),
        size,
        model: phone(
            with_scanner(phone_named("w8-resign"), ScanPreview::Shell),
            0.0,
        ),
        to_end: false,
        focus: None,
        scroll: None,
    });
    for name in PHONE_ENDS {
        out.push(Image {
            name: format!("phone-{name}-end"),
            size,
            model: phone(phone_named(name), 0.0),
            to_end: true,
            focus: None,
            scroll: None,
        });
    }
    // A short phone draws the recovery phrase in two frames: the library's
    // warning with "Show the phrase", then the words.
    for (name, shown) in [("s4-phrase-warning", false), ("s4-phrase-shown", true)] {
        let mut model = sample_named("s4-phrase");
        if let Screen::Phrase(p) = &mut model.screen {
            p.shown = shown;
        }
        out.push(Image {
            name: format!("phone-{name}"),
            size: (width, SHORT_PHONE),
            model: phone(model, 0.0),
            to_end: false,
            focus: None,
            scroll: None,
        });
    }
    // The recovery phrase after a screenshot taken while its words are drawn:
    // the warning takes their place, in the grid's space, until "Show the
    // phrase" is pressed, so the page is one frame.
    let mut model = sample_named("s4-phrase");
    if let Screen::Phrase(p) = &mut model.screen {
        p.photographed = true;
    }
    out.push(Image {
        name: "phone-s4-phrase-photographed".to_owned(),
        size,
        model: phone(model, 0.0),
        to_end: false,
        focus: None,
        scroll: None,
    });
    // The recovery phrase while the screen is recorded or mirrored: the words
    // are a placeholder as tall as their grid.
    let mut model = sample_named("s4-phrase");
    model.captured = true;
    out.push(Image {
        name: "phone-s4-phrase-captured".to_owned(),
        size,
        model: phone(model, 0.0),
        to_end: false,
        focus: None,
        scroll: None,
    });
    // The field the keyboard came up for: the recovery phrase, and the first
    // destination on Send's page 1.
    for (name, field) in [
        ("s6-restore", screens::phrase_id()),
        ("w4-send-to", screens::destination_id(0)),
    ] {
        out.push(Image {
            name: format!("phone-{name}-keyboard"),
            size,
            model: phone(phone_named(name), KEYBOARD),
            to_end: false,
            focus: Some(field),
            scroll: None,
        });
    }
    // The unlock with the device's own check, on a phone whose shell offers
    // it: Unlock with the device unlock on, the offer on the wallet's page,
    // and Settings with the unlock's row, scrolled to it.
    out.push(Image {
        name: "phone-s7-unlock-device".to_owned(),
        size,
        model: phone(
            with_device(sample_named("s7-unlock"), DeviceUnlock::On),
            0.0,
        ),
        to_end: false,
        focus: None,
        scroll: None,
    });
    let mut offer = with_device(sample_named("w1-wallet"), DeviceUnlock::Off);
    offer.device_offer = true;
    out.push(Image {
        name: "phone-w1-wallet-offer".to_owned(),
        size,
        model: phone(offer, 0.0),
        to_end: false,
        focus: None,
        scroll: None,
    });
    out.push(Image {
        name: "phone-w11-settings-device".to_owned(),
        size,
        model: phone(
            with_device(sample_named("w11-settings"), DeviceUnlock::On),
            0.0,
        ),
        to_end: false,
        focus: None,
        scroll: Some(DEVICE_ROW_SCROLL),
    });
    out
}

/// `model` on a phone whose shell offers the device unlock with Face ID, with
/// the unlock in `state`.
fn with_device(mut model: Model, state: DeviceUnlock) -> Model {
    model.device = Some(face_id_device());
    model.prefs.device_unlock = state;
    model
}

/// `model` on a phone whose shell gives a scanner, which draws the camera's
/// preview as `preview` says.
fn with_scanner(mut model: Model, preview: ScanPreview) -> Model {
    model.scanner = Some(scanner(preview));
    model
}

/// The QR scanner of a shell that gives all three scan fields. A drawing
/// starts no camera, so its functions do nothing.
fn scanner(preview: ScanPreview) -> Scanner {
    fn copy(_: &str) {}
    fn share(_: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn back() {}
    fn start(_: ScanDone) -> Result<(), String> {
        Ok(())
    }
    fn cancel() {}
    let host = Host {
        private_dir: PathBuf::from(PHONE_DIR),
        copy,
        share,
        back,
        notices: include_str!("../../../THIRD-PARTY-NOTICES.txt"),
        captured: None,
        device_check: None,
        keep_password: None,
        release_password: None,
        forget_password: None,
        scan: Some(start as ScanStart),
        cancel_scan: Some(cancel as fn()),
        scan_preview: Some(preview),
    };
    let (replies, _received) = iced::futures::channel::mpsc::unbounded();
    Scanner::of(&host, replies).expect("a shell that gives every scan field offers a scanner")
}

/// `model` with the scan of its first destination under way, its camera's
/// latest frame the stand-in.
fn scanning_first(mut model: Model) -> Model {
    let scanning = match &model.screen {
        Screen::Wallet(WalletPage {
            page: Page::Send(send),
            ..
        }) => Some(Scanning {
            ask: 1,
            row: 0,
            form: FormOf::Send(send.from),
            rows: send.form.rows.len(),
            typed: send.form.rows[0].clone(),
        }),
        _ => None,
    };
    model.scanning = scanning;
    model.scan_preview = Some(stand_in_frame());
    model
}

/// A stand-in for the frame a camera gives the scanner's page: a dark room, a
/// light card, and a code of dark modules on it with its three corner marks,
/// in 320 by 240 pixels, a 4 by 3 frame. It is drawn, not read from a
/// camera.
fn stand_in_frame() -> iced::widget::image::Handle {
    const W: u32 = 320;
    const H: u32 = 240;
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let room = 20 + y / 12;
            let mut pixel = [room as u8, (room + 6) as u8, (room + 10) as u8, 255];
            if (50..270).contains(&x) && (14..226).contains(&y) {
                pixel = [226, 230, 224, 255];
            }
            if (100..220).contains(&x) && (50..170).contains(&y) {
                let (cx, cy) = ((x - 100) / 6, (y - 50) / 6);
                let corner = (cx < 3 && cy < 3) || (cx >= 17 && cy < 3) || (cx < 3 && cy >= 17);
                if corner || (cx * 5 + cy * 3 + cx * cy) % 4 < 2 {
                    pixel = [20, 24, 26, 255];
                }
            }
            rgba.extend_from_slice(&pixel);
        }
    }
    iced::widget::image::Handle::from_rgba(W, H, rgba)
}

/// `model` as a phone held sideways draws it: 844 by 390, with the insets of a
/// notch at the sides and the home indicator at the foot.
fn phone_sideways(model: Model) -> Model {
    let mut model = phone(model, 0.0);
    model.safe_area = iced::mobile::SafeArea::new(iced::Padding {
        top: 0.0,
        right: 47.0,
        bottom: 21.0,
        left: 47.0,
    });
    model
}

/// The device of a shell that offers Face ID. A
/// drawing asks nothing of it, so its functions answer as a shell does when the
/// person declines.
fn face_id_device() -> Device {
    fn copy(_: &str) {}
    fn share(_: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn back() {}
    fn check() -> Option<DeviceCheck> {
        Some(DeviceCheck::FaceId)
    }
    fn keep(_: &[u8; 32], _: &str) -> Result<(), String> {
        Ok(())
    }
    fn release(_: &[u8; 32], _: &str, answer: Box<dyn FnOnce(Released) + Send>) {
        answer(Released::Declined);
    }
    fn forget() -> Result<(), String> {
        Ok(())
    }
    let host = Host {
        private_dir: PathBuf::from(PHONE_DIR),
        copy,
        share,
        back,
        notices: include_str!("../../../THIRD-PARTY-NOTICES.txt"),
        captured: None,
        device_check: Some(check),
        keep_password: Some(keep),
        release_password: Some(release),
        forget_password: Some(forget),
        scan: None,
        cancel_scan: None,
        scan_preview: None,
    };
    let (answers, _received) = iced::futures::channel::mpsc::unbounded();
    Device::of(&host, answers).expect("a shell that gives every device field offers the unlock")
}

/// The phone's wallet page in nanoMCM, the unit a person can choose in place
/// of MCM, in a window `width` logical pixels wide. It is laid out by `fit`,
/// with the phone set, so that the check covers the balance in that unit.
/// It is not drawn: the images are of MCM, as the desktop's are.
fn nanomcm_samples(width: u32) -> Vec<Image> {
    let mut model = sample_named("w1-wallet");
    model.prefs.unit = AmountUnit::NanoMcm;
    vec![Image {
        name: "phone-w1-wallet-nanomcm".to_owned(),
        size: (width, PHONE.1),
        model: phone(model, 0.0),
        to_end: false,
        focus: None,
        scroll: None,
    }]
}

/// Whether an image named `name` is among those the command line asks for:
/// with `--only` prefixes, one of them starts it; `--except` prefixes leave
/// it out.
fn selected(name: &str, only: &[String], except: &[String]) -> bool {
    (only.is_empty() || only.iter().any(|p| name.starts_with(p.as_str())))
        && !except.iter().any(|p| name.starts_with(p.as_str()))
}

/// The pixels of `model`'s window at `size`, as RGBA. `to_end` scrolls the
/// page to its end first, as a person scrolls it; `focus` gives that field the
/// focus, and the page is scrolled to it as the application scrolls it when
/// the keyboard comes up (`screens::reveal_focused`); `scroll` puts the page at
/// that offset from its top. The frame's instant is taken before the focus, so
/// the caret is drawn on every run.
fn draw(
    renderer: &mut iced::Renderer,
    model: &Model,
    size: (u32, u32),
    to_end: bool,
    focus: Option<Id>,
    scroll: Option<f32>,
) -> Vec<u8> {
    let theme = theme::theme();
    #[allow(clippy::cast_precision_loss)]
    let bounds = Size::new(size.0 as f32, size.1 as f32);
    let mut ui = UserInterface::build(screens::view(model), bounds, Cache::default(), renderer);
    let mut messages = Vec::new();
    let frame = [Event::Window(window::Event::RedrawRequested(
        iced::time::Instant::now(),
    ))];
    let _ = ui.update(
        &frame,
        Cursor::Unavailable,
        renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    if let Some(field) = focus {
        ui.operate(renderer, &mut focusable::focus::<()>(field));
        let mut operation: Box<dyn Operation> = Box::new(screens::reveal_focused());
        loop {
            ui.operate(renderer, operation.as_mut());
            match operation.finish() {
                Outcome::Chain(next) => operation = next,
                _ => break,
            }
        }
        let _ = ui.update(
            &frame,
            Cursor::Unavailable,
            renderer,
            &mut clipboard::Null,
            &mut messages,
        );
    }
    if let Some(y) = scroll {
        let mut to = scrollable::scroll_to(
            screens::page_id(),
            AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        );
        ui.operate(renderer, &mut to);
        let _ = ui.update(
            &frame,
            Cursor::Unavailable,
            renderer,
            &mut clipboard::Null,
            &mut messages,
        );
    }
    if to_end {
        let mut snap = scrollable::snap_to(
            screens::page_id(),
            RelativeOffset {
                x: None,
                y: Some(1.0),
            },
        );
        ui.operate(renderer, &mut snap);
        let _ = ui.update(
            &frame,
            Cursor::Unavailable,
            renderer,
            &mut clipboard::Null,
            &mut messages,
        );
    }
    let style = theme.base();
    ui.draw(
        renderer,
        &theme,
        &renderer::Style {
            text_color: style.text_color,
        },
        Cursor::Unavailable,
    );
    renderer.screenshot(Size::new(size.0, size.1), 1.0, style.background_color)
}

fn encode(rgba: &[u8], size: (u32, u32)) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, size.0, size.1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

fn decode(path: &Path) -> Result<((u32, u32), Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| format!("{}: too large", path.display()))?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(format!("{}: not 8-bit RGBA", path.display()));
    }
    buf.truncate(info.buffer_size());
    Ok(((info.width, info.height), buf))
}

/// `bytes` in the standard base64 alphabet, padded, in lines of 76
/// characters: the form a log carries an image in.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let group = (first << 16) | (second << 8) | third;
        for i in 0..4 {
            if i <= chunk.len() {
                let index = (group >> (18 - 6 * i)) & 0x3f;
                text.push(char::from(ALPHABET[index as usize]));
            } else {
                text.push('=');
            }
        }
    }
    text.as_bytes()
        .chunks(76)
        .map(|line| std::str::from_utf8(line).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// What `model`'s window at `width` by `height` logical pixels draws past its
/// right edge, as `OVERFLOW name: left..right` lines, each text's content after
/// its extent (`tawara_app::overflow`).
fn overflow_lines(
    renderer: &iced::Renderer,
    name: &str,
    model: &Model,
    width: f32,
    height: f32,
) -> Vec<String> {
    overflow::past_the_edge(screens::view(model), renderer, width, height)
        .into_iter()
        .map(|past| {
            let (left, right) = (past.bounds.x, past.bounds.x + past.bounds.width);
            match past.text {
                Some(text) => format!("OVERFLOW {name}: {left:.1}..{right:.1} {text:?}"),
                None => format!("OVERFLOW {name}: {left:.1}..{right:.1}"),
            }
        })
        .collect()
}

/// The phone images of the screens that show all of their words at once: the
/// welcome screen, and the recovery phrase in each of its frames, while the
/// screen is captured and after a screenshot. `--fit` holds them to the part
/// of the window a person sees between the safe area's insets, with no page
/// that scrolls and no word wider than its box, as their tests do.
const WHOLE: [&str; 7] = [
    "phone-s0-welcome",
    "phone-s4-phrase",
    "phone-s4-phrase-warning",
    "phone-s4-phrase-shown",
    "phone-s4-phrase-captured",
    "phone-s4-phrase-photographed",
    "phone-w4-send-scan-preview",
];

/// What `model`'s window at `width` by `height` logical pixels draws outside
/// the part of it a person sees, between the insets of `model`'s safe area, as
/// `OVERFLOW name: problem` lines (`tawara_app::overflow::viewport_problems`).
fn viewport_lines(
    renderer: &iced::Renderer,
    name: &str,
    model: &Model,
    width: f32,
    height: f32,
) -> Vec<String> {
    overflow::viewport_problems(
        screens::view(model),
        renderer,
        Size::new(width, height),
        model.safe_area.padding(0.0),
    )
    .into_iter()
    .map(|problem| format!("OVERFLOW {name}: {problem}"))
    .collect()
}

/// The phone images selected, laid out at `width` logical pixels: prints
/// every overflow (at most ten a sample) and reports whether there was one.
/// The check is at 360, the narrowest phone (`tawara_app::app::NARROWEST`);
/// another width is for `--fit --phone-width`.
fn fit(renderer: &iced::Renderer, only: &[String], except: &[String], width: u32) -> bool {
    let mut failed = false;
    #[allow(clippy::cast_precision_loss)]
    let logical = width as f32;
    for image in phones(width).into_iter().chain(nanomcm_samples(width)) {
        // A phone held sideways is wider than it is tall, and is not held to
        // the phone's width.
        if image.size == SIDEWAYS || !selected(&image.name, only, except) {
            continue;
        }
        let mut model = image.model;
        model.width = logical;
        #[allow(clippy::cast_precision_loss)]
        let height = image.size.1 as f32;
        let lines = if WHOLE.contains(&image.name.as_str()) {
            viewport_lines(renderer, &image.name, &model, logical, height)
        } else {
            overflow_lines(renderer, &image.name, &model, logical, height)
        };
        for line in lines.into_iter().take(10) {
            println!("{line}");
            failed = true;
        }
    }
    failed
}

/// The narrowest width the phone layout is drawn at, as the image sizes
/// name it (`tawara_app::app::NARROWEST`, in logical pixels).
const NARROWEST_PX: u32 = 360;

/// The command line (see the module documentation).
#[derive(Default)]
struct Options {
    check: bool,
    fit: bool,
    dir: Option<PathBuf>,
    only: Vec<String>,
    except: Vec<String>,
    phone_width: Option<u32>,
}

fn options() -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => options.check = true,
            "--fit" => options.fit = true,
            "--only" | "--except" | "--phone-width" => {
                let value = args.next().ok_or_else(|| format!("{arg} needs a value"))?;
                match arg.as_str() {
                    "--only" => options.only.push(value),
                    "--except" => options.except.push(value),
                    _ => {
                        options.phone_width = Some(
                            value
                                .parse()
                                .map_err(|_| format!("--phone-width {value}: not a width"))?,
                        );
                    }
                }
            }
            _ => options.dir = Some(PathBuf::from(&arg)),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(options) => options,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if options.check && options.phone_width.is_some() {
        eprintln!("--phone-width draws a width to look at, and is not checked: leave it out");
        return ExitCode::FAILURE;
    }
    let dir = options
        .dir
        .clone()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots"));
    if let Ok(mut font_system) = graphics::text::font_system().write() {
        for bytes in fonts::FILES {
            font_system.load_font(std::borrow::Cow::Borrowed(bytes));
        }
    }
    let Some(mut renderer) = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
        fonts::BODY,
        iced::Pixels(16.0),
        Some("tiny-skia"),
    )) else {
        eprintln!("the tiny-skia renderer did not start");
        return ExitCode::FAILURE;
    };
    let (only, except) = (&options.only, &options.except);
    if options.fit {
        let width = options.phone_width.unwrap_or(NARROWEST_PX);
        return if fit(&renderer, only, except, width) {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        };
    }
    let mut images = desktop();
    images.extend(phones(options.phone_width.unwrap_or(PHONE.0)));
    let names: Vec<String> = images.iter().map(|image| image.name.clone()).collect();
    let mut failed = false;
    if options.check {
        // The phone screens are held to 360 logical pixels as well.
        failed |= fit(&renderer, only, except, NARROWEST_PX);
    }
    if !options.check
        && let Err(e) = std::fs::create_dir_all(&dir)
    {
        eprintln!("{}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    let print = std::env::var_os("TAWARA_SCREENSHOTS_PRINT").is_some();
    for image in images {
        if !selected(&image.name, only, except) {
            continue;
        }
        let Image {
            name,
            size,
            mut model,
            to_end,
            focus,
            scroll,
        } = image;
        // The window the screen is drawn in is this wide.
        #[allow(clippy::cast_precision_loss)]
        {
            model.width = size.0 as f32;
        }
        let rgba = draw(&mut renderer, &model, size, to_end, focus, scroll);
        let path = dir.join(format!("{name}.png"));
        if options.check {
            match decode(&path) {
                Ok((drawn_size, committed)) if drawn_size == size && committed == rgba => {
                    println!("same     {name}");
                }
                Ok((drawn_size, committed)) => {
                    let differing = committed
                        .chunks(4)
                        .zip(rgba.chunks(4))
                        .filter(|(a, b)| a != b)
                        .count();
                    let fresh = std::env::temp_dir().join(format!("{name}.drawn.png"));
                    let bytes = encode(&rgba, size);
                    let _ = bytes.as_ref().map(|b| std::fs::write(&fresh, b));
                    eprintln!(
                        "DIFFERS  {name}: committed {}x{}, drawn {}x{}, {differing} pixel(s) \
                         differ; what was drawn is at {}",
                        drawn_size.0,
                        drawn_size.1,
                        size.0,
                        size.1,
                        fresh.display()
                    );
                    if print && let Ok(b) = &bytes {
                        print_image(&name, b);
                    }
                    failed = true;
                }
                Err(e) => {
                    eprintln!("MISSING  {name}: {e}");
                    if print && let Ok(b) = encode(&rgba, size) {
                        print_image(&name, &b);
                    }
                    failed = true;
                }
            }
        } else {
            match encode(&rgba, size).and_then(|bytes| {
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
            }) {
                Ok(()) => println!("wrote    {}", path.display()),
                Err(e) => {
                    eprintln!("{e}");
                    failed = true;
                }
            }
        }
    }
    // Unfiltered, an image in the folder that no sample draws is stale.
    if options.check
        && only.is_empty()
        && except.is_empty()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "png") {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !names.contains(&stem) {
                    eprintln!("STALE    {stem}: no sample draws it");
                    failed = true;
                }
            }
        }
    }
    if failed {
        eprintln!(
            "\nThe screenshots in {} are not what the screens draw. Run `cargo run -p \
             tawara-app --example screenshots` and commit the result.",
            dir.display()
        );
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// A drawn image, printed in a log between markers, so that CI's check can
/// show what it drew.
fn print_image(name: &str, png: &[u8]) {
    println!("----- BEGIN {name}.png -----");
    println!("{}", base64(png));
    println!("----- END {name}.png -----");
}
