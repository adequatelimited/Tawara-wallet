// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Amounts: nanoMCM, the unit of the library and the wire, and MCM, the
//! unit a person reads and types. One MCM is 1,000,000,000 nanoMCM.
//!
//! Conversion is exact in both directions with no floating point. An amount
//! not representable in nanoMCM (a tenth decimal place) is refused, never
//! rounded, because a rounded payment is a different payment.

use core::fmt;

/// nanoMCM in one MCM.
pub const NANO_PER_MCM: u64 = 1_000_000_000;

/// Decimal places in an MCM amount.
pub const MCM_DECIMALS: usize = 9;

/// Why typed text is not an amount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmountError {
    /// Nothing was typed.
    Empty,
    /// A character other than digits and one decimal point.
    NotANumber,
    /// More decimal places than nanoMCM can hold.
    TooPrecise { decimals: usize },
    /// Larger than the protocol's 64-bit amount.
    TooLarge,
}

impl fmt::Display for AmountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AmountError::Empty => f.write_str("no amount was given"),
            AmountError::NotANumber => f.write_str(
                "an amount is digits with at most one decimal point, such as 12.5; no signs, \
                 spaces or thousands separators",
            ),
            AmountError::TooPrecise { decimals } => write!(
                f,
                "{decimals} decimal places is more than an amount can carry: one nanoMCM is \
                 0.000000001 MCM, nine places"
            ),
            AmountError::TooLarge => f.write_str("that amount is larger than any balance can be"),
        }
    }
}

impl std::error::Error for AmountError {}

/// MCM as a person types it (`12`, `12.5`, `0.000000001`) to nanoMCM.
pub fn parse_mcm(text: &str) -> Result<u64, AmountError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(AmountError::Empty);
    }
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t, ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return Err(AmountError::NotANumber);
    }
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AmountError::NotANumber);
    }
    if frac.len() > MCM_DECIMALS {
        return Err(AmountError::TooPrecise {
            decimals: frac.len(),
        });
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| AmountError::TooLarge)?
    };
    let mut frac_nano: u64 = 0;
    for (i, b) in frac.bytes().enumerate() {
        let digit = u64::from(b - b'0');
        let scale =
            10u64.pow(u32::try_from(MCM_DECIMALS - 1 - i).map_err(|_| AmountError::TooLarge)?);
        frac_nano += digit * scale;
    }
    whole
        .checked_mul(NANO_PER_MCM)
        .and_then(|n| n.checked_add(frac_nano))
        .ok_or(AmountError::TooLarge)
}

/// A whole number of nanoMCM as typed.
pub fn parse_nano(text: &str) -> Result<u64, AmountError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(AmountError::Empty);
    }
    if !t.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AmountError::NotANumber);
    }
    t.parse().map_err(|_| AmountError::TooLarge)
}

/// nanoMCM as MCM with all nine decimal places and no grouping:
/// `12480.537214906`, `0.000000500`.
#[must_use]
pub fn format_mcm(nano: impl Into<u128>) -> String {
    let nano = nano.into();
    let per = u128::from(NANO_PER_MCM);
    format!("{}.{:09}", nano / per, nano % per)
}

/// nanoMCM as MCM without the low-order zeros: `12480.537214906`, `3.2`,
/// `500`. Used for display; form fields use [`format_mcm`].
#[must_use]
pub fn format_mcm_short(nano: impl Into<u128>) -> String {
    let full = format_mcm(nano);
    full.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `digits` with a comma every three from the right: `1,402,624`.
#[must_use]
pub fn group(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// nanoMCM as MCM for reading: grouped, without the low-order zeros
/// (`9,215.402118`, `2,000`).
#[must_use]
pub fn show_mcm(nano: impl Into<u128>) -> String {
    let short = format_mcm_short(nano);
    match short.split_once('.') {
        Some((whole, frac)) => format!("{}.{frac}", group(whole)),
        None => group(&short),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_short_form_drops_only_low_order_zeros() {
        assert_eq!(format_mcm_short(500 * NANO_PER_MCM), "500");
        assert_eq!(format_mcm_short(3_200_000_000_u64), "3.2");
        assert_eq!(format_mcm_short(12_480_537_214_906_u64), "12480.537214906");
        assert_eq!(format_mcm_short(500_u64), "0.0000005");
        assert_eq!(format_mcm_short(0_u64), "0");
        assert_eq!(format_mcm_short(10 * NANO_PER_MCM), "10");
        assert_eq!(show_mcm(2_000 * NANO_PER_MCM), "2,000");
        assert_eq!(show_mcm(9_215_402_118_000_u64), "9,215.402118");
        assert_eq!(group("1402624"), "1,402,624");
        assert_eq!(group("100"), "100");
    }

    #[test]
    fn parses_exactly() {
        assert_eq!(parse_mcm("12"), Ok(12 * NANO_PER_MCM));
        assert_eq!(parse_mcm("12.5"), Ok(12_500_000_000));
        assert_eq!(parse_mcm(" 0.000000001 "), Ok(1));
        assert_eq!(parse_mcm(".5"), Ok(500_000_000));
        assert_eq!(parse_mcm("7."), Ok(7 * NANO_PER_MCM));
        assert_eq!(parse_mcm("12480.537214906"), Ok(12_480_537_214_906));
        assert_eq!(parse_mcm("18446744073.709551615"), Ok(u64::MAX));
    }

    #[test]
    fn refuses_rather_than_rounds() {
        assert_eq!(
            parse_mcm("0.0000000001"),
            Err(AmountError::TooPrecise { decimals: 10 })
        );
        assert_eq!(
            parse_mcm("18446744073.709551616"),
            Err(AmountError::TooLarge)
        );
        assert_eq!(
            parse_mcm("99999999999999999999"),
            Err(AmountError::TooLarge)
        );
        for bad in [
            "-1", "+1", "1,000", "1 000", "1e3", "1.2.3", ".", "abc", "١",
        ] {
            assert_eq!(parse_mcm(bad), Err(AmountError::NotANumber), "{bad}");
        }
        assert_eq!(parse_mcm("  "), Err(AmountError::Empty));
    }

    #[test]
    fn nano_is_whole_numbers_only() {
        assert_eq!(parse_nano("500"), Ok(500));
        assert_eq!(parse_nano("1.5"), Err(AmountError::NotANumber));
        assert_eq!(
            parse_nano("18446744073709551616"),
            Err(AmountError::TooLarge)
        );
    }

    #[test]
    fn formats_with_nine_places_and_round_trips() {
        assert_eq!(format_mcm(12_480_537_214_906_u64), "12480.537214906");
        assert_eq!(format_mcm(500_u64), "0.000000500");
        assert_eq!(format_mcm(0_u64), "0.000000000");
        for n in [0, 1, 500, NANO_PER_MCM, 12_480_537_214_906, u64::MAX] {
            assert_eq!(parse_mcm(&format_mcm(n)), Ok(n));
        }
        // A sum of balances can pass u64; it is written exactly.
        assert_eq!(
            format_mcm(u128::from(u64::MAX) + 5),
            "18446744073.709551620"
        );
    }
}
