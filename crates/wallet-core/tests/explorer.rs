// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The explorer's reads against the scripted node: the chain, the queue, a
//! block, a tag and a hash. None needs a store.

mod support;

use support::*;
use tawara_wallet_core::explorer::{
    BlockAt, BlockKind, Found, IndexState, LATEST_BLOCKS, LedgerRead, OperationKind, Party,
    QUEUE_ROWS, Query,
};
use tawara_wallet_core::view::AccountId;
use tawara_wallet_core::{Activity, Command, Refusal, RefusalKind, Reply};

fn refusal(reply: Reply) -> Refusal {
    match reply {
        Reply::Refused(r) => r,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn id(n: u8) -> [u8; 32] {
    [n; 32]
}

fn account(n: u32) -> AccountId {
    AccountId::from_tag(tag(n))
}

/// The scripted chain's hash for block `index`: its number, in hex.
fn block_hash(index: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&index.to_be_bytes());
    out
}

#[test]
fn the_chain_is_read_with_each_blocks_solve_time_and_the_average() {
    let mut h = Harness::with_node();
    h.chain.pseudo(996);
    match h.call(Command::Chain) {
        Reply::Chain(Ok(view)) => {
            assert_eq!(view.tip, 1_000);
            let indexes: Vec<u64> = view.blocks.iter().map(|b| b.index).collect();
            assert_eq!(indexes, (991..=1_000).rev().collect::<Vec<_>>());
            assert_eq!(view.blocks.len() as u64, LATEST_BLOCKS);
            // The scripted chain makes a block a minute; the last one shown
            // has its time too, from the block below it read for it.
            assert!(
                view.blocks.iter().all(|b| b.solve_ms == Some(60_000)),
                "{:?}",
                view.blocks
            );
            assert_eq!(view.average_ms, Some(60_000));
            let newest = &view.blocks[0];
            let reward = newest.reward.as_ref().expect("a normal block's reward");
            assert_eq!(reward.to, Party::Account(account(MINER)));
            assert_eq!(reward.amount, u128::from(REWARD));
            let pseudo = &view.blocks[4];
            assert_eq!(pseudo.kind, Some(BlockKind::Pseudo));
            assert_eq!(pseudo.reward, None, "a pseudo-block pays no one");
            assert!(view.text.contains("the 10 newest"), "{}", view.text);
            assert!(
                !view.text.contains(" 990 "),
                "the page is for the blocks shown: {}",
                view.text
            );
        }
        other => panic!("expected the chain, got {other:?}"),
    }
    // The chain reorganized between reading 998 and 997: 998 names a parent
    // that is not the 997 read, so no solve time is told between them.
    h.chain.fork(998);
    match h.call(Command::Chain) {
        Reply::Chain(Ok(view)) => {
            let solves: Vec<Option<i64>> = view.blocks.iter().map(|b| b.solve_ms).collect();
            assert_eq!(solves[2], None, "998 and 997 are of different branches");
            assert!(
                solves
                    .iter()
                    .enumerate()
                    .all(|(i, s)| i == 2 || *s == Some(60_000)),
                "{solves:?}"
            );
        }
        other => panic!("expected the chain, got {other:?}"),
    }
    // The block a span below the tip is not served: the blocks are still
    // shown, with no average.
    h.chain.unserve(900);
    match h.call(Command::Chain) {
        Reply::Chain(Ok(view)) => {
            assert_eq!(view.blocks.len() as u64, LATEST_BLOCKS);
            assert_eq!(view.average_ms, None);
        }
        other => panic!("expected the chain, got {other:?}"),
    }
    // A chain shorter than the span has no average either.
    h.chain.set_tip(60);
    match h.call(Command::Chain) {
        Reply::Chain(Ok(view)) => assert_eq!(view.average_ms, None),
        other => panic!("expected the chain, got {other:?}"),
    }
    // A block of the walk not served: nothing is shown as the walk.
    h.chain.unserve(55);
    match h.call(Command::Chain) {
        Reply::Chain(Err(refused)) => {
            assert_eq!(refused.index, None, "blocks are not the index");
            assert!(!refused.text.is_empty());
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(
        refusal(Harness::new().call(Command::Chain)).kind,
        RefusalKind::NoNode
    );
}

/// The explorer's reads run on the worker that holds an open store, and
/// none of the library's calls looks for a stop: a cancel stops each walk
/// between two requests.
#[test]
fn a_block_time_out_of_all_reason_is_no_solve_time_and_stops_nothing() {
    // The Mesh parser takes any i64 for a block's time: the differences
    // overflow, and the worker still answers.
    let mut h = Harness::with_node();
    h.chain.set_time(1_000, i64::MAX);
    h.chain.set_time(999, i64::MIN);
    h.chain.set_time(900, i64::MIN);
    // Older than the block below it: no time either, not a negative one.
    h.chain.set_time(995, 0);
    match h.call(Command::Chain) {
        Reply::Chain(Ok(view)) => {
            assert_eq!(view.blocks[0].solve_ms, None, "MAX less MIN");
            assert_eq!(view.blocks[1].solve_ms, None, "MIN less 998's");
            assert_eq!(view.blocks[5].solve_ms, None, "older than 994");
            assert_eq!(view.blocks[4].solve_ms, Some(996 * 60_000), "a long solve");
            assert_eq!(view.blocks[2].solve_ms, Some(60_000), "the rest as before");
            assert_eq!(view.average_ms, None, "MAX less MIN");
        }
        other => panic!("expected the chain, got {other:?}"),
    }
    match h.call(Command::Block(BlockAt::Number(1_000))) {
        Reply::Block(Ok(block)) => assert_eq!(block.summary.solve_ms, None),
        other => panic!("expected the block, got {other:?}"),
    }
    // The worker is still there for the next command.
    assert!(matches!(
        h.call(Command::Block(BlockAt::Number(998))),
        Reply::Block(Ok(_))
    ));
}

#[test]
fn a_cancel_stops_each_explorer_read_between_requests() {
    let walks = [
        ("chain", Command::Chain),
        ("pending", Command::Pending { count: 5 }),
        ("block", Command::Block(BlockAt::Number(995))),
        ("tag", Command::Tag(account(5))),
        ("find", Command::Find([9; 32])),
    ];
    for (what, command) in walks {
        let mut h = Harness::with_node();
        h.chain.queue(&[id(1), id(2)]);
        let gate = h.chain.close_gate();
        let calls = h.chain.calls();
        let sent = h.handle.send(command).expect("worker running");
        let _: Activity = h.wait_busy(sent);
        gate.wait_reached(1);
        h.handle.cancel();
        gate.open();
        assert_eq!(
            refusal(h.wait_for(sent)).kind,
            RefusalKind::Cancelled,
            "{what}"
        );
        assert_eq!(
            h.chain.calls() - calls,
            1,
            "{what}: one request, and none after the cancel"
        );
    }
}

#[test]
fn the_queue_is_read_with_its_first_transactions_whole() {
    let mut h = Harness::with_node();
    h.chain
        .queue(&[id(1), id(2), id(3), id(4), id(5), id(6), id(7)]);
    h.chain
        .pending(id(1), tag(5), &[(tag(6), 700), (tag(7), 300)]);
    h.chain.pending(id(2), tag(8), &[(tag(5), 40)]);
    // The third left the queue before it was asked for.
    for n in 4..=7 {
        h.chain.pending(id(n), tag(9), &[(tag(5), 1)]);
    }
    let calls = h.chain.calls();
    match h.call(Command::Pending { count: 5 }) {
        Reply::Pending(Ok(view)) => {
            assert_eq!(view.waiting, 7);
            assert_eq!(view.rows.len(), 5, "the first five read whole");
            let first = view.rows[0].transaction.as_ref().expect("read");
            assert_eq!(first.destinations().count(), 2);
            assert_eq!(first.sent(), 1_000, "what it sent, the change not among it");
            assert_eq!(first.fee(), u128::from(FEE));
            assert_eq!(first.source(), Some(&Party::Account(account(5))));
            assert_eq!(view.rows[2].transaction, None, "it left the queue");
            assert!(view.text.contains("left the queue"), "{}", view.text);
        }
        other => panic!("expected the queue, got {other:?}"),
    }
    assert_eq!(
        h.chain.calls() - calls,
        6,
        "the ids, then five transactions"
    );
    // All of it, as "View all pending" asks: up to the command line's most.
    let calls = h.chain.calls();
    match h.call(Command::Pending {
        count: QUEUE_ROWS + 1,
    }) {
        Reply::Pending(Ok(view)) => {
            assert_eq!((view.waiting, view.rows.len()), (7, 7));
            assert!(view.rows[6].transaction.is_some());
        }
        other => panic!("expected the queue, got {other:?}"),
    }
    assert_eq!(
        h.chain.calls() - calls,
        8,
        "the ids, then every transaction"
    );
}

#[test]
fn a_block_is_read_whole_with_its_solve_time_and_confirmations() {
    let mut h = Harness::with_node();
    h.chain
        .block_spend(995, id(1), tag(5), &[(tag(6), 700), (tag(7), 300)]);
    h.chain.block_spend(995, id(2), tag(8), &[(tag(5), 40)]);
    let by_number = match h.call(Command::Block(BlockAt::Number(995))) {
        Reply::Block(Ok(block)) => block,
        other => panic!("expected the block, got {other:?}"),
    };
    let b = &by_number;
    assert_eq!(b.summary.index, 995);
    assert_eq!(b.summary.kind, Some(BlockKind::Normal));
    assert_eq!(b.summary.solve_ms, Some(60_000));
    assert_eq!(b.confirmations(), Some(6), "995 to 1,000");
    assert_eq!(b.parent, 994);
    let reward = b.summary.reward.as_ref().expect("its reward");
    assert_eq!(reward.to, Party::Account(account(MINER)));
    assert_eq!(b.spends.len(), 2, "the reward is not a spend");
    assert_eq!(b.spends[0].block, Some(995));
    assert_eq!(b.spends[0].time_ms, Some(995 * 60_000));
    assert_eq!(b.moved(), 1_040);
    assert_eq!(b.paid(), 3);
    assert_eq!(b.fees, 2 * u128::from(FEE));
    let figures = b.figures.as_ref().expect("its figures");
    assert_eq!(
        figures.haiku,
        vec![
            "winter frost settles",
            "the API keeps its quiet count",
            "block after block"
        ]
    );
    assert_eq!(figures.counted, 2);
    assert!(b.text.contains("block 995"), "{}", b.text);

    // By its hash: the same block.
    match h.call(Command::Block(BlockAt::Hash(block_hash(995)))) {
        Reply::Block(Ok(block)) => assert_eq!(block.summary, by_number.summary),
        other => panic!("expected the block, got {other:?}"),
    }

    // A pseudo-block: no reward, no haiku.
    h.chain.pseudo(990);
    match h.call(Command::Block(BlockAt::Number(990))) {
        Reply::Block(Ok(block)) => {
            assert_eq!(block.summary.kind, Some(BlockKind::Pseudo));
            assert_eq!(block.summary.reward, None);
            assert!(block.figures.expect("figures").haiku.is_empty());
        }
        other => panic!("expected the block, got {other:?}"),
    }

    // The block below not served: the block is shown without its time.
    h.chain.unserve(980);
    match h.call(Command::Block(BlockAt::Number(981))) {
        Reply::Block(Ok(block)) => assert_eq!(block.summary.solve_ms, None),
        other => panic!("expected the block, got {other:?}"),
    }

    // Block 1's parent is genesis, which `/block` cannot be asked for.
    let calls = h.chain.calls();
    match h.call(Command::Block(BlockAt::Number(1))) {
        Reply::Block(Ok(block)) => assert_eq!(block.summary.solve_ms, None),
        other => panic!("expected the block, got {other:?}"),
    }
    assert_eq!(
        h.chain.calls() - calls,
        2,
        "the block and the tip, no parent"
    );

    // Above the tip: the library's page for the refusal.
    match h.call(Command::Block(BlockAt::Number(2_000))) {
        Reply::Block(Err(refused)) => {
            assert_eq!(refused.index, None);
            assert!(!refused.text.is_empty());
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_tag_is_read_from_the_ledger_and_from_the_index() {
    let mut h = Harness::with_node();
    h.chain.hold(tag(5), address(5, 3), 7_000);
    h.chain
        .index_transfer(tag(5), tag(6), 400, 600, 990, "INV-1");
    h.chain.index_transfer(tag(8), tag(5), 40, 0, 995, "");
    match h.call(Command::Tag(account(5))) {
        Reply::Tag(view) => {
            assert_eq!(view.account, account(5));
            assert_eq!(
                view.ledger,
                LedgerRead::Held {
                    address: format!("0x{}", hex_of(&address(5, 3))),
                    balance: 7_000
                }
            );
            let history = view.history.expect("the index's rows");
            assert_eq!(history.total, 2);
            assert_eq!(history.transactions[0].block, Some(995), "newest first");
        }
        other => panic!("expected the tag, got {other:?}"),
    }
    // The node did not resolve it: shown as what the node said.
    match h.call(Command::Tag(account(9))) {
        Reply::Tag(view) => assert_eq!(view.ledger, LedgerRead::Unresolved),
        other => panic!("expected the tag, got {other:?}"),
    }
    // No index: the ledger still answers, and the history says why not.
    h.chain.set_no_index(true);
    match h.call(Command::Tag(account(5))) {
        Reply::Tag(view) => {
            assert!(matches!(view.ledger, LedgerRead::Held { .. }));
            let refused = view.history.expect_err("no index");
            assert_eq!(refused.index, Some(IndexState::Absent));
        }
        other => panic!("expected the tag, got {other:?}"),
    }
    h.chain.set_no_index(false);
    // An older page, from an offset.
    match h.call(Command::TagHistory {
        account: account(5),
        from: 1,
    }) {
        Reply::TagHistory(Ok(older)) => {
            assert_eq!(older.transactions.len(), 1);
            assert_eq!(older.transactions[0].block, Some(990));
            assert_eq!(older.next, 2);
            assert!(!older.more());
        }
        other => panic!("expected an older page, got {other:?}"),
    }
}

#[test]
fn a_hash_finds_a_transaction_then_a_block() {
    let mut h = Harness::with_node();
    h.chain
        .index_transfer(tag(5), tag(6), 400, 600, 990, "INV-1");
    // The scripted index numbers its transactions from 1.
    let mut first = [0u8; 32];
    first[31] = 1;
    match h.call(Command::Find(first)) {
        Reply::Found(found) => match *found {
            Found::Transaction { transaction, text } => {
                assert_eq!(transaction.block, Some(990));
                assert!(
                    transaction
                        .operations
                        .iter()
                        .any(|o| o.kind == OperationKind::Destination && o.memo == "INV-1")
                );
                assert!(!text.is_empty());
            }
            other => panic!("expected the transaction, got {other:?}"),
        },
        other => panic!("expected an answer, got {other:?}"),
    }
    match h.call(Command::Find(block_hash(995))) {
        Reply::Found(found) => match *found {
            Found::Block(block) => assert_eq!(block.summary.index, 995),
            other => panic!("expected the block, got {other:?}"),
        },
        other => panic!("expected an answer, got {other:?}"),
    }
    match h.call(Command::Find(id(0xee))) {
        Reply::Found(found) => match *found {
            Found::Neither {
                searched,
                transaction,
                block,
            } => {
                assert!(searched, "the index answered: none");
                assert_eq!(transaction.index, None);
                assert!(!transaction.text.is_empty());
                assert!(!block.text.is_empty());
            }
            other => panic!("expected neither, got {other:?}"),
        },
        other => panic!("expected an answer, got {other:?}"),
    }
    // No index: a block is still found by its hash, and a miss says the
    // node runs no index.
    h.chain.set_no_index(true);
    match h.call(Command::Find(block_hash(995))) {
        Reply::Found(found) => assert!(matches!(*found, Found::Block(_))),
        other => panic!("expected an answer, got {other:?}"),
    }
    match h.call(Command::Find(id(0xee))) {
        Reply::Found(found) => match *found {
            Found::Neither {
                searched,
                transaction,
                ..
            } => {
                assert!(!searched, "the index was not read");
                assert_eq!(transaction.index, Some(IndexState::Absent));
            }
            other => panic!("expected neither, got {other:?}"),
        },
        other => panic!("expected an answer, got {other:?}"),
    }
}

#[test]
fn the_search_field_is_read_as_the_command_line_reads_its_arguments() {
    assert_eq!(Query::parse(" 871172 "), Ok(Query::Block(871_172)));
    assert!(Query::parse("0").expect_err("genesis").contains("block 0"));
    let hash = "7c".repeat(32);
    assert_eq!(Query::parse(&hash), Ok(Query::Hash([0x7c; 32])));
    assert_eq!(
        Query::parse(&format!("0x{hash}")),
        Ok(Query::Hash([0x7c; 32]))
    );
    // Sixty-four decimal digits are a hash, not a number.
    let digits = "1".repeat(64);
    assert_eq!(Query::parse(&digits), Ok(Query::Hash([0x11; 32])));
    let t = account(5);
    assert_eq!(Query::parse(&t.hex()), Ok(Query::Tag(t)));
    let base58 = t.destination().expect("a destination");
    assert_eq!(Query::parse(&base58), Ok(Query::Tag(t)));
    // A typo the checksum catches.
    let mut typo: Vec<char> = base58.chars().collect();
    typo[3] = if typo[3] == '2' { '3' } else { '2' };
    let typo: String = typo.into_iter().collect();
    assert!(Query::parse(&typo).is_err(), "{typo}");
    // Bare hex is not a tag: half a ledger address looks the same.
    let bare = t.hex().trim_start_matches("0x").to_owned();
    assert!(
        Query::parse(&bare)
            .expect_err("bare")
            .contains(&format!("0x{bare}")),
        "the refusal says how to write it"
    );
    assert!(Query::parse(&format!("0x{}", "ab".repeat(40))).is_err());
    assert!(
        Query::parse(&format!("0x{}", "00".repeat(20)))
            .expect_err("zero")
            .contains("all-zero")
    );
    assert!(Query::parse("").is_err());
    assert!(Query::parse("99999999999999999999").is_err());
    assert!(Query::parse("0xzz").is_err());
}
