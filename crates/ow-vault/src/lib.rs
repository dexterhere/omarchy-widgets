//! Encrypted secret store. Format and rationale: docs/VAULT_FORMAT.md.
//!
//! Never invents primitives: XChaCha20-Poly1305 and Argon2id from RustCrypto.
//! Errors carry a category only, never names or values.
#![forbid(unsafe_code)]

mod format;
mod store;

pub use format::{MAX_NAME, MAX_RECORDS, MAX_VALUE, Profile, VaultError};
pub use store::Vault;
