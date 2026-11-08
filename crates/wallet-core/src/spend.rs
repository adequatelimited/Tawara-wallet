// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! A spend request, checked before anything is planned.
//!
//! The command-line wallet checks a spend's arguments in its parser
//! (`cli::args`), before it prompts or reserves anything, and those checks
//! are private to it. The same rules are applied here, to the same inputs,
//! so a spend the command line would refuse as a usage error is refused
//! here before the worker asks the node anything:
//!
//! - a destination is Base58 over a tag and its CRC16, or `0x` and forty hex
//!   characters; bare hex is refused, because it would also accept the
//!   second half of an eighty-character ledger address, a tag nobody holds;
//! - the all-zero tag is refused: its checksum is zero too, so the CRC16
//!   cannot catch it, and nothing can ever spend what is sent to it;
//! - a reference follows the node's rule (`mesh::spend::reference_is_valid`,
//!   worded by `cli::args::REFERENCE_RULE`), or is empty;
//! - one to 256 destinations, no tag twice;
//! - "everything" (the command line's `all`) only for a single destination;
//! - the fee is a total, and defaults to 500 nanoMCM per destination, the
//!   node's floor.
//!
//! Everything the node itself enforces (the fee floor, a zero amount, a
//! destination equal to the source, an amount over the balance) is
//! `SpendPlan::new`'s, in the library, and is not repeated.
//!
//! The checks produce the library's `cli::args::Spend`, the value the command
//! line's parser produces, so the worker lays it out with the command line's
//! `cli::plan_spend`.

use core::fmt;

use mochimo_crypto::addr::Tag;
use mochimo_crypto::cli::args::{Spend, SpendTo};
use mochimo_crypto::consts::{ADDR_REF_LEN, ADDR_TAG_LEN, MFEE};
use mochimo_crypto::mesh::spend::reference_is_valid;
use mochimo_crypto::tx::MAX_DESTINATIONS;

use crate::view::AccountId;

/// How much to send to one destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Amount {
    /// This many nanoMCM.
    Nano(u64),
    /// The whole balance less the fee, read from the same ledger
    /// observation the plan is built on, so the change is zero and the
    /// account is emptied. Only for a single destination.
    Everything,
}

/// One destination as entered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestinationInput {
    /// The destination as typed or pasted.
    pub to: String,
    pub amount: Amount,
    /// The reference field as typed; empty for none.
    pub reference: String,
}

/// A spend as requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpendRequest {
    /// The account the spend is from.
    pub from: AccountId,
    pub destinations: Vec<DestinationInput>,
    /// The fee as a total, or `None` for the node's floor (500 nanoMCM per
    /// destination).
    pub fee: Option<u64>,
    /// The block after which the spend may not be included, or 0 for none.
    pub blk_to_live: u64,
}

/// Why a spend was refused before anything was planned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpendInputError {
    /// No destinations, or more than the protocol carries.
    Count { given: usize },
    /// Destination `index` (one-based) is not a destination.
    NotADestination {
        index: usize,
        given: String,
        why: String,
    },
    /// Destination `index` is bare hex with no `0x`.
    BareHex { index: usize, given: String },
    /// Destination `index` is the all-zero tag.
    ZeroTag { index: usize, given: String },
    /// Destination `index`'s reference breaks the node's rule.
    Reference { index: usize, given: String },
    /// Two destinations have one tag.
    Duplicate { first: usize, second: usize },
    /// "Everything" with more than one destination.
    EverythingNeedsOneDestination,
}

impl fmt::Display for SpendInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let max = usize::from(MAX_DESTINATIONS);
        match self {
            SpendInputError::Count { given } => {
                write!(
                    f,
                    "a spend carries 1 to {max} destinations and {given} were given"
                )
            }
            SpendInputError::NotADestination { index, given, why } => write!(
                f,
                "destination {index}: `{given}` is not a destination -- {why}. A destination is \
                 Base58 over a tag and its CRC16, which is what every Mochimo wallet emits; `0x` \
                 and {} hex characters is accepted too.",
                ADDR_TAG_LEN * 2
            ),
            SpendInputError::BareHex { index, given } => write!(
                f,
                "destination {index}: `{given}` is a bare hex tag. Write it as `0x{given}` if \
                 that is what you mean. Hex carries no checksum, so it is accepted only when you \
                 say it is hex: the second half of an eighty-character ledger address is also \
                 forty hex characters, and it is a tag nobody holds."
            ),
            SpendInputError::ZeroTag { index, given } => write!(
                f,
                "destination {index}: `{given}` is the all-zero tag, and this wallet will not use \
                 it. Its checksum is zero as well -- crc16 of twenty zero bytes is 0 -- so the \
                 CRC16 that refuses every other mistyped destination cannot refuse this one. It is \
                 what an uninitialised or truncated buffer produces, the chain will credit it, and \
                 nothing can ever spend it again."
            ),
            SpendInputError::Reference { index, given } => {
                write!(
                    f,
                    "destination {index}: `{given}` is not a valid memo. A memo is up to 16 \
                     capital letters and digits, in groups of one kind separated by single \
                     hyphens, each group's neighbours of the other kind"
                )
            }
            SpendInputError::Duplicate { first, second } => write!(
                f,
                "destinations {first} and {second} are the same tag. The network would accept it, \
                 but one tag twice in one spend is almost always a mistyped payee and the money \
                 does not come back; send twice if you mean it"
            ),
            SpendInputError::EverythingNeedsOneDestination => f.write_str(
                "\"everything\" means the whole balance less the fee and is only available for a \
                 single destination; give each destination its own amount",
            ),
        }
    }
}

impl std::error::Error for SpendInputError {}

/// A destination as typed, to its tag. Public so that a page can check its
/// destinations before it asks for a plan.
/// `index` is the destination's place, one based, as the error names it.
pub fn parse_destination(text: &str, index: usize) -> Result<Tag, SpendInputError> {
    let t = text.trim();
    let tag = if let Some(hex) = t.strip_prefix("0x") {
        tag_from_hex(hex).ok_or_else(|| SpendInputError::NotADestination {
            index,
            given: t.to_owned(),
            why: format!(
                "`0x` must be followed by exactly {} hex characters",
                ADDR_TAG_LEN * 2
            ),
        })?
    } else if t.chars().count() == ADDR_TAG_LEN * 2 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SpendInputError::BareHex {
            index,
            given: t.to_owned(),
        });
    } else {
        mochimo_crypto::addr::tag_from_base58(t).map_err(|e| SpendInputError::NotADestination {
            index,
            given: t.to_owned(),
            why: e.to_string(),
        })?
    };
    if tag.iter().all(|&b| b == 0) {
        return Err(SpendInputError::ZeroTag {
            index,
            given: t.to_owned(),
        });
    }
    Ok(tag)
}

fn tag_from_hex(hex: &str) -> Option<Tag> {
    if hex.len() != ADDR_TAG_LEN * 2 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; ADDR_TAG_LEN];
    for (slot, pair) in out.iter_mut().zip(hex.as_bytes().chunks(2)) {
        let s = core::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(s, 16).ok()?;
    }
    Some(out)
}

/// A reference as typed, to the sixteen-byte field: NUL-padded, checked by
/// the node's rule. Empty text is the zero field. Public for the same reason
/// as [`parse_destination`].
pub fn parse_reference(text: &str, index: usize) -> Result<[u8; ADDR_REF_LEN], SpendInputError> {
    let mut field = [0u8; ADDR_REF_LEN];
    if text.is_empty() {
        return Ok(field);
    }
    let refused = || SpendInputError::Reference {
        index,
        given: text.to_owned(),
    };
    if !text.is_ascii() || text.len() > ADDR_REF_LEN {
        return Err(refused());
    }
    field[..text.len()].copy_from_slice(text.as_bytes());
    if !reference_is_valid(&field) {
        return Err(refused());
    }
    Ok(field)
}

/// Check a spend request by the command line's rules, to the spend its
/// parser would produce: [`Amount::Everything`] is an amount of `None`.
pub(crate) fn check(req: &SpendRequest) -> Result<Spend, SpendInputError> {
    let n = req.destinations.len();
    if n == 0 || n > usize::from(MAX_DESTINATIONS) {
        return Err(SpendInputError::Count { given: n });
    }
    let mut dsts: Vec<SpendTo> = Vec::with_capacity(n);
    for (i, d) in req.destinations.iter().enumerate() {
        let index = i + 1;
        let to = parse_destination(&d.to, index)?;
        let reference = parse_reference(&d.reference, index)?;
        let amount = match d.amount {
            Amount::Nano(v) => Some(v),
            Amount::Everything if n == 1 => None,
            Amount::Everything => return Err(SpendInputError::EverythingNeedsOneDestination),
        };
        if let Some(first) = dsts.iter().position(|e| e.to == to) {
            return Err(SpendInputError::Duplicate {
                first: first + 1,
                second: index,
            });
        }
        dsts.push(SpendTo {
            to,
            reference,
            amount,
        });
    }
    let count = u64::try_from(n).unwrap_or(u64::MAX);
    Ok(Spend {
        tag: req.from.tag(),
        dsts,
        fee_total: req.fee.unwrap_or(MFEE.saturating_mul(count)),
        blk_to_live: req.blk_to_live,
        expect_balance: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b58(tag: &Tag) -> String {
        mochimo_crypto::addr::tag_to_base58(tag).expect("tag renders")
    }

    fn req(dsts: Vec<DestinationInput>) -> SpendRequest {
        SpendRequest {
            from: AccountId::from_tag([7u8; 20]),
            destinations: dsts,
            fee: None,
            blk_to_live: 0,
        }
    }

    fn to(text: &str, amount: Amount) -> DestinationInput {
        DestinationInput {
            to: text.to_owned(),
            amount,
            reference: String::new(),
        }
    }

    #[test]
    fn base58_and_prefixed_hex_both_parse() {
        let tag = [0x05u8; 20];
        assert_eq!(parse_destination(&b58(&tag), 1), Ok(tag));
        assert_eq!(
            parse_destination(&format!("0x{}", "05".repeat(20)), 1),
            Ok(tag)
        );
    }

    #[test]
    fn bare_hex_zero_tag_and_typos_are_refused() {
        assert!(matches!(
            parse_destination(&"05".repeat(20), 1),
            Err(SpendInputError::BareHex { .. })
        ));
        assert!(matches!(
            parse_destination(&format!("0x{}", "00".repeat(20)), 1),
            Err(SpendInputError::ZeroTag { .. })
        ));
        assert!(matches!(
            parse_destination(&b58(&[0u8; 20]), 1),
            Err(SpendInputError::ZeroTag { .. })
        ));
        let mut typo = b58(&[0x05u8; 20]);
        let last = typo.pop().unwrap();
        typo.push(if last == 'a' { 'b' } else { 'a' });
        assert!(matches!(
            parse_destination(&typo, 1),
            Err(SpendInputError::NotADestination { .. })
        ));
        assert!(matches!(
            parse_destination("0x1234", 1),
            Err(SpendInputError::NotADestination { .. })
        ));
    }

    #[test]
    fn references_follow_the_nodes_rule() {
        assert_eq!(parse_reference("", 1), Ok([0u8; 16]));
        let r = parse_reference("AB-00-EF", 1).unwrap();
        assert_eq!(&r[..8], b"AB-00-EF");
        assert_eq!(&r[8..], &[0u8; 8]);
        for bad in ["AB-CD-EF", "ABC-", "-123", "abc", "ÀB", "ABCDEFGHIJKLMNOPQ"] {
            assert!(parse_reference(bad, 1).is_err(), "{bad}");
        }
        let text = parse_reference("ABC-", 2).unwrap_err().to_string();
        assert!(text.contains("is not a valid memo"), "{text}");
    }

    #[test]
    fn the_fee_defaults_to_the_floor_per_destination() {
        let a = b58(&[1u8; 20]);
        let b = b58(&[2u8; 20]);
        let c = check(&req(vec![
            to(&a, Amount::Nano(10)),
            to(&b, Amount::Nano(20)),
        ]))
        .unwrap();
        assert_eq!(c.fee_total, 1000);
        assert_eq!(c.dsts.len(), 2);
        assert_eq!(c.tag, [7u8; 20]);
    }

    #[test]
    fn everything_only_alone_and_no_tag_twice() {
        let a = b58(&[1u8; 20]);
        let b = b58(&[2u8; 20]);
        let one = check(&req(vec![to(&a, Amount::Everything)])).unwrap();
        assert!(one.spends_everything());
        assert_eq!(
            check(&req(vec![
                to(&a, Amount::Everything),
                to(&b, Amount::Nano(1))
            ])),
            Err(SpendInputError::EverythingNeedsOneDestination)
        );
        assert_eq!(
            check(&req(vec![
                to(&a, Amount::Nano(1)),
                to(&b, Amount::Nano(1)),
                to(&a, Amount::Nano(2))
            ])),
            Err(SpendInputError::Duplicate {
                first: 1,
                second: 3
            })
        );
    }

    #[test]
    fn one_to_two_hundred_fifty_six_destinations() {
        assert_eq!(
            check(&req(Vec::new())),
            Err(SpendInputError::Count { given: 0 })
        );
        let many: Vec<DestinationInput> = (0..257u32)
            .map(|i| {
                let mut tag = [9u8; 20];
                tag[..4].copy_from_slice(&i.to_be_bytes());
                to(&b58(&tag), Amount::Nano(1))
            })
            .collect();
        assert_eq!(
            check(&req(many)),
            Err(SpendInputError::Count { given: 257 })
        );
    }
}
