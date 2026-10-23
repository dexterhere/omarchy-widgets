# Handoff: what is done and what is left

Written 2026-10-17 when work was stopped at the maintainer's request. The website repo has the detailed note (`omarchy-widgets-web/docs/HANDOFF.md`); this one covers the code repo.

## Done (merged to `develop` via PR #24; check promotion to `staging` and `main` on GitHub)

- Cargo workspace with `#![forbid(unsafe_code)]`, strict lints, CI (fmt, clippy, tests, cargo-deny, cargo-audit, weekly fuzz). Actions are referenced by tag (e.g. `actions/checkout@v7`), not pinned to commit SHAs.
- `ow-proto`: bounded line codec and protocol v0 types.
- `owd`: private Unix socket (0700 dir, 0600 socket, same-user peers only, start lock, timeouts, connection caps), process hardening (private umask, no core dumps, non-dumpable, quiet panics), grants (deny by default, field filtering, SIGHUP reload that fails closed), rate limiting, redacted audit log (fails closed), provider hub (cache, subscriptions, push, stale data with backoff, worker threads, per-push grant re-check), `system` provider (CPU, memory, load).
- `ow-vault`: passphrase-mode vault (Argon2id + XChaCha20-Poly1305, single sealed payload, atomic writes, rotation), format in `docs/VAULT_FORMAT.md` (design reviewed by Codex, implementation reviewed twice).
- `ow` CLI: `status`, `vault init|status|list|set|remove|rotate|wipe --yes` (secrets only from a no-echo prompt or stdin).
- Five fuzz targets; threat model v0; README, LICENSE, SECURITY, CODE_OF_CONDUCT, CONTRIBUTING; `packaging/PKGBUILD` (build blocked until a signing key is pinned) and a sandboxed systemd user unit (offline exposure 2.5 OK; verified as a transient unit, not yet as an installed unit); original pixel wordmark in `assets/`.

## Left, in order of importance

1. **Phase 0 spikes S1 to S13** (`docs/BUILD_PLAN.md`): nothing has been run against the real Omarchy shell or keyring. They need your approval each time they touch `~/.config/omarchy` or the real keyring. S4 (what a same-user process can read from the keyring under autologin) decides whether keyring mode is worth building.
2. **`spec/widget.schema.json` and `spec/layout.schema.json`** (JSON Schema 2020-12) with tests; the website currently vendors a DRAFT copy.
3. Vault: keyring and TPM modes, key caching in the kernel keyring, daemon unlock, `ow grants`, `ow audit tail`, admin ops and the D-1 decision (who may call them).
4. Network providers (GitHub device flow first) with the egress allow-list and ETag polling; then `ai-usage`, `docker`, `gmail`, `calendar/tasks`.
5. QML bridge plugin and kit, the desktop canvas, Widget Center panel, `ow new|check|install|place|snapshot|rollback|doctor|publish`, first widgets, packaging end to end.
6. **External review of the vault and protocol** before any public beta (release gate).
7. Verify `omarchy plugin add` pin support (S7), then fix the website's manual install wording if it changes.

## Note for the web repo (not edited from here)

The daemon now serves three more topics. `registry/providers.json` should add to the `system` provider's `serving` list: `system.time.hm`, `system.time.hms`, `system.time.date`, `system.time.tz`, `system.temp.celsius`; and give the `battery` provider a `serving` list: `battery.status.percent`, `battery.status.state`, `battery.status.timeLeft`. Field names equal the catalog's. `battery.status` answers `unavailable` on machines without a battery (desktop PCs) and `timeLeft` is an empty string when unknown, so the battery widget should render an empty state for both.

## Decisions waiting for you

Project name and the question to the Omarchy maintainers; host strategy and contacting `anishfn/omarchy-widgets`; release signing key (pin the fingerprint in `packaging/PKGBUILD`, then sign tags); update decision D-7 and `design/`: the placeholder chord `Super + Shift + W` conflicts with Omarchy's default "Writing" binding, the website's emulator proposes `Super + Alt + W`.

## Resume

`cargo test --workspace` and the checks in `CONTRIBUTING.md`; run the daemon only against a short throwaway directory (`D=$(mktemp -d -p /tmp owd.XXXX); cargo run -p owd -- --config-dir "$D"`).
