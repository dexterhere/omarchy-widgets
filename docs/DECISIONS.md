# Decision log

Append newest at the bottom. Format: date, ID, decision, reason, evidence, what would change it.

## Standing decisions (from planning, 2026-10-17)

- D-0 Strategy: build on Omarchy's plugin system, do not rebuild runtime, marketplace or desktop host. Reason: they exist. Would change if the host/upstream path is closed off.
- D-0 Safety model: spec widgets plus reviewed code widgets; no sandbox claims. Would change if a separate-process host proves feasible.
- D-0 Data layer: vault plus provider daemon in v1 with a published threat model.

## Pending (decide in Phase 0)

- D-1 Authorization of admin operations on the daemon socket.
- D-2 Host strategy (contribute / fork / standalone).
- D-3 Project name and fallback.
- D-4 Sidecar `widget.json` vs folded into manifest.
- D-5 Shared bridge singleton vs per-widget client.
- D-6 Monorepo + mirror repos vs single-commit fetch for monorepo widgets.
- D-7 Default Center shortcut chord.

## Log

- 2026-10-17 ENV Test machine is the daily-driver laptop, bare metal (no VM), Omarchy 4.0.4-1, Quickshell 0.3.1-1, Hyprland 0.56.2-2, TPM2 present (/dev/tpmrm0), Wayland. Evidence: `pacman -Q`, `systemd-detect-virt` = none. Consequence: daemon work runs on throwaway XDG dirs only; any spike touching the real shell or keyring needs explicit approval (CLAUDE.md). Would change if a VM is provisioned.
- 2026-10-17 REPO Branch model main (production) / staging (review) / develop (integration); protected, PR + green `rust` check required, approvals 0 while single maintainer. Secret scanning + push protection on. Would change when a second maintainer joins (raise approvals to 1, enforce_admins).
- 2026-10-17 DEP ow-proto uses `serde`, `serde_json`, `thiserror` (ubiquitous, actively maintained, no unsafe in our use). Recheck with `cargo audit` in CI.
- 2026-10-17 SOCK `SO_PEERCRED` read via `rustix::net::sockopt::socket_peercred` (safe, no unsafe in our code). Server is thread-per-connection with a cap of 32 and a polling accept loop; upgrade to tokio when providers need async. Evidence: crates/owd/tests/socket.rs. Known limit: different-uid refusal is not integration-tested yet (needs a second uid; try `unshare`/container). Unix socket paths must be under 108 bytes, so `--config-dir` must be short.
- 2026-10-17 DEP owd adds `rustix` (net, process, fs), `signal-hook`, `sd-notify`; dev: `tempfile`. All widely used; recheck via `cargo audit`/`cargo deny` in CI.
- 2026-10-17 REVIEW (owd socket) Independent review fixes: flock'd lock file serialises starts; slots freed by RAII guard; first-frame timeout 10s, idle 600s, write timeout 5s; disconnect after 8 consecutive bad frames; process umask 0077; `ow status` errors on EOF. DAEMON_SPEC section 10 unit was wrong: the runtime dir was read-only under ProtectSystem=strict. Unit now uses RuntimeDirectory/StateDirectory/ConfigurationDirectory plus extra hardening; still unverified under systemd, and the data dir has no matching directive (open).
- 2026-10-17 GRANTS Grants load once at startup from `$XDG_CONFIG_HOME/owd/grants.toml`; must be mode 0600 and ours, unknown keys rejected, any error aborts startup (fail closed). Missing file = no grants. Order in the data path: grant check, then rate limit (so limiter memory is bounded by grants, not client input), then provider. Widget id is client-claimed: guardrail only for code widgets (D-1 unresolved). Rate limit constants: burst 10, 2/s per (widget, topic). Audit entries are typed (no payload field), ids clipped to 128 chars, JSON-escaped, rotated at 1 MiB (single `.1` backup). Known gap: a same-uid client can flood denied requests to rotate real entries out of the audit log; rate-limit denied audit writes if this matters. `subscribe` returns `unknown_topic` after grant checks until the scheduler exists. No hot reload and no `grants.set`/`audit.tail` admin ops yet (restart to apply grants).
- 2026-10-17 DEP ow-core adds `toml`, `serde`, `thiserror`, `rustix` (fs, process).
- 2026-10-17 REVIEW (grants/audit) Fixes: bad-frame cap counts only unparsable frames; audit write failure on an allowed request returns `internal` and releases no data (fail closed); audit reopens if the file is deleted/replaced, tightens mode to 0600, keeps 5 numbered rotations; state dir ownership/mode checked like the runtime dir; grants loaded via O_NOFOLLOW|O_NONBLOCK and checked on the descriptor; duplicate (widget, topic) grants rejected; SIGHUP reloads grants and any load error drops to deny-all. `fields` filtering is top-level keys only (providers must not nest mixed-sensitivity data under one key). Residual: a same-uid denied-request flood can still push old entries out after 5 rotations (about 6 MiB).
