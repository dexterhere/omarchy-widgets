# Vault file format v0 (`vault.owv`)

Status: v0.1 after Codex design review (10 findings, all addressed below). Reviewed before implementation. Implements DAEMON_SPEC section 7. Nothing here is novel cryptography: it composes XChaCha20-Poly1305 and Argon2id from RustCrypto, with a random 32-byte data key wrapped by a master key.

## Threat it addresses

A copy of the vault file (backup, snapshot, stolen disk, dotfile repo) must not reveal secrets, and must not be silently modified. It does **not** protect against a live same-user process that can reach the master key (keyring mode), against malware that reads daemon memory, or against **whole-file rollback** to an older valid file (no freshness counter; a TPM monotonic counter could add this later). Because the whole vault is one sealed payload, a hybrid of old and new records cannot be built from two snapshots. Secret names are inside the sealed payload and are not visible in a stolen file; the header reveals only the mode and the Argon2 profile.

## Crates (maintenance evidence, checked 2026-10-17)

| Crate | Version | Last release | Notes |
|---|---|---|---|
| chacha20poly1305 | 0.11.0 | 2026-08-05 | RustCrypto/AEADs, about 96M downloads |
| argon2 | 0.6.0 | 2026-08-27 | RustCrypto/password-hashes, about 64M downloads |
| zeroize | 1.9.1 | 2026-10-06 | RustCrypto/utils, wipes secrets on drop |
| getrandom | 0.4.3 | 2026-06-17 | OS CSPRNG for keys, salts, nonces |

`cargo audit` reported no advisories on 2026-10-17 for the current lockfile. Not adopted: `secrecy` (last release 2024-10; `zeroize::Zeroizing` covers our need, one dependency fewer). Keyring backends (`oo7` is 0.7 beta, `keyring` 4.2) are deferred until spike S4 measures what a same-user process can read; the master key source is a trait so the file format does not depend on the choice.

## Layout (integers big-endian; all `len` fields count the 16-byte tag)

```
header:
  magic            4   "OWV0"
  version          1   0
  mode             1   1 keyring | 2 passphrase | 3 tpm
  file_id         16   random, fixed for the life of the file
  params           N   N is fixed by mode, nothing else is accepted:
                         passphrase (N=17): profile u8, salt[16]
                         keyring / tpm (N=16): key_slot_id[16]
wrapped data key:
  nonce           24
  ciphertext      48   32-byte data key + tag; key = master key
                       AAD = every header byte above
payload:
  nonce           24
  len              4   18..=1048592  (plaintext "{}" + tag up to 1 MiB + tag)
  ciphertext       N   key = data key
                       AAD = header bytes || wrapped data key bytes
  (end of file: any trailing byte is an error)
```

Plaintext payload: one compact JSON object mapping name to value, keys sorted. Name: 1..=64 bytes of `[a-z0-9._-]`. Value: UTF-8 string up to 16 KiB. At most 1024 records and 1 MiB of plaintext in total. Duplicate keys, other types and unknown shapes are rejected.

Why one sealed payload: record rollback, deletion resurrection and name leakage disappear (nothing is individually replaceable), and the AAD ties the payload to the exact header and wrapped key.

## Argon2id (passphrase mode)

Argon2id, version 0x13, 32-byte output, salt from the header. Parameters come only from an allow-list keyed by `profile`; any other value is rejected **before** the KDF runs. The header is unauthenticated until the unwrap succeeds, so the allow-list (not a floor/ceiling range) is what bounds KDF cost.

| profile | m (KiB) | t | p | use |
|---|---|---|---|---|
| 1 | 19456 | 2 | 1 | minimum, import only |
| 2 | 65536 | 3 | 1 | default for new vaults |
| 3 | 262144 | 3 | 1 | strong |

Passphrase bytes: the UTF-8 bytes exactly as entered; no Unicode normalization (document this, since the same visual string can differ). The CLI strips exactly one trailing `\n` or `\r\n` when reading stdin; the library never trims. New vaults require at least 12 bytes.

## Operations

- **Nonces**: every AEAD seal (wrap, re-wrap, payload) uses a fresh 24-byte OS-CSPRNG nonce. A sealed blob may be copied unchanged; re-sealing with an old nonce is forbidden.
- **set/remove**: open, change the map, reseal the payload with a fresh nonce, write a new file: temp file in the same directory, mode 0600, `fsync`, rename, `fsync` directory. Advisory `flock` against concurrent writers.
- **open**: bounds-check every length with checked arithmetic before slicing or allocating, require exact EOF, validate `mode` and `params` length, run the KDF or fetch the key, unwrap the data key (wrong key and tampering are reported identically), open the payload, validate its shape.
- **rotate** (master key, passphrase, profile or mode): always generate a **new data key**, the wrapped key and payload are always re-sealed. An archived old header plus old master key can therefore neither decrypt later data nor be spliced onto a new file (the payload AAD covers the header and wrapped key).
- **keyring / tpm rotation protocol** (crash safety): store the new key under a new random `key_slot_id`; write the new vault; reopen and verify it with the new slot; only then delete the old slot. On startup, slots that are not the current file's slot are orphans: report them, and delete them only after a successful open of the current file.
- **wipe**: removes the local vault file and the cached/derived key; in keyring/tpm mode also deletes and verifies deletion of the key slot (that gives cryptographic erasure of every copy). In passphrase mode backups and snapshots remain decryptable by anyone who knows the passphrase. User-facing text must never claim copies were erased.

## Memory and logging

Keys and decrypted values live in `Zeroizing` buffers, are never `Debug`-printed, never logged, never in argv or environment. Error values carry a category only (`Corrupt`, `WrongKey`, `TooLarge`, `Io`), never names or values.

## Tests required (before merge)

Round trip incl. arbitrary names/values (property test); flip every byte class (magic, version, mode, file_id, params, wrap nonce/ct, payload nonce/len/ct) and expect failure; splice the payload of one vault onto the header of another and onto an older header of the same vault; undersized and oversized `len`, trailing bytes, truncation; unknown profile rejected without running the KDF; wrong `params` length per mode; wrong passphrase; rotation keeps values and an archived pre-rotation header plus old master key cannot open the post-rotation payload; duplicate JSON keys and bad names rejected; fake-secret leak test across logs, audit, argv, environ and non-vault files.

## Open questions

1. Whole-file rollback: accepted for v0 and documented; revisit with a TPM monotonic counter.
2. TPM mode parameters beyond the slot id are decided in the S4 spike.
