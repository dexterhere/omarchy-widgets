//! Encrypted secret store. Format and rationale: docs/VAULT_FORMAT.md.
//!
//! Never invents primitives: XChaCha20-Poly1305 and Argon2id from RustCrypto.
//! Errors carry a category only, never names or values.
#![forbid(unsafe_code)]

mod format;
mod store;

pub use format::{MAX_NAME, MAX_RECORDS, MAX_VALUE, Profile, VaultError};
pub use store::{Info, Vault, inspect, wipe};

/// Fuzzing entry point: parse and try to open arbitrary bytes with a fixed key. Must never
/// panic or allocate beyond the format's bounds. Returns whether the bytes were a valid vault.
#[doc(hidden)]
pub fn fuzz_open(bytes: &[u8]) -> bool {
    format::fuzz_decode(bytes)
}

/// Fuzzing entry point for the decrypted payload parser, which the AEAD tag makes unreachable
/// from `fuzz_open`. Must never panic; accepts only canonical, bounded payloads.
#[doc(hidden)]
pub fn fuzz_payload(bytes: &[u8]) -> bool {
    format::fuzz_records(bytes)
}
