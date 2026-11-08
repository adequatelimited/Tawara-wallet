// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Entropy from the operating system's generator.
//!
//! The library takes every random input as a parameter and has no generator
//! of its own: the phrase entropy, the store's salt and the nonce seed when
//! a store is created, and a fresh nonce seed each time one is opened. This
//! module draws them.
//!
//! # Mechanism
//!
//! `getrandom` 0.2, which `ring` already uses for TLS keys, so it is in
//! every build on every platform. It calls:
//!
//! | platform | call |
//! |---|---|
//! | Windows | `BCryptGenRandom` with `BCRYPT_USE_SYSTEM_PREFERRED_RNG`, as the command-line wallet calls it |
//! | Linux, Android | the `getrandom` system call, which waits until the kernel's pool is initialised; `/dev/urandom` only on kernels without it |
//! | macOS | `getentropy` |
//! | iOS | `CCRandomGenerateBytes` (CommonCrypto, in libSystem; `SecRandomCopyBytes` calls into it) |
//!
//! Unlike reading `/dev/urandom`, the system call blocks until the pool is
//! seeded, and on Android and iOS it needs no file access in the sandbox.
//!
//! Every failure is an error. A short or failed draw is never replaced by
//! anything weaker, and the buffer is zeroized when it is dropped.

use core::fmt;

use mochimo_crypto::cli::create::{CreateEntropy, ENTROPY_LEN};
use mochimo_crypto::keystore::{NONCE_SEED_LEN, SALT_LEN};
use zeroize::Zeroizing;

/// The generator failed. Nothing was created or opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntropyUnavailable(String);

impl fmt::Display for EntropyUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the operating system's random generator failed ({}); nothing was created or opened",
            self.0
        )
    }
}

impl std::error::Error for EntropyUnavailable {}

/// `N` bytes from the operating system's generator.
pub(crate) fn os_bytes<const N: usize>() -> Result<Zeroizing<[u8; N]>, EntropyUnavailable> {
    let mut buf = Zeroizing::new([0u8; N]);
    getrandom::getrandom(&mut buf[..]).map_err(|e| EntropyUnavailable(e.to_string()))?;
    Ok(buf)
}

/// A fresh nonce seed for opening a store (`keystore::Unlock`).
pub(crate) fn nonce_seed() -> Result<Zeroizing<[u8; NONCE_SEED_LEN]>, EntropyUnavailable> {
    os_bytes::<NONCE_SEED_LEN>()
}

/// The entropy for creating a store (`cli::create::CreateEntropy`): three
/// separate draws, not one split three ways.
pub(crate) fn create_entropy() -> Result<Zeroizing<CreateEntropy>, EntropyUnavailable> {
    Ok(Zeroizing::new(CreateEntropy {
        phrase: *os_bytes::<ENTROPY_LEN>()?,
        salt: *os_bytes::<SALT_LEN>()?,
        nonce_seed: *os_bytes::<NONCE_SEED_LEN>()?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_are_full_width_and_differ() {
        let a = os_bytes::<32>().expect("generator");
        let b = os_bytes::<32>().expect("generator");
        assert_ne!(
            *a, *b,
            "two 256-bit draws are equal only if the generator is broken"
        );
        assert_ne!(*a, [0u8; 32]);
    }

    #[test]
    fn create_entropy_draws_three_independent_values() {
        let e = create_entropy().expect("generator");
        assert_eq!(e.phrase.len(), ENTROPY_LEN);
        assert_ne!(e.phrase, e.nonce_seed);
        assert_ne!(e.salt[..], e.phrase[..SALT_LEN]);
    }

    #[test]
    fn the_refusal_says_nothing_was_made() {
        let text = EntropyUnavailable("errno 38".into()).to_string();
        assert!(text.contains("nothing was created or opened"), "{text}");
    }
}
