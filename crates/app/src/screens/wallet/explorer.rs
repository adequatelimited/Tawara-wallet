// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The explorer: the chain and the node's queue, one block, one account by
//! its tag and one transaction, each read from the node.
//!
//! Everything here is the node's, read through the library: what it serves
//! of its chain, its queue and its transaction index. Nothing is the
//! store's, apart from saying which accounts are the store's own ("Yours",
//! "Your account"), and nothing on these pages writes. The figures the node
//! does not send (a block's solve time, the average, the next neogenesis,
//! confirmations) are worked out as the page says, from what it did send.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, space, text_input};
use iced::{Alignment, Color, Element, Length, Padding};
use tawara_wallet_core::explorer::{
    self, BlockAt, BlockDetail, BlockKind, BlockSummary, ChainView, LedgerRead, OperationKind,
    OperationView, Party, TagView, TransactionView,
};
use tawara_wallet_core::view::AccountId;

use super::activity::party;
use super::{copy, frame, name, pair, report, settings, short};
use crate::app::{
    BLOCK_ROWS, BlockPage, ExplorerPage, Message, Model, QueuePage, ReportKey, TagPage, To,
    TransactionPage, WalletMsg, WalletPage,
};
use crate::icon::{self, Icon};
use crate::tap::tap;
use crate::theme::{self, color, space as sp};
use crate::ui::{self, Size, t, ty};

/// A link to `to`, in `face`: a block's number, an account's name.
fn link<'a>(content: String, face: ui::Type, to: impl Into<Message>) -> Element<'a, Message> {
    button(ui::label(content, face))
        .padding(0)
        .style(theme::button(theme::Button::Link))
        .on_press(to.into())
        .into()
}

fn open(to: To) -> WalletMsg {
    WalletMsg::Open(to)
}

/// A block's number, grouped, as a link to its page.
fn block_link<'a>(index: u64, face: ui::Type) -> Element<'a, Message> {
    link(
        ui::group(&index.to_string()),
        face,
        open(To::Block(BlockAt::Number(index))),
    )
}

/// Who an operation names, as a link to the account's page when it names
/// one; a ledger address is not an account to look up, nor a destination.
fn party_link<'a>(op: &OperationView, face: ui::Type) -> Element<'a, Message> {
    match &op.party {
        Party::Account(id) => link(name(*id), face, open(To::Tag(*id))),
        _ => t(party(op), face, color::TEXT_SECONDARY).into(),
    }
}

/// `ms` as a solve time reads: "58s", "1m 06s", "1h 02m".
fn duration(ms: i64) -> String {
    let secs = ms.max(0) / 1_000;
    match secs {
        ..60 => format!("{secs}s"),
        60..3_600 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3_600, secs % 3_600 / 60),
    }
}

/// A block's kind as a pill: neogenesis green, pseudo amber, normal raised;
/// a block the node sent no figures for has none.
fn kind_pill<'a>(kind: Option<BlockKind>) -> Element<'a, Message> {
    let (word, fill, ink) = match kind {
        Some(BlockKind::Neogenesis) => ("Neogenesis", color::ACCENT_SOFT, color::ACCENT),
        Some(BlockKind::Pseudo) => ("Pseudo", color::WARNING_SOFT, color::WARNING),
        Some(BlockKind::Normal) => ("Normal", color::BG_RAISED, color::TEXT_PRIMARY),
        None => ("Not sent", color::BG_SURFACE, color::TEXT_MUTED),
    };
    ui::pill(word, ty::BADGE, fill, ink, 22.0)
}

/// How tall a figure's tile is, whether or not it has a line under its
/// value, so tiles side by side line up.
const TILE: f32 = 104.0;

/// How tall a figure's tile is on a phone, where two sit side by side at 360
/// logical pixels. The tallest figure there is the balance: its value over
/// two lines, with its unit beneath, which takes about 127 pixels. The tile
/// is a little taller than that so that no figure touches the edge of its
/// tile, and every tile is this tall, so the two in a row line up: a Shrink
/// tile beside a shorter one would leave the pair of different heights.
const TILE_PHONE: f32 = 136.0;

/// An account's destination shortened to its first and last four
/// characters, for a narrow column ("3kPx…Wq7R", as the blocks table shows
/// a miner).
fn brief(id: AccountId) -> String {
    let full = id.destination().unwrap_or_else(|| id.hex());
    let chars: Vec<char> = full.chars().collect();
    if chars.len() <= 10 {
        return full;
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

/// A figure in a tile: its label, its value, and a line under it. The tile is
/// [`TILE`] tall, or [`TILE_PHONE`] in the phone layout.
fn stat<'a>(
    compact: bool,
    label: &'static str,
    value: String,
    under: Option<String>,
    accent: bool,
) -> Element<'a, Message> {
    // On a phone a tile is too narrow for an amount such as a balance or the
    // fees on one line. A zero-width space before the decimal point lets it
    // break there, the whole part over the fraction, rather than at whichever
    // digit reaches the edge; a value that fits is drawn unchanged.
    let value = if compact && value.starts_with(|c: char| c.is_ascii_digit()) {
        value.replacen('.', "\u{200B}.", 1)
    } else {
        value
    };
    let mut body = column![
        t(label, ty::NOTE, color::TEXT_MUTED),
        t(
            value,
            ty::STAT_VALUE,
            if accent {
                color::ACCENT
            } else {
                color::TEXT_PRIMARY
            }
        )
        .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(sp::S6);
    if let Some(under) = under {
        body = body.push(t(under, ty::TINY, color::TEXT_SECONDARY));
    }
    container(body)
        .padding(sp::S16)
        .width(Length::Fill)
        .height(Length::Fixed(if compact { TILE_PHONE } else { TILE }))
        .style(if accent {
            theme::stat_tile_accent
        } else {
            theme::stat_tile
        })
        .into()
}

/// The overview's auto-update switch: on, the chain is read again every
/// [`crate::app::EXPLORER_EVERY`] while the overview is shown.
fn auto_switch<'a>(model: &Model) -> Element<'a, Message> {
    container(
        iced::widget::toggler(model.explorer_auto)
            .label("Auto-update")
            .on_toggle(|on| WalletMsg::AutoRead(on).into())
            .size(20.0)
            .text_size(ty::CHIP.size)
            .font(ty::CHIP.font)
            .spacing(sp::S8)
            .style(theme::switch),
    )
    .padding(Padding::from([sp::S8, 0.0]))
    .id(AUTO_SWITCH)
    .into()
}

/// The ids of the overview's two auto-update switches, the header's and Last
/// block's.
/// They draw nothing; the tests find the switches by them.
const AUTO_SWITCH: &str = "auto-update";
const LAST_BLOCK_SWITCH: &str = "last-block-auto-update";

/// The overview's Last block tile: the newest block's age, and, while the
/// auto-update is off, the switch under it, so that the person who turned it
/// off can turn it on where the age stops being current.
fn last_block<'a>(model: &Model, age: String) -> Element<'a, Message> {
    let compact = model.compact();
    let mut body = column![
        t("Last block", ty::NOTE, color::TEXT_MUTED),
        t(age, ty::STAT_VALUE, color::TEXT_PRIMARY).wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(sp::S6);
    if !model.explorer_auto {
        body = body.push(
            container(
                iced::widget::toggler(false)
                    .label("Auto-update")
                    .on_toggle(|on| WalletMsg::AutoRead(on).into())
                    .size(16.0)
                    .text_size(ty::TINY.size)
                    .font(ty::TINY.font)
                    .spacing(sp::S6)
                    .style(theme::switch),
            )
            .id(LAST_BLOCK_SWITCH),
        );
    }
    container(body)
        .padding(sp::S16)
        .width(Length::Fill)
        .height(Length::Fixed(if compact { TILE_PHONE } else { TILE }))
        .style(theme::stat_tile)
        .into()
}

/// Rows of tiles: all in one row in a wide window, `per` to a row below. On a
/// phone they are two to a row whatever `per` says: at 360 three to a row
/// would leave each tile about 100 logical pixels wide, and the balance's
/// value would wrap past the height of its tile. A last row of one keeps the
/// width of the others, a blank standing for the second, so that the tiles
/// line up.
fn tiles<'a>(model: &Model, tiles: Vec<Element<'a, Message>>, per: usize) -> Element<'a, Message> {
    let compact = model.compact();
    let per = if model.width >= crate::app::WIDE {
        tiles.len().max(1)
    } else if compact {
        2
    } else {
        per.max(1)
    };
    let mut rows = column![].spacing(sp::S12);
    let mut current = row![].spacing(sp::S12);
    let mut n = 0;
    for tile in tiles {
        current = current.push(tile);
        n += 1;
        if n == per {
            rows = rows.push(current.height(Length::Shrink));
            current = row![].spacing(sp::S12);
            n = 0;
        }
    }
    if n > 0 {
        if compact {
            while n < per {
                current = current.push(space().width(Length::Fill));
                n += 1;
            }
        }
        rows = rows.push(current);
    }
    rows.into()
}

/// The path back to the explorer: "Explorer / Blocks / 871,172". On a phone it
/// wraps, and its last part wraps at its glyphs.
fn crumbs<'a>(compact: bool, section: &'static str, here: String) -> Element<'a, Message> {
    let here = if compact {
        t(here, ty::BODY_SMALL, color::TEXT_PRIMARY).wrapping(Wrapping::WordOrGlyph)
    } else {
        t(here, ty::BODY_SMALL, color::TEXT_PRIMARY)
    };
    let line = row![
        link("Explorer".to_owned(), ty::BODY_SMALL, open(To::Explorer)),
        t("/", ty::BODY_SMALL, color::TEXT_MUTED),
        t(section, ty::BODY_SMALL, color::TEXT_MUTED),
        t("/", ty::BODY_SMALL, color::TEXT_MUTED),
        here,
    ]
    .spacing(sp::S8)
    .align_y(Alignment::Center);
    if compact {
        line.wrap().into()
    } else {
        line.into()
    }
}

/// A column head, `portion` parts of the row's width.
fn head<'a>(label: String, portion: u16, end: bool) -> Element<'a, Message> {
    let cell = container(t(label, ty::TABLE_HEADER, color::TEXT_MUTED))
        .width(Length::FillPortion(portion));
    if end {
        cell.align_x(Alignment::End).into()
    } else {
        cell.into()
    }
}

/// A cell, `portion` parts of the row's width.
fn cell<'a>(
    content: impl Into<Element<'a, Message>>,
    portion: u16,
    end: bool,
) -> Element<'a, Message> {
    let cell = container(content).width(Length::FillPortion(portion));
    if end {
        cell.align_x(Alignment::End).into()
    } else {
        cell.into()
    }
}

fn table_row<'a>(cells: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut r = row![].spacing(sp::S12).align_y(Alignment::Center);
    for c in cells {
        r = r.push(c);
    }
    container(r)
        .padding(Padding::from([sp::S12, 0.0]))
        .width(Length::Fill)
        .into()
}

/// A card around a page's list: 24 at the sides and 20 above and below on the
/// desktop, and 16 all round on a phone, where every card keeps 16 pixels of
/// padding.
fn list_card<'a>(compact: bool, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    if compact {
        ui::card_for(true, content).into()
    } else {
        ui::card(content)
            .padding(Padding::from([sp::S20, sp::S24]))
            .into()
    }
}

/// A row of a list, drawn as selected when it is the person's own: the
/// highlight is inset by 8 at the sides, so the row's text is too.
fn highlight<'a>(yours: bool, line: Element<'a, Message>) -> Element<'a, Message> {
    if yours {
        container(line)
            .padding(Padding::from([0.0, sp::S8]))
            .style(theme::row_selected)
            .into()
    } else {
        line
    }
}

/// `n` of a thing, counted in words: "1 tx", "2 txs".
fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A transaction as the phone lists it (a block's spends and the queue's
/// page, in two lines): its id, with "Yours" beside it
/// when an account of this store sent it, and its amount on the same line;
/// under them, who it left, how many destinations it paid and its fee. The
/// amount has no unit there: a highlighted row leaves about 280 logical
/// pixels at 360, and "Yours", the id and the amount with its unit do not
/// fit in that, so the unit would wrap onto a line of its own. The fee
/// beneath names the unit.
fn tx_stacked<'a>(
    model: &'a Model,
    id: Element<'a, Message>,
    tx: &'a TransactionView,
) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let yours = ours(model, tx.source());
    let mut top = row![id].spacing(sp::S8).align_y(Alignment::Center);
    if yours {
        top = top.push(ui::pill(
            "Yours",
            ty::BADGE,
            color::ACCENT,
            color::TEXT_ON_ACCENT,
            20.0,
        ));
    }
    top = top.push(space().width(Length::Fill)).push(
        t(
            ui::amount(tx.sent(), unit),
            ty::TABLE_AMOUNT,
            color::TEXT_PRIMARY,
        )
        .wrapping(Wrapping::WordOrGlyph),
    );
    let source: Element<'a, Message> = match tx
        .operations
        .iter()
        .find(|o| o.kind == OperationKind::Source)
    {
        Some(op) => party_link(op, ty::MONO_SMALL),
        None => t("—", ty::MONO_SMALL, color::TEXT_MUTED).into(),
    };
    let under = row![
        source,
        t(
            counted(tx.destinations().count(), "destination", "destinations"),
            ty::NOTE,
            color::TEXT_SECONDARY
        ),
        t(
            format!("fee {} {}", ui::amount(tx.fee(), unit), ui::unit_name(unit)),
            ty::NOTE,
            color::TEXT_SECONDARY
        ),
    ]
    .spacing(sp::S8)
    .align_y(Alignment::Center)
    .wrap();
    let line = container(
        column![top.width(Length::Fill), under]
            .spacing(sp::S6)
            .width(Length::Fill),
    )
    .padding(Padding::from([sp::S12, 0.0]))
    .width(Length::Fill);
    highlight(yours, line.into())
}

/// The miner of a block, as a link to its account when the node named one,
/// or the party it named; a dash for a block with no reward.
fn miner_link<'a>(b: &'a BlockSummary) -> Element<'a, Message> {
    match &b.reward {
        Some(r) => match &r.to {
            Party::Account(id) => link(brief(*id), ty::MONO_SMALL, open(To::Tag(*id))),
            other => t(
                match other {
                    Party::Ledger(hex) => short(hex),
                    Party::Other(text) => text.clone(),
                    Party::Account(_) => String::new(),
                },
                ty::MONO_SMALL,
                color::TEXT_SECONDARY,
            )
            .into(),
        },
        None => t("—", ty::MONO_SMALL, color::TEXT_MUTED).into(),
    }
}

/// Whether `party` is one of the open store's accounts.
fn ours(model: &Model, party: Option<&Party>) -> bool {
    match (party, &model.wallet) {
        (Some(Party::Account(id)), Some(w)) => w.accounts.iter().any(|a| a.id == *id),
        _ => false,
    }
}

/// The tip as the explorer last read it, or as the node last said.
fn tip(model: &Model) -> Option<u64> {
    match &model.chain.last {
        Some(Ok(c)) => Some(c.tip),
        _ => model.node.tip.map(|(tip, _)| tip),
    }
}

/// How many blocks stand on `block`, itself among them, at the higher of
/// the tip read with the page a transaction was opened from and the
/// explorer's own, which can be older.
fn confirmations(block: u64, read: Option<u64>, explorer: Option<u64>) -> Option<u64> {
    read.max(explorer)?.checked_sub(block)?.checked_add(1)
}

// ---- Overview --------------------------------------------------------------

/// The overview: the chain, the node's queue, and the search field.
pub fn overview<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    e: &'a ExplorerPage,
) -> Element<'a, Message> {
    let compact = model.compact();
    let network = model.node.network.as_ref().map_or_else(
        || "The chain".to_owned(),
        |n| format!("{} {}", n.blockchain, n.network),
    );
    let reading = model.chain.reading || model.pending.reading;
    let mut actions: Vec<Element<'a, Message>> = Vec::new();
    if let Some(at) = model.chain.read_ms {
        actions.push(
            container(
                row![
                    ui::dot(color::ACCENT, 8.0),
                    t(
                        format!("Read {}", ui::age(model.clock_ms, at)),
                        ty::CHIP,
                        color::TEXT_SECONDARY
                    ),
                ]
                .spacing(sp::S8)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([sp::S8, sp::S14]))
            .style(theme::pill(color::BG_SURFACE, color::TEXT_SECONDARY))
            .into(),
        );
    }
    actions.push(auto_switch(model));
    actions.push(
        ui::button_with(
            if reading { "Reading…" } else { "Read again" },
            theme::Button::Secondary,
            Size::Header,
            Some(Icon::Refresh),
            (!reading && model.node.url.is_some()).then(|| WalletMsg::ReadChain.into()),
        )
        .into(),
    );
    let mut body = vec![search(compact, e)];
    if let Some(m) = &e.missed {
        body.push(report::show_in(
            compact,
            page,
            ReportKey::Explorer,
            report::missed_transaction(m),
        ));
        body.push(report::show_in(
            compact,
            page,
            ReportKey::Missed,
            report::missed_block(m),
        ));
    }
    match &model.chain.last {
        Some(Ok(chain)) => {
            body.push(figures(model, chain));
            let right = column![queue(model), kinds(compact)].spacing(if compact {
                sp::S16
            } else {
                sp::S24
            });
            body.push(pair(model, latest(model, chain), right.into(), 7, 5));
        }
        Some(Err(refusal)) => body.push(report::show_in(
            compact,
            page,
            ReportKey::Read,
            report::explorer(refusal, "its newest blocks"),
        )),
        None => body.push(
            ui::card_for(
                compact,
                ui::helper(if model.chain.reading {
                    "Reading the chain…"
                } else if model.node.url.is_none() {
                    "No API is chosen: the explorer reads the chain from it."
                } else {
                    "Not read yet."
                }),
            )
            .into(),
        ),
    }
    frame(model, page, "Explorer", network, actions, body)
}

/// The search field: a block's number or hash, a transaction's id, an
/// address or a tag.
fn search(compact: bool, e: &ExplorerPage) -> Element<'_, Message> {
    // A phone's field is narrower, so its placeholder is shorter.
    let placeholder = if compact {
        "Block, id, address or tag"
    } else {
        "Block number or hash, transaction id, address or tag"
    };
    let field = text_input(placeholder, &e.search)
        .font(ty::FIELD.font)
        .size(ty::FIELD.size)
        .padding(Padding::from([sp::S10, 0.0]))
        .style(theme::bare_field)
        .on_input(|s| WalletMsg::ExplorerTyped(s).into())
        .on_submit(WalletMsg::ExplorerFind.into());
    let bar = container(
        row![
            icon::icon(Icon::Search, 18.0, 2.0, color::TEXT_MUTED),
            field,
            ui::button_with(
                if e.finding.is_some() {
                    "Looking…"
                } else {
                    "Search"
                },
                theme::Button::Primary,
                Size::Medium,
                None,
                e.finding.is_none().then(|| WalletMsg::ExplorerFind.into()),
            ),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([sp::S6, sp::S8]).left(sp::S16))
    .width(Length::Fill)
    .style(theme::card);
    let mut col = column![bar].spacing(sp::S8);
    if let Some(why) = &e.invalid {
        col = col
            .push(t(why.clone(), ty::BODY_SMALL, color::WARNING).wrapping(Wrapping::WordOrGlyph));
    }
    col.into()
}

/// The chain's figures, as tiles.
fn figures<'a>(model: &'a Model, chain: &'a ChainView) -> Element<'a, Message> {
    let compact = model.compact();
    let newest = chain.blocks.first();
    let waiting = match (&model.pending.last, &model.mempool.last) {
        (Some(Ok(p)), _) => Some(p.waiting),
        (_, Some(Ok(m))) => Some(m.waiting),
        _ => None,
    };
    let next = explorer::next_neogenesis(chain.tip);
    tiles(
        model,
        vec![
            stat(
                compact,
                "Block height",
                ui::group(&chain.tip.to_string()),
                None,
                false,
            ),
            last_block(
                model,
                newest.map_or_else(|| "—".to_owned(), |b| ui::age(model.clock_ms, b.time_ms)),
            ),
            stat(
                compact,
                "Average Block Time",
                chain.average_ms.map_or_else(|| "—".to_owned(), duration),
                None,
                false,
            ),
            stat(
                compact,
                "Mempool",
                waiting.map_or_else(|| "—".to_owned(), |w| ui::group(&w.to_string())),
                waiting.map(|_| "transactions waiting".to_owned()),
                false,
            ),
            stat(
                compact,
                "Next neogenesis",
                ui::group(&next.to_string()),
                Some(format!("in {} blocks", next - chain.tip)),
                false,
            ),
        ],
        3,
    )
}

/// The newest blocks, as a table. On a phone there are no columns to head,
/// so each block is two lines, one under the other.
fn latest<'a>(model: &'a Model, chain: &'a ChainView) -> Element<'a, Message> {
    if model.compact() {
        let mut list = column![ui::section_label("Latest blocks"), ui::divider()].spacing(sp::S4);
        for b in &chain.blocks {
            list = list.push(block_stacked(model, b)).push(ui::divider());
        }
        return ui::card_for(true, list).into();
    }
    let unit = model.prefs.unit;
    let mut list = column![
        ui::section_label("Latest blocks"),
        table_row(vec![
            head("Block".to_owned(), 3, false),
            head("Type".to_owned(), 4, false),
            head("Txs".to_owned(), 2, true),
            head("Solve".to_owned(), 3, true),
            head("Validator".to_owned(), 4, false),
            head(format!("Reward ({})", ui::unit_name(unit)), 5, true),
            head("Age".to_owned(), 3, true),
        ]),
        ui::divider(),
    ]
    .spacing(sp::S4);
    for b in &chain.blocks {
        list = list.push(block_row(model, b)).push(ui::divider());
    }
    ui::card(list).into()
}

/// A block of the newest, as a phone lists it: its number, its kind and its
/// reward on the first line, the transactions it holds, its solve time, its
/// miner and its age under them.
fn block_stacked<'a>(model: &'a Model, b: &'a BlockSummary) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let spends = b.transactions - usize::from(b.reward.is_some());
    let reward = b.reward.as_ref().map_or_else(
        || "—".to_owned(),
        |r| format!("{} {}", ui::amount(r.amount, unit), ui::unit_name(unit)),
    );
    let top = row![
        block_link(b.index, ty::MONO_SMALL),
        kind_pill(b.kind),
        space().width(Length::Fill),
        t(reward, ty::TABLE_AMOUNT, color::TEXT_PRIMARY).wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(sp::S8)
    .align_y(Alignment::Center);
    let under = row![
        t(
            counted(spends, "tx", "txs"),
            ty::NOTE,
            color::TEXT_SECONDARY
        ),
        t(
            format!(
                "solve {}",
                b.solve_ms.map_or_else(|| "—".to_owned(), duration)
            ),
            ty::NOTE,
            color::TEXT_SECONDARY
        ),
        miner_link(b),
        t(
            ui::age(model.clock_ms, b.time_ms),
            ty::NOTE,
            color::TEXT_MUTED
        ),
    ]
    .spacing(sp::S8)
    .align_y(Alignment::Center)
    .wrap();
    container(column![top, under].spacing(sp::S6))
        .padding(Padding::from([sp::S12, 0.0]))
        .width(Length::Fill)
        .into()
}

fn block_row<'a>(model: &'a Model, b: &'a BlockSummary) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let spends = b.transactions - usize::from(b.reward.is_some());
    table_row(vec![
        cell(block_link(b.index, ty::MONO_SMALL), 3, false),
        cell(kind_pill(b.kind), 4, false),
        cell(
            t(spends.to_string(), ty::TABLE_BODY, color::TEXT_PRIMARY),
            2,
            true,
        ),
        cell(
            t(
                b.solve_ms.map_or_else(|| "—".to_owned(), duration),
                ty::TABLE_BODY,
                color::TEXT_SECONDARY,
            ),
            3,
            true,
        ),
        cell(miner_link(b), 4, false),
        cell(
            t(
                b.reward
                    .as_ref()
                    .map_or_else(|| "—".to_owned(), |r| ui::amount(r.amount, unit)),
                ty::TABLE_AMOUNT,
                color::TEXT_PRIMARY,
            ),
            5,
            true,
        ),
        cell(
            t(
                ui::age(model.clock_ms, b.time_ms)
                    .trim_end_matches(" ago")
                    .to_owned(),
                ty::TABLE_BODY,
                color::TEXT_SECONDARY,
            ),
            3,
            true,
        ),
    ])
}

/// The node's queue, as the mempool card.
fn queue(model: &Model) -> Element<'_, Message> {
    let unit = model.prefs.unit;
    let mut card = column![].spacing(sp::S4);
    match &model.pending.last {
        Some(Ok(p)) => {
            card = card.push(
                row![
                    ui::section_label("Mempool"),
                    space().width(Length::Fill),
                    t(
                        format!("{} waiting", ui::group(&p.waiting.to_string())),
                        ty::NOTE,
                        color::TEXT_MUTED
                    ),
                ]
                .align_y(Alignment::Center),
            );
            if p.waiting == 0 {
                card = card.push(ui::helper("No pending transactions."));
            }
            for (n, r) in p.rows.iter().enumerate() {
                card = card
                    .push(pending_row(model.compact(), r, n, unit))
                    .push(ui::divider());
            }
            let unread = p.waiting.saturating_sub(p.rows.len());
            if unread > 0 {
                card = card.push(
                    container(ui::helper(format!(
                        "{} more waiting, not read. The mempool is this API's own; another API's \
                         may differ.",
                        ui::group(&unread.to_string())
                    )))
                    .padding(Padding::from([sp::S8, 0.0])),
                );
            }
            if p.waiting > 0 {
                card = card.push(link(
                    "View all pending".to_owned(),
                    ty::LINK,
                    open(To::Queue),
                ));
            }
        }
        Some(Err(_)) => {
            card = card.push(ui::section_label("Mempool")).push(t(
                "The API did not serve its mempool.",
                ty::BODY_SMALL,
                color::TEXT_SECONDARY,
            ));
        }
        None => {
            card = card.push(ui::section_label("Mempool")).push(ui::helper(
                if model.pending.reading {
                    "Reading the queue…"
                } else {
                    "Not read yet."
                },
            ));
        }
    }
    ui::card_for(model.compact(), card).into()
}

fn pending_row(
    compact: bool,
    r: &explorer::PendingRow,
    n: usize,
    unit: tawara_wallet_core::preferences::AmountUnit,
) -> Element<'_, Message> {
    let Some(tx) = &r.transaction else {
        return container(
            column![
                t(short(&r.id), ty::MONO_SMALL, color::TEXT_SECONDARY),
                t(
                    "Left the queue before it was read: mined since, or dropped.",
                    ty::TINY,
                    color::TEXT_MUTED
                ),
            ]
            .spacing(sp::S2),
        )
        .padding(Padding::from([sp::S10, 0.0]))
        .into();
    };
    let destinations = tx.destinations().count();
    let details = t(
        format!(
            "{destinations} {} · fee {}",
            if destinations == 1 {
                "destination"
            } else {
                "destinations"
            },
            ui::amount(tx.fee(), unit)
        ),
        ty::TINY,
        color::TEXT_MUTED,
    );
    let content: Element<'_, Message> = if compact {
        // On a phone the id and the amount share the first line, and the
        // destinations and the fee sit beneath them. A column of text takes
        // its own width first, and the amount is then laid out in what is left
        // of the row: at 360 that is less than the amount needs, and it would
        // overflow the card. Here the id leaves the amount its width, and an
        // amount longer still wraps by glyph rather than overflowing.
        column![
            row![
                t(short(&r.id), ty::MONO_SMALL, color::TEXT_PRIMARY),
                space().width(Length::Fill),
                t(
                    ui::amount(tx.sent(), unit),
                    ty::TABLE_AMOUNT,
                    color::TEXT_PRIMARY
                )
                .wrapping(Wrapping::WordOrGlyph),
            ]
            .align_y(Alignment::Center),
            details,
        ]
        .spacing(sp::S2)
        .into()
    } else {
        row![
            column![
                t(short(&r.id), ty::MONO_SMALL, color::TEXT_PRIMARY),
                details
            ]
            .spacing(sp::S2)
            .width(Length::Fill),
            t(
                ui::amount(tx.sent(), unit),
                ty::TABLE_AMOUNT,
                color::TEXT_PRIMARY
            ),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Center)
        .into()
    };
    // A row acts on release, so a drag that starts on it scrolls the list.
    tap(container(content)
        .padding(Padding::from([sp::S10, 0.0]))
        .width(Length::Fill))
    .on_tap(WalletMsg::OpenPending(n).into())
    .into()
}

/// What each kind of block is (the legend), in the library's words.
fn kinds<'a>(compact: bool) -> Element<'a, Message> {
    let kind = |k, words: &'static str| {
        row![
            container(kind_pill(Some(k))).width(Length::Fixed(110.0)),
            t(words, ty::BODY_SMALL, color::TEXT_SECONDARY)
                .wrapping(Wrapping::Word)
                .width(Length::Fill),
        ]
        .spacing(sp::S12)
    };
    ui::card_for(
        compact,
        column![
            ui::section_label("Block types"),
            kind(
                BlockKind::Neogenesis,
                "At every 256th block, it carries the ledger."
            ),
            kind(
                BlockKind::Pseudo,
                "If the validators don't finalize a block in time, a placeholder block is minted."
            ),
            kind(
                BlockKind::Normal,
                "A normal block containing transactions as finalized by the network's PoS \
                 Validator quorum."
            ),
        ]
        .spacing(sp::S14),
    )
    .into()
}

/// The overview's "View all pending": every transaction waiting in the
/// node's queue, read whole up to the command line's most for one read.
pub fn queue_page<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    q: &'a QueuePage,
) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let compact = model.compact();
    let mut body = vec![crumbs(compact, "Mempool", "All pending".to_owned())];
    let subtitle = match &q.read {
        Some(Ok(p)) => {
            // The phone has no columns to head: each transaction is its own
            // stacked row.
            let mut list = column![].spacing(sp::S4);
            if !compact {
                list = list
                    .push(table_row(vec![
                        head("Transaction id".to_owned(), 6, false),
                        head("From".to_owned(), 4, false),
                        head("Destinations".to_owned(), 3, true),
                        head(format!("Amount ({})", ui::unit_name(unit)), 4, true),
                        head("Fee".to_owned(), 3, true),
                    ]))
                    .push(ui::divider());
            }
            if p.waiting == 0 {
                list = list.push(
                    container(ui::helper("No pending transactions."))
                        .padding(Padding::from([sp::S12, 0.0])),
                );
            }
            for (n, r) in p.rows.iter().enumerate() {
                list = list.push(queued(model, r, n)).push(ui::divider());
            }
            let unread = p.waiting.saturating_sub(p.rows.len());
            if unread > 0 {
                list = list.push(
                    container(ui::helper(format!(
                        "{} more waiting, not read: one read takes at most {} whole, as the \
                         command line's does.",
                        ui::group(&unread.to_string()),
                        explorer::QUEUE_ROWS
                    )))
                    .padding(Padding::from([sp::S12, 0.0])),
                );
            }
            body.push(list_card(compact, list));
            format!(
                "{} {} waiting in the mempool",
                ui::group(&p.waiting.to_string()),
                if p.waiting == 1 {
                    "transaction"
                } else {
                    "transactions"
                }
            )
        }
        Some(Err(refusal)) => {
            body.push(report::show_in(
                compact,
                page,
                ReportKey::Explorer,
                report::explorer(refusal, "its queue"),
            ));
            "Not served".to_owned()
        }
        None => {
            body.push(ui::card_for(compact, ui::helper("Reading the queue…")).into());
            "Reading…".to_owned()
        }
    };
    frame(
        model,
        page,
        "Mempool",
        subtitle,
        vec![
            ui::button_with(
                if q.read.is_none() {
                    "Reading…"
                } else {
                    "Read again"
                },
                theme::Button::Secondary,
                Size::Header,
                Some(Icon::Refresh),
                q.read.is_some().then(|| open(To::Queue).into()),
            )
            .into(),
        ],
        body,
    )
}

/// One transaction of the queue's page, opening whole; one that left
/// the queue before it was read says so.
fn queued<'a>(model: &'a Model, r: &'a explorer::PendingRow, n: usize) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let compact = model.compact();
    let Some(tx) = &r.transaction else {
        if compact {
            return container(
                column![
                    t(short(&r.id), ty::MONO_SMALL, color::TEXT_SECONDARY),
                    t(
                        "Left the queue before it was read: mined since, or dropped.",
                        ty::TINY,
                        color::TEXT_MUTED
                    ),
                ]
                .spacing(sp::S2),
            )
            .padding(Padding::from([sp::S12, 0.0]))
            .width(Length::Fill)
            .into();
        }
        return table_row(vec![
            cell(
                t(short(&r.id), ty::MONO_SMALL, color::TEXT_SECONDARY),
                6,
                false,
            ),
            cell(
                t(
                    "Left the queue before it was read: mined since, or dropped.",
                    ty::TINY,
                    color::TEXT_MUTED,
                ),
                14,
                false,
            ),
        ]);
    };
    if compact {
        return tx_stacked(
            model,
            link(short(&r.id), ty::MONO_SMALL, WalletMsg::OpenPending(n)),
            tx,
        );
    }
    let mut id = row![link(
        short(&r.id),
        ty::MONO_SMALL,
        WalletMsg::OpenPending(n)
    )]
    .spacing(sp::S8)
    .align_y(Alignment::Center);
    if ours(model, tx.source()) {
        id = id.push(ui::pill(
            "Yours",
            ty::BADGE,
            color::ACCENT,
            color::TEXT_ON_ACCENT,
            20.0,
        ));
    }
    let source: Element<'a, Message> = match tx
        .operations
        .iter()
        .find(|o| o.kind == OperationKind::Source)
    {
        Some(op) => party_link(op, ty::MONO_SMALL),
        None => t("—", ty::MONO_SMALL, color::TEXT_MUTED).into(),
    };
    table_row(vec![
        cell(id, 6, false),
        cell(source, 4, false),
        cell(
            t(
                tx.destinations().count().to_string(),
                ty::TABLE_BODY,
                color::TEXT_PRIMARY,
            ),
            3,
            true,
        ),
        cell(
            t(
                ui::amount(tx.sent(), unit),
                ty::TABLE_AMOUNT,
                color::TEXT_PRIMARY,
            ),
            4,
            true,
        ),
        cell(
            t(
                ui::amount(tx.fee(), unit),
                ty::TABLE_BODY,
                color::TEXT_SECONDARY,
            ),
            3,
            true,
        ),
    ])
}

// ---- Block -----------------------------------------------------------------

/// One block, whole.
pub fn block<'a>(model: &'a Model, page: &'a WalletPage, b: &'a BlockPage) -> Element<'a, Message> {
    let (title, here) = match b.at {
        BlockAt::Number(n) => {
            let n = ui::group(&n.to_string());
            (format!("Block {n}"), n)
        }
        BlockAt::Hash(h) => {
            let hex: String = h.iter().map(|x| format!("{x:02x}")).collect();
            ("Block".to_owned(), short(&hex))
        }
    };
    let compact = model.compact();
    let mut body = vec![crumbs(compact, "Blocks", here)];
    let mut actions = Vec::new();
    let subtitle = match &b.read {
        Some(Ok(detail)) => {
            let index = detail.summary.index;
            actions.push(kind_pill(detail.summary.kind));
            // The node's number for the block is any u64: the blocks either
            // side are offered only where they can be numbered, and block 0
            // is never asked for (the node serves its newest for it).
            let older = index.checked_sub(1).filter(|n| *n > 0);
            actions.push(
                ui::button_with(
                    ui::group(&index.saturating_sub(1).to_string()),
                    theme::Button::Secondary,
                    Size::Small,
                    Some(Icon::ChevronLeft),
                    older.map(|n| open(To::Block(BlockAt::Number(n))).into()),
                )
                .into(),
            );
            let newer = index
                .checked_add(1)
                .filter(|_| detail.tip.or_else(|| tip(model)).is_none_or(|t| index < t));
            actions.push(
                ui::button_with(
                    ui::group(&index.saturating_add(1).to_string()),
                    theme::Button::Secondary,
                    Size::Small,
                    Some(Icon::ChevronRight),
                    newer.map(|n| open(To::Block(BlockAt::Number(n))).into()),
                )
                .into(),
            );
            body.extend(block_body(model, page, detail));
            let mut line = ui::full_time(detail.summary.time_ms, model.zone);
            if let Some(n) = detail.confirmations() {
                line.push_str(&format!(
                    " · {} {}",
                    ui::group(&n.to_string()),
                    if n == 1 {
                        "confirmation"
                    } else {
                        "confirmations"
                    }
                ));
            }
            line
        }
        Some(Err(refusal)) => {
            body.push(report::show_in(
                compact,
                page,
                ReportKey::Explorer,
                report::block_refused(refusal, matches!(b.at, BlockAt::Hash(_))),
            ));
            "Not served".to_owned()
        }
        None => {
            body.push(ui::card_for(compact, ui::helper("Reading the block…")).into());
            "Reading…".to_owned()
        }
    };
    let title = match &b.read {
        Some(Ok(detail)) => format!("Block {}", ui::group(&detail.summary.index.to_string())),
        _ => title,
    };
    frame(model, page, &title, subtitle, actions, body)
}

/// The block's haiku, as the node sent it. On the desktop it is as tall as
/// the three rows of figures beside it; on a phone, where it sits above them,
/// it is as tall as its lines wrap to, so that no line is cut short.
fn haiku_panel<'a>(compact: bool, d: &'a BlockDetail) -> Element<'a, Message> {
    let lines = d
        .figures
        .as_ref()
        .map(|f| f.haiku.as_slice())
        .unwrap_or_default();
    let mut col = column![t("Block haiku", ty::SECTION_LABEL, color::ACCENT)].spacing(sp::S8);
    if lines.is_empty() {
        col = col.push(t(
            match (d.summary.kind, &d.figures) {
                (_, None) => "The API sent none of this block's figures.",
                (Some(BlockKind::Normal), _) => "The API sent no haiku for this block.",
                _ => "Only a normal block carries a haiku.",
            },
            ty::BODY,
            color::TEXT_SECONDARY,
        ));
    } else {
        for line in lines {
            col =
                col.push(t(line.clone(), ty::HAIKU, color::TEXT_PRIMARY).wrapping(Wrapping::Word));
        }
        // For correctness: the text is the node's, and nothing here
        // checks it against the nonce it is made from.
        col = col.push(t(
            "The API sends this haiku; Tawara does not check it against the block's nonce.",
            ty::TINY,
            color::TEXT_MUTED,
        ));
    }
    let panel = container(col.spacing(sp::S10))
        .width(Length::Fill)
        .style(theme::hero_card);
    if compact {
        panel.padding(sp::S20).into()
    } else {
        panel
            .padding(sp::S32)
            // As tall as the three rows of figures beside it.
            .height(Length::Fixed(3.0 * TILE + 2.0 * sp::S12))
            .center_y(Length::Fixed(3.0 * TILE + 2.0 * sp::S12))
            .into()
    }
}

fn block_body<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    d: &'a BlockDetail,
) -> Vec<Element<'a, Message>> {
    let compact = model.compact();
    let unit = model.prefs.unit;
    let haiku = haiku_panel(compact, d);
    let figures = column![
        row![
            stat(
                compact,
                "Transactions",
                d.spends.len().to_string(),
                None,
                false
            ),
            stat(
                compact,
                "Solve time",
                d.summary.solve_ms.map_or_else(|| "—".to_owned(), duration),
                None,
                false
            ),
        ]
        .spacing(sp::S12),
        // No difficulty: under proof of stake it is a fixed stamp, not a
        // figure.
        row![
            stat(
                compact,
                "Validator reward",
                d.summary
                    .reward
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), |r| ui::amount(r.amount, unit)),
                d.summary
                    .reward
                    .as_ref()
                    .map(|_| ui::unit_name(unit).to_owned()),
                false
            ),
            stat(compact, "Fees", ui::amount(d.fees, unit), None, false),
        ]
        .spacing(sp::S12),
        row![
            stat(
                compact,
                "Destinations paid",
                d.paid().to_string(),
                None,
                false
            ),
            space().width(Length::Fill),
        ]
        .spacing(sp::S12),
    ]
    .spacing(sp::S12);
    // On a phone a fact is its label over its value, and the facts take the
    // width of the card.
    let mut facts = column![].spacing(sp::S4);
    if compact {
        facts = facts.width(Length::Fill);
    }
    let fact = |label: &'static str, value: Element<'a, Message>| -> Element<'a, Message> {
        if compact {
            column![
                column![t(label, ty::BODY_SMALL, color::TEXT_SECONDARY), value]
                    .spacing(sp::S4)
                    .padding(Padding::from([sp::S12, 0.0])),
                ui::divider(),
            ]
            .into()
        } else {
            column![
                row![
                    t(label, ty::BODY_SMALL, color::TEXT_SECONDARY).width(Length::FillPortion(2)),
                    container(value).width(Length::FillPortion(9)),
                ]
                .spacing(sp::S12)
                .align_y(Alignment::Center)
                .padding(Padding::from([sp::S12, 0.0])),
                ui::divider(),
            ]
            .into()
        }
    };
    let mono = |text: String| -> Element<'a, Message> {
        t(text, ty::MONO_SMALL, color::TEXT_PRIMARY)
            .wrapping(Wrapping::WordOrGlyph)
            .into()
    };
    facts = facts.push(fact("Block hash", mono(d.summary.hash.clone())));
    facts = facts.push(fact(
        "Previous block",
        if d.parent > 0 {
            row![
                block_link(d.parent, ty::MONO_SMALL),
                t(short(&d.parent_hash), ty::MONO_SMALL, color::TEXT_MUTED),
            ]
            .spacing(sp::S10)
            .into()
        } else {
            mono(d.parent_hash.clone())
        },
    ));
    if let Some(f) = &d.figures {
        facts = facts
            .push(fact("Merkle root", mono(f.root.clone())))
            .push(fact("Nonce", mono(f.nonce.clone())))
            .push(fact(
                "Size",
                t(
                    format!(
                        "{} bytes · least fee {} {}",
                        ui::group(&f.size.to_string()),
                        ui::amount(f.minimum_fee, unit),
                        ui::unit_name(unit)
                    ),
                    ty::TABLE_BODY,
                    color::TEXT_PRIMARY,
                )
                .into(),
            ));
    }
    if let Some(r) = &d.summary.reward {
        let miner: Element<'a, Message> = match &r.to {
            Party::Account(id) => link(
                id.destination().unwrap_or_else(|| id.hex()),
                ty::MONO_SMALL,
                open(To::Tag(*id)),
            ),
            _ => mono(party(&OperationView {
                kind: OperationKind::Reward,
                party: r.to.clone(),
                amount: 0,
                memo: String::new(),
            })),
        };
        facts = facts.push(fact("Validator", miner));
    }
    vec![
        pair(model, haiku, figures.into(), 1, 1),
        ui::card_for(compact, facts)
            .padding(Padding::from([
                sp::S8,
                if compact { sp::S16 } else { sp::S24 },
            ]))
            .into(),
        spends(model, page, d),
    ]
}

/// A block's transactions, [`BLOCK_ROWS`] at a time.
fn spends<'a>(model: &'a Model, page: &'a WalletPage, d: &'a BlockDetail) -> Element<'a, Message> {
    let compact = model.compact();
    let unit = model.prefs.unit;
    let from = match &page.page {
        crate::app::Page::Block(b) => b.rows_from,
        _ => 0,
    }
    .min(d.spends.len().saturating_sub(1) / BLOCK_ROWS * BLOCK_ROWS);
    let to = (from + BLOCK_ROWS).min(d.spends.len());
    let mut list = column![
        row![
            ui::section_label("Transactions"),
            space().width(Length::Fill),
            t(
                if d.spends.is_empty() {
                    "none".to_owned()
                } else {
                    format!(
                        "{}–{} of {}",
                        from + 1,
                        to,
                        ui::group(&d.spends.len().to_string())
                    )
                },
                ty::NOTE,
                color::TEXT_MUTED
            ),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(sp::S4);
    if d.spends.is_empty() {
        list = list.push(
            container(ui::helper(match d.summary.kind {
                Some(BlockKind::Pseudo) => "A pseudo-block carries no transactions.",
                Some(BlockKind::Neogenesis) => {
                    "A neogenesis block carries the ledger, not transactions."
                }
                _ => "Normal block with no transactions included.",
            }))
            .padding(Padding::from([sp::S12, 0.0])),
        );
        return list_card(compact, list);
    }
    // The phone has no columns to head: each transaction is a stacked row.
    if !compact {
        list = list
            .push(table_row(vec![
                head("Transaction id".to_owned(), 6, false),
                head("From".to_owned(), 4, false),
                head("Destinations".to_owned(), 3, true),
                head(format!("Amount ({})", ui::unit_name(unit)), 4, true),
                head("Fee".to_owned(), 3, true),
            ]))
            .push(ui::divider());
    }
    for (n, tx) in d.spends.iter().enumerate().skip(from).take(BLOCK_ROWS) {
        let item = if compact {
            let id = link(short(&tx.id), ty::MONO_SMALL, WalletMsg::OpenSpend(n));
            tx_stacked(model, id, tx)
        } else {
            spend_row(model, n, tx)
        };
        list = list.push(item).push(ui::divider());
    }
    if d.spends.len() > BLOCK_ROWS {
        list = list.push(
            row![
                space().width(Length::Fill),
                ui::button_with(
                    "Previous",
                    theme::Button::Secondary,
                    Size::Small,
                    None,
                    (from > 0).then(|| WalletMsg::BlockRows(from - BLOCK_ROWS).into()),
                ),
                ui::button_with(
                    "Next",
                    theme::Button::Secondary,
                    Size::Small,
                    None,
                    (to < d.spends.len()).then(|| WalletMsg::BlockRows(to).into()),
                ),
            ]
            .spacing(sp::S8)
            .padding(Padding::from([sp::S12, 0.0]).bottom(0.0)),
        );
    }
    list_card(compact, list)
}

/// One of a block's transactions as the desktop's table has it: its id, a
/// "Yours" pill when an account of this store sent it, who it left, how many
/// destinations it paid, what it sent and its fee.
fn spend_row<'a>(model: &'a Model, n: usize, tx: &'a TransactionView) -> Element<'a, Message> {
    let unit = model.prefs.unit;
    let mut id = row![link(short(&tx.id), ty::MONO_SMALL, WalletMsg::OpenSpend(n))]
        .spacing(sp::S8)
        .align_y(Alignment::Center);
    let yours = ours(model, tx.source());
    if yours {
        id = id.push(ui::pill(
            "Yours",
            ty::BADGE,
            color::ACCENT,
            color::TEXT_ON_ACCENT,
            20.0,
        ));
    }
    let source: Element<'a, Message> = match tx
        .operations
        .iter()
        .find(|o| o.kind == OperationKind::Source)
    {
        Some(op) => party_link(op, ty::MONO_SMALL),
        None => t("—", ty::MONO_SMALL, color::TEXT_MUTED).into(),
    };
    let line = table_row(vec![
        cell(id, 6, false),
        cell(source, 4, false),
        cell(
            t(
                tx.destinations().count().to_string(),
                ty::TABLE_BODY,
                color::TEXT_PRIMARY,
            ),
            3,
            true,
        ),
        cell(
            t(
                ui::amount(tx.sent(), unit),
                ty::TABLE_AMOUNT,
                color::TEXT_PRIMARY,
            ),
            4,
            true,
        ),
        cell(
            t(
                ui::amount(tx.fee(), unit),
                ty::TABLE_BODY,
                color::TEXT_SECONDARY,
            ),
            3,
            true,
        ),
    ]);
    highlight(yours, line)
}

// ---- Account ---------------------------------------------------------------

/// One account, by its tag: what the ledger holds for it, its three
/// forms, and its history from the node's index.
pub fn tag<'a>(model: &'a Model, page: &'a WalletPage, tp: &'a TagPage) -> Element<'a, Message> {
    let account = tp.account;
    let base58 = account.destination().unwrap_or_else(|| account.hex());
    let own = model
        .wallet
        .as_ref()
        .and_then(|w| w.accounts.iter().find(|a| a.id == account));
    let mut actions: Vec<Element<'a, Message>> = Vec::new();
    if let Some(row) = own {
        actions.push(ui::pill(
            format!("Your account · {}", super::kind(row)),
            ty::CHIP,
            color::ACCENT,
            color::TEXT_ON_ACCENT,
            28.0,
        ));
    }
    actions.push(copy(page, &base58, "Copy"));
    // Only for an account the person typed: one reached through a link was
    // named by the node.
    if tp.typed {
        actions.push(
            ui::button_with(
                "Send to this account",
                theme::Button::Primary,
                Size::Header,
                Some(Icon::Send),
                model
                    .busy
                    .is_none()
                    .then(|| WalletMsg::SendTo(account).into()),
            )
            .into(),
        );
    }
    let mut body = vec![crumbs(model.compact(), "Accounts", name(account))];
    match &tp.read {
        Some(view) => body.extend(tag_body(model, page, tp, view, own)),
        None => body.push(ui::card_for(model.compact(), ui::helper("Reading the account…")).into()),
    }
    frame(model, page, "Account", base58, actions, body)
}

fn tag_body<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    tp: &'a TagPage,
    view: &'a TagView,
    own: Option<&'a tawara_wallet_core::view::AccountRow>,
) -> Vec<Element<'a, Message>> {
    let compact = model.compact();
    let unit = model.prefs.unit;
    let history = view.history.as_ref().ok();
    let balance = match &view.ledger {
        LedgerRead::Held { balance, .. } => ui::amount(*balance, unit),
        _ => "—".to_owned(),
    };
    let mut figures = vec![stat(
        compact,
        "Balance",
        balance,
        matches!(view.ledger, LedgerRead::Held { .. }).then(|| ui::unit_name(unit).to_owned()),
        false,
    )];
    // Known only for this store's accounts, so shown only for them.
    if let Some(row) = own {
        figures.push(stat(
            compact,
            "Current key on ledger",
            settings::on_ledger(row).map_or_else(|| "—".to_owned(), |i| format!("#{i}")),
            None,
            false,
        ));
    }
    figures.extend([
        stat(
            compact,
            "Transactions",
            history.map_or_else(|| "—".to_owned(), |h| ui::group(&h.total.to_string())),
            history.map(|_| "in the transaction index".to_owned()),
            false,
        ),
        stat(
            compact,
            "Last active",
            history
                .and_then(|h| h.transactions.first())
                .and_then(|t| t.block)
                .map_or_else(|| "—".to_owned(), |b| ui::group(&b.to_string())),
            None,
            false,
        ),
    ]);
    let mut body = vec![tiles(model, figures, 2)];
    if view.ledger == LedgerRead::Unresolved {
        body.push(report::show_in(
            compact,
            page,
            ReportKey::Ledger,
            report::unresolved(),
        ));
    }
    body.push(forms(compact, view));
    match &view.history {
        Ok(h) => {
            body.push(tag_history(model, tp, view.account, h));
        }
        Err(refusal) => body.push(report::show_in(
            compact,
            page,
            ReportKey::Explorer,
            report::explorer(refusal, "its transaction index"),
        )),
    }
    body
}

/// One account, three forms: the address to share, the tag that never
/// changes, and the ledger address that changes with every spend.
fn forms(compact: bool, view: &TagView) -> Element<'_, Message> {
    let account = view.account;
    // On a phone each form is its label over its value.
    let form = |title: &'static str, note: &'static str, value: Element<'static, Message>| {
        if compact {
            column![
                column![
                    column![
                        t(title, ty::TABLE_NAME, color::TEXT_PRIMARY),
                        t(note, ty::TINY, color::TEXT_MUTED).wrapping(Wrapping::Word),
                    ]
                    .spacing(sp::S2),
                    value,
                ]
                .spacing(sp::S6)
                .padding(Padding::from([sp::S14, 0.0])),
                ui::divider(),
            ]
        } else {
            column![
                row![
                    column![
                        t(title, ty::TABLE_NAME, color::TEXT_PRIMARY),
                        t(note, ty::TINY, color::TEXT_MUTED).wrapping(Wrapping::Word),
                    ]
                    .spacing(sp::S2)
                    .width(Length::FillPortion(3)),
                    container(value).width(Length::FillPortion(9)),
                ]
                .spacing(sp::S16)
                .align_y(Alignment::Center)
                .padding(Padding::from([sp::S14, 0.0])),
                ui::divider(),
            ]
        }
    };
    let mono = |text: String, ink: Color| -> Element<'static, Message> {
        t(text, ty::MONO_SMALL, ink)
            .wrapping(Wrapping::WordOrGlyph)
            .into()
    };
    let ledger: Element<'static, Message> = match &view.ledger {
        LedgerRead::Held { address, .. } => {
            // The tag, then the hash of the key it is held at now.
            let (tag, key) = address.split_at(address.len().min(42));
            row![
                mono(tag.to_owned(), color::TEXT_PRIMARY),
                mono(key.to_owned(), color::ACCENT),
            ]
            .wrap()
            .into()
        }
        LedgerRead::Unresolved => t(
            "Not found, or the lookup failed: see above.",
            ty::BODY_SMALL,
            color::TEXT_SECONDARY,
        )
        .wrapping(Wrapping::Word)
        .into(),
        LedgerRead::Refused(why) => t(why.clone(), ty::BODY_SMALL, color::WARNING)
            .wrapping(Wrapping::WordOrGlyph)
            .into(),
    };
    list_card(
        compact,
        column![
            ui::section_label("One account, three forms"),
            form(
                "Address",
                "Share this · it carries a checksum",
                mono(
                    account.destination().unwrap_or_else(|| account.hex()),
                    color::TEXT_PRIMARY
                ),
            ),
            form(
                "Tag (hex)",
                "The account's 20 bytes · never changes",
                mono(account.hex(), color::TEXT_PRIMARY),
            ),
            form(
                "Ledger address",
                "The tag and the hash of its current key · changes with every spend · not \
                 somewhere to send funds",
                ledger,
            ),
        ]
        .spacing(sp::S4),
    )
}

/// The account's transactions from the node's index.
fn tag_history<'a>(
    model: &'a Model,
    tp: &'a TagPage,
    account: AccountId,
    h: &'a tawara_wallet_core::explorer::AccountHistory,
) -> Element<'a, Message> {
    let compact = model.compact();
    let unit = model.prefs.unit;
    let mut list = column![
        row![
            ui::section_label("History"),
            space().width(Length::Fill),
            t(
                "From the API's transaction index",
                ty::NOTE,
                color::TEXT_MUTED
            ),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(sp::S4);
    // The phone has no columns to head: each entry is two lines instead.
    if compact {
        list = list.push(ui::divider());
    } else {
        list = list
            .push(table_row(vec![
                head("Block".to_owned(), 3, false),
                head("Transaction".to_owned(), 4, false),
                head("Counterparty".to_owned(), 5, false),
                head("Memo".to_owned(), 4, false),
                head(format!("Amount ({})", ui::unit_name(unit)), 4, true),
            ]))
            .push(ui::divider());
    }
    if h.transactions.is_empty() {
        list = list.push(
            container(ui::helper(
                "The transaction index holds no transaction for this account.",
            ))
            .padding(Padding::from([sp::S12, 0.0])),
        );
    }
    for (n, tx) in h.transactions.iter().enumerate() {
        let (who, reference) = counterparty(tx, account);
        let net = tx.net(account);
        let figure = ui::amount(net.unsigned_abs(), unit);
        let (amount, ink) = match net {
            1.. => (format!("+{figure}"), color::ACCENT),
            ..0 => (format!("−{figure}"), color::TEXT_PRIMARY),
            0 => (figure, color::TEXT_SECONDARY),
        };
        if compact {
            // The entry's id and its signed amount, then the block, who it was
            // with and its reference, under them.
            let entry = column![
                row![
                    link(short(&tx.id), ty::MONO_SMALL, WalletMsg::OpenTagRow(n)),
                    space().width(Length::Fill),
                    t(amount, ty::TABLE_AMOUNT, ink).wrapping(Wrapping::WordOrGlyph),
                ]
                .spacing(sp::S8)
                .align_y(Alignment::Center),
                row![
                    match tx.block {
                        Some(b) => block_link(b, ty::NOTE),
                        None => t("—", ty::NOTE, color::TEXT_MUTED).into(),
                    },
                    who,
                    t(reference, ty::MONO_SMALL, color::TEXT_SECONDARY)
                        .wrapping(Wrapping::WordOrGlyph),
                ]
                .spacing(sp::S8)
                .align_y(Alignment::Center)
                .wrap(),
            ]
            .spacing(sp::S6);
            list = list
                .push(
                    container(entry)
                        .padding(Padding::from([sp::S12, 0.0]))
                        .width(Length::Fill),
                )
                .push(ui::divider());
        } else {
            list = list
                .push(table_row(vec![
                    cell(
                        match tx.block {
                            Some(b) => block_link(b, ty::MONO_SMALL),
                            None => t("—", ty::MONO_SMALL, color::TEXT_MUTED).into(),
                        },
                        3,
                        false,
                    ),
                    cell(
                        link(short(&tx.id), ty::MONO_SMALL, WalletMsg::OpenTagRow(n)),
                        4,
                        false,
                    ),
                    cell(who, 5, false),
                    cell(
                        t(reference, ty::MONO_SMALL, color::TEXT_SECONDARY)
                            .wrapping(Wrapping::WordOrGlyph),
                        4,
                        false,
                    ),
                    cell(t(amount, ty::TABLE_AMOUNT, ink), 4, true),
                ]))
                .push(ui::divider());
        }
    }
    if h.more() {
        let unread = h.unread();
        let note = ui::helper(format!(
            "The index holds {} older {} for this account, not read yet. They are read \
             100 at a time.",
            ui::group(&unread.to_string()),
            if unread == 1 { "row" } else { "rows" }
        ));
        let read = ui::button_with(
            if tp.reading_older {
                "Reading…"
            } else {
                "Read older"
            },
            theme::Button::Secondary,
            Size::Small,
            None,
            (!tp.reading_older).then(|| WalletMsg::ReadOlderTag.into()),
        );
        list = list.push(
            row![container(note).width(Length::Fill), read]
                .spacing(sp::S12)
                .align_y(Alignment::Center)
                .padding(Padding::from([sp::S12, 0.0])),
        );
    }
    list_card(compact, list)
}

/// Who a transaction moved value with, for `account`, and the references
/// on what reached it or what it paid: the payer of a payment received;
/// the payee of a spend, or how many it paid.
fn counterparty<'a>(tx: &'a TransactionView, account: AccountId) -> (Element<'a, Message>, String) {
    let more = |n: usize| {
        if n > 1 {
            format!(" +{}", n - 1)
        } else {
            String::new()
        }
    };
    let memos = |ops: Vec<&OperationView>| {
        let refs: Vec<&str> = ops
            .iter()
            .map(|o| o.memo.as_str())
            .filter(|m| !m.is_empty())
            .collect();
        refs.first()
            .map_or_else(|| "—".to_owned(), |r| format!("{r}{}", more(refs.len())))
    };
    if tx.rewards(account) {
        return (
            t("Mining reward", ty::MONO_SMALL, color::TEXT_SECONDARY).into(),
            "—".to_owned(),
        );
    }
    let payees: Vec<&OperationView> = tx.paid_out(account).collect();
    let payers: Vec<&OperationView> = tx.paid_by(account).collect();
    let paid_out = tx.net(account) < 0 || payers.is_empty();
    if paid_out && !payees.is_empty() {
        let who: Element<'a, Message> = if payees.len() == 1 {
            party_link(payees[0], ty::MONO_SMALL)
        } else {
            t(
                format!("{} destinations", payees.len()),
                ty::MONO_SMALL,
                color::TEXT_SECONDARY,
            )
            .into()
        };
        return (who, memos(payees));
    }
    let received: Vec<&OperationView> = tx
        .destinations()
        .filter(|o| o.party == Party::Account(account))
        .collect();
    let who: Element<'a, Message> = match payers.first() {
        Some(op) if payers.len() == 1 => party_link(op, ty::MONO_SMALL),
        Some(op) => row![
            party_link(op, ty::MONO_SMALL),
            t(more(payers.len()), ty::MONO_SMALL, color::TEXT_MUTED)
        ]
        .into(),
        None => t("—", ty::MONO_SMALL, color::TEXT_MUTED).into(),
    };
    (who, memos(received))
}

// ---- Transaction -----------------------------------------------------------

/// One transaction, whole, as the index, a block or the queue listed it.
pub fn transaction<'a>(
    model: &'a Model,
    page: &'a WalletPage,
    tp: &'a TransactionPage,
) -> Element<'a, Message> {
    let compact = model.compact();
    let unit = model.prefs.unit;
    let tx = &tp.transaction;
    let mut body = vec![crumbs(compact, "Transactions", short(&tx.id))];
    let mut head = column![].spacing(sp::S8);
    match tx.block {
        Some(b) => {
            let mut line = row![
                t("In block", ty::BODY_SMALL, color::TEXT_SECONDARY),
                block_link(b, ty::BODY_SMALL)
            ]
            .spacing(sp::S8)
            .align_y(Alignment::Center);
            if let Some(n) = confirmations(b, tp.tip, tip(model)) {
                line = line.push(t(
                    format!(
                        "· {} {}",
                        ui::group(&n.to_string()),
                        if n == 1 {
                            "confirmation"
                        } else {
                            "confirmations"
                        }
                    ),
                    ty::BODY_SMALL,
                    color::TEXT_MUTED,
                ));
            }
            head = head.push(line);
        }
        None => {
            head = head.push(t(
                "Waiting in the mempool: in no block yet.",
                ty::BODY_SMALL,
                color::WARNING,
            ));
        }
    }
    if let Some(ms) = tx.time_ms {
        head = head.push(t(
            ui::full_time(ms, model.zone),
            ty::NOTE,
            color::TEXT_MUTED,
        ));
    }
    let mut card = column![head].spacing(sp::S20);
    for (kind, title) in [
        (OperationKind::Source, "From"),
        (OperationKind::Destination, "To"),
        (OperationKind::Reward, "Reward"),
    ] {
        let ops: Vec<&OperationView> = tx.operations.iter().filter(|o| o.kind == kind).collect();
        if ops.is_empty() {
            continue;
        }
        let mut list = column![ui::section_label(title)].spacing(sp::S10);
        for op in ops {
            let mut who = column![party_link(op, ty::MONO_SMALL)].spacing(sp::S2);
            if ours(model, Some(&op.party)) {
                who = who.push(t("this keystore's account", ty::TINY, color::ACCENT));
            }
            if !op.memo.is_empty() {
                who = who.push(t(op.memo.clone(), ty::MONO_TINY, color::TEXT_MUTED));
            }
            let figure = t(
                ui::amount(op.amount.unsigned_abs(), unit),
                ty::TABLE_AMOUNT,
                color::TEXT_PRIMARY,
            );
            list = list.push(if compact {
                // A memo can be wider than the box: the party takes half the
                // width and the amount the other half, so the amount is never
                // laid out in what the memo leaves it.
                row![
                    who.width(Length::FillPortion(1)),
                    container(figure.wrapping(Wrapping::WordOrGlyph))
                        .width(Length::FillPortion(1))
                        .align_x(Alignment::End),
                ]
                .spacing(sp::S12)
                .align_y(Alignment::Center)
            } else {
                row![who.width(Length::Fill), figure]
                    .spacing(sp::S12)
                    .align_y(Alignment::Center)
            });
        }
        card = card.push(
            container(list)
                .padding(sp::S14)
                .width(Length::Fill)
                .style(theme::field_box),
        );
    }
    // On a phone each fact is its label over its value; on the desktop the
    // label is beside it.
    let id_label = t("Transaction id", ty::BODY_SMALL, color::TEXT_SECONDARY);
    let id = t(tx.id.clone(), ty::MONO_SMALL, color::TEXT_PRIMARY).wrapping(Wrapping::WordOrGlyph);
    let id_fact: Element<'a, Message> = if compact {
        column![id_label, id].spacing(sp::S4).into()
    } else {
        row![
            id_label.width(Length::FillPortion(2)),
            id.width(Length::FillPortion(7)),
        ]
        .spacing(sp::S12)
        .into()
    };
    let mut facts = column![id_fact].spacing(sp::S10);
    let fee = tx.fee();
    if fee > 0 {
        let fee_label = t("Fee", ty::BODY_SMALL, color::TEXT_SECONDARY);
        let fee_value = t(
            format!("{} {}", ui::amount(fee, unit), ui::unit_name(unit)),
            ty::TABLE_BODY,
            color::TEXT_PRIMARY,
        );
        let fee_fact: Element<'a, Message> = if compact {
            column![fee_label, fee_value].spacing(sp::S4).into()
        } else {
            row![
                fee_label.width(Length::FillPortion(2)),
                fee_value.width(Length::FillPortion(7)),
            ]
            .spacing(sp::S12)
            .into()
        };
        facts = facts.push(fee_fact);
    }
    card = card.push(facts).push(ui::helper(if tp.from_index {
        "As the transaction index lists it: the source at its gross amount, and the change back \
         to it as a destination of its own."
    } else {
        "As its block or the mempool lists it: the source at what left it, net of its change, \
         which is not listed."
    }));
    body.push(ui::card_for(compact, card).into());
    frame(
        model,
        page,
        "Transaction",
        short(&tx.id),
        vec![copy(page, &tx.id, "Copy id")],
        body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_solve_time_reads_as_the_rendering_writes_it() {
        assert_eq!(duration(58_000), "58s");
        assert_eq!(duration(66_000), "1m 06s");
        assert_eq!(duration(949_000), "15m 49s");
        assert_eq!(duration(3_720_000), "1h 02m");
        assert_eq!(duration(-5), "0s", "a clock behind its parent's");
    }

    #[test]
    fn a_transactions_confirmations_are_counted_from_the_newer_tip() {
        // Its block read at 1,001, the explorer's chain at 1,000.
        assert_eq!(confirmations(1_001, Some(1_001), Some(1_000)), Some(1));
        assert_eq!(confirmations(995, Some(1_001), Some(1_000)), Some(7));
        // The explorer read since, further on.
        assert_eq!(confirmations(995, Some(1_001), Some(1_003)), Some(9));
        assert_eq!(confirmations(1_001, None, Some(1_000)), None, "above it");
        assert_eq!(confirmations(5, None, None), None, "no tip read");
        assert_eq!(confirmations(0, Some(u64::MAX), None), None, "overflows");
    }

    use crate::app::{Explored, Level, Missed, NARROWEST, NodeState};
    use crate::harness::{Driver, finger_lift, finger_move, finger_press, fits};
    use crate::ui::Zone;
    use iced::advanced::layout;
    use iced::advanced::widget::Tree;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use tawara_wallet_core::NetworkName;
    use tawara_wallet_core::explorer::{IndexState, LedgerRead, TagView};
    use tawara_wallet_core::preferences::Preferences;
    use tawara_wallet_core::sample;

    /// The width the phone's explorer is held to: the narrowest a phone is
    /// laid out at.
    const PHONE: f32 = NARROWEST;

    /// The sample store's explorer as a phone draws it: the chain, the queue
    /// and the node, read three seconds before the clock stands, as the
    /// screenshot example reads them.
    fn phone_model() -> Model {
        let mut model = Model::new(Preferences::default(), None);
        model.width = PHONE;
        model.clock_ms = sample::TIP_MS + 48_000;
        model.zone = Zone::Fixed(0);
        model.node = NodeState {
            url: Some("https://node.example".to_owned()),
            tip: Some((871_173, Duration::from_millis(42))),
            solved_ms: Some(sample::TIP_MS),
            network: Some(NetworkName {
                blockchain: "mochimo".to_owned(),
                network: "mainnet".to_owned(),
            }),
            ..NodeState::default()
        };
        model.wallet = Some(sample::wallet_view());
        model.mempool = read(sample::mempool());
        model.chain = read(sample::chain());
        model.pending = read(sample::pending());
        model.chain.read_ms = Some(model.clock_ms - 3_000);
        model
    }

    /// A value the explorer has read, with no read on its way.
    fn read<T>(value: T) -> Explored<T> {
        Explored {
            last: Some(Ok(value)),
            ..Explored::default()
        }
    }

    /// The overview with a hash typed in that names nothing (the example's
    /// `e1-explorer-missed`).
    fn missed_search() -> ExplorerPage {
        let typed = "7c3e9a1f05b84d62e1a9c7f30d58b2e46a1f9c03e7d5b28a4f6e1c90d02b5e84";
        let mut hash = [0u8; 32];
        for (i, b) in hash.iter_mut().enumerate() {
            *b = u8::from_str_radix(&typed[i * 2..i * 2 + 2], 16).unwrap_or(0);
        }
        let (transaction, block) = sample::not_found(hash);
        ExplorerPage {
            search: typed.to_owned(),
            missed: Some(Box::new(Missed {
                searched: true,
                transaction,
                block,
            })),
            ..ExplorerPage::default()
        }
    }

    /// The sample store's account, typed into the search field.
    fn typed_tag() -> TagPage {
        let view = sample::tag_view();
        TagPage {
            account: view.account,
            read: Some(Box::new(view)),
            reading_older: false,
            typed: true,
        }
    }

    /// An account another transaction named, which the node did not resolve.
    fn unresolved_tag() -> TagPage {
        let payee = sample::block().spends[0]
            .destinations()
            .find_map(|o| match o.party {
                Party::Account(id) => Some(id),
                _ => None,
            })
            .expect("the sample block's spend has a payee");
        TagPage {
            account: payee,
            read: Some(Box::new(TagView {
                account: payee,
                ledger: LedgerRead::Unresolved,
                history: Err(sample::index_refusal(IndexState::Absent)),
            })),
            reading_older: false,
            typed: false,
        }
    }

    /// The height `root` takes when laid out `width` wide, with room below it.
    fn height_of(mut root: Element<'_, Message>, width: f32) -> f32 {
        let renderer = crate::harness::renderer();
        let mut tree = Tree::new(root.as_widget());
        let limits = layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 4_000.0));
        root.as_widget_mut()
            .layout(&mut tree, &renderer, &limits)
            .bounds()
            .height
    }

    #[test]
    fn e1_fits_the_narrowest_phone() {
        let model = phone_model();
        let page = WalletPage::default();
        let plain = ExplorerPage::default();
        assert_eq!(fits(overview(&model, &page, &plain), PHONE), Ok(()));
        let search = missed_search();
        assert_eq!(fits(overview(&model, &page, &search), PHONE), Ok(()));
    }

    #[test]
    fn e2_fits_the_narrowest_phone() {
        let model = phone_model();
        let page = WalletPage::default();
        let read = BlockPage {
            at: BlockAt::Number(871_172),
            read: Some(Ok(Box::new(sample::block()))),
            rows_from: 0,
        };
        assert_eq!(fits(block(&model, &page, &read), PHONE), Ok(()));
    }

    #[test]
    fn e3_fits_the_narrowest_phone() {
        let model = phone_model();
        let page = WalletPage::default();
        assert_eq!(fits(tag(&model, &page, &typed_tag()), PHONE), Ok(()));
        // The unresolved account, with its report open, is the tallest.
        let mut open = WalletPage::default();
        open.open.insert(ReportKey::Ledger, Level::Summary);
        assert_eq!(fits(tag(&model, &open, &unresolved_tag()), PHONE), Ok(()));
    }

    #[test]
    fn e4_fits_the_narrowest_phone() {
        let model = phone_model();
        let page = WalletPage::default();
        let (found, text) = sample::transaction();
        let read = TransactionPage {
            transaction: found,
            from_index: true,
            text: Some(text),
            tip: None,
        };
        assert_eq!(fits(transaction(&model, &page, &read), PHONE), Ok(()));
    }

    #[test]
    fn e5_fits_the_narrowest_phone() {
        let model = phone_model();
        let page = WalletPage::default();
        let queued = QueuePage {
            read: Some(Ok(sample::queue())),
        };
        assert_eq!(fits(queue_page(&model, &page, &queued), PHONE), Ok(()));
    }

    #[test]
    fn the_phones_figures_are_two_to_a_row_with_the_last_row_keeping_the_width() {
        // Five figures: two rows of two, and a third row of one whose blank
        // stands for the second, so the tile keeps the width of the others.
        let model = phone_model();
        let figures: Vec<Element<'_, Message>> = (0..5)
            .map(|_| {
                space()
                    .width(Length::Fill)
                    .height(Length::Fixed(10.0))
                    .into()
            })
            .collect();
        let mut root = tiles(&model, figures, 3);
        let renderer = crate::harness::renderer();
        let mut tree = Tree::new(root.as_widget());
        let limits = layout::Limits::new(
            iced::Size::ZERO,
            iced::Size::new(PHONE - 2.0 * sp::S16, 4_000.0),
        );
        let node = root.as_widget_mut().layout(&mut tree, &renderer, &limits);
        let per_row: Vec<usize> = node
            .children()
            .iter()
            .map(|row| row.children().len())
            .collect();
        assert_eq!(per_row, [2, 2, 2], "the phone's figures, two to a row");
        let first = node.children()[0].children()[0].bounds().width;
        let lone = node.children()[2].children()[0].bounds().width;
        assert_eq!(lone, first, "a lone figure keeps the width of the others");
    }

    /// The layout `root` takes at `width` logical pixels, with room below it.
    fn laid_out(mut root: Element<'_, Message>, width: f32) -> layout::Node {
        let renderer = crate::harness::renderer();
        let mut tree = Tree::new(root.as_widget());
        let limits = layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 4_000.0));
        root.as_widget_mut().layout(&mut tree, &renderer, &limits)
    }

    /// The width `text` takes when nothing bounds it.
    fn natural_width(text: &str, face: ui::Type) -> f32 {
        let text: Element<'_, Message> = t(text.to_owned(), face, color::TEXT_PRIMARY).into();
        laid_out(text, 10_000.0).bounds().width
    }

    #[test]
    fn a_phone_queue_row_keeps_its_amount_whole() {
        // An amount narrower than its text is drawn past its row, over the
        // card's edge: the row gives the amount what its neighbour leaves.
        let model = phone_model();
        let pending = sample::pending();
        let r = &pending.rows[0];
        let tx = r
            .transaction
            .as_ref()
            .expect("the sample's first row was read");
        let sent = ui::amount(tx.sent(), model.prefs.unit);
        let root = laid_out(
            pending_row(true, r, 0, model.prefs.unit),
            PHONE - 2.0 * sp::S16,
        );
        // The row's container, its column, then its first line: the id, the
        // space and the amount.
        let amount = &root.children()[0].children()[0].children()[2];
        assert!(
            amount.bounds().width + 0.5 >= natural_width(&sent, ty::TABLE_AMOUNT),
            "{sent} is {} wide, but laid out {} wide",
            natural_width(&sent, ty::TABLE_AMOUNT),
            amount.bounds().width
        );
    }

    #[test]
    fn a_yours_row_keeps_its_amount_whole() {
        // The sample block's first spend is this store's: its row is the
        // highlighted one, with its id, the pill and the amount on one line.
        let model = phone_model();
        let detail = sample::block();
        let tx = &detail.spends[0];
        let id = link(short(&tx.id), ty::MONO_SMALL, WalletMsg::OpenSpend(0));
        let sent = ui::amount(tx.sent(), model.prefs.unit);
        let root = laid_out(tx_stacked(&model, id, tx), PHONE - 2.0 * sp::S16);
        // The highlight, its line, the column, then the first line: the id,
        // the pill, the space and the amount.
        let amount = &root.children()[0].children()[0].children()[0].children()[3];
        assert!(
            amount.bounds().width + 0.5 >= natural_width(&sent, ty::TABLE_AMOUNT),
            "{sent} is {} wide, but laid out {} wide",
            natural_width(&sent, ty::TABLE_AMOUNT),
            amount.bounds().width
        );
    }

    /// The rows of the queue a finger's press and lift at (`x`, `y`) open.
    fn opened_by_tap(driver: &mut Driver<'_, Message>, x: f32, y: f32) -> Vec<usize> {
        let (messages, _, _) = driver.send(&[finger_press(1, x, y), finger_lift(1, x, y)]);
        messages
            .iter()
            .filter_map(|m| match m {
                Message::Wallet(WalletMsg::OpenPending(n)) => Some(*n),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tap_on_a_queue_row_opens_it_and_a_drag_from_one_does_not() {
        let model = phone_model();
        let mut driver = Driver::new(queue(&model), (360, 1_200));
        // Where each row opens when tapped: the first and the last of the
        // heights down the middle of the card at which it does.
        let mut spans: BTreeMap<usize, (f32, f32)> = BTreeMap::new();
        for y in 0..1_200 {
            let y = y as f32;
            for n in opened_by_tap(&mut driver, 180.0, y) {
                spans
                    .entry(n)
                    .and_modify(|span| span.1 = y)
                    .or_insert((y, y));
            }
        }
        assert!(spans.len() >= 2, "rows open when tapped: {spans:?}");
        let (top, bottom) = spans[&0];
        assert!(
            bottom - top > 30.0,
            "a row is tall enough to drag within: {top}..{bottom}"
        );
        // A drag that starts on the first row and goes 20 pixels, still on
        // it, is a scroll, not a tap: the row publishes nothing.
        let (messages, _, _) = driver.send(&[
            finger_press(2, 180.0, top + 10.0),
            finger_move(2, 180.0, top + 30.0),
            finger_lift(2, 180.0, top + 30.0),
        ]);
        let opened: Vec<_> = messages
            .iter()
            .filter(|m| matches!(m, Message::Wallet(WalletMsg::OpenPending(_))))
            .collect();
        assert!(opened.is_empty(), "the drag opened a row: {opened:?}");
    }

    #[test]
    fn a_phone_haiku_is_as_tall_as_its_lines_wrap_to() {
        let mut detail = sample::block();
        if let Some(figures) = detail.figures.as_mut() {
            figures.haiku = vec![
                "the lantern light falls across the wet stones of the garden".to_owned(),
                "a heron stands in the shallows and does not move at all".to_owned(),
                "the evening bell is heard again beyond the hills".to_owned(),
            ];
        }
        let desktop = 3.0 * TILE + 2.0 * sp::S12;
        // The page's width at 360: the window less its 16-pixel margins.
        let phone = height_of(haiku_panel(true, &detail), PHONE - 2.0 * sp::S16);
        assert!(
            phone > desktop,
            "the phone's panel is {phone} tall, no taller than the desktop's {desktop}"
        );
        assert_eq!(
            height_of(haiku_panel(false, &detail), 1_000.0),
            desktop,
            "the desktop's panel keeps its height"
        );
    }
}
