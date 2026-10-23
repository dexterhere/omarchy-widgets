# Build plan

Durations are rough guesses; rebase after Phase 0. Each phase ends in something a stranger can install. Update checkboxes as work completes.

## Working agreements

- One concern per commit, tests with the code. Security rules in `CLAUDE.md` are release blockers.
- Develop against throwaway XDG dirs. A disposable Omarchy 4 VM or machine snapshot is needed for anything touching the real shell (Phase 0, item 1).
- Anything uncertain about Omarchy/Quickshell goes into `docs/OPEN_QUESTIONS.md` with a proposed experiment; answers go into `docs/DECISIONS.md`.
- Human-only tasks (contacting maintainers, naming, publishing, money, accounts) are listed below as **[Dexter]**. The agent reminds, never performs.

## Repository layout (placeholder names)

```
/                     LICENSE (MIT), README, CLAUDE.md, SECURITY.md, CODE_OF_CONDUCT.md
crates/               ow-proto, ow-vault, ow-core, ow-providers, owd, ow
shell/
  host/               canvas + edit mode (or upstream contribution)
  bridge/             service plugin: socket -> QML singleton
  kit/                shared QML components, theme helpers, lint rules
  center/             Widget Center panel (thin client of ow)
widgets/              first-party widgets, each a valid Omarchy plugin dir + widget.json
spec/                 JSON Schemas: widget.json, spec layout, registry listing, pack
registry/             listing files, packs, build scripts producing index.json
site/                 static site, playground, web renderer (TypeScript)
docs/                 PROJECT_BRIEF, DAEMON_SPEC, BUILD_PLAN, DECISIONS, OPEN_QUESTIONS, threat model
design/               approved visual demo (reference only)
packaging/            PKGBUILD, systemd unit
.github/workflows/    CI
```

Note: `site/` and `registry/` live in the web repo (dexterhere/dexterhere-omarchy-widgets-web), not in this repo; the layout above shows them for the overall plan.

Note: an Omarchy plugin install requires exactly one `manifest.json` at a repository root. Monorepo widgets therefore need mirror repos (or Plugin Depot-style single-commit fetch) for standalone `omarchy plugin add`. Decide this in Phase 0.

## Progress (updated 2026-10-17)

Phase 1, epic 1 (daemon core) is largely done ahead of the Phase 0 spikes because it is host-independent:

- [x] Workspace, lints, CI (fmt, clippy, test, deny, audit), fuzz targets and fuzz CI
- [x] DAEMON_SPEC step 1 and 2: skeleton, socket server, identity, codec (systemd unit written, **not yet verified** under `systemctl --user`)
- [x] Step 3: grants, rate limit, redacted audit log
- [x] Step 4 (partly): vault format v0.1 and passphrase mode, `ow vault` CLI. Keyring and TPM modes wait for spike S4; kernel-keyring key caching and daemon unlock are not built
- [x] Daemon-side vault unlock, passphrase mode (AUTH-UNLOCK): `vault.unlock|lock|status`, attempt limit and lockout, idle lock, `locked` error plumbing, `ow vault unlock|lock|status`, `ow vault set` tty passphrase, daemon leak test. Not done: keyring auto-unlock, network providers, grants admin ops, remove-then-reload flow
- [x] Step 5: provider hub, scheduler, push, `system` provider
- [ ] Step 6 network providers, step 7 hardening/PKGBUILD/outside review
- [ ] Epics 2 to 7 (bridge and kit, canvas, `ow` manager verbs, Widget Center, first widgets, packaging)

Phase 0 spikes S1 to S13 and all **[Dexter]** tasks are untouched. Machine facts: bare metal Omarchy 4.0.4, Quickshell 0.3.1, Hyprland 0.56.2, TPM2 present (see DECISIONS.md).

## Phase 0: spikes and decisions (about 2 weeks)

Goal: replace guesses with facts and settle the host and name. No production code beyond throwaway spikes, except the daemon skeleton if it falls out naturally.

Human tasks
- [ ] **[Dexter]** Open a discussion with the author of `anishfn/omarchy-widgets` proposing: free-form canvas, external package loader, provider API. Agent drafts the message on request.
- [ ] **[Dexter]** Decide host strategy after the reply or a one-week silence (contribute, fork under MIT with credit, or standalone).
- [ ] **[Dexter]** Pick the name; ask the Omarchy maintainers about using "Omarchy" in it; keep a fallback.
- [ ] **[Dexter]** Create the repo (and org if wanted).

Environment
- [ ] Omarchy 4 test machine or VM with a restorable snapshot. Record Omarchy version, Quickshell version, Hyprland version in DECISIONS.md.

Spikes (each: write a minimal experiment in `spikes/<name>/`, record result and exact evidence)
- [ ] S1 Canvas surface: a service plugin draws a `WlrLayer.Bottom` surface, click-through except one interactive rectangle (empty input mask plus one region). Check focus stealing, lock screen behavior, multi-monitor.
- [ ] S2 Edit surface: Overlay-layer surface with free-form drag and resize that writes anchors and sizes to JSON; Escape exits; keyboard nudge works.
- [ ] S3 Socket bridge: a QML service plugin connects to a Unix socket served by a tiny Rust stub, with `SO_PEERCRED` uid check; survives `rescanPlugins`; reconnects.
- [ ] S4 Vault round trip: keyring, passphrase and TPM modes. **Measure what an arbitrary same-uid process can read from the keyring under Omarchy 4 autologin.**
- [ ] S5 Manifest tolerance: does `omarchy plugin validate` accept extra top-level manifest keys? If yes, can `widget.json` fold into the manifest?
- [ ] S6 Plugin dependencies: can one plugin import another's singleton? Decides shared bridge singleton vs per-widget client.
- [ ] S7 `omarchy plugin add`: does it accept a commit SHA or a subdirectory, or always track HEAD?
- [ ] S8 Theme tokens: which shell theme singletons and components may third-party QML import, and are they stable? Read the shipped schema (sources disagree on 22 vs 24 colors).
- [ ] S9 Settings form: how far does the manifest's settings form go (types, validation, per-instance values)?
- [ ] S10 Existing data: what do `omarchy.monitor` and `omarchy.agents` expose that we can read instead of re-collecting?
- [ ] S11 Headless previews: can the shell run on a nested/headless compositor in CI and produce screenshots?
- [ ] S12 Marketplace reality: what does the live catalog contain by category; where are the real gaps?
- [ ] S13 Keybinding: confirm a free chord for the Center on stock Omarchy 4; prototype the consented, reversible `bindings.lua` edit (against a copy).

Documents
- [x] Threat model v0 (`docs/threat-model.md`) from PROJECT_BRIEF section 10, updated with S4 results. (Draft exists; S4 update still pending.)
- [ ] Spec schema v0 (`spec/widget.schema.json`, `spec/layout.schema.json`).
- [ ] DECISIONS.md filled for D-1 (admin ops authorization), host strategy, name, sidecar vs manifest, shared singleton vs per-widget client, monorepo vs mirror strategy.

Exit criteria: every spike has a recorded result; the three "shape-changing" questions (S4 keyring behavior, S5/S6 plugin contract, host decision) are answered; Phase 1 is rebased.

## Phase 1: foundation (about 6 weeks)

Goal: a user can install the daemon, put a themed widget on the desktop, arrange it freely, and the data path is secure.

Epics (in order; each has acceptance criteria)
1. **Daemon core** (DAEMON_SPEC section 12 steps 1 to 5): skeleton, socket and identity, grants and audit, vault, local providers. Acceptance: `systemctl --user start owd`; `ow status`; fake-secret regression test green; fuzz targets build; System provider pushes live values.
2. **Bridge and kit**: bridge service plugin, QML singleton, kit components (card, ring, bar, sparkline, list row), theme helpers, qmllint rules including "no color literals". Acceptance: a throwaway widget shows live CPU in two themes and updates on `omarchy theme set` without restart.
3. **Canvas** (in the chosen host): free-form place/resize/remove, anchors and size classes, edit overlay, tray with undo, per-monitor layout keyed by monitor set, snapshot before first change. Acceptance: matches the behaviors listed in PROJECT_BRIEF section 6; layout survives restart, resolution change, monitor hotplug.
4. **Package spec v1 and `ow` core verbs**: `ow new`, `check`, `install`, `enable`, `place`, `snapshot`, `rollback`, `doctor`, all with `--json --yes`. Acceptance: scaffold, check and install a code widget end to end on the test machine; rollback restores `shell.json` and plugin state.
5. **Widget Center v0**: opens by key and bar icon; Installed and Layouts tabs; inspector (size class, variant, accent role, remove); grants screen; audit viewer. Acceptance: target look and behavior in `design/` (reference only; use real theme tokens, not hex).
6. **First widgets**: Clock variants, System, Battery (via existing providers). Acceptance: each has empty/loading/error states and passes `ow check` on three themes.
7. **Packaging**: PKGBUILD for `owd` + `ow`, unit file, install docs. Acceptance: fresh Omarchy VM goes from zero to a working widget in under 10 minutes following the README.

Release gate for a public alpha: the secure-data checklist in DAEMON_SPEC section 11 passes; threat model published; SECURITY.md with private disclosure address **[Dexter]**.

## Phase 2: suite and public beta v0.1 (about 6 weeks)

New providers and widgets: GitHub (device flow), Gmail (IMAP then BYO-OAuth), Calendar (ICS/CalDAV), Tasks (local file, encrypted), Docker (read-only subset), AI usage (adapters: start with the harnesses that keep local JSONL session logs, such as Claude Code and Codex; verify formats and add fixtures), Music variants. Discover tab reading the registry index. Gate: **public beta v0.1 only after an external review of the vault and protocol.**

## Phase 3: maker (about 8 weeks)

Levels 0 and 3 first (scaffolder polish, agent skill and `llms.txt`); then spec widgets with the generic renderer (QML) and the TypeScript web renderer with golden-screenshot parity tests; then the visual designer panel and the browser playground.

## Phase 4: ecosystem (ongoing)

Registry and CI previews, website (gallery, widget pages, packs, docs, security), trust tiers and review process, RFC process, contribution ladder, translations, more providers and harness adapters.

## Definition of done (every change)

- Tests added or updated, CI green (`fmt`, `clippy -D warnings`, tests, `deny`, `audit`, `qmllint` where relevant).
- No secret in any log or file in the regression test.
- Docs updated (this file's checkboxes, DECISIONS.md, user docs if user-facing).
- User-facing text honest about limits (code widgets are not sandboxed).
