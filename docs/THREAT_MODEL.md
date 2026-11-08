# Threat model

This document lists what Tawara protects, from whom, the controls that do it
and where they are in the source, how each control is checked, and what
remains. It covers this repository, the mobile shells (the Tawara-mobile
repository) and the parts of the wallet library and the iced fork that
Tawara relies on. The library's invariants (I1, I2, I4) are those of its
`docs/specification.md` at the pinned commit.

Nothing here is a new control. A threat with no protection, or a protection
with no check, is a gap, listed in section 5.

**Status** values used in section 3:

- **verified**: a named test or CI check exercises the control. Where that
  check runs only on an emulator, a simulator or a desktop host, the row says
  so. No check has yet been run on a physical phone.
- **source only**: the control is in the source and was reviewed, but
  nothing exercises it.
- **owed**: the control, or the check it needs, does not exist yet.

## Screens named here

| name | what it is |
|---|---|
| Create | choose a folder and password for a new store |
| Phrase | the new recovery phrase, shown once |
| Confirm words | three words of the phrase typed back before the store is written |
| Restore | a store made from an existing phrase |
| Unlock | the password, or the device unlock on iOS |
| Review | the library's "NOT SIGNED" page for a spend laid out |
| Sent page | a signed spend, its bytes and the three facts about them |
| Account page | one account's state and its actions |
| Re-sign | the reserved spend typed again and signed again byte for byte |
| Submit | saved signed bytes sent again, or a saved copy of a reverted settle |
| Recovery | every account's report, the resync of an account the chain shows ahead, and the confirmed rollback of one it shows behind |

## 1. Scope and assets

| asset | where it lives | what its loss or misuse costs |
|---|---|---|
| **The recovery phrase and the master seed** | The phrase exists only while a store is created or restored: in the worker's `PendingCreate` (`crates/wallet-core/src/worker.rs`), in `PhraseForDisplay` for display (`crates/wallet-core/src/secret.rs`), and for a restore only as the `SecretText` argument of `create_from_phrase`. The master seed is in the open store and in the worker's session copy. | Everything in every account derived from it, permanently. |
| **The one-time keys and the key index** | WOTS+ keys are derived from the seed one position at a time. The index of the next unused key is in the store. | A key that signs twice leaks enough to forge (I1). Anything that rolls the index back, or lets a second copy run, leads to key reuse. |
| **The store** | `keystore/` in the default or chosen folder: the header, encrypted records, the lock file. | Encrypted under the password (Argon2id, then ChaCha20-Poly1305). A copy can be attacked offline by guessing the password, and an old copy is a rollback. |
| **The store password** | Typed on Create, Restore and Unlock, held by the worker only for the command that carries it. On iOS, where the device unlock is offered, the interface holds a copy from a password unlock until the offer is answered, the store locks, or the app leaves the foreground. A kept password is in the system's secure storage only once the person accepts, and is released only by the device's check. Anyone who knows the phone's passcode can open the wallet with the kept password. | With a copy of the store, the seed. |
| **Typed secrets** | The password, the phrase and the confirmation words in the interface's form fields while typed (`crates/app/src/app.rs`), then `SecretText` (`secret.rs`). A pasted phrase is also on the system clipboard. | As the password or the seed. |
| **The signed transaction (retry artifact)** | Returned by the worker once a spend is signed and submitted, kept in the interface for the run (`Model::signed`), saved by the person to a file or a share sheet, or copied as hex. A copy is also kept beside the keystore until the account spends again and more than 256 blocks have passed (`crates/wallet-core/src/retained.rs`). | Not secret: it authorises only the spend already made. Losing it can strand funds after a reverted settle (T9). Anyone holding it can submit that same spend, and it shows its payees and amounts. |
| **Addresses, balances and history** | On screen; asked of the node. The preferences file holds the node's URL, the store's folder and the device-unlock setting (`crates/wallet-core/src/preferences.rs`). `store-id` beside it is 32 random bytes that name no secret (`crates/wallet-core/src/store_identifier.rs`). | Privacy: who pays whom, how much is held, and that a store's accounts belong together. |
| **The node connection** | TLS to the chosen node. | The node's answers drive reconciliation, spend amounts, settling and every balance shown. |
| **The build** | The library and iced fork pins, the lockfile, the toolchain pin, CI. | A changed dependency runs with the seed in reach. |

Out of scope: the library's cryptography and its own proofs (this document
relies on its invariants and says where); the node's own security and the
network's consensus; and a person who knowingly gives their phrase or
password away.

## 2. Adversaries and trust boundaries

### Trust boundaries

1. **The worker and the interface.** One thread, `tawara-wallet-core`, owns
   the `Keystore` and the `Wallet`. The interface sends commands and receives
   events; events carry no secret except a new phrase. `crates/app` and
   `crates/desktop` never name the library
   (`wallet_core_and_the_interface_stay_apart` in
   `crates/wallet-core/tests/policy.rs`).
2. **The process and its platform.** The operating system, other users,
   other apps, the clipboard, the input method, accessibility services,
   screen capture, backups and sync. The store's file checks are the
   library's.
3. **The wallet and the node.** TLS authenticates the node's name, not what
   it says. The node is trusted for chain facts: balances, the tip, which
   address an account is at, whether a spend landed (I4).
4. **The application and the library.** The library is trusted, pinned by
   full commit hash and never patched.
5. **The source and the build.** Dependencies, the iced fork, the toolchain,
   the CI runners, packaging and signing.
6. **Tawara and the person.** The person decides which node to trust, where
   to keep the store, what to type, and whether to read a refusal.
7. **The camera and the codes it reads.** Anyone can print a code or place
   one over another, so scanned text is untrusted until it is checked as a
   destination.

### Adversaries

| # | adversary | can | wants |
|---|---|---|---|
| A1 | **A thief with an unlocked device**, or anyone at an unattended desktop | use the open wallet until it locks; read the screen | to spend; to see the phrase |
| A2 | **A thief with a locked device**, a lost laptop, a disposed disk | read the disk; on a phone, only what data protection leaves readable | to copy the store and guess the password |
| A3 | **Malware on the desktop** running as the same user | read the store's files, the clipboard and the screen; log keys; read process memory | the password, the phrase, a swapped destination |
| A4 | **Another app on a phone** | read the clipboard within platform limits; use accessibility services if granted; receive shared text | the phrase, the password, the artifact, addresses |
| A5 | **Shoulder surfing and screen capture**: a person nearby, a recording, screen sharing, the app switcher's snapshot | see what is drawn | the phrase; balances and addresses |
| A6 | **Backups and cloud sync** (a mechanism, not an adversary) | keep and restore older copies of the store | (rolls back the key index) |
| A7 | **A malicious or broken node**, or a network attacker in front of one | answer anything; refuse; delay; learn every tag asked about and the asker's address | to mislead, freeze accounts, link accounts, hide a rollback |
| A8 | **A reorganisation** | revert a block a settle relied on | (leaves a store that recorded a settle the chain no longer shows) |
| A9 | **The supply chain**: a crate, the iced fork, the library's repository, the toolchain, CI runners, packaging | put code in the build | the seed, the password, a changed destination |
| A10 | **The person's own mistakes** | lose the phrase, run two wallets on one phrase, paste the wrong destination, ignore a warning | (accidental loss of funds) |
| A11 | **Whoever prints or places a scanned code** | make a code hold any text and put it where the person scans | a payment to their own account |

## 3. Threats, protections and status

### 3.1 Key reuse

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T1 | **A key signs twice within one store**: a bug, a repeated click, a spend queued behind a lock, a plan used twice. | The library: only two routes produce a signature; `sign_spend` consumes a receipt that exists only after the advanced index is durable (I1, I2); its raw signer is crate-private and `raw-backend` is denied (`deny.toml`) and kept off by the feature pin. The worker: one pending plan, taken by `confirm_send` and checked by id; a spend is never stopped between its reservation and its submission, and a spend still queued behind a cancel, a move to the background or the last handle's drop does not sign. Re-signing reproduces the reserved bytes and nothing else. | The library's suites (`invariants`, `keystore`, `recon`, `signing`, `spend`, `kat`), including `signature_is_not_released_before_the_index_is_durable`. `tests/worker.rs`: `resigning_reproduces_the_reserved_bytes_and_nothing_else` and the cancel, background and drop tests. `policy.rs`: `library_is_pinned_by_full_commit_hash_and_never_patched`. `cargo deny` in CI. | I2's crash model is a kill at a syscall boundary. Power loss, an `fsync` that reports success without flushing, and page loss are outside it. A phone whose battery dies mid-commit is in that class. | verified (library and desktop hosts); on a phone, owed |
| T2 | **A restored older copy of the store** rolls back the index and any open reservation (Time Machine, File History, shadow copies). | Every account is reconciled against the chain before anything signs; an account the chain has moved past is set aside, and nothing signs from it until it is resynced to the key the chain shows, which happens on its own (I4). | `tests/worker.rs`: `a_diverged_store_is_reopened_and_advanced_on_acknowledgement`, `an_account_set_aside_at_open_settles_and_rejoins_once_it_reconciles`. | A copy taken before a spend was reserved, restored while that spend has not landed, reconciles as in sync, and a new spend signs with the key the lost reservation used. Accepted. No user-facing note about backup tools exists. | verified |
| T3 | **Backups and transfers on a phone** copy the store elsewhere. | Android: the store is in `no_backup/`, with `android:allowBackup="false"` and `dataExtractionRules` excluding every domain from cloud backup and device transfer. iOS: `NSURLIsExcludedFromBackupKey` on the store's folder. A phone has no other place for the store (T24). | Tawara-mobile CI: the manifest's `allowBackup`, the rules attribute, `no_backup` mode `drwx------`, and the folder's backup attribute on the simulator. | The rules' contents are checked by no script. Whether backups actually skip the folder is not observed. | verified (emulator and simulator, attributes only); effect on a device, owed |
| T4 | **The store in a cloud-sync folder on the desktop**, which copies and may restore it. | The default folders are outside sync folders: `%LOCALAPPDATA%` (never Roaming), `~/Library/Application Support`, `$XDG_DATA_HOME` (`crates/wallet-core/src/location.rs`). A chosen folder inside iCloud Drive, a macOS `CloudStorage` provider, OneDrive, Dropbox, Google Drive, Box, pCloud, Nextcloud, ownCloud, MEGA, Synology Drive, Seafile or a roaming profile raises a warning, on the path as given and with links resolved. It warns and does not refuse. | `location.rs` tests for the defaults and the warnings. | The person may keep the folder after the warning. A provider not on the list is not detected. The tests do not cover every listed provider. | verified (in part) |
| T5 | **Two live installs on one phrase**: a phone and a desktop restored from the same phrase, or a copied store folder. Each signs the same position before either spend lands. | More than one install on a phrase is intended (owner, 2026-11-08). When one install's spend lands, the other finds the account ahead and resyncs it to the key the chain shows on its own (`App::resync`), so the next spend from either signs a fresh key. The divergence summaries name another install of the same phrase as a cause (`crates/app/src/screens/wallet/report.rs`). | `crates/app/src/app.rs`: `an_account_the_chain_shows_ahead_is_resynced_once_for_each_index`, `a_resync_answers_into_the_store_and_leaves_the_page_shown`. | Two installs that each sign before either spend lands sign the same key position twice. Accepted by the owner (2026-11-08). | accepted |
| T6 | **A key index moved wrongly**: forward past keys a live install still needs, or backward onto a key that has signed. | Forward: the resync, on its own or from Recovery, moves an account only to the index the library's live report matched to the chain's address, re-checked at the write. Backward: only from Recovery, only for an account the chain shows behind, to the index the chain shows, after the person confirms a warning that rolling back can sign a one-time key a second time; the library re-checks the live report at the write and refuses while a spend is reserved (I4). There is no "show recovery phrase" or "export keystore" control. | `app.rs`: `recovery_shows_every_report_and_resyncs_to_the_index_the_chain_shows`; `tests/worker.rs`: `a_cancelled_advance_moves_no_key_index`. | A rollback confirmed while the reverted spend can still land signs its key twice. Nothing records earlier rollbacks, so a node that shows the account behind again can lead to a second rollback and a third signature of one key. Accepted by the owner (2026-11-08), behind the confirmation. No rollback is offered while a reverted settle's kept copy applies (T9). | verified (Recovery) |
| T7 | **An interrupted send**: the app ended, killed or out of background time between reservation and submission. | The index is durable before a signature exists (I2); the reservation stays open, and the spend can be re-signed byte for byte (Re-sign) or its saved bytes submitted (Submit). The iOS shell ends the process rather than be suspended holding the key. The sent page is kept for the run however it is left. | Library: `index_is_durable_before_the_receipt_under_syscall_kill_not_power_loss`. `tests/worker.rs`: `resigning_reproduces_the_reserved_bytes_and_nothing_else`. `app.rs`: `a_signed_spend_left_unsaved_is_gone_and_one_saved_is_kept_and_resigned_from`. | Power loss, as T1. | verified (kill at a syscall boundary); a kill by the system on a phone, owed |
| T8 | **Settling too early, or on a lying node's word.** | Settling takes one observation and no confirmation depth, by the library's design: too early costs one skipped key and creates no reuse; too late freezes the account. | Library `recon` and `spend` suites. | A skipped key; an account frozen until the chain shows the spend. | verified (library) |
| T9 | **A reorganisation reverts a block a settle relied on.** | The library reports the account "behind", with the retained block and every cause that fits, and refuses it. Every signed spend's bytes are kept beside the keystore until the account spends again and more than 256 blocks have passed, whatever height the node reports, and the worker sends that copy again on its own (`Command::ResendRetained`). A saved artifact can also be sent from the account page's "Submit saved artifact" (`Command::ResubmitSaved`). The library's `recon::resubmit_reverted_settle` sends a copy only if its signed part hashes to the retained digest, its signature recovers to the source key, the chain shows the settle reverted, and the balance and block-to-live still hold; it signs nothing and moves no key index. | Library: `a_reverted_settle_is_reported_behind_with_the_retained_block_and_every_cause` (scripted chain). `tests/worker.rs`: `a_saved_copy_of_a_reverted_settle_is_sent_again_and_the_account_is_read_again` and the refusal tests beside it. `crates/app/src/app/wallet.rs`: `w9_sends_a_saved_copy_of_a_reverted_settle_as_resubmit_and_the_rest_as_before`. `crates/app/src/screens/wallet/account.rs`: `a_reverted_settle_offers_submit_saved_artifact_and_no_remedy`. | Without a kept or saved copy, funds at that key wait on the chain. A store made from the same phrase elsewhere could advance to the reverted key's index and sign with it (T5). Where no copy can be sent, Recovery's confirmed rollback moves the account back to the key the chain shows (T6). | verified (scripted; not a live reorganisation) |

### 3.2 Secrets in memory, in the interface and in logs

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T10 | **An unlocked wallet left unattended** (A1). | Idle lock after five minutes (`DEFAULT_IDLE_LOCK` in `worker.rs`), configurable to 1, 2, 5, 10 or 15 minutes and never off, effective at once. The period counts from the person's input only (key presses, mouse buttons, wheel scrolls, touches); polls and timers never keep the store open, and the period is checked before each queued command. Locking drops the `Keystore`, the session's seed copy, a waiting phrase and a pending plan. Explicit Lock in the sidebar and the compact bar. | `tests/worker.rs`: `an_idle_store_locks_itself`, `touching_keeps_the_store_open_and_polling_does_not`, `the_idle_period_stops_a_walk_to_a_far_index_and_locks`. | Until the idle period ends, anyone at the device can spend: no password is asked before signing, since the library cannot check one against an open store; the confirmation is the library's "NOT SIGNED" page and a separate step. The desktop does not lock when the window is minimised or loses focus. The lock also waits for a node request in flight, up to the transport's timeout. | verified |
| T11 | **The key kept in memory while a phone is in the background** (A1, A2). | Every move to the background locks the worker and wipes typed fields (`WorkerHandle::background`). iOS: the shell holds a UIKit background task until the interface has taken the worker's answer, and ends the process if time runs out or none is granted. Android: `Suspended` asks for the lock and does not wait; after `Destroy` the process ends with `libc::_exit`. | `app.rs`: `a_shell_hears_the_move_done_only_once_the_interface_has_wiped`. Tawara-mobile CI: `background.locks` (simulator), `home.locks` and `back.locks` (emulator), each waiting for a log line. | Android may suspend the process before the lock has finished, with the key in memory. No exit path on iOS is exercised by any check. | verified (simulator and emulator, log lines); on a device, owed |
| T12 | **Typed secrets left in the interface** across a lock, a move to the background, a screen left, or the worker stopping. | `SecretText::take` moves a field's buffer into `Zeroizing<String>` and empties the field. `wipe` takes and drops, so the bytes are zeroed. `wipe_typed` covers every screen that holds a secret and runs when a screen is left and on every move to the background. Text from a field drawn before the latest move is refused and wiped (`Typed::drawn`). The stopped screen zeroes what the screen before it held; the worker's queue is dropped before `Stopped` is sent. The password copy held for the device unlock's offer is dropped by the same events (`release_device`). | `secret.rs`: `take_moves_the_buffer_and_empties_the_field`. `app.rs`: `a_stopped_worker_takes_the_secrets_on_the_screen_with_it` and the background and lock tests. | While typed, a secret is a plain `String` in the form, and each keystroke delivers the whole value in a new `String` inside a `Typed` message; only the final take is a move. The forms, `Screen` and `Model` derive `Debug` over plaintext fields (nothing prints them). iced's text field copies its value into a list of graphemes on each draw, freed without zeroing, and the input method keeps what it keeps. | verified (take and wipe); typing-time copies, owed |
| T13 | **Memory read from outside**: swap, a core dump, a hibernation image, a debugger. | None. The library leaves memory unlocked and core dumps on, by design: the mitigations need `unsafe`, and an attacker who can read process memory can also read the store and wait for the password. | — | As stated. On a phone the platform sandbox is the guard. | owed |
| T14 | **Secrets in logs, panic text or on disk.** | `SecretText`, `PhraseForDisplay`, `Typed`, the paste read (`Clip`) and the worker's event wrapper print nothing of their text in `Debug`. No workspace crate uses a logging crate; the desktop binary prints only a start-up error. Preferences hold no secret and sit beside `keystore/`, never inside it. Release builds unwind, so `Drop` scrubs still run on a panic. | `secret.rs`: `debug_shows_neither_text_nor_length`. `app.rs`: `a_secret_typed_is_never_in_a_message_log`, `a_pasted_read_is_never_in_a_message_log`. `preferences.rs` tests. `policy.rs`: `release_profile_keeps_the_library_scrubs`. | The derived `Debug` of T12. Panic messages from the library or iced go to stderr. The dependency graph includes `log` and `tracing`, whose output is not inventoried. The phone shells log lifecycle lines to logcat and the system log. | verified (the types named) |
| T15 | **Weak or partial randomness.** | `getrandom` reads the operating system's generator (`BCryptGenRandom`, `getrandom`/`/dev/urandom`, `getentropy`, `CCRandomGenerateBytes`). A failed draw refuses with nothing created or opened (`crates/wallet-core/src/entropy.rs`). | `entropy.rs` tests. | None known. | verified |

### 3.3 The recovery phrase and the password on screen and at the keyboard

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T16 | **The phrase seen or captured while shown** (A5). | Shown once, at creation, behind a confirmation of three words; never shown again, and no control exists to show it. Android: `FLAG_SECURE` on the whole app, set before the first frame. iOS: a cover with no wallet content over the key window on `WillResignActive`, removed on `DidBecomeActive`. While iOS reports the screen as captured, the phrase is blanked, and after a screenshot taken while the words are drawn a warning takes their place until "Show the phrase" is pressed (`crates/app/src/host.rs`, `crates/app/src/screens/first_run.rs`); the shell reports capture state. | Tawara-mobile CI: `flag_secure` reads `SECURE` from `dumpsys`; `background.covered` reads `cover added` in the log; `scene.window` reads the key window. | The desktop has no protection: screenshots, recording and screen sharing see the phrase. No check shows that an Android screenshot, recording or recents thumbnail is blank, or that the iOS switcher's snapshot is. iOS capture state can read inactive while a recording runs, so blanking is best effort. On Android, `FLAG_SECURE` does not reliably stop every recording on API 30 and lower. | Android and iOS: verified (flag set, cover added); effect on a device, owed. Desktop: owed |
| T17 | **The input method learns or suggests a password or phrase** (A4). | iced marks a secure field's input purpose as secure, which reaches winit as `ImePurpose::Password`. On Wayland it becomes a sensitive-data hint. On Android winit drops it, and on iOS it is ignored. | — | On both phones the keyboard is an ordinary one for the password and phrase fields and may suggest and learn. | owed |
| T18 | **Accessibility services read a secret field** (A4). | No mechanism. iced at this pin publishes no accessibility tree, so a service likely sees no field at all; that is an inference, not a check. | — | Unknown until checked. The same inference means a screen reader cannot use Tawara. | owed |
| T19 | **Copy or cut of a secret field.** | iced refuses copy and cut in a `secure(true)` field. The password, confirmation-word and restore-phrase fields are secure (`crates/app/src/screens/first_run.rs`, `crates/app/src/ui.rs`). Paste into them is allowed. | No test. | — | source only |
| T20 | **The phrase on the clipboard.** | No copy control on Phrase or Confirm words. | — | A phrase pasted into Restore stays on the system clipboard; Tawara wipes its own copy of the read, not the system clipboard (`crates/app/src/paste.rs`). | source only; the pasted phrase, owed |

### 3.4 The store at rest

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T21 | **A copied store attacked offline** (A2, A3). | Argon2id at the library's recommended cost, 64 MiB and three passes, one lane (`Kdf::RECOMMENDED`); ChaCha20-Poly1305 with the header as associated data; a wrong password and any tampering give one refusal. The test-only cheap KDF never appears outside test code. Password floor of 12 characters (`MIN_PASSWORD_LEN`). | Library keystore suite; `policy.rs`: `cheap_kdf_appears_only_in_test_code`. | A weak password of 12 or more characters is guessable; there is no complexity rule. One lane, not RFC 9106's four, because the library builds Argon2 without threads; the cost was measured on a desktop, not a phone. | verified |
| T22 | **A thief reads a locked phone's storage** (A2). | iOS: `NSFileProtectionComplete` on the store's folder and its contents at launch. Android: the app's private storage, under the system's file encryption. | Tawara-mobile CI: `store.protected` shows only that setting the attribute succeeded; the simulator does not enforce protection classes. | iOS protection is applied at launch only; whether files written later inherit the folder's class is not established, and nothing reads the class back. Android's file encryption is not shown to keep the store unreadable on a locked phone. | verified (call succeeds on the simulator); effect on a device, owed |
| T23 | **Other users or processes read or redirect the store** (A3). | The library's checks on every open and create: owner and mode on Unix (Android and iOS included), access lists on Windows, a linked store directory refused. It makes the store folder `0700` and files `0600` or a private access list. Folders Tawara makes are `0700` on Unix (`prepare_parent` in `worker.rs`; `preferences.rs`). | `tests/worker.rs`: `a_store_the_library_will_not_write_leaves_its_phrase_waiting` (a `0777` folder refused); the library's keystore suite. | Same-user malware can read the store and log the password; nothing on a desktop prevents it. The library on a real phone's file system is unchecked. | verified (desktop hosts, emulator, simulator) |
| T24 | **A store placed outside the protected folder on a phone.** | No folder field on Create, Restore or Unlock on a phone; the store is always `keystore/` in `Host::private_dir`. A folder outside it is refused before anything is sent; `..` and links do not lead out (`crates/app/src/app.rs`). | `app.rs` confinement tests. | Enforced in `crates/app` only; `wallet-core`'s commands accept any folder. | verified |
| T25 | **Two writers on one store**: two windows, or Tawara and the command line. | The store's lock is held while the wallet is unlocked; the library's refusal is shown on Unlock. | `tests/worker.rs`: `unlocking_refuses_a_wrong_password_a_missing_store_and_a_held_one`. | The lock is meaningful on a local file system only; a store on a network share is not guarded. | verified |

### 3.5 The node

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T26 | **A network attacker reads or rewrites node traffic** (A7). | `https://` required; `http://` only to `127.0.0.0/8`, `::1` and `localhost`, with no override (`crates/wallet-core/src/node.rs`). The library's transport: rustls with `ring` and the bundled `webpki-roots`; redirects off; timeouts; request and reply size caps. Platform roots are not used, so an installed inspection root is not trusted. A second TLS stack in C is banned in `deny.toml`. | `node.rs`: `loopback_is_the_only_plaintext_accepted`; `tests/worker.rs`: `the_node_is_chosen_https_or_loopback_only`. | A certificate wrongly issued by a root in Mozilla's store. No pinning. | verified |
| T27 | **A malicious or broken node** (A7). | The default node is `https://api.mochimo.org`; the person can choose another or none. Changing or clearing the node detaches the open store and drops a pending plan. The library: a returned address must begin with the tag asked for; a submit's echoed id must match the one computed locally; the store's index never moves back, so a lying node cannot make one store reuse a key (I4). Replies without the tip's timestamp or the genesis block are refused, and an impossible block time is never a panic. Text from the node is sanitised before display (`crates/wallet-core/src/explorer.rs`). "Account not found" is never read as an account that does not exist. "Send to this account" is offered only for an address the person typed. A balance not known is never counted as zero (`crates/wallet-core/src/view.rs`). | `tests/worker.rs`: `clearing_or_changing_the_node_stops_the_open_wallet_using_the_old_one`; `explorer.rs` and `tests/explorer.rs`; `view.rs` tests. | Balances, the tip, history and settlement are the node's word; a lying node can make the wallet refuse, skip a key or settle early, and can show false balances and incoming payments. Combined with a rolled-back copy (T2) or a second wallet (T5), a node that hides the chain's position can let a key sign twice (I4). There is no second node to compare with. | verified (the protections named) |
| T28 | **The node learns who the person is and what they hold** (A7). | The explorer reads only when asked, never on a timer. | — | The node sees every tag of the store on each open and refresh, and so learns that they belong together, with the asker's network address and timing. The library's proxy features are banned in `deny.toml`, so routing is only at the system level. | owed |

### 3.6 Signed transactions and the clipboard

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T29 | **The signed bytes are lost** before the spend settles. | The sent page opens with the three facts about the bytes, which are never softened. Desktop: "Save artifact" writes to Downloads under a new name, never overwriting, and shows the path (`save_artifact_in` in `worker.rs`); "Copy hex". A banner stays until the bytes are saved, and Dismiss asks for confirmation. A copy is also kept beside the keystore until the account spends again and more than 256 blocks have passed (T9). Phone: `Host::share` asks the system to open its share sheet; an `Err` is a refusal, an `Ok` says only that it was asked, so the spend still counts as not saved (`crates/app/src/host.rs`). | `report.rs`: `the_sent_summary_states_the_three_facts`; `crates/app/src/app/wallet.rs` save tests; `worker.rs`: `save_artifact_never_overwrites`. | Leaving the sent page drops unsaved bytes from the interface; the kept copy remains. On a phone, saving depends on the shell providing the share sheet. | desktop: verified. Phone: owed |
| T30 | **The signed bytes reach someone else**: the clipboard, Downloads, a share target. | The bytes authorise only the spend already reserved and submitted. iOS's share sheet excludes Copy. | — | Anyone holding them can submit that same spend again (Submit needs no store), and they show its payees and amounts. On Android the share chooser is the system's own window, outside `FLAG_SECURE`: it previews the text, and its "Copy text" action copies it with no sensitive mark, which Tawara cannot set. Accepted for the sheet's lifetime. | source only |
| T31 | **The clipboard is read or swapped** (A3, A4): a copied destination replaced by an attacker's. | Copy buttons copy only public material: destinations, transaction ids and the signed hex. On a phone every Copy button goes through the shell's protected write: Android marks it sensitive (`ClipDescription` `IS_SENSITIVE`, honoured from API 33); iOS makes it local-only, expiring after two minutes. Paste buttons read only on a tap, and a late read, or one for a form no longer shown, is dropped. The review lists each destination to check against its payee before signing. | Tawara-mobile CI: the copy is on the simulator's pasteboard and gone after 121 s. `app.rs` paste tests; `send.rs` tests. | Desktop copies have no expiry or sensitivity mark. Android's mark is not checked by any script, does nothing below API 33, and Android has no expiry. Copy or cut from a hardware keyboard in a non-secure field goes through iced's clipboard, which on a phone sets no protection. A swapped destination is caught only if the person checks it on the review. | iOS: verified on the simulator for the expiry only. Android: source only. Desktop: owed |
| T32 | **A signed transaction broadcast by accident.** | On a phone, Return in the Submit field does nothing (`RETURN_BROADCASTS` in `crates/app/src/screens/wallet/send.rs`). | `send.rs`: `return_in_the_signed_bytes_field_submits_on_the_host`, `return_in_the_signed_bytes_field_does_nothing_on_a_phone`. | On the desktop, Return on Submit and the Submit button send at once with no confirmation, in both modes. That the constant is false on phones is checked only by review and the phone builds. | verified (desktop host) |

### 3.7 The person's own mistakes

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T33 | **The phrase or the password is lost.** | The phrase is confirmed by three words before the store is written; Restore first says which wallets' phrases are compatible: a phrase from any other wallet derives a different wallet, with no error. | `tests/worker.rs` create tests. | Neither can be recovered. There is no password-change control, because the library does not support one. | verified |
| T34 | **A wrong destination or amount.** | Destinations are checked by the library's parser, with its checksum; the review lists every destination in wire order with the library's figures, titled for an emptying spend when the change is zero; signing is a separate step. An account chosen by name stays the chosen one. | `app.rs` and the planned-page tests. | The person does not check. | verified |
| T35 | **Pressing on past a refusal.** | Five states fail closed, each with a banner, a summary and the library's page; no button resets, deletes or restores the store or moves an index to make a refusal go away; the library's protective text is shown in its own words. | `report.rs` tests; `crates/wallet-core/src/sample.rs` tests. | — | verified |
| T36 | **A counterfeit Tawara** installed in place of the real one. | The packaging scripts (`packaging/macos.sh`, `packaging/linux.sh`, `packaging/windows.sh`) build the packages from pinned tools. Signing and notarisation run outside CI, where the keys are. | The packaging job checks the unsigned packages' contents (`packaging/check.sh`). Nothing checks a signature. | Until signed packages are published, a user cannot tell a genuine build. | owed |

### 3.8 The supply chain

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T37 | **A changed or malicious crate.** | `Cargo.lock` committed and every resolving command run `--locked`. `cargo deny`: advisories with `unmaintained = "all"` and `yanked = "deny"`, each ignore with a reason; permissive licences only; wildcards denied; unknown registries and git sources denied, with the library and the iced fork the only git sources; feature bans on `ureq`, `argon2`, `serde_json`, `native-tls`, `openssl-sys` and the library's `raw-backend`; `digest` 0.11 denied. | `cargo deny --locked check` in CI, with cargo-deny 0.20.2 built from crates.io with `--locked`. | A new advisory is seen only when CI runs. Updates are reviewed by hand. `deny.toml` has no rule against `rustls-native-certs` or `rustls-platform-verifier`, so the choice of bundled roots is held by review only. | verified |
| T38 | **The library changed under the wallet.** | Pinned by full hash, never patched, vendored or overridden; no `[patch]`, `[replace]`, `vendor/` or cargo-config override; the licence text pinned by hash. | `policy.rs`: `library_is_pinned_by_full_commit_hash_and_never_patched`; `cargo deny` (a changed licence text fails it). | Trust in the library's repository and its maintainers. | verified |
| T39 | **The iced fork changed under the wallet.** | Pinned by full hash to a release tag; every fork package at that commit, once; one `winit`, from the fork; `default-features = false` with the listed features (`deny.toml`). | `policy.rs` fork-pin tests. | Trust in the fork's repository. Each move of the pin also needs a manual desktop check on macOS, Windows and Linux, which no job runs. | verified |
| T40 | **CI subverted.** | No third-party action: no `uses:` line; plain commands; `permissions: {}`; an anonymous clone fetching the commit by SHA; CRLF checkouts refused; nothing uploaded. The toolchain is pinned with its components (`rust-toolchain.toml`). | The workflow itself. | Runner images are `-latest`; `apt-get` installs are unpinned; `rustup` comes from the image. A screenshot that differs is printed as base64 into the public log; it is drawn from fixed sample data and the published test vector, not a secret. | verified (the commands and the CRLF refusal); source only (no `uses:` line, `permissions: {}`, no upload) |
| T41 | **The release profile undone**, skipping `Drop` scrubs or letting balances wrap. | `panic = "unwind"` and `overflow-checks = true` in the root manifest; any override that turns overflow checks off or sets `panic = "abort"` fails a test. | `policy.rs`: `release_profile_keeps_the_library_scrubs`. | Overrides from outside the repository (`$CARGO_HOME`, `RUSTFLAGS`), which CI does not set. | verified |
| T42 | **Packaging and signing** subverted or absent. | Packaging is in the repository (`packaging/`, `.github/workflows/packages.yml`): pinned tools, each download checked against its SHA-256, the glibc floor checked, and a build job with no token permission and no secret; only the release job, on a tag, may write a release. Signing is not in CI. The Windows installer is made from an NSIS archive whose hash is recorded. | `packaging/check.sh` and the packaging job, on unsigned packages. | As T36. NSIS publishes no checksum, so the archive's hash was trusted on first use, cross-checked against SourceForge's published SHA-1 and MD5. | owed |

### 3.9 The device unlock (iOS)

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T43 | **The kept password opens the store for whoever passes the phone's check.** The system prompt may accept Face ID, Touch ID or the passcode, so anyone who knows the passcode can open the wallet. A face or a finger can be compelled. | The unlock is off until the person accepts the offer, made once, after a password unlock (`make_offer`, `answer_offer` in `crates/app/src/app.rs`). The password is copied only from the unlock's send until the answer, and `release_device` drops it on a lock, a move to the background and a stop. It is kept under the store identifier (`store_identifier.rs`); an item labelled with another identifier, or with none to match, is deleted and not used. A kept password the store refuses is deleted and the unlock turned off. Turning the unlock off deletes the password first. A declined check changes nothing, and a late answer is ignored, its password wiped. The shell keeps the item in the Keychain under `.userPresence` and `kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly`. The password field stays on Unlock. | `app.rs`: `the_device_unlock_is_offered_once_after_a_password_unlock`, `declining_the_offer_keeps_nothing`, `a_device_unlock_sends_unlock_with_the_password_it_released`, `a_wrong_kept_password_is_forgotten_and_the_form_is_shown`, `a_mismatched_identifier_forgets_the_password_and_shows_the_form`, `a_declined_check_keeps_the_password`, `turning_the_unlock_off_forgets_the_password`, `a_password_released_after_the_app_left_the_foreground_is_not_used`, `the_offer_waits_on_the_wallet_and_a_lock_takes_it_away`. `host.rs`: `a_released_password_and_an_answer_print_no_text`. `store_identifier.rs` tests. | A passcode, or a compelled face or finger, opens the wallet with the kept password; a passcode can be watched or shared. The shell's own copies of the password are not zeroed by Tawara, since the calls carry text. A Keychain item that survives an uninstall is reached by no check until a later keep replaces it. | verified in the app with a fake shell; the Keychain side and the device, owed |

### 3.10 QR scanning on a phone

| # | threat | protections | checked by | residual | status |
|---|---|---|---|---|---|
| T44 | **A scanned text fills a destination row it should not** (A11): not a destination, too long, not UTF-8, or scanned for a row or page that has since changed. | The text is trimmed, bounded at 128 bytes, read as UTF-8 and checked by `parse_destination` (`crates/app/src/scan.rs`). It fills only the row the scan began for, and only while the page, its form and that row are unchanged (`scan_fits` and `scanned` in `crates/app/src/app/wallet.rs`). A refusal shows under that row's address. No other message takes the text, so it never reaches a password or phrase field. The scanner page draws the camera frame and none of the code's text. | `scan.rs` tests; `app.rs` scan tests; `send.rs` scanner-page tests. | A valid destination that is the wrong one fills its row; only the review catches it (T34). | verified in the application; the shells' scanners, owed |
| T45 | **The camera keeps reading after the scan is left** (A1, A2): the page is left, the store locks, or the app moves to the background. | The scan stops when the page or its row changes; on a lock; on a move to the background, where the shell stops the camera on its own thread (`host::left_foreground`) and the application drops the scan; on Back on the scanner page (`stop_scan`); and when the shell's reply arrives. A reply to an ended scan is dropped. | `app.rs`: `a_read_stops_the_camera_as_its_reply_is_taken`, `leaving_the_page_locking_or_backgrounding_stops_the_camera`, `back_on_the_scanner_page_cancels_the_scan_and_keeps_the_form` and related tests. | The shells must stop the camera when told, which only a device shows. | verified in the application; on a device, owed |

## 4. Platform specifics

### Desktop: Windows, macOS, Linux

- **Store.** `%LOCALAPPDATA%\Tawara\keystore` (never Roaming);
  `~/Library/Application Support/Tawara/keystore`;
  `$XDG_DATA_HOME/tawara/keystore` or `~/.local/share/tawara/keystore`.
  Another folder may be chosen; a synced one raises a warning (T4). Unix
  folders Tawara makes are `0700`; on Windows they inherit the parent's
  access list, and the library makes the store's own folder private.
- **Backups.** Time Machine, File History and shadow copies may keep copies
  of the store (T2).
- **Locking.** The idle period and Lock only. Minimising the window or
  switching to another does not lock; `crates/desktop` never calls
  `left_foreground`.
- **Screen.** No protection from screenshots, recording or screen sharing.
- **Clipboard.** iced's write, with no expiry or sensitivity mark. Keyboard
  paste only; no Paste buttons.
- **Input methods.** On Wayland the secret fields are marked sensitive to the
  input method. On X11, macOS and Windows this is not known to reach the
  input method; not checked.
- **Same-user malware** can read the store, the clipboard and the screen and
  log keys (A3). The store's encryption and the password are what remain.
- **Verified** on every pull request: build, clippy, tests, docs and
  `cargo deny` on Windows, macOS and Linux; on Linux, a start-up check on
  wgpu and on tiny-skia, which passes if the process lives and its log holds
  no panic (a panic on another thread does not end the process).

### Android

- **Store.** `no_backup/keystore`, made `0700`; `allowBackup="false"` and
  `dataExtractionRules` excluding every domain. No folder field.
- **Screen.** `FLAG_SECURE` on the window before the first frame, for the
  whole app, except the system's share chooser, which it does not cover.
- **Locking.** `Suspended` (the window destroyed) locks and wipes, without
  waiting. Back moves the task to the background and never finishes the
  activity, and so locks, except on pages 2 to 4 of Send and on the scanner
  page while the application draws its preview: there Back steps back a
  page, or cancels the scan, inside the application, keeping the form, and
  does not lock. After `Destroy` the process ends with `libc::_exit`,
  without waiting for the lock.
- **Clipboard.** Copy buttons mark the copy sensitive, honoured from API 33;
  Tawara's minimum is API 26. No expiry and no local-only copy.
  Hardware-keyboard copy in non-secure fields is unprotected.
- **Keyboard.** Ordinary; the secure purpose is dropped (T17).
- **Configuration changes.** The API 35 set is handled, so a change of
  country code, network code or grammatical gender does not recreate the
  Activity.
- **Camera.** `android.permission.CAMERA`, with `android.hardware.camera`
  and `android.hardware.camera.any` not required, so the app installs on a
  phone with no camera. The permission is requested through JNI and then
  polled. Frames reach the application through `host::preview_frame`. The
  camera stops when the scan ends, on a lock, on a move to the background,
  and on Back on the scanner page (T45).
- **Verified** in Tawara-mobile CI on an x86_64 API 35 emulator: the
  manifest's attributes, `SECURE` in `dumpsys`, `no_backup` mode, and the
  lock's log lines on Home and Back. Nothing on a physical phone, and
  arm64 start-up is not established by the x86_64 image.

### iOS

- **Store.** `Library/Application Support/Tawara/keystore`, made `0700`,
  with `NSFileProtectionComplete` and `NSURLIsExcludedFromBackupKey` on the
  folder. Protection is set at launch and not read back.
- **Screen.** A cover over the key window when the app is about to become
  inactive, so the switcher's snapshot shows nothing while there is a key
  window; with none, no cover is added and the log says so. While the app
  is active, the phrase is blanked when iOS reports the screen captured,
  and a warning follows a screenshot (T16).
- **Locking.** On entering the background a UIKit background task holds the
  process until the interface has taken the worker's answer; if time runs
  out, or none is granted, the process ends itself.
- **Clipboard.** Copy buttons write a local-only item that expires after two
  minutes. Hardware-keyboard copy in non-secure fields is unprotected.
- **Keyboard.** Ordinary; the secure purpose is ignored (T17).
- **Scenes.** The scene manifest and the fork's scene adoption keep the app
  launching on current SDKs.
- **Device unlock.** See T43.
- **Camera.** UIKit's own scanner screen, presented as the share sheet is,
  with `AVCaptureMetadataOutput` decoding the code; usage text in
  `NSCameraUsageDescription`. The camera stops when the scan ends and on a
  move to the background (T45).
- **Verified** in Tawara-mobile CI on the simulator: the protection call
  returns success, the backup attribute, folder mode and access list, the
  cover added, the key window in a scene, the lock on background, and the
  pasteboard expiry. The simulator does not enforce data protection.
  Nothing on a physical phone.

## 5. Known gaps

### 5.1 Gaps in controls or checks

| gap | threat |
|---|---|
| No warning before a divergence, outside Recovery, that one phrase must not be live in two wallets. | T5 |
| No desktop protection for the phrase on screen. | T16 |
| The phone keyboard may learn or suggest what is typed in secret fields. | T17 |
| Typed secrets are plain `String`s in the forms and in each keystroke's message until submitted; a field that never leaves a copy would need iced's `text_input` changed. | T12 |
| `Model`, `Screen` and the secret-holding forms derive `Debug` over plaintext. A manual `Debug` that prints `(..)`, with a test, would close it. | T12, T14 |
| No check that `FLAG_SECURE` blanks screenshots, recordings and the recents thumbnail from the first frame; CI reads only the flag. | T16 |
| No check that the iOS switcher's snapshot is covered; CI reads the cover's log line. | T16 |
| The desktop does not lock on minimise or loss of focus. | T10 |
| No mechanism or check keeps secret fields from accessibility services. | T18 |
| No test that copy and cut are refused in secret fields. | T19 |
| A phrase pasted into Restore stays on the system clipboard. | T20 |
| No inventory of what the apps log, including what the `log` and `tracing` crates in the graph receive. | T14 |
| iOS file protection is set at launch only and never read back; Android's locked-phone protection is not established. | T22 |
| The library's Unix file handling (directory handle, `flock`, `fsync`, owner and mode checks) has not run on a physical phone. | T1, T23 |
| The Android backup rules' contents are checked by no script, and no check shows a backup skips the store on either phone. | T3 |
| On phones, saving the signed bytes depends on the shell's share sheet. | T29 |
| The Android share chooser previews the signed transaction outside `FLAG_SECURE` and offers an unmarked "Copy text". | T30 |
| Phone confinement of the store is enforced in `crates/app` only, not in `wallet-core`. | T24 |
| No user-facing note about desktop backup tools. | T2 |
| Desktop copies have no expiry or sensitivity mark. | T31 |
| Hardware-keyboard copy and cut in non-secure fields on a phone bypass the shell's protected write. Routing iced's clipboard writes through `Host::copy` on phones would close it. | T31 |
| What follows if Android suspends the process before the lock has finished is not established. | T11 |
| `deny.toml` does not ban `rustls-native-certs` or `rustls-platform-verifier`. | T26, T37 |
| The iOS capture state can read inactive while a recording runs, so blanking the phrase is best effort. | T16 |
| A Keychain item left by an earlier install may survive an uninstall; only a later keep replaces it. | T43 |
| The shells' QR scanners have not run on a device. | T44, T45 |

### 5.2 Device checks not yet run

None has a recorded result on a physical phone. Each should run on an arm64
Android phone and an iPhone, on the builds to be released.

| check | for |
|---|---|
| **Text entry.** Type into the password and phrase fields: does the keyboard suggest, autocorrect or learn? Also accented and non-Latin text, composition, glide typing, and paste from another app. | T17 |
| **`FLAG_SECURE`.** From a cold launch, a screenshot and a screen recording show nothing from the first frame; the recents thumbnail is blank; the same after rotating and after a return from the background. | T16 |
| **The Android chooser.** "Save artifact" on the sent page: what a screen recording shows, what "Copy text" copies, and what each target receives. | T30 |
| **Android exit after `Destroy`.** Back from the foreground, then with "Don't keep activities" on; read the log for the lock's completion and for SIGABRT. | T11 |
| **iOS data protection.** The protection class of the store's folder and files after a commit, read on the phone; the files cannot be opened while the phone is locked. | T22 |
| **The iOS switcher snapshot** shows the cover, with no wallet content, from every screen. | T16 |
| **Accessibility.** TalkBack and VoiceOver on Create, Restore and Unlock: what is read out of the secret fields, and whether the app can be used at all. | T18 |
| **The library on a phone's file system**: create, open, commit, a second open refused, a group-writable folder and a linked folder refused. | T1, T23 |
| **Touch and first gestures.** The first tap after a cold launch and after a return; a drag that starts on a field or a button of a compact form; Paste; the keyboard shown again on a tap of the focused field. | usability |
| **The background lock**: Home, the app switcher, a call and the screen locking, each during a node request and during a spend; the wallet is locked on return and nothing typed survives. | T11 |
| **Backup and transfer leave the store out.** Android: `bmgr` and a device-to-device transfer. iOS: a device backup and its restore. | T3 |
| **arm64 Android start-up.** A cold launch on an arm64 phone, then an unlock. | Android |
| **A kill mid-send.** The system kills the app between a reservation and its submission; on relaunch the reservation is still open and the spend can be re-signed or submitted. On Android, also split screen. | T7, T11 |
| **Copy mark on Android 13 or later.** A Copy button's copy is marked sensitive, and the clipboard preview shows no text. | T31 |
| **Hardware-keyboard copy and cut.** iOS: local-only (not received over Handoff) and expiring after two minutes. Android: the sensitive mark. | T31 |
| **iPhone SE layout.** The wordmark stands clear of the status bar on a cold launch. | usability |
| **Drawing, speed and memory.** Real-GPU drawing, frame pacing, unlock time, memory and scrolling. | usability |
| **Screen recording and mirroring of the phrase (iPhone).** Start a recording from Control Center, from QuickTime over a cable, or a mirror by AirPlay or a display adapter, then open the phrase: the words must not be visible. A screenshot while the words are drawn must bring up the warning. | T16 |
| **The device unlock.** With Face ID and Touch ID: unlock with the check; with a non-matching face or finger, then the passcode; cancel; background the app during the check. Then reinstall and check whether the Keychain item survives and whether a keep replaces it. | T43 |
| **The QR scanner.** Allow and refuse the camera; scan a destination, a non-destination and an overlong code; scan from paper and from a screen; end a scan by a read, Cancel, Back, leaving the page, a lock and a move to the background, and check that the camera light goes out each time. | T44, T45 |

### 5.3 Residual risks

Accepted: the backup rollback of T2, and the choice of bundled TLS roots
(T26). The other residuals named in section 3 are:

1. T1, T7: power loss during a send is outside the library's durability claim.
2. T5: two live wallets on one phrase.
3. T9: a reverted settle with no kept or saved copy.
4. T10: no password before signing, and the desktop's lock on idle only.
5. T13: memory not locked, core dumps not suppressed.
6. T21: the 12-character floor with no complexity rule.
7. T27: the node's word on balances, settlement and the chain's position.
8. T28: what the node learns.
9. T30: the signed bytes as something anyone holding them can submit, and
   the Android share chooser.
10. T16 and T31 on the desktop.
11. T4: a synced folder kept after its warning, or a provider not listed.
12. T11: Android may suspend the process before the lock has finished.
13. T32: on the desktop, Return on Submit and the Submit button send at once.
14. T33: a lost phrase or password cannot be recovered, and there is no
    password-change control.
15. T16 on an iPhone while the app is active: blanking is best effort, and a
    screenshot cannot be blocked, only followed by a warning.
16. T43: the kept password opens the store with the passcode alone, and a
    face or a finger can be compelled. The offer says that anyone who knows
    the passcode can open the wallet, and the unlock is off until accepted.
17. T44: a code printed or placed over another can give a valid but wrong
    destination; only the review catches it.

A change that adds, removes or weakens a control in section 3 updates its
row in the same pull request.
