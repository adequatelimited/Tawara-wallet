// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! A reader of the codes the Receive page draws, for its tests: version 3 at
//! error correction Q, read module by module by the standard's layout
//! (ISO/IEC 18004), with every block checked against its error correction
//! codewords. It is written here rather than taken from a decoder crate such
//! as `rqrr`, so the tests add no dependency.
//!
//! The codes under test are drawn, not damaged, so nothing is corrected: a
//! code is read only when both copies of the format information, every
//! codeword, every segment and the padding are where the standard puts them,
//! and one module of a codeword out of place is an error. The finder, timing
//! and alignment patterns and the seven remainder modules are not read.

/// The quiet zone the reader expects round the code, in modules.
const QUIET: usize = 4;

/// The modules across a version 3 code, without its quiet zone.
const W: usize = 29;

/// The modules across a version 3 code with its quiet zone.
pub(super) const SIDE: usize = W + 2 * QUIET;

/// The codewords of a version 3 code: 70, in two blocks of 17 data and 18
/// error correction codewords each at level Q. Seven remainder modules follow
/// them, which carry nothing.
const CODEWORDS: usize = 70;
const BLOCKS: usize = 2;
const DATA: usize = 17;
const EC: usize = 18;

/// The format information's mask, which the standard XORs into the BCH
/// code of the EC level and mask (ISO/IEC 18004, 7.9.4).
const FORMAT_MASK: u32 = 0x5412;

/// The alphanumeric mode's characters, by their values.
const ALPHANUMERIC: &[u8; 45] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

/// The text a code encodes, read from its modules with the quiet zone
/// included. It reads version 3 at error correction Q and nothing else. Each
/// block must satisfy its 18 error correction codewords, so a module of the
/// data or of the error correction that is not the encoder's is an error,
/// never a different text.
pub(super) fn decode(grid: &[Vec<bool>]) -> Result<String, String> {
    let side = grid.len();
    if side != SIDE || grid.iter().any(|row| row.len() != side) {
        return Err(format!("{side} modules across, not {SIDE}"));
    }
    for (r, row) in grid.iter().enumerate() {
        for (c, &dark) in row.iter().enumerate() {
            let inside = (QUIET..side - QUIET).contains(&r) && (QUIET..side - QUIET).contains(&c);
            if dark && !inside {
                return Err(format!("a dark module in the quiet zone at {r},{c}"));
            }
        }
    }
    let module = |r: usize, c: usize| grid[r + QUIET][c + QUIET];

    // The format information, twice: round the top-left finder, and split
    // between the bottom-left and the top-right. The first bit read is the
    // most significant.
    let first: [(usize, usize); 15] = [
        (8, 0),
        (8, 1),
        (8, 2),
        (8, 3),
        (8, 4),
        (8, 5),
        (8, 7),
        (8, 8),
        (7, 8),
        (5, 8),
        (4, 8),
        (3, 8),
        (2, 8),
        (1, 8),
        (0, 8),
    ];
    let second: Vec<(usize, usize)> = (W - 7..W)
        .rev()
        .map(|r| (r, 8))
        .chain((W - 8..W).map(|c| (8, c)))
        .collect();
    let read = |at: &[(usize, usize)]| {
        at.iter()
            .fold(0u32, |bits, &(r, c)| (bits << 1) | u32::from(module(r, c)))
    };
    let format = read(&first);
    if read(&second) != format {
        return Err("the two copies of the format information differ".to_owned());
    }
    let data = (0..32u32)
        .find(|&d| format_code(d) == format)
        .ok_or_else(|| format!("{format:015b} is no format information"))?;
    if data >> 3 != 0b11 {
        return Err(format!(
            "error correction bits {:02b}, not 11 (Q)",
            data >> 3
        ));
    }
    let mask = (data & 7) as usize;

    // The codewords, read in the zigzag and unmasked as they are read.
    let stream: Vec<bool> = placement()
        .into_iter()
        .map(|(r, c)| module(r, c) ^ mask_bit(mask, r, c))
        .collect();
    let mut codewords = [0u8; CODEWORDS];
    for (k, byte) in codewords.iter_mut().enumerate() {
        *byte = stream[8 * k..8 * k + 8]
            .iter()
            .fold(0u8, |v, &bit| (v << 1) | u8::from(bit));
    }

    // The blocks are interleaved a codeword from each in turn: first the
    // data codewords, then the error correction codewords.
    let mut blocks = [[0u8; DATA + EC]; BLOCKS];
    for (k, &byte) in codewords.iter().enumerate() {
        if k < BLOCKS * DATA {
            blocks[k % BLOCKS][k / BLOCKS] = byte;
        } else {
            let k = k - BLOCKS * DATA;
            blocks[k % BLOCKS][DATA + k / BLOCKS] = byte;
        }
    }
    for (b, block) in blocks.iter().enumerate() {
        if syndromes(block, EC).iter().any(|&s| s != 0) {
            return Err(format!(
                "block {b} does not agree with its error correction codewords"
            ));
        }
    }
    let message: Vec<u8> = blocks
        .iter()
        .flat_map(|block| &block[..DATA])
        .copied()
        .collect();

    let mut reader = Reader {
        bytes: &message,
        at: 0,
    };
    let mut text = Vec::new();
    loop {
        // The terminator may be cut short when the message fills the code.
        if reader.left() < 4 {
            if reader.take(reader.left())? != 0 {
                return Err("the message ends in bits that are no terminator".to_owned());
            }
            break;
        }
        match reader.take(4)? {
            0b0000 => break,
            0b0001 => {
                // Numeric: three digits in ten bits, then two in seven or one in four.
                let mut left = reader.take(10)?;
                while left >= 3 {
                    let v = reader.take(10)?;
                    if v > 999 {
                        return Err(format!("{v} is no three digits"));
                    }
                    text.extend(format!("{v:03}").bytes());
                    left -= 3;
                }
                if left > 0 {
                    let width = if left == 2 { 7 } else { 4 };
                    let v = reader.take(width)?;
                    if v >= 10usize.pow(left as u32) {
                        return Err(format!("{v} is no {left} digits"));
                    }
                    text.extend(format!("{v:0left$}").bytes());
                }
            }
            0b0010 => {
                // Alphanumeric: two characters in eleven bits, then one in six.
                let mut left = reader.take(9)?;
                while left >= 2 {
                    let v = reader.take(11)?;
                    text.push(alphanumeric(v / 45)?);
                    text.push(alphanumeric(v % 45)?);
                    left -= 2;
                }
                if left == 1 {
                    let v = reader.take(6)?;
                    text.push(alphanumeric(v)?);
                }
            }
            0b0100 => {
                // Byte: eight bits a character.
                let n = reader.take(8)?;
                for _ in 0..n {
                    text.push(reader.take(8)? as u8);
                }
            }
            mode => return Err(format!("mode {mode:04b} is none of the three read here")),
        }
    }

    padding(&message, reader.at)?;
    String::from_utf8(text).map_err(|e| e.to_string())
}

/// Whether `message` is padded as the standard pads it after the terminator,
/// which ends at bit `at`: zeros to the end of the byte, then the two pad
/// codewords in turn to the end of the message.
fn padding(message: &[u8], at: usize) -> Result<(), String> {
    let mut reader = Reader { bytes: message, at };
    let to_byte = (8 - at % 8) % 8;
    if reader.take(to_byte)? != 0 {
        return Err("the bits after the terminator are not zero".to_owned());
    }
    for (i, &byte) in message[reader.at / 8..].iter().enumerate() {
        let pad = if i % 2 == 0 { 0xEC } else { 0x11 };
        if byte != pad {
            return Err(format!(
                "{byte:#04x} where the pad codeword {pad:#04x} goes"
            ));
        }
    }
    Ok(())
}

/// The modules that hold the codewords, in the order they are read: the
/// zigzag from the bottom-right corner, two columns at a time, skipping the
/// function patterns and the vertical timing pattern. Positions are inside
/// the quiet zone. There are 567: the 560 bits of the 70 codewords, then the
/// seven remainder modules.
pub(super) fn placement() -> Vec<(usize, usize)> {
    let function = |r: usize, c: usize| {
        (r <= 8 && (c <= 8 || c >= W - 8))
            || (r >= W - 8 && c <= 8)
            || r == 6
            || c == 6
            || ((20..=24).contains(&r) && (20..=24).contains(&c))
    };
    let mut out = Vec::new();
    let mut col = W - 1;
    let mut upward = true;
    loop {
        if col == 6 {
            col = 5;
        }
        let rows: Vec<usize> = if upward {
            (0..W).rev().collect()
        } else {
            (0..W).collect()
        };
        for r in rows {
            for c in [col, col - 1] {
                if !function(r, c) {
                    out.push((r, c));
                }
            }
        }
        upward = !upward;
        if col <= 2 {
            break;
        }
        col -= 2;
    }
    out
}

/// The position in the drawn grid, quiet zone included, of the module at
/// `at` inside it.
pub(super) fn in_grid(at: (usize, usize)) -> (usize, usize) {
    (at.0 + QUIET, at.1 + QUIET)
}

/// The bits of a message, read most significant first.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn left(&self) -> usize {
        8 * self.bytes.len() - self.at
    }

    fn take(&mut self, n: usize) -> Result<usize, String> {
        if n > self.left() {
            return Err("the message ends inside a segment".to_owned());
        }
        let mut v = 0;
        for i in self.at..self.at + n {
            v = (v << 1) | usize::from((self.bytes[i / 8] >> (7 - i % 8)) & 1);
        }
        self.at += n;
        Ok(v)
    }
}

fn alphanumeric(i: usize) -> Result<u8, String> {
    ALPHANUMERIC
        .get(i)
        .copied()
        .ok_or_else(|| format!("{i} is no alphanumeric character"))
}

/// The format information the standard gives `data`, its two error
/// correction bits and three mask bits: a BCH code of fifteen bits, masked
/// with [`FORMAT_MASK`].
fn format_code(data: u32) -> u32 {
    let mut rem = data << 10;
    for bit in (10..15).rev() {
        if rem & (1 << bit) != 0 {
            rem ^= 0x537 << (bit - 10);
        }
    }
    ((data << 10) | rem) ^ FORMAT_MASK
}

/// Whether the module at `r`, `c` is inverted by `mask`, by the standard's
/// eight patterns (ISO/IEC 18004, 7.8.2).
fn mask_bit(mask: usize, r: usize, c: usize) -> bool {
    match mask {
        0 => (r + c).is_multiple_of(2),
        1 => r.is_multiple_of(2),
        2 => c.is_multiple_of(3),
        3 => (r + c).is_multiple_of(3),
        4 => (r / 2 + c / 3).is_multiple_of(2),
        5 => (r * c) % 2 + (r * c) % 3 == 0,
        6 => ((r * c) % 2 + (r * c) % 3).is_multiple_of(2),
        _ => ((r + c) % 2 + (r * c) % 3).is_multiple_of(2),
    }
}

/// The powers of the field's generator, 2, in GF(256) under the standard's
/// polynomial x^8 + x^4 + x^3 + x^2 + 1 (0x11D), and their logarithms.
fn field() -> ([u8; 255], [u8; 256]) {
    let mut exp = [0u8; 255];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    for (i, e) in exp.iter_mut().enumerate() {
        *e = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11D;
        }
    }
    (exp, log)
}

/// The syndromes of a Reed-Solomon `block` with `ec` error correction
/// codewords at its end: the block, as a polynomial whose first codeword is
/// the highest power, evaluated at each root of the standard's generator,
/// 2^0 to 2^(ec - 1). Every one is zero exactly when the error correction
/// codewords are the ones the data asks for.
fn syndromes(block: &[u8], ec: usize) -> Vec<u8> {
    let (exp, log) = field();
    let mul = |a: u8, b: u8| {
        if a == 0 || b == 0 {
            0
        } else {
            exp[(usize::from(log[usize::from(a)]) + usize::from(log[usize::from(b)])) % 255]
        }
    };
    (0..ec)
        .map(|i| block.iter().fold(0u8, |s, &byte| mul(s, exp[i]) ^ byte))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_information_is_the_standards() {
        // ISO/IEC 18004's table: error correction M with mask 0, and L with mask 0.
        assert_eq!(format_code(0b00000), 0b101010000010010);
        assert_eq!(format_code(0b01000), 0b111011111000100);
    }

    #[test]
    fn the_error_correction_check_passes_the_standards_example_and_nothing_else() {
        // ISO/IEC 18004's worked example: "01234567" at version 1, level M,
        // with its 16 data codewords and their 10 error correction codewords.
        let block: [u8; 26] = [
            0x10, 0x20, 0x0C, 0x56, 0x61, 0x80, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11,
            0xEC, 0x11, 0xA5, 0x24, 0xD4, 0xC1, 0xED, 0x36, 0xC7, 0x87, 0x2C, 0x55,
        ];
        assert_eq!(syndromes(&block, 10), vec![0; 10]);
        for k in 0..block.len() {
            let mut changed = block;
            changed[k] ^= 1;
            assert!(
                syndromes(&changed, 10).iter().any(|&s| s != 0),
                "a change to codeword {k} passes"
            );
        }
    }

    #[test]
    fn the_padding_is_the_standards_and_nothing_else() {
        // A terminator ending at bit 12 of a five-byte message: four zeros to
        // the end of the second byte, then 0xEC, 0x11 and 0xEC.
        assert_eq!(padding(&[0x40, 0x10, 0xEC, 0x11, 0xEC], 12), Ok(()));
        assert!(padding(&[0x40, 0x11, 0xEC, 0x11, 0xEC], 12).is_err());
        assert!(padding(&[0x40, 0x10, 0x11, 0xEC, 0x11], 12).is_err());
        assert!(padding(&[0x40, 0x10, 0xEC, 0x11, 0x00], 12).is_err());
    }

    #[test]
    fn the_codewords_and_the_remainder_fill_version_3() {
        // The 841 modules of a 29 by 29 code less its function patterns.
        assert_eq!(placement().len(), 8 * CODEWORDS + 7);
    }
}
