# Provider daemon (`owd`) specification, draft v0

> **Status (checked against `crates/` on 2026-10-23).** This is a design document; the code is the source of truth. Each section below ends with an **Implemented / Not implemented** note. Summary: the Unix socket server, protocol ops `hello`/`snapshot`/`subscribe`/`unsubscribe`/`status`, grants, audit log, rate limiting, process hardening, the passphrase-mode vault (library plus `ow vault`) and the `system` provider are implemented. Admin ops `vault.unlock`, `vault.lock` and `vault.status` (passphrase mode) and `grants.reload` are implemented. `ow install|list|remove` manage spec widgets and their grants (`docs/INSTALL_DESIGN.md`). Not implemented: `config.toml`, the other admin ops, vault keyring/TPM modes and keyring auto-unlock, every provider except `system`, any network access or egress allow-list, `tracing`. The design text is kept as intent; where it differs from the code, the status note says so. The unit in `packaging/owd.service` has been verified only offline and as a transient unit, not as an installed unit. Nothing here makes code widgets sandboxed: they run as the user.

Audience: the agent that builds it. This is a proposal derived from the project plan; where it says **verify** or **decide**, record the outcome in `docs/DECISIONS.md`. Visual explanations are in `design/` (artboards "What the provider daemon is", "One request, step by step", "How to build it").

## 1. What it is

`owd` is one small background process (Rust, systemd user service) that:

1. holds credentials in an encrypted vault and is the only thing that ever reads them;
2. fetches and polls data from local and remote sources through provider modules;
3. caches results (sensitive data in RAM only);
4. serves widgets over a local Unix socket, filtered by per-widget grants;
5. writes a redacted audit log of every access.

Why: keeps tokens out of QML, keeps a crashing integration from taking down the shell, and stops ten widgets from making ten identical API calls.

What it is not: a sandbox for code widgets. Code widgets run unsandboxed as the user and can bypass any same-uid guardrail. Every doc and UI string must say so.

**Status:** items 3 to 5 are implemented for the `system` provider only (RAM cache and scheduler in `owd/src/hub.rs`). Item 1 is not wired up: the vault library exists but `owd` does not depend on `ow-vault` and never opens it. Item 2 covers local `/proc` data only; there is no network access.

## 2. Components and trust boundaries

```
widgets (QML/spec) -> bridge plugin (QML service) --unix socket--> owd
                                                                   |-- socket server (SO_PEERCRED)
                                                                   |-- grants + audit
                                                                   |-- provider scheduler + RAM cache
                                                                   |-- providers (system, battery, media, ai-usage, github, docker, gmail, ...)
                                                                   |-- vault (encrypted on disk) -> keyring / passphrase / TPM
```

Boundaries that matter:
- shell process <-> daemon: same uid. `SO_PEERCRED` rejects other users; it does not distinguish the bridge from other same-uid processes.
- spec widget <-> everything: hard boundary, because only the renderer (trusted code) talks to the bridge.
- code widget <-> everything: guardrail only.
- daemon <-> network: hard boundary enforced by a host allow-list inside the HTTP wrapper (design only; see status).

**Status:** Implemented: socket server with `SO_PEERCRED` identity, grants, audit, provider scheduler and RAM cache, the `system` provider. Not implemented: the bridge plugin side, vault integration, providers other than `system`, any HTTP wrapper or egress allow-list (there is no HTTP client in the workspace, so the daemon makes no network connections today; that is a property of the current code, not an enforced boundary).

## 3. Workspace layout (crates)

```
crates/
  ow-proto/      wire types (serde), frame codec, protocol version, JSON Schema export
  ow-vault/      key management, sealed records, file format, modes (keyring/passphrase/tpm)
  ow-core/       grants, audit log, cache, scheduler, provider trait, egress-restricted HTTP client
  ow-providers/  one module per provider; fixtures for tests
  owd/           daemon binary (config, socket server, signal handling, sd_notify)
  ow/            CLI (vault, grants, audit, status first; manager verbs later)

(Design layout. What exists today is listed in the status note below.)
packaging/       systemd unit, PKGBUILD
deny.toml
```

**Status (actual layout):**
- `ow-proto`: wire types, frame codec (`MAX_LINE` 64 KiB), `PROTOCOL = 0`. No JSON Schema export.
- `ow-vault`: passphrase mode only (mode byte 2). No keyring or TPM mode. See `docs/VAULT_FORMAT.md`.
- `ow-core`: `grants`, `audit`, `ratelimit`, `paths`, `harden`, and the synchronous `Provider` trait (`provider.rs`). The cache and scheduler are not here; they live in `owd/src/hub.rs`. There is no HTTP client.
- `ow-providers`: only `system`.
- `owd`: socket server, connection handling, hub (cache and scheduler), SIGHUP grants reload, `sd_notify`. No config file.
- `ow`: `status`, `doctor [--json]`, `vault init|status|unlock|lock|list|set|remove|rotate|wipe`, and `install <id>`, `list`, `remove <id>` for spec widgets (CLI verbs below). No `grants`, `audit` or `change-mode` verbs. `ow` is also a library (`crates/ow/src/lib.rs`) so tests can drive the install code in-process; it is the only crate that depends on `ow-egress`.
- `packaging/` holds the unit, the release `PKGBUILD` (signed tags only), `PKGBUILD-git` (developer preview), a local installer and its test; see `docs/PACKAGING.md`.

All crates `#![forbid(unsafe_code)]`. If `prctl`, `mlock` or `setrlimit` need syscalls, use `rustix` or `nix`; if a truly unsafe block remains, isolate it in one audited module with a comment explaining why.

## 4. Paths and permissions

| What | Path | Mode |
|---|---|---|
| Data socket | `$XDG_RUNTIME_DIR/owd/owd.sock` (dir 0700) | 0600 |
| Config | `$XDG_CONFIG_HOME/owd/config.toml` | 0600 |
| Grants | `$XDG_CONFIG_HOME/owd/grants.toml` | 0600 |
| Vault | `$XDG_DATA_HOME/owd/vault.owv` | 0600, atomic writes |
| Audit log | `$XDG_STATE_HOME/owd/audit.jsonl` | 0600, opened O_APPEND, rotated; not tamper-evident |

Every path derives from XDG variables so tests can point them at a temp dir. The daemon refuses to start if the socket dir is group/world accessible or not owned by the current uid.

**Status:** Implemented: the socket, grants, vault and audit paths above match `crates/ow-core/src/paths.rs`; `--config-dir D` maps them to `D/{config,data,state,run}/owd`; the socket directory refusal when it is group/world accessible is implemented. Not implemented: `config.toml` is neither read nor created, so the Config row is design only.

## 5. Wire protocol v0 (line-delimited JSON)

Framing: one JSON object per line (`\n`), UTF-8, max 64 KiB per line (reject larger and close). Every message has `"v": 0`. Unknown fields are ignored; unknown ops return an error. Requests carry a client-chosen integer `id`.

Client -> daemon:

```
{"v":0,"id":1,"op":"hello","client":"bridge","clientVersion":"0.1.0"}
{"v":0,"id":2,"op":"snapshot","widget":"io.github.you.github-activity","topic":"github.prs"}
{"v":0,"id":3,"op":"subscribe","widget":"io.github.you.github-activity","topic":"github.prs","params":{}}
{"v":0,"id":4,"op":"unsubscribe","sub":7}
{"v":0,"id":5,"op":"status"}
```

Admin ops, on the same socket in v0 (decision D-1 still open): implemented `vault.unlock`, `vault.lock`, `vault.status`, `grants.reload`; design only `grants.list`, `grants.set`, `audit.tail`, `vault.set`, `vault.remove`, `vault.wipe`.

Implemented admin frames (same uid only, via `SO_PEERCRED`; they carry no `widget` field, and one that does is refused with `denied` and audited by op name):

```
{"v":0,"id":N,"op":"vault.unlock","passphrase":"<hex of the passphrase bytes>"}
  ok:    {"v":0,"id":N,"ok":true,"state":"unlocked"}
  wrong passphrase, missing or unreadable vault (one answer): error "denied", no topic
  lockout in force, or another attempt running: error "rate_limited"
  bad hex, empty, over 20480 bytes, JSON escapes, missing field: error "bad_request"
{"v":0,"id":N,"op":"vault.lock"}      ok: {"state":"locked"}
{"v":0,"id":N,"op":"vault.status"}
  locked:   {"ok":true,"state":"locked"}  plus "lockoutSecsLeft" while attempts are blocked
  unlocked: {"ok":true,"state":"unlocked","count":N,"names":[...],"idleSecsLeft":S}
```

The passphrase is hex so it is arbitrary bytes and has no JSON escapes. It is decoded outside serde's tagged/flattened buffering into zeroizing memory, the frame buffers are wiped after use, `Debug` is redacted, and parse errors carry a code only. `vault.status` never returns values and does not count as use (it does not extend the idle timer). Policy constants (`crates/owd/src/unlock.rs`): 5 free failures, then a lockout of 30 s that doubles per further consecutive failure up to 15 min; one unlock attempt at a time; idle auto-lock after 15 min without a secret being resolved (a visible widget that polls keeps the vault open). Counters live in memory and reset on daemon restart. A secret-backed topic answers `locked` (to snapshot and subscribe) while the vault is locked; lock, idle lock and every fresh unlock purge the cache entries of secret-backed providers, and a result fetched across a lock is discarded. Review follow-ups, now in the code: the idle timer and lockout deadline run on CLOCK_BOOTTIME, so time spent suspended counts as idle. Any `widget` key on an admin op, including `null`, makes it a widget claim and it is refused. Cache entries carry the vault key epoch; the lock and the cache purge happen together under the hub state lock, and snapshot, cached, initial and scheduled pushes compare the epoch under that lock (a reply already past its last check can still be in flight on another connection when a lock lands). Fetch workers are counted separately from cache entries. An unlock whose audit line cannot be written is rolled back and answers `internal`. Refused widget-shaped admin frames, and data requests whose topic starts with `vault.`, share one small rate-limit bucket checked before the grant lookup; over the limit they are refused with no audit line. `ow vault set|remove|rotate|wipe` send `vault.lock` to the daemon afterwards (best effort), because the daemon's decrypted copy would otherwise go stale. Providers get secrets through `FetchCtx::with_secret` (closure) or a `SecretHandle` that borrows the context.

`grants.reload` (no payload) makes the daemon re-read `grants.toml` through the same function SIGHUP uses; a failed reload fails closed (no grants, everything denied):

```
{"v":0,"id":N,"op":"grants.reload"}
  ok:    {"v":0,"id":N,"ok":true,"state":"reloaded"}
  unreadable, insecure or invalid file: error "internal" (the daemon now denies every widget)
  more than the small admin burst: error "rate_limited"
  a frame with a "widget" key (null included): error "denied", audited by op name, like the vault ops
```

Its audit entries have widget `admin`, topic `grants`, op `reload`, decision allow or deny (a failed reload is a deny). `ow install` and `ow remove` call it after editing `grants.toml`; if no daemon is running they say the grants apply at the next start.

Audit entries for admin events have widget `admin`, topic `vault` and op `unlock`, `lock`, `autolock`, `lockout` or the refused op's name; no payload exists in the entry type.

Daemon -> client:

```
{"v":0,"id":1,"ok":true,"daemonVersion":"0.1.0","protocol":0}
{"v":0,"id":3,"ok":true,"sub":7}
{"v":0,"sub":7,"topic":"github.prs","ts":1760700000,"stale":false,"data":{"open":3,"review":1}}
{"v":0,"id":2,"error":{"code":"denied","topic":"github.prs"}}
```

Error codes (all eight exist in `ErrorCode`): `bad_request`, `denied`, `unknown_topic`, `provider_error`, `rate_limited`, `locked` (vault locked), `unsupported_version`, `internal`. Error messages never contain secrets, tokens or upstream response bodies.

Push messages include `stale: true` and the last good `ts` when a provider fails; the client shows the age instead of blanking.

Subscriptions are reference-counted per `(topic, params)`; the scheduler polls only while at least one subscriber is visible (the bridge reports visibility; hidden or suspended widgets unsubscribe). *Design only: there is no visibility reporting and no bridge yet.*

Design decision D-1 (decide in Phase 0): who may call admin ops? Same-uid processes cannot be told apart by `SO_PEERCRED`. Options: (a) same socket, accept that it is a guardrail; (b) second admin socket; (c) require an interactive confirmation (polkit-style prompt or a one-time code shown in the Widget Center) for `grants.set` and `vault.*`. Recommended start: (a) with the honest note, evaluate (c) before 1.0.

**Status:** Implemented ops: `hello`, `snapshot`, `subscribe`, `unsubscribe`, `status`. `hello` and `status` both reply `{"v":0,"id":N,"ok":true,"daemonVersion":..,"protocol":0}`. `PROTOCOL` is 0; `MAX_LINE` is 64 KiB. A snapshot reply carries `{topic,ts,stale,data}`; a push is `{v,sub,topic,ts,stale,data}`. `locked` is emitted for secret-backed topics while the vault is locked (no secret-backed provider ships yet; a test-only provider proves the path). Admin ops `grants.*`, `audit.tail`, `vault.set|remove|wipe` are not implemented and return `bad_request`; D-1 is still pending. Unsupported protocol versions return `unsupported_version`.

## 6. Grants and audit

`grants.toml`:

```toml
[[grant]]
widget = "io.github.you.github-activity"
topics = ["github.prs"]
fields = ["open", "review"]      # optional; absent = all fields of the topic
approved_at = "2026-10-17T12:00:00Z"
```

- Deny by default. A request is allowed only if a grant covers `(widget, topic)`; field filtering strips non-granted fields from `data`.
- The bridge enforces grants too, but the daemon is authoritative.
- Rate limit per widget per topic (token bucket); exceed -> `rate_limited`.
- Audit line (JSONL, no payloads): `{"ts":..., "widget":..., "topic":..., "op":"snapshot|subscribe", "decision":"allow|deny", "bytes":N}`. Redaction test: no field value ever appears in the audit log.
- `ow audit tail` and the Widget Center read it through `audit.tail`.

**Status:** Implemented:
- Deny by default, field filtering (top-level keys only), `deny_unknown_fields` on the grants file, duplicate grants rejected.
- Grants file must be mode 0600 and owned by the current user; reload on SIGHUP, and a failed reload fails closed (no grants, everything denied).
- Rate limit: token bucket per (widget, topic), burst 10, 2 per second, then `rate_limited`.
- Limits: 32 connections, 32 subscriptions per connection, 16 per widget, 256 total.
- Audit ops logged are `snapshot`, `subscribe` and push; a push is logged only when it is denied. Rotation at 1 MiB, keeping 5 numbered files.

Not implemented: `audit.tail`, `ow audit tail`, any `ow grants` command. Grants are still plain file edits, except that `ow install <id>` appends the entries of a spec widget and `ow remove <id>` deletes every entry whose `widget` equals the id (text edit that keeps other entries and comments, proven with the daemon's own parser; atomic replace, mode 0600).

### `ow doctor`

```
ow [--config-dir DIR] doctor [--json]
```

Read-only diagnostics; the only "verb" added for it is the CLI command, there is **no new wire op** (it sends the existing `status` and `vault.status` through the same trusted client as every other verb: socket directory private and yours, peer uid checked, one request line, reply capped at 64 KiB). It never writes, never runs `systemctl` or any program, never uses the network, and never reads the vault file (mode and owner only). Each check prints `OK`, `WARN`, `FAIL` or `SKIP` (not checked, with the reason), one plain sentence, and a `fix:` line where there is one. Exit code 1 if any check is `FAIL`, otherwise 0; `--json` prints `{"ok":bool,"checks":[{"id","level","message","fix"}]}`.

Checks: data/config/state directories (0700, yours, not a symlink), socket directory and socket (0700 / 0600), daemon answers `status`, daemon version equals `ow` version, vault state (locked, or unlocked with secret NAMES, never values), vault file (present, regular, yours, no group/other access), `grants.toml` (loaded with the daemon's own loader: mode, owner, size, parse, duplicates), every installed widget (files unchanged since install by SHA-256, then the install validators), `owd.service` installed for the user (files only: the user's `systemd/user`, `/etc/systemd/user`, `/usr/lib/systemd/user`; its `ExecStart` binary exists) and enabled (a link in `default.target.wants`), and the plugin folder. Text that came from the daemon or from files is cut and cleaned before printing. In `--config-dir` mode only the sandbox is searched for the unit, and the plugin folder is `SKIP`: its location is known only for a normal session (`~/.config/omarchy/plugins`, SPIKES S5, Omarchy 4.0.6; whether the shell honours `XDG_CONFIG_HOME` there is not verified). Whether the shell has the plugin enabled is not checked.

**Status:** implemented and tested (unit tests with hostile fixtures in `crates/ow/src/doctor.rs`; the real binary against a throwaway in-process daemon in `crates/ow/tests/doctor_cli.rs`). A missing data directory is a `WARN`, because a daemon started by hand works without it; the systemd unit does not.

### CLI verbs for spec widgets

```
ow install <id> [--registry https://<host>] [--allow-planned] [--dry-run] [--yes --accept-access <hash>]
ow list
ow remove <id> [--yes]
```

Design, trust statements and limits: `docs/INSTALL_DESIGN.md`. Summary: the registry index (compiled-in host, or `--registry`, never an environment variable) names repo and commit; `manifest.json`, `widget.json`, `layout.json` are fetched from raw.githubusercontent.com at that commit through `ow-egress` (https, no redirects, 256 KB, JSON only), validated, shown as a plain-language access summary, and installed to `$XDG_DATA_HOME/owd/widgets/<id>/` only after the person types `yes` on a terminal (or passes `--yes` with the exact `--accept-access` hash of that access: registry, repo, commit, file hashes and grants; `--yes` is refused with a non-default `--registry`). `planned` widgets need `--allow-planned`; `removed` ones are never installed. Files are staged and swapped, grants are written second, and any failure rolls back. **Status:** implemented and tested against a fake byte source; a live fetch was checked once with `--dry-run` (the production index lists seven concept entries with placeholder commits, so nothing is installable yet).

## 7. Vault

File `vault.owv`, versioned, atomic writes (write temp in same dir, `fsync`, rename, `fsync` dir), mode 0600, advisory lock against concurrent writers.

Structure (original proposal; the implementation differs, see the status note at the end of this section):
- Header: magic `OWV0`, format version, mode (`keyring|passphrase|tpm`), mode parameters (e.g. Argon2id salt and cost parameters), wrapped data key (nonce + ciphertext of a random 32-byte data key under the master key).
- Records: `name`, `nonce` (24 bytes, XChaCha20-Poly1305), `ciphertext+tag`. AAD binds the format version, record name and a per-file random id, so records cannot be swapped between names or files.
- Key rotation: generate a new master key, re-wrap the data key; optionally rotate the data key and re-encrypt records.

Crypto: XChaCha20-Poly1305 for records and key wrap, Argon2id for passphrase mode (parameters stored in the header, minimum floor enforced), 24-byte random nonces from the OS CSPRNG. Candidate crates: `chacha20poly1305`, `argon2`, `zeroize`, `secrecy`, and `oo7` or `keyring` for Secret Service. **Verify maintenance status of each before adopting; record in DECISIONS.md.** Do not hand-roll primitives. Consider `age` as a reference for file-format design, not as a substitute.

Master key by mode:
- **Keyring (default)**: random master key stored in Secret Service under a fixed attribute set. Verify on Omarchy 4 how the keyring behaves with automatic login after disk unlock; document the result in the UI text for this mode.
- **Passphrase**: Argon2id from a typed passphrase; the design sketched the derived key in the kernel keyring (`keyctl`); what is built instead: `ow vault unlock` sends the passphrase once over the 0600 socket, `owd` derives the key and holds it in zeroizing memory, and it locks on idle or `ow vault lock`; the daemon reports `locked` meanwhile.
- **TPM-bound**: key sealed via `systemd-creds` where a TPM2 exists. Spike only in Phase 0; ship if it round-trips cleanly.

Memory handling: keys in `secrecy`/`zeroize` types, `mlock` where permitted, `PR_SET_DUMPABLE=0`, `RLIMIT_CORE=0`, never in argv/env, never logged, never `Debug`-printed.

Secret input: values come from stdin or a no-echo prompt, never as command-line arguments.

Lifecycle: `ow vault init|status|set <name>|remove <name>|rotate|wipe|change-mode`. Removing a secret purges all cache entries derived from it.

**Status:**
- Implemented: `ow-vault` seals ONE payload under a wrapped data key (see `docs/VAULT_FORMAT.md`), not per-record nonces as sketched above. Passphrase mode only (mode byte 2), Argon2id, XChaCha20-Poly1305. `ow vault init|status|list|set|remove|rotate|wipe` exist (`wipe` requires `--yes`). Keys use `zeroize`; the daemon process sets non-dumpable and `RLIMIT_CORE=0` (`ow-core/src/harden.rs`).
- Also implemented: `ow vault unlock|lock|status` (daemon side, see section 5) and the daemon holding the unlocked vault. `ow vault set` takes the passphrase from `/dev/tty` and the value from stdin when stdin is redirected; with no terminal it fails unless `--passphrase-stdin` is given.
- Not implemented: keyring and TPM modes and keyring auto-unlock, `ow vault change-mode`, the kernel-keyring timeout, `mlock`, reloading the daemon's copy when `ow vault set|remove` changes the file (a fresh `vault.unlock` re-reads it and purges caches; an automatic remove-then-reload flow is open). The `secrecy` crate is deliberately not used (zeroize only).

## 8. Providers

Trait sketch (original design; names are placeholders, the actual trait is described in the status note below):

```rust
trait Provider: Send + Sync {
    fn id(&self) -> &'static str;                   // "github"
    fn topics(&self) -> &'static [TopicSpec];       // name, JSON Schema of output, sensitivity, default interval
    fn egress_hosts(&self) -> &'static [&'static str];
    fn secrets(&self) -> &'static [&'static str];   // names it may read from the vault
    async fn fetch(&self, topic: &str, params: &Value, ctx: &mut Ctx) -> Result<Value, ProviderError>;
}
```

- `Ctx` exposes only: the provider's own allow-listed secrets, an HTTP client restricted to `egress_hosts()` (no redirects to other hosts, system TLS roots, timeouts, size limits), and a logger that redacts.
- Output is validated against the topic's schema; unknown fields are dropped. This is what makes field-level grants meaningful.
- Sensitivity: `low` (may be cached on disk if configured) or `high` (RAM only unless the user opts in to encrypted, time-limited persistence).
- Scheduler: per-topic interval with jitter, exponential backoff on errors, conditional requests (`ETag`/`If-None-Match`) where supported, stale flag on failure.
- First providers, in order: `system` (CPU, memory, disk, network from `/proc`), `battery` (UPower over D-Bus), `media` (MPRIS), then `github` (device flow, notifications and PRs), `ai-usage`, `docker` (read-only), `gmail` (IMAP), `calendar`/`tasks`.
- `ai-usage`: one adapter per harness, detected by presence of its local session store, each versioned and tested against fixture logs; counts and timings only; enabled per harness; `unsupported_version` state rather than wrong numbers.
- `docker`: talk to the local Docker socket but expose only an allow-listed read-only subset (list/inspect containers, stats). Start/stop/exec require a separate per-widget opt-in and are out of v0.1.

**Status:**
- Implemented: the `Provider` trait in `ow-core/src/provider.rs` is synchronous (no `async`, no `Ctx`, no `egress_hosts` or `secrets`). A `TopicSpec` is `{name, sensitivity, interval, fields, params}`; undeclared output fields are dropped and undeclared params rejected. Served now (all sensitivity low, no parameters): provider `system`: `system.cpu {usage_pct}`, `system.memory {total_mib, used_mib, used_pct}`, `system.load {load1, load5, load15}`, `system.time {hm "10:09", hms "10:09:42", date "Mon 19 Oct", tz string}` (1 s interval; local 24 h time, zone from `TZ` or `/etc/localtime` via jiff, DEP-TIME; `tz` is the tzdata abbreviation, or an offset such as "+0545" where the zone has none), `system.temp {celsius number}` (5 s; max over CPU hwmon drivers coretemp, k10temp, zenpower, cpu_thermal, x86_pkg_temp, else max over all hwmon `temp*_input`, else thermal zones; readings outside -50..150 C ignored; `unavailable` when no sensor); provider `battery`: `battery.status {percent number 0..100, state string, timeLeft string}` (10 s; first power supply of type Battery in natural name order (BAT2 before BAT10; any name such as macsmc-battery or CMB0 counts when its `type` says Battery, and a supply with no readable `type` counts only if its name starts with BAT); state is Charging, Discharging, Full, Not charging or Unknown; percent from `capacity`, else energy or charge ratio; `timeLeft` is "3h 20m" or "45m" while charging or discharging with a nonzero rate, otherwise an empty string; no battery is `unavailable`, unusable files are `format`). Both read sysfs only, with an injectable root for tests. Scheduler backoff, stale flag and RAM cache are in `owd/src/hub.rs`. `tz` is the tzdata abbreviation or a numeric offset, so Asia/Kathmandu shows "+0545" while the widget's sampleData says "NPT". jiff caches the system zone for about 5 minutes, so a time zone change can take that long to show in `system.time`. Local files are read through one helper: opened O_NONBLOCK and O_NOFOLLOW (final component only), must be a regular file, at most 4 KiB for sysfs and 256 KiB for /proc, so a FIFO, a symlink to /dev/zero or an oversized file is an error instead of a hang. `TZ` check at startup: jiff reads whatever file `TZ` names, so `owd` refuses to start (exit code 2, one line, value never printed) when `TZ` is set, non-empty and not a plain zone name: at most 64 characters from `A-Za-z0-9_+/:.-`, no leading `/` (after one optional `:`) and no `..` segment. POSIX rule strings are refused too; use a zone name.
- Not implemented: `battery`, `media`, `github`, `docker`, `gmail`, `calendar`, `ai-usage`; per-topic JSON Schema validation; any network provider, HTTP client or egress allow-list; ETag polling; disk or network topics for `system`.

## 9. Logging and diagnostics

`tracing` with a redaction layer; never log payloads, tokens, URLs with query strings, mail subjects, or calendar titles. `journalctl --user -u owd` is the log sink. `ow status` prints daemon version, vault mode and lock state, providers and their last success/failure, never values.

**Status:** Logging uses plain `eprintln!` (to the journal when run under systemd), not `tracing`, and there is no redaction layer; the code simply never prints payloads. `ow status` prints the raw JSON reply line (`{"v":0,"id":1,"ok":true,"daemonVersion":..,"protocol":0}`); it does not show vault mode, lock state or providers.

## 10. systemd user unit (verify with `systemd-analyze --user security owd`)

The unit lives in `packaging/owd.service`; read that file rather than a copy here. It is a `Type=notify` user service with `UMask=0077`, `RuntimeDirectory=owd`, `StateDirectory=owd` and `ConfigurationDirectory=owd` (all mode 0700), `ProtectSystem=strict`, `ProtectHome=read-only` with `ReadWritePaths=%h/.local/share/owd` (no `-` prefix: a missing directory stops the service with a clear error instead of leaving the vault directory read-only), `ProtectProc=invisible`, `NoNewPrivileges`, `PrivateTmp`, `MemoryDenyWriteExecute`, an empty `CapabilityBoundingSet`, `SystemCallFilter=@system-service`, `RestrictAddressFamilies` for unix/inet/inet6, `KeyringMode=private`, and kernel and cgroup protections. `ProcSubset=pid`, `IPAddressDeny` and `PrivateUsers` are deliberately not set (reasons are in the file's header comments).

**Status:** `systemd-analyze --user security --offline=yes` reports exposure 2.3 OK (2026-10-24), `verify` complains only that the binary is not installed yet. Verified 2026-10-17 as a transient unit with throwaway directories. It has not been run as an installed unit against the real home directories. The data directory `~/.local/share/owd` is created outside the unit (systemd has no `DataDirectory=`; an `ExecStartPre=` would run inside the same read-only sandbox) by `ow vault init`, the installer, or `install -d -m 0700`; `ow doctor` checks it. A low exposure score does not make code widgets sandboxed; the unit confines `owd`, not widgets. Docker and D-Bus allowances are not needed yet because those providers do not exist.

## 11. Testing and CI

- Unit: frame codec (including oversize, truncated, invalid UTF-8), grant matching, field filtering, rate limiter, scheduler backoff, vault record seal/open, tamper tests (flip every byte class: header, nonce, ciphertext, AAD swap), wrong passphrase, rotation.
- Property tests: codec round-trips; vault round-trips for arbitrary names/values.
- Fuzz targets (`cargo-fuzz`): socket frame parser, grants parser, vault file parser. Build in CI, run on a schedule.
- Integration: spawn `owd` against a temp XDG tree with a fake provider; a test client verifies hello, subscribe, push, denied, stale, rate limit, reconnect.
- Security regression tests: run a full session with a recognizable fake secret and assert it appears nowhere in logs, audit file, argv (`/proc/<pid>/cmdline`), environment (`/proc/<pid>/environ`), or any file except the encrypted vault. Assert socket dir and file modes. Assert a connection from a different uid is refused (test in a container or with `unshare` if available).
- Egress tests: a provider trying a non-allow-listed host or a cross-host redirect fails.
- CI gates: `fmt`, `clippy -D warnings`, `cargo deny check`, `cargo audit`, tests, fuzz build. SBOM and signed release artifacts at release time.

**Status:** Not done: a different-uid connection refusal is not integration-tested, and there are no egress tests (there is no egress). Other items in this list are covered to the extent the crates' tests and `fuzz/` targets cover them; this note does not claim full coverage of the list.

## 12. Build order (each step ends with something runnable)

1. **Skeleton**: workspace, `owd` starts/stops under `systemctl --user`, logs to journal. Done when it restarts cleanly and `ow status` reaches it.
2. **Socket and identity**: socket with correct modes, `SO_PEERCRED`, codec, `hello`/`status`. Done when a second uid cannot connect and the parser survives fuzzing.
3. **Grants and audit**: deny by default, field filtering, audit log. Done when an ungranted topic is refused and leaves an audit line.
4. **Vault**: file format, keyring and passphrase modes, CLI. Done when tampered files fail to open, keys are zeroized, and the secret-leak regression test passes.
5. **Local providers**: `system`, `battery`, `media`, scheduler, RAM cache, push. Done when a spec "System" card in the shell updates live.
6. **Network providers**: `github` (device flow), egress allow-list, ETag polling. Done when the token appears in no log and no widget message.
7. **Harden and ship**: systemd sandbox, `cargo-deny`/`audit`, fuzz in CI, PKGBUILD, threat-model doc, security policy. Done when `systemd-analyze security` is green and an outside review of vault and protocol passes.

**Status:** Steps 1, 2, 3 and 5 are done (step 5 for the `system` provider only, and step 2's "second uid cannot connect" check is untested as noted in section 11). Step 4 is partial: passphrase mode only; the daemon unlocks and holds the vault, keyring auto-unlock is not built. Steps 6 and 7 are open.

## 13. Open design questions for this component

1. D-1 admin operations authorization (section 5).
2. Does the Quickshell side allow a service plugin to keep a persistent Unix socket connection (`Quickshell.Io` socket types), and what is the reconnect story across `rescanPlugins`?
3. Can a plugin import another plugin's singleton, or must each widget open its own client through the kit?
4. Keyring behavior under Omarchy 4 autologin: measure what an arbitrary same-uid process can read.
5. Do `omarchy.monitor` and `omarchy.agents` already expose data we should read instead of re-collecting?
