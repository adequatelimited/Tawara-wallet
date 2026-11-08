// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Reports built from the wallet library's pages: a title, a summary in the
//! application's own words, the causes that commonly lie behind it, and the
//! library's full output.
//!
//! Each summary says what the library's page says and nothing more. The
//! cases come from what the worker reports (`DivergenceKind`, `NoticeKind`,
//! the replies' fields), never from parsing the page.

use iced::widget::{column, container, row};
use iced::{Alignment, Element, Length};
use tawara_wallet_core::explorer::{ExplorerRefusal, IndexState};
use tawara_wallet_core::view::{AccountRow, AccountState, DivergenceKind, NoticeKind, WalletView};
use tawara_wallet_core::{AccountReport, Discovered, PlanView, SentView};

use super::name;
use crate::app::{Done, Message, Missed, ReportKey, WalletPage};
use crate::icon::{self, Icon};
use crate::theme::{self, color, space as sp};
use crate::ui::{self, t, ty};

/// How a report is coloured.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Something stopped, or needs attention.
    Warning,
    /// Something to know.
    Note,
}

/// A report: its title, its summary, the causes that commonly lie behind
/// it, and the library's page.
pub struct Report<'a> {
    pub tone: Tone,
    pub title: String,
    pub summary: Vec<String>,
    /// What the list of causes is headed.
    pub causes_title: &'static str,
    pub causes: Vec<&'static str>,
    /// The library's output, whole; `None` where the text is the
    /// application's own and the summary already says all of it.
    pub full: Option<&'a str>,
    /// Why it failed, in one short line drawn under the title, for a failure
    /// whose title does not say why.
    pub reason: Option<String>,
}

impl<'a> Report<'a> {
    fn new(tone: Tone, title: impl Into<String>) -> Report<'a> {
        Report {
            tone,
            title: title.into(),
            summary: Vec::new(),
            causes_title: "Common causes",
            causes: Vec::new(),
            full: None,
            reason: None,
        }
    }

    /// The failure's reason, from `text`, drawn under the title.
    fn because(mut self, text: &str) -> Report<'a> {
        self.reason = reason_of(text);
        self
    }

    fn says(mut self, line: impl Into<String>) -> Report<'a> {
        self.summary.push(line.into());
        self
    }

    fn causes(mut self, causes: &[&'static str]) -> Report<'a> {
        for c in causes {
            if !self.causes.contains(c) {
                self.causes.push(c);
            }
        }
        self
    }

    fn full(mut self, text: &'a str) -> Report<'a> {
        self.full = Some(text);
        self
    }
}

/// `r` as [`show`] draws it, or with less padding when `compact`. A report
/// is drawn as its title line, with its icon in its tone's colour and the
/// reason beneath when there is one; the summary, causes and library page
/// are not drawn. `page` and `key` are unused and kept for the callers'
/// signature.
pub fn show_in<'a>(
    compact: bool,
    _page: &WalletPage,
    _key: ReportKey,
    r: Report<'a>,
) -> Element<'a, Message> {
    let (lead, ink, style): (Icon, _, fn(&iced::Theme) -> container::Style) = match r.tone {
        Tone::Warning => (Icon::Warning, color::WARNING, theme::callout_warning),
        Tone::Note => (Icon::ShieldCheck, color::ACCENT, theme::callout_accent),
    };
    let mut said = column![t(r.title, ty::ROW_TITLE, color::TEXT_PRIMARY)].spacing(sp::S2);
    if let Some(reason) = r.reason {
        said = said.push(
            t(reason, ty::NOTE, color::TEXT_SECONDARY)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        );
    }
    container(
        row![icon::icon(lead, 18.0, 2.0, ink), said.width(Length::Fill)]
            .spacing(sp::S10)
            .align_y(Alignment::Center),
    )
    .padding(if compact {
        [sp::S12, sp::S14]
    } else {
        [sp::S14, sp::S16]
    })
    .width(Length::Fill)
    .style(style)
    .into()
}

// ---------------------------------------------------------------- causes

const NEVER_A_CRASH: &str = "It is never a crash during a send: the key index is saved before \
                             any signature exists, so an interrupted send shows up as a \
                             reservation, not as this.";
const OLDER_COPY: &str = "This keystore is an older copy: restored from a backup, or copied before \
                          a spend was made from the newer one.";
const SECOND_WALLET: &str = "Another install of the same recovery phrase spent from this \
                             account.";
const NOT_LANDED: &str = "A spend from this account has not landed yet.";
const MOVED_AHEAD: &str = "This keystore was moved ahead of the chain.";
const OTHER_CHAIN: &str = "The API answers for another network, or is behind the chain.";
const SPENT_FURTHER: &str = "The account has spent more times than the search reaches.";
const NOT_THIS_SEED: &str = "This recovery phrase is not the one that made the account.";
const NOT_PAID: &str = "It has not been paid yet: a new account is unknown to the chain until it \
                        first receives something.";
const EMPTIED: &str = "It was emptied: the ledger keeps no entry for an account at zero.";
const LOOKUP: &str = "The API serves another network, or the lookup failed: try again later, or \
                      another API.";
const NODE_DOWN: &str = "The API is down, busy or slow.";
const OFFLINE: &str = "This computer is offline.";
const NODE_ADDRESS: &str =
    "The API's address is mistyped or has changed: check it with Change API.";
const DAMAGED: &str = "The keystore's files could not be read as written.";
const ODD_ANSWER: &str = "The API gave an answer that could not be understood: try again, or \
                          another API.";

/// What a divergence of `kind` is, in a few words.
fn in_brief(kind: DivergenceKind) -> String {
    match kind {
        DivergenceKind::Ahead { gap } => {
            format!("the chain is {} ahead of the keystore", keys(gap))
        }
        DivergenceKind::Behind { gap } => format!("the chain is {} behind the keystore", keys(gap)),
        DivergenceKind::Unlocated => {
            "the chain's address is not among the keys searched".to_owned()
        }
        DivergenceKind::ReservationUnexplained => {
            "something moved it that this keystore did not".to_owned()
        }
        DivergenceKind::NotFound => "the API does not find it".to_owned(),
        DivergenceKind::Unreachable => "the API could not be reached for it".to_owned(),
        DivergenceKind::NoMaster => "its keys cannot be derived".to_owned(),
        DivergenceKind::Failed => "reconciling it failed".to_owned(),
    }
}

fn keys(n: u32) -> String {
    if n == 1 {
        "1 key".to_owned()
    } else {
        format!("{n} keys")
    }
}

/// The causes that commonly lie behind a divergence of `kind`.
fn causes_of(kind: DivergenceKind) -> &'static [&'static str] {
    match kind {
        DivergenceKind::Ahead { .. } => &[OLDER_COPY, SECOND_WALLET, NEVER_A_CRASH],
        DivergenceKind::Behind { .. } => &[NOT_LANDED, MOVED_AHEAD, OTHER_CHAIN],
        DivergenceKind::Unlocated => &[SPENT_FURTHER, NOT_THIS_SEED, OTHER_CHAIN],
        DivergenceKind::ReservationUnexplained => &[SECOND_WALLET, OLDER_COPY],
        DivergenceKind::NotFound => &[NOT_PAID, EMPTIED, LOOKUP],
        DivergenceKind::Unreachable => &[NODE_DOWN, OFFLINE],
        DivergenceKind::NoMaster => &[],
        DivergenceKind::Failed => &[DAMAGED, ODD_ANSWER],
    }
}

/// A diverged account's report.
pub fn diverged(kind: DivergenceKind, report: &str) -> Report<'_> {
    let (tone, title, summary) = match kind {
        DivergenceKind::Ahead { gap } => (
            Tone::Warning,
            format!(
                "Resyncing: the chain is {} ahead of this keystore",
                keys(gap)
            ),
            "A spend from this account landed that this keystore does not record, most often \
             from another install of the same recovery phrase. Tawara moves the account to the \
             key the chain shows on its own; until then it signs nothing from it, and the other \
             accounts are unaffected.",
        ),
        DivergenceKind::Behind { gap } => (
            Tone::Warning,
            format!(
                "No Coins Available: the chain is {} behind this keystore",
                keys(gap)
            ),
            "The chain shows this account at an earlier key than this keystore holds. Tawara \
             pauses it rather than sign from a key the chain does not expect; the other \
             accounts are unaffected.",
        ),
        DivergenceKind::Unlocated => (
            Tone::Warning,
            "No Coins Available: the chain's address is not among the keys searched".to_owned(),
            "The address the chain holds for this account is none of the keys this keystore \
             reached around its index. Tawara pauses it; the other accounts are unaffected.",
        ),
        DivergenceKind::ReservationUnexplained => (
            Tone::Warning,
            "No Coins Available: something moved this account that this keystore did not"
                .to_owned(),
            "A spend from this account is reserved, and the chain shows the account at neither \
             the key that signed it nor its change key. Tawara pauses it; the other accounts \
             are unaffected.",
        ),
        DivergenceKind::NotFound => (
            Tone::Note,
            "The API does not find this account".to_owned(),
            "The API answered \"account not found\". It says that for an account it holds no \
             entry for, for one at a zero balance, and when a lookup fails, and it does not say \
             which. Receiving to the account brings it back.",
        ),
        DivergenceKind::Unreachable => (
            Tone::Warning,
            "The API could not be reached for this account".to_owned(),
            "Nothing was reconciled for it, and nothing is wrong with the account itself. Refresh \
             once the API answers.",
        ),
        DivergenceKind::NoMaster => (
            Tone::Warning,
            "This keystore cannot derive this account's keys".to_owned(),
            "It is a derived account, and the keystore holds no master seed to derive its keys \
             from.",
        ),
        DivergenceKind::Failed => (
            Tone::Warning,
            "Reconciling this account failed".to_owned(),
            "The keystore could not be read for it, or the API's answer could not be understood; \
             the full report says which. The other accounts are unaffected.",
        ),
    };
    Report::new(tone, title)
        .says(summary)
        .causes(causes_of(kind))
        .full(report)
}

/// The store's notice, shown on the wallet page.
pub fn notice(wallet: &WalletView) -> Option<Report<'_>> {
    let notice = wallet.notice.as_ref()?;
    let paused: Vec<(&AccountRow, DivergenceKind)> = wallet
        .accounts
        .iter()
        .filter_map(|a| match a.state {
            // A new account is not paused.
            AccountState::Diverged { kind, .. } if !a.is_new() => Some((a, kind)),
            _ => None,
        })
        .collect();
    let count = wallet.accounts.len();
    // Only new accounts kept the store from being whole: nothing to report.
    if notice.kind == NoticeKind::NotWhole && paused.is_empty() {
        return None;
    }
    let report = match notice.kind {
        NoticeKind::NotWhole | NoticeKind::WillNotStart => {
            let n = paused.len();
            let all_unfound = paused.iter().all(|(_, k)| *k == DivergenceKind::NotFound);
            let title = if all_unfound {
                if n == 1 {
                    "The API does not find 1 account".to_owned()
                } else {
                    format!("The API does not find {n} accounts")
                }
            } else if notice.kind == NoticeKind::WillNotStart {
                "The wallet did not open: its accounts need a look".to_owned()
            } else if n == 1 {
                "1 account paused: look at it before spending from it".to_owned()
            } else {
                format!("{n} accounts paused: look at them before spending from them")
            };
            let mut r = Report::new(
                if all_unfound {
                    Tone::Note
                } else {
                    Tone::Warning
                },
                title,
            );
            r = if all_unfound {
                r.says(
                    "The API answered \"account not found\". That is what a new account looks \
                     like until it is first paid, and what an emptied one looks like; nothing has \
                     been lost.",
                )
            } else {
                r.says(format!(
                    "Tawara could not match {} of this keystore's {count} accounts against the \
                     chain, so nothing can be sent from {} until {} looked at. Pausing protects \
                     the funds: no one-time key is ever used twice. {}",
                    if n == 1 {
                        "1".to_owned()
                    } else {
                        n.to_string()
                    },
                    if n == 1 { "it" } else { "them" },
                    if n == 1 { "it is" } else { "they are" },
                    if n == count {
                        ""
                    } else {
                        "The other accounts are unaffected."
                    }
                ))
            };
            for (account, kind) in &paused {
                r = r.says(format!("{}: {}.", name(account.id), in_brief(*kind)));
                r = r.causes(causes_of(*kind));
            }
            r.says("Each account's own page, from the list below, has its summary and report.")
        }
        NoticeKind::NoNode => Report::new(Tone::Warning, "No API chosen").says(notice.text.clone()),
        NoticeKind::NodeChanged => {
            Report::new(Tone::Note, "The API was changed").says(notice.text.clone())
        }
        NoticeKind::Cancelled => {
            Report::new(Tone::Note, "Reconciling was cancelled").says(notice.text.clone())
        }
        NoticeKind::NodeSilent => Report::new(Tone::Warning, "The API did not answer")
            .says(first_paragraph(&notice.text))
            .causes(&[NODE_DOWN, OFFLINE, NODE_ADDRESS]),
        NoticeKind::NodeRefused => Report::new(Tone::Warning, "The API could not be used")
            .because(&notice.text)
            .says(first_paragraph(&notice.text))
            .causes(&[NODE_ADDRESS, NODE_DOWN]),
    };
    // The worker's own texts are the summary; the library's pages are
    // offered whole.
    Some(match notice.kind {
        NoticeKind::NoNode | NoticeKind::NodeChanged | NoticeKind::Cancelled => report,
        _ => report.full(&notice.text),
    })
}

/// The most a reason line holds before it is cut, in characters.
const REASON_MOST: usize = 200;

/// A failure's reason in one short line, from the library's text: the part
/// after "FAILED:" where a line has it, otherwise the first line without an
/// "error:" label; its first sentence when the line is long, cut at
/// [`REASON_MOST`] characters; capitalised and ended with a full stop.
/// `None` for an empty text.
fn reason_of(text: &str) -> Option<String> {
    let lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let mut line = lines
        .clone()
        .find_map(|l| l.split_once("FAILED:").map(|(_, why)| why.trim()))
        .or_else(|| lines.clone().next())?;
    for label in ["error:", "Error:", "refused:"] {
        line = line.strip_prefix(label).map_or(line, str::trim);
    }
    let mut reason = line.to_owned();
    if reason.chars().count() > REASON_MOST
        && let Some(end) = reason.find(". ")
    {
        reason.truncate(end + 1);
    }
    if reason.chars().count() > REASON_MOST {
        reason = reason.chars().take(REASON_MOST - 1).collect::<String>();
        reason.push('…');
    }
    let mut chars = reason.chars();
    let first = chars.next()?;
    let mut reason = first.to_uppercase().chain(chars).collect::<String>();
    if !reason.ends_with(['.', '…', '!', '?']) {
        reason.push('.');
    }
    Some(reason)
}

/// A text's first paragraph, its lines joined.
fn first_paragraph(text: &str) -> String {
    ui::reflow(text.split("\n\n").next().unwrap_or(text))
}

/// A spend laid out and not yet signed.
pub fn plan(p: &PlanView, index: Option<u32>) -> Report<'_> {
    let mut r = Report::new(
        if p.empties_account {
            Tone::Warning
        } else {
            Tone::Note
        },
        if p.empties_account {
            "Not signed yet, and this empties the account"
        } else {
            "Not signed yet: check every destination against its payee"
        },
    )
    .says(
        "Nothing has been signed and no key has been reserved. Compare each destination, \
         character for character, with what the payee's wallet shows; they are listed in the \
         order that goes on the wire.",
    )
    .says(format!(
        "Signing first reserves {}. From then, these exact figures (each destination, amount \
         and reference, the fee and the block-to-live) are the only spend that key will sign: \
         re-signing later needs them exactly.",
        index.map_or_else(|| "the account's key".to_owned(), |i| format!("key #{i}"))
    ));
    if p.empties_account {
        r = r.says(
            "The change is zero, so this empties the account. Once it lands the API answers \
             \"account not found\" for it, and nothing can be done with it until it is paid \
             again.",
        );
    }
    r.full(&p.text)
}

/// What the sent summary says about the copy kept beside the keystore.
pub(crate) const KEPT_COPY: &str = "Tawara keeps a copy of the signed bytes beside the keystore \
                                    until the account spends again and more than 256 blocks \
                                    have passed, and sends it again by itself if a \
                                    reorganisation reverts the spend. Re-signing needs exactly \
                                    the same destinations, amounts, memos, fee and \
                                    block-to-live.";

/// A signed spend: the three facts, stated plainly.
pub fn sent(s: &SentView) -> Report<'_> {
    let r = if s.submitted {
        Report::new(
            Tone::Warning,
            "Sent to the API: not yet accepted by the network",
        )
        .says(
            "Tawara sent the signed spend to the API. That is not the network accepting it: the \
             network checks it after Tawara has gone, and if it refuses, nothing says so here.",
        )
    } else {
        Report::new(Tone::Warning, "Signed, and not sent to the API")
            .because(&s.text)
            .says(
            "The spend is signed, and sending it to the API failed; the full report says why. The \
             bytes on this page are the spend: Submit saved artifact sends them again.",
        )
    };
    r.says(
        "Every check that can run here passed. The ledger, the balance and the block-to-live are \
         the network's to check.",
    )
    .says(KEPT_COPY)
    .says("Settle from the account's page once the chain has moved, to learn what happened.")
    .full(&s.text)
}

/// A discovery sweep.
pub fn found<'a>(text: &'a str, rows: &[Discovered]) -> Report<'a> {
    let held = rows.iter().filter(|r| r.ledger_balance.is_some()).count();
    let to = rows.iter().map(|r| r.account_index).max().unwrap_or(0);
    let mut r = Report::new(
        Tone::Note,
        format!(
            "Searched accounts 0 to {to}: the chain holds {held} of {}",
            rows.len()
        ),
    )
    .says(
        "Nothing was written. An account the chain holds and this keystore does not can be added \
         from the list.",
    );
    if held < rows.len() {
        r = r.says(
            "\"Not found\" is what the API says for an account it holds no entry for. It does not \
             mean the account does not exist.",
        );
        r.causes_title = "Why the API may not find one";
        r = r.causes(&[NOT_PAID, EMPTIED, LOOKUP, NOT_THIS_SEED]);
    }
    r.full(text)
}

/// An account added, or not.
pub fn added(ok: bool, text: &str) -> Report<'_> {
    if ok {
        Report::new(Tone::Note, "Account added")
            .says(
                "The keystore now holds it at the key the chain shows; it is in the wallet's list.",
            )
            .full(text)
    } else {
        Report::new(Tone::Warning, "The account was not added")
            .because(text)
            .says("Nothing was written. The full report says why.")
            .full(text)
    }
}

/// Saved transaction bytes submitted.
pub fn submitted(accepted: bool, text: &str) -> Report<'_> {
    if accepted {
        Report::new(
            Tone::Warning,
            "Sent to the API: not yet accepted by the network",
        )
        .says(
            "Only the bytes' layout was checked here. The network checks the rest after Tawara \
             has gone, and if it refuses, nothing says so here.",
        )
        .says("Settle from the account's page once the chain has moved, to learn what happened.")
        .full(text)
    } else {
        Report::new(Tone::Warning, "Not submitted")
            .because(text)
            .says("Nothing reached the API. The full report says why.")
            .full(text)
    }
}

/// A saved copy of a reverted settle, written to the node. The write shows
/// only that the bytes reached the node, as a submission does. A refusal is
/// the page's error, not this report.
pub fn resubmitted(text: &str) -> Report<'_> {
    Report::new(
        Tone::Warning,
        "Sent to the API: not yet accepted by the network",
    )
    .says(
        "Only the send is confirmed here. The network checks the copy after Tawara has gone, and \
         if it refuses, nothing says so here.",
    )
    .says("The account was read again after the write, and its row shows what that read found.")
    .full(text)
}

/// What a check or a settlement found, read from the account's state
/// afterwards.
pub fn done<'a>(done: Done, account: &AccountRow, text: &'a str) -> Report<'a> {
    let r = match (done, &account.state) {
        // An account set aside when the wallet opened spends again only once
        // Refresh reopens the wallet with it.
        (Done::Settled, AccountState::InSync { .. }) if !account.spendable => Report::new(
            Tone::Note,
            "Settled: the account spends again after Refresh",
        )
        .says(
            "The chain shows the change key: the spend landed, and the keystore records it. The \
             account was set aside when the wallet opened, so Refresh on the wallet's page \
             opens the wallet with it.",
        ),
        (Done::Settled, AccountState::InSync { .. }) => {
            Report::new(Tone::Note, "Settled: the account can spend again").says(
                "The chain shows the change key: the spend landed, and the keystore records it.",
            )
        }
        (Done::Settled, AccountState::SpendOutstanding { .. }) => {
            Report::new(Tone::Warning, "Not settled yet: the spend has not landed").says(
                "The chain still holds the key that signed. Settle again once it has moved; \
                 until then the account does not spend again.",
            )
        }
        (Done::Settled, _) => Report::new(Tone::Note, "Settlement")
            .says("The account's state above is what the chain showed."),
        (Done::Checked, _) => Report::new(Tone::Note, "Checked against the chain just now")
            .says("The account's state above is what the chain showed."),
    };
    r.full(text)
}

const NO_INDEX: &str = "Transactions are read from the API's transaction index, which an API runs \
                        only when its operator sets one up. Nothing is wrong with the keystore or \
                        its accounts.";
const OTHER_NODE: &str = "Another API may run one: Change API, in the sidebar or in Settings.";
const INDEX_DOWN: &str = "The API answered, and runs a transaction index, but the index did not: \
                          its database is not connected yet, or not at all, or the search failed. \
                          Nothing is wrong with the keystore or its accounts.";
const READ_AGAIN: &str = "Read again in a while: an index that is starting answers once its \
                          database is connected.";
const OTHER_INDEX: &str = "Another API's index may answer: Change API, in the sidebar or in \
                           Settings.";

/// An explorer read the node did not serve; `what` says what it was for.
pub fn explorer<'a>(refusal: &'a ExplorerRefusal, what: &str) -> Report<'a> {
    let r = match refusal.index {
        Some(IndexState::Absent) => {
            let mut r = Report::new(Tone::Note, "This API keeps no transaction index")
                .says(NO_INDEX)
                .causes(&[OTHER_NODE]);
            r.causes_title = "What helps";
            r
        }
        Some(IndexState::Unavailable) => {
            let mut r = Report::new(Tone::Warning, "This API's transaction index did not answer")
                .says(INDEX_DOWN)
                .causes(&[READ_AGAIN, OTHER_INDEX]);
            r.causes_title = "What helps";
            r
        }
        None => Report::new(Tone::Warning, format!("The API did not serve {what}"))
            .says("Nothing was read, and nothing was written.")
            .causes(&[NODE_DOWN, OFFLINE, NODE_ADDRESS]),
    };
    r.full(&refusal.text)
}

const ABOVE_TIP: &str = "The block is above the API's tip: the chain has not reached it yet.";
const ARCHIVE: &str = "An API serves a block by its hash only from its archive of blocks, which \
                       may not hold it. By its number, it serves any block up to its tip.";

/// A block the node did not serve, asked for by its hash or by its number.
pub fn block_refused(refusal: &ExplorerRefusal, by_hash: bool) -> Report<'_> {
    Report::new(Tone::Warning, "The API did not serve this block")
        .says("Nothing was read, and nothing was written.")
        .causes(&[if by_hash { ARCHIVE } else { ABOVE_TIP }])
        .causes(&[NODE_DOWN, OFFLINE, NODE_ADDRESS])
        .full(&refusal.text)
}

/// What the node's index said of a transaction hash that named nothing.
pub fn missed_transaction(m: &Missed) -> Report<'_> {
    if !m.searched {
        return explorer(&m.transaction, "its transaction index");
    }
    let mut r = Report::new(
        Tone::Note,
        "The transaction index holds no transaction with this id",
    )
    .says(
        "The index answered, and holds no transaction with this id. That is not the same as there \
         being none: an index holds only what it has read.",
    )
    .causes(&[
        "The id is mistyped. An id has 64 hex digits.",
        "The transaction is still waiting in the mempool, or was never accepted.",
        OTHER_INDEX,
    ]);
    r.causes_title = "What it can mean";
    r.full(&m.transaction.text)
}

/// A tag the node did not resolve. The middleware answers the same for no
/// ledger entry, an entry at zero and a failed lookup (see the library's
/// `recon`), so the page cannot say which.
pub fn unresolved() -> Report<'static> {
    let mut r = Report::new(Tone::Note, "Account not found or lookup failed")
        .says(
            "The API did not resolve this tag. It gives the same answer for a tag the ledger \
             holds no entry for, for one held at zero, and for a lookup that failed, so this does \
             not say whether the account exists.",
        )
        .causes(&[
            NOT_PAID,
            "It was emptied, or is held at zero: the API answers for a balance of zero as for no \
             entry.",
            LOOKUP,
        ]);
    r.causes_title = "Why it can be";
    r
}

/// What the node said of a block hash that named nothing.
pub fn missed_block(m: &Missed) -> Report<'_> {
    let mut r = Report::new(Tone::Note, "The API serves no block with this hash")
        .says(ARCHIVE)
        .causes(&[NODE_DOWN, OFFLINE]);
    r.causes_title = "Or";
    r.full(&m.block.text)
}

/// One account's report on the recovery page: what the chain shows for it
/// now.
pub fn review(a: &AccountReport) -> Report<'_> {
    let r = match &a.state {
        AccountState::Diverged { kind, .. } => return diverged(*kind, &a.text),
        AccountState::InSync { .. } if a.spendable => Report::new(
            Tone::Note,
            "Address confirmed: the chain holds it at the key this keystore expects",
        )
        .says("Nothing needs doing for it."),
        AccountState::InSync { .. } => Report::new(
            Tone::Note,
            "Address confirmed: the chain holds it at the key this keystore expects",
        )
        .says(
            "It was set aside when the wallet opened, so it spends again once Refresh opens \
             the wallet with it.",
        ),
        AccountState::SpendOutstanding { .. } => {
            Report::new(Tone::Warning, "A spend is reserved and not settled").says(
                "Settle it from the account's page once the chain shows it landed, or re-sign \
                 it there if its signed bytes were lost.",
            )
        }
        AccountState::SpendLanded { .. } => Report::new(Tone::Warning, "The spend landed")
            .says("Settling it, from the account's page, records it and frees the account."),
        AccountState::NotReconciled => {
            Report::new(Tone::Note, "Not reconciled").says("No API answered for it.")
        }
    };
    r.full(&a.text)
}

/// What an acknowledged advance did.
pub fn advanced(ok: bool, text: &str) -> Report<'_> {
    let r =
        if ok {
            Report::new(Tone::Note, "The advance is done, as the library reports it")
                .says("The reports above are read again from the chain.")
        } else {
            Report::new(Tone::Warning, "Not advanced").because(text).says(
            "The library did not move the account: an advance goes only to the index its live \
             report names. Its page says what it found.",
        )
        };
    r.full(text)
}

/// A refusal too long to read as a line: its first paragraph, and the rest
/// on request. `None` for one that reads as a line.
pub fn refused(text: &str) -> Option<Report<'_>> {
    let long = text.trim().contains('\n') || text.len() > 240;
    long.then(|| {
        Report::new(Tone::Warning, "Refused")
            .because(text)
            .says(first_paragraph(text))
            .full(text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_says_its_reason_in_one_short_line() {
        assert_eq!(
            reason_of(
                "signed 3 destinations
submission FAILED: connection refused
more"
            ),
            Some("Connection refused.".to_owned())
        );
        assert_eq!(
            reason_of(
                "error: the artifact is not hex: odd length
Nothing was sent."
            ),
            Some("The artifact is not hex: odd length.".to_owned())
        );
        let long = format!("The first sentence is short. {}", "word ".repeat(80));
        assert_eq!(
            reason_of(&long),
            Some("The first sentence is short.".to_owned())
        );
        let unbroken = "x".repeat(400);
        let cut = reason_of(&unbroken).unwrap();
        assert_eq!(cut.chars().count(), REASON_MOST);
        assert!(cut.ends_with('…'));
        assert_eq!(
            reason_of(
                "  
 "
            ),
            None
        );

        // The failures whose titles do not say why carry it; a success does not.
        assert!(
            added(false, "the chain holds no such account")
                .reason
                .is_some()
        );
        assert!(added(true, "added").reason.is_none());
        assert!(
            submitted(false, "submission FAILED: timed out")
                .reason
                .is_some()
        );
        assert!(
            advanced(false, "the live report names index 12")
                .reason
                .is_some()
        );
    }
    use tawara_wallet_core::sample;
    use tawara_wallet_core::view::Notice;

    #[test]
    fn a_store_not_whole_is_summarized_with_the_causes_that_fit() {
        let wallet = sample::wallet_view();
        let r = notice(&wallet).expect("the sample store is not whole");
        assert!(r.tone == Tone::Warning);
        assert_eq!(
            r.title,
            "1 account paused: look at it before spending from it"
        );
        assert!(
            r.summary
                .iter()
                .any(|l| l.contains("the chain is 2 keys ahead")),
            "{:?}",
            r.summary
        );
        assert!(r.causes.contains(&OLDER_COPY) && r.causes.contains(&SECOND_WALLET));
        assert_eq!(r.full, wallet.notice.as_deref());
    }

    #[test]
    fn accounts_the_node_does_not_find_are_told_calmly() {
        let mut wallet = sample::wallet_view();
        for a in &mut wallet.accounts {
            a.state = AccountState::Diverged {
                kind: DivergenceKind::NotFound,
                report: "account not found".to_owned(),
                advance_to: None,
                rollback_to: None,
                reverted_settle: false,
            };
        }
        let r = notice(&wallet).unwrap();
        assert!(r.tone == Tone::Note);
        assert_eq!(r.title, "The API does not find 3 accounts");
        assert_eq!(r.causes.first(), Some(&NOT_PAID));
    }

    #[test]
    fn the_workers_own_words_are_the_summary_and_the_librarys_are_offered() {
        let silent = sample::unreconciled_view();
        let r = notice(&silent).unwrap();
        assert_eq!(r.title, "The API did not answer");
        assert!(r.causes.contains(&NODE_DOWN));
        assert!(r.full.is_some());

        let mut no_node = sample::unreconciled_view();
        no_node.notice = Some(Notice {
            kind: NoticeKind::NoNode,
            text: "No API is chosen.".to_owned(),
        });
        let r = notice(&no_node).unwrap();
        assert_eq!(r.summary, vec!["No API is chosen.".to_owned()]);
        assert_eq!(r.full, None, "nothing more to show");
    }

    #[test]
    fn each_kind_of_divergence_names_its_own_causes() {
        for kind in [
            DivergenceKind::Ahead { gap: 1 },
            DivergenceKind::Behind { gap: 2 },
            DivergenceKind::Unlocated,
            DivergenceKind::ReservationUnexplained,
            DivergenceKind::NotFound,
            DivergenceKind::Unreachable,
            DivergenceKind::NoMaster,
            DivergenceKind::Failed,
        ] {
            let r = diverged(kind, "the report");
            assert!(!r.title.is_empty() && !r.summary.is_empty());
            assert_eq!(r.causes, causes_of(kind).to_vec());
            assert_eq!(r.full, Some("the report"));
        }
        assert!(
            diverged(DivergenceKind::Ahead { gap: 1 }, "")
                .title
                .contains("1 key ahead")
        );
    }

    #[test]
    fn the_sent_summary_states_the_three_facts() {
        let s = sample::sent_view();
        let r = sent(&s);
        assert!(r.title.contains("not yet accepted"), "{}", r.title);
        let all = r.summary.join(" ");
        assert!(all.contains("not the network accepting it"), "{all}");
        assert!(all.contains("Every check that can run here passed"));
        assert!(all.contains("sends it again by itself"), "{all}");
        assert_eq!(r.full, Some(s.text.as_str()));
    }

    #[test]
    fn a_long_refusal_folds_and_a_short_one_reads_as_a_line() {
        assert!(refused("wrong password").is_none());
        let long = "first paragraph\nstill the first\n\nthe second";
        let r = refused(long).unwrap();
        assert_eq!(
            r.summary,
            vec!["first paragraph still the first".to_owned()]
        );
        assert_eq!(r.full, Some(long));
    }

    #[test]
    fn a_banner_is_its_title_alone_and_opens_nothing() {
        // Nothing is drawn under the title, however much the report holds.
        use crate::harness::{Driver, mouse_move, mouse_press, mouse_release, texts};
        let page = WalletPage::default();
        let report = || {
            Report::new(Tone::Warning, "A title")
                .says("A summary")
                .causes(&[NODE_DOWN])
                .full("The library's page")
        };
        for compact in [false, true] {
            let drawn: Vec<String> = texts(
                show_in(compact, &page, ReportKey::Notice, report()),
                (800.0, 400.0),
            )
            .into_iter()
            .map(|(text, _)| text)
            .collect();
            assert_eq!(drawn, ["A title"], "compact {compact}");
        }
        let mut driver = Driver::new(
            show_in(false, &page, ReportKey::Notice, report()),
            (800, 400),
        );
        let (messages, _, _) =
            driver.send(&[mouse_move(20.0, 20.0), mouse_press(), mouse_release()]);
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn a_settlement_of_an_account_set_aside_says_it_spends_after_refresh() {
        use tawara_wallet_core::view::{AccountKind, AccountRow, AccountState};
        let row = AccountRow {
            id: tawara_wallet_core::view::AccountId::from_tag([2; 20]),
            kind: AccountKind::Derived,
            index: 7,
            state: AccountState::InSync { balance: 1 },
            spendable: false,
            number: Some(0),
            reserved: false,
        };
        let r = done(Done::Settled, &row, "the library's page");
        assert_eq!(r.title, "Settled: the account spends again after Refresh");
        assert_eq!(r.full, Some("the library's page"));
    }

    #[test]
    fn a_settlement_the_worker_reports_says_the_account_can_spend_again() {
        use tawara_wallet_core::view::{AccountKind, AccountRow, AccountState};
        let row = AccountRow {
            id: tawara_wallet_core::view::AccountId::from_tag([1; 20]),
            kind: AccountKind::Derived,
            index: 7,
            state: AccountState::InSync { balance: 1 },
            spendable: true,
            number: Some(0),
            reserved: false,
        };
        let r = done(Done::Settled, &row, "the library's page");
        assert_eq!(r.title, "Settled: the account can spend again");
        assert_eq!(r.full, Some("the library's page"));
    }
}
