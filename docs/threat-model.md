# Threat model (v0, plain language)

Status: draft. Covers what is built today (daemon, grants, audit, vault, CLI). Items marked **unverified** wait for Phase 0 spikes and must not be promised in the UI or docs until measured.

## What we protect

Credentials (tokens, app passwords), and data derived from them (mail, calendar, notifications, agent usage). The goal is that none of it appears in plain text outside process memory, and that a widget receives only what the user granted.

## Who we defend against, and how well

| Threat | Protection | What still gets through |
|---|---|---|
| A copy of your files (backup, snapshot, stolen disk, dotfile repo) | The vault file is encrypted (XChaCha20-Poly1305). In passphrase mode the key comes from your passphrase through Argon2id. Config files hold references only. | A weak passphrase can be guessed offline. The minimum is 12 bytes; use a long one. A copy of the vault plus the passphrase opens everything, so deleting the live file does not erase old copies. |
| Another user on the machine | Socket directory 0700, socket 0600, peer uid checked on every connection. | Root can do anything. |
| Another **process of your own user** | Nothing, by design of Unix. Same-uid processes can reach the socket and can ptrace unless the kernel forbids it. The daemon sets itself non-dumpable, which blocks reading its memory through `/proc` for most cases. | A malicious program running as you can ask the daemon for anything its caller identity (a self-claimed widget id) is granted, can read your keyring, and can read any file you own. **Unverified**: exactly what it can read from the keyring on Omarchy 4 with automatic login (spike S4). |
| A buggy or malicious **code widget** (QML) | Review tiers, declared capabilities, lint (later). Grants are checked by the daemon, but the widget id is self-claimed. | Code widgets are not sandboxed. They run as you and can bypass any same-uid guardrail. Treat installing one like installing a program. |
| A **spec widget** (declarative, no code) | Only the trusted renderer talks to the bridge; the widget sees granted fields only. | Whatever you granted. |
| Leaks through logs, argv, environment, crash dumps | Secrets are read only from a no-echo prompt or stdin, never argv or env. Audit log entries have no payload field. Panic messages are not logged. Core dumps off, process non-dumpable. Tested by running the real binaries and searching their output and files. | Your terminal scrollback and screen recordings. |
| A hostile local client of the daemon | 64 KiB line limit, bad-frame disconnect, caps on connections, subscriptions, cache keys, queue depth and provider output size; parameters limited to what each topic declares. | A same-user client can still use its share of those limits. |
| Tampered vault file | Everything is authenticated (header, wrapped key, payload). Wrong key and tampering look the same. Fuzzed. | Rolling the whole file back to an older valid copy is not detected (no freshness counter). |
| Supply chain | Locked dependencies, `cargo audit` and `cargo deny` in CI, small crypto surface from RustCrypto. | A compromised upstream release. Signed releases and SBOM are planned. |

## Honest limits

1. **Same-user malware wins.** The vault protects copies of your data and keeps secrets out of logs and configs. It cannot protect against software that runs as you while the vault is unlocked.
2. **Keyring mode is weaker than it sounds** on a desktop that logs in automatically. This is why passphrase mode exists and is the only mode implemented so far.
3. **Widget identity is self-claimed.** Until the bridge authenticates widgets (open decision D-1), grants stop honest widgets from over-reading, not dishonest ones.
4. **We do not claim to sandbox QML.** No "secure widget" wording anywhere.

## Data handling rules

- Sensitive data stays in memory by default. Nothing is written to disk by providers today.
- Removing a secret removes it from the vault file. Backups you made elsewhere are not touched.
- No telemetry, no cloud relay.
- The audit log records who asked for which topic and how many bytes, never values. It rotates at 1 MiB and keeps five files.

## Verification we run

Unit and integration tests for each rule above, a fake-secret leak test over stdout, stderr and every file written, process-level hardening tests, independent reviews (separate Claude agents and Codex) of every security-relevant change with findings fixed before merge, five fuzz targets, `cargo audit` and `cargo deny` on every PR.

## Not yet done

External review of the vault and protocol (required before a public beta), keyring and TPM modes (spike S4), authenticating widgets to the daemon (D-1), systemd sandbox verification with `systemd-analyze security`, signed releases.
