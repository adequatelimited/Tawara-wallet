// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The application side of the QR scanner: the scanner a shell provides, the
//! text it reads, and the checks that text must pass before a destination row
//! takes it.
//!
//! A shell gives a scanner by giving all three scan fields of [`Host`] (see
//! [`Scanner::of`]). Only phones do, so the desktop offers no scan.

use iced::futures::channel::mpsc::UnboundedSender;
use tawara_wallet_core::spend::parse_destination;

use crate::host::{Host, ScanDone, ScanPreview, ScanStart};

/// The longest a scanned text may be, in bytes, once surrounding whitespace is
/// trimmed. The longest destination accepted is 42 characters (`0x` and forty
/// hex digits) and a Base58 destination is at most 31, so 128 leaves room for
/// any text that could be one. A longer text is refused before the library's
/// check copies it into an error.
pub const MAX_BYTES: usize = 128;

/// What a row shows under its address when a scanned text is empty, longer than
/// [`MAX_BYTES`], or not UTF-8.
pub(crate) const EMPTY: &str = "This code holds no text, so it is not a destination.";
pub(crate) const OVERLONG: &str =
    "This code holds more text than a destination can have, so it is not one.";
pub(crate) const NOT_TEXT: &str =
    "This code holds no text this wallet can read, so it is not a destination.";

/// The text a scanner read, on its way into the model. Its `Debug` prints
/// nothing of it, as [`Clip`](crate::app::Clip)'s does: a scanned text is
/// untrusted, and no message log may show one.
#[derive(Clone)]
pub struct Code(pub(crate) Vec<u8>);

impl core::fmt::Debug for Code {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Code(..)")
    }
}

/// A shell's scanner, as the application holds it once the shell gives all
/// three scan fields (see [`Scanner::of`]). Each reply goes to the interface as
/// `(row, ask, text)`: the destination row the scan was begun for, the number
/// of that scan, and the text, or `None` when the scan was cancelled or refused.
#[derive(Clone)]
pub struct Scanner {
    start: ScanStart,
    cancel: fn(),
    preview: ScanPreview,
    replies: UnboundedSender<(usize, u64, Option<Code>)>,
}

impl core::fmt::Debug for Scanner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Scanner(..)")
    }
}

impl Scanner {
    /// The scanner `host` offers, with its replies sent to `replies`. `None`
    /// where `host` gives fewer than the three scan fields, as the desktop's
    /// `Host` does not exist at all.
    #[must_use]
    pub fn of(
        host: &Host,
        replies: UnboundedSender<(usize, u64, Option<Code>)>,
    ) -> Option<Scanner> {
        let (Some(start), Some(cancel), Some(preview)) =
            (host.scan, host.cancel_scan, host.scan_preview)
        else {
            return None;
        };
        Some(Scanner {
            start,
            cancel,
            preview,
            replies,
        })
    }

    /// Who draws the camera's preview (see [`ScanPreview`]).
    #[must_use]
    pub fn preview(&self) -> ScanPreview {
        self.preview
    }

    /// Starts a scan for destination row `row`, as scan number `ask`. The
    /// shell's reply comes to the interface later, on the channel this scanner
    /// was given, and may come before this returns.
    pub(crate) fn start(&self, row: usize, ask: u64) -> Result<(), String> {
        let replies = self.replies.clone();
        let done: ScanDone = Box::new(move |text| {
            // The interface may have closed by now, with nothing left to answer.
            let _ = replies.unbounded_send((row, ask, text.map(Code)));
        });
        (self.start)(done)
    }

    /// Stops the shell's camera (see [`Host::cancel_scan`]).
    pub(crate) fn cancel(&self) {
        (self.cancel)();
    }

    /// The function that stops the camera, for a move to the background, which
    /// the shell calls on its own thread (see `App::leaving_hook`).
    pub(crate) fn cancel_fn(&self) -> fn() {
        self.cancel
    }
}

/// What a scanned text is as a destination for row `row`, zero based: its
/// surrounding whitespace trimmed, at most [`MAX_BYTES`] long, UTF-8, and a
/// destination by `parse_destination`. `Ok` holds the trimmed destination.
/// `Err` is what the row shows under its address: the library's message where
/// it has one, otherwise `EMPTY`, `OVERLONG` or `NOT_TEXT`.
pub fn destination(bytes: &[u8], row: usize) -> Result<String, String> {
    let trimmed = bytes.trim_ascii();
    if trimmed.is_empty() {
        return Err(EMPTY.to_owned());
    }
    if trimmed.len() > MAX_BYTES {
        return Err(OVERLONG.to_owned());
    }
    let text = core::str::from_utf8(trimmed).map_err(|_| NOT_TEXT.to_owned())?;
    // The destination is numbered from one, as the library names it.
    parse_destination(text, row + 1).map_err(|e| e.to_string())?;
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tawara_wallet_core::view::AccountId;

    /// A destination the library accepts, as Receive prints one.
    fn a_destination() -> String {
        AccountId::from_tag([9; 20])
            .destination()
            .expect("a tag renders")
    }

    /// A destination in the `0x` form: forty hex digits, which carry no checksum.
    const HEX: &str = "0x1111111111111111111111111111111111111111";

    #[test]
    fn a_destination_is_trimmed_and_checked_as_the_library_checks_it() {
        let padded = format!("  \t{}\r\n", a_destination());
        assert_eq!(destination(padded.as_bytes(), 0), Ok(a_destination()));
        assert_eq!(destination(HEX.as_bytes(), 1), Ok(HEX.to_owned()));
    }

    #[test]
    fn a_text_is_bounded_once_trimmed_and_not_before() {
        let longest = format!("{HEX}{}", " ".repeat(MAX_BYTES));
        assert_eq!(
            destination(longest.as_bytes(), 0),
            Ok(HEX.to_owned()),
            "the whitespace around a destination does not count"
        );
        let over = "1".repeat(MAX_BYTES + 1);
        assert_eq!(destination(over.as_bytes(), 0), Err(OVERLONG.to_owned()));
    }

    #[test]
    fn an_empty_text_and_a_text_that_is_not_utf8_are_refused() {
        assert_eq!(destination(b" \n\t", 0), Err(EMPTY.to_owned()));
        assert_eq!(
            destination(&[0xff, b'0', b'x'], 0),
            Err(NOT_TEXT.to_owned())
        );
        assert_eq!(
            destination(&[b'0', b'x', 0xc3], 0),
            Err(NOT_TEXT.to_owned()),
            "a truncated sequence is not UTF-8"
        );
    }

    #[test]
    fn a_text_that_is_not_a_destination_is_refused_in_the_library_words() {
        let why = destination(b"https://example.com/pay", 2).expect_err("refused");
        assert!(why.starts_with("destination 3: `https://example.com/pay` is not a destination"));
        let zero = format!("0x{}", "00".repeat(20));
        let why = destination(zero.as_bytes(), 0).expect_err("refused");
        assert!(why.contains("all-zero tag"), "{why}");
    }

    #[test]
    fn a_scanner_is_offered_only_with_all_three_scan_fields() {
        fn start(_: ScanDone) -> Result<(), String> {
            Ok(())
        }
        fn cancel() {}
        let (replies, _received) = iced::futures::channel::mpsc::unbounded();
        let host = |scan, cancel_scan, scan_preview| Host {
            private_dir: std::path::PathBuf::from("/nonexistent"),
            copy: |_| {},
            share: |_, _| Ok(()),
            back: || {},
            notices: include_str!("../../../THIRD-PARTY-NOTICES.txt"),
            captured: None,
            device_check: None,
            keep_password: None,
            release_password: None,
            forget_password: None,
            scan,
            cancel_scan,
            scan_preview,
        };
        let all = host(
            Some(start as ScanStart),
            Some(cancel as fn()),
            Some(ScanPreview::Shell),
        );
        assert!(Scanner::of(&all, replies.clone()).is_some());
        assert!(
            Scanner::of(
                &host(None, Some(cancel), Some(ScanPreview::Shell)),
                replies.clone()
            )
            .is_none()
        );
        assert!(
            Scanner::of(
                &host(Some(start), None, Some(ScanPreview::Shell)),
                replies.clone()
            )
            .is_none()
        );
        assert!(Scanner::of(&host(Some(start), Some(cancel), None), replies).is_none());
    }
}
