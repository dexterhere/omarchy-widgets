# Vault file format v0 (`vault.owv`)

Status: proposal, reviewed before implementation. Implements DAEMON_SPEC section 7. Nothing here is novel cryptography: it composes XChaCha20-Poly1305 and Argon2id from RustCrypto, with a random 32-byte data key wrapped by a master key.

## Threat it addresses

A copy of the vault file (backup, snapshot, stolen disk, dotfile repo) must not reveal secrets, and must not be silently modified. It does **not** protect against a live same-user process that can reach the master key (keyring mode), against malware that reads daemon memory, or against rollback to an older valid file (no freshness counter; TPM monotonic counters could add this later).

## Crates (maintenance evidence, checked 2026-10-17)

| Crate | Version | Last release | Notes |
|---|---|---|---|
| chacha20poly1305 | 0.11.0 | 2026-08-05 | RustCrypto/AEADs, about 96M downloads |
| argon2 | 0.6.0 | 2026-08-27 | RustCrypto/password-hashes, about 64M downloads |
| zeroize | 1.9.1 | 2026-10-06 | RustCrypto/utils, wipes secrets on drop |
| getrandom | 0.4.3 | 2026-06-17 | OS CSPRNG for keys, salts, nonces |

`cargo audit` reported no advisories on 2026-10-17 for the current lockfile. Not adopted: `secrecy` (last release 2024-10; `zeroize::Zeroizing` covers our need, one dependency fewer). Keyring backends (`oo7` is 0.7 beta, `keyring` 4.2) are deferred until spike S4 measures what a same-user process can read; the master key source is a trait so the file format does not depend on the choice.

## Layout (all integers big-endian)

```
header:
  magic            4   "OWV0"
  version          1   0
  mode             1   1 keyring | 2 passphrase | 3 tpm
  file_id         16   random, fixed for the life of the file
  params_len       2   <= 64
  params           N   passphrase: m_cost_kib u32, t_cost u32, p u32, salt[16]; others: empty
wrapped data key:
  nonce           24
  ciphertext      48   32-byte data key + 16-byte tag
                       AAD = all header bytes above
manifest:
  nonce           24
  len              4   <= 66560
  ciphertext       N   JSON array of record names + tag
                       key = data key, AAD = "manifest" || file_id
records (count = names in manifest):
  name_len         1   1..=64, charset [a-z0-9._-]
  name             N
  nonce           24
  ct_len           4   <= 65552 (64 KiB value + tag)
  ciphertext       N   key = data key, AAD = version || file_id || name_len || name
```

Limits enforced before any allocation or KDF work: file <= 8 MiB, <= 1024 records, all lengths above. Argon2id parameters are checked against a floor (m >= 19456 KiB, t >= 2, p >= 1) and a ceiling (m <= 1 GiB, t <= 10, p <= 8) because the header is read before authentication; the floor stops a tampered header from weakening the KDF, the ceiling stops it from exhausting memory.

## Operations

- **set/remove**: decrypt manifest, change it, write a new file: temp file in the same directory, mode 0600, `fsync`, rename, `fsync` directory. Advisory `flock` against concurrent writers. Fresh random nonce every time a record or the manifest is sealed.
- **open**: parse with bounds, derive/fetch the master key, unwrap the data key (failure = wrong key or tampering, reported identically), open manifest, open every record, require records == manifest names exactly (no extras, no missing, no duplicates).
- **rotate**: new master key (or passphrase and salt), re-wrap the data key, rewrite header. `rotate --data-key` additionally re-seals every record under a fresh data key.
- **wipe**: overwrite is not meaningful on modern storage; remove the file and the master key from its source.

## Memory and logging

Keys and decrypted values live in `Zeroizing` buffers, are never `Debug`-printed, never logged, never placed in argv or environment. Error values carry a category only (`Corrupt`, `WrongKey`, `TooLarge`, `Io`), never names or values.

## Tests required (before merge)

Round trip incl. arbitrary names/values (property test); flip every byte class (magic, version, mode, file_id, params, wrapped key nonce/ct, manifest nonce/ct, record name/nonce/ct) and expect failure; swap two records' ciphertexts; delete a record; append a record; truncate; wrong passphrase; Argon2 parameters below floor and above ceiling rejected without running the KDF; oversize lengths rejected without allocating; rotation (master and data key) keeps values and invalidates the old key; fake-secret leak test across logs, audit, argv, environ and non-vault files.

## Open questions

1. Rollback protection: accept the limit for v0 and document it, or add a monotonic counter in the TPM mode only?
2. Should `name` be authenticated against the manifest order too (it is by set membership, not position)?
